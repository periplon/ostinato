use std::cmp::Ordering;

use bigdecimal::BigDecimal;
use num_bigint::{BigInt, BigUint};
use serde_json::Value as Json;
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::error::LangError;

const MAX_TEMPLATE_NESTING: usize = 256;
const MAX_PRINTF_FIELD: usize = 1024 * 1024;
const MAX_TEMPLATE_INTEGER_DIGITS: usize = 1024 * 1024;

/// Render a Go `text/template`-compatible subset against a JSON context.
///
/// Supported syntax:
///
/// * `{{ path.to.value }}` (root-relative, as before)
/// * `{{ .path }}` (relative to the current dot) and `{{ $.path }}` (root)
/// * `{{ . }}` and `{{ $ }}`
/// * `{{ if pipeline }}...{{ else if pipeline }}...{{ else }}...{{ end }}`
/// * `{{ range pipeline }}...{{ else }}...{{ end }}`
/// * `{{ with pipeline }}...{{ else }}...{{ end }}`
/// * `{{/* comment */}}`
/// * pipelines `value | func arg...` with `eq`, `ne`, `lt`, `le`, `gt`,
///   `ge`, `and`, `or`, `not`, `len`, `index`, `print`, `printf`
/// * whitespace trim markers `{{-` and `-}}`
/// * quoted strings, integers/floats, `true`, `false`, `null` and
///   parenthesized pipelines
///
/// # Errors
/// Returns [`LangError::Template`] for malformed actions, unknown functions,
/// unknown references and execution errors.
pub fn render(template: &str, context: &Json) -> Result<String, LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        render_impl(template, context)
    })
}

fn render_impl(template: &str, context: &Json) -> Result<String, LangError> {
    let tokens = tokenize(template)?;
    let nodes = parse_nodes(&tokens)?;
    let engine = Engine { root: context };
    let mut out = String::new();
    engine.render_nodes(&nodes, context, &mut out)?;
    Ok(out)
}

/// Resolve a dotted path in a JSON value. Array indices are numeric segments.
///
/// # Errors
/// Returns [`LangError::Template`] when the path does not exist.
pub fn lookup_path<'a>(context: &'a Json, path: &str) -> Result<&'a Json, LangError> {
    let segments: Vec<String> = path.split('.').map(str::to_owned).collect();
    lookup_segments(context, &segments, path)
}

// ---------------------------------------------------------------------------
// Positions and errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Position {
    line: usize,
    column: usize,
}

struct LineMap {
    starts: Vec<usize>,
}

impl LineMap {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        Self { starts }
    }

    fn position(&self, offset: usize) -> Position {
        let line_index = self
            .starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.starts.get(line_index).copied().unwrap_or(0);
        Position {
            line: line_index + 1,
            column: offset.saturating_sub(line_start) + 1,
        }
    }
}

fn error_at(position: Position, message: impl Into<String>) -> LangError {
    LangError::Template(format!(
        "{} (at line {}, column {})",
        message.into(),
        position.line,
        position.column
    ))
}

fn locate_error(error: LangError, position: Position) -> LangError {
    match error {
        LangError::Template(message) => error_at(position, message),
        other => other,
    }
}

fn is_go_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n')
}

fn is_go_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum TokenKind {
    Text(String),
    Action(String),
}

#[derive(Debug)]
struct Token {
    kind: TokenKind,
    position: Position,
}

struct ScannedAction {
    content: String,
    trim_right: bool,
    next: usize,
}

fn tokenize(template: &str) -> Result<Vec<Token>, LangError> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut cursor = 0usize;
    let mut trim_next = false;
    let lines = LineMap::new(template);

    while cursor < template.len() {
        let Some(relative) = template[cursor..].find("{{") else {
            push_text(
                &mut tokens,
                &template[cursor..],
                cursor,
                &mut trim_next,
                &lines,
            );
            break;
        };
        let start = cursor + relative;
        if start > cursor {
            push_text(
                &mut tokens,
                &template[cursor..start],
                cursor,
                &mut trim_next,
                &lines,
            );
        }
        let position = lines.position(start);
        let after_marker = &template[start + 2..];
        let trim_left = after_marker.starts_with('-')
            && after_marker[1..].chars().next().is_some_and(is_go_space);
        let content_start = if trim_left { start + 3 } else { start + 2 };

        let after_content = &template[content_start..];
        let trimmed_len = after_content.trim_start_matches(is_go_space).len();
        let first_non_space = after_content.len() - trimmed_len;

        if after_content[first_non_space..].starts_with("/*") {
            let comment_open = content_start + first_non_space;
            let (trim_right, next) = scan_comment(template, comment_open, position)?;
            if trim_left {
                trim_previous_text(&mut tokens);
            }
            cursor = next;
            trim_next = trim_right;
            continue;
        }

        let scanned = scan_action(template, content_start, position)?;
        if trim_left {
            trim_previous_text(&mut tokens);
        }
        tokens.push(Token {
            kind: TokenKind::Action(scanned.content),
            position,
        });
        cursor = scanned.next;
        trim_next = scanned.trim_right;
    }

    Ok(tokens)
}

fn push_text(
    tokens: &mut Vec<Token>,
    text: &str,
    offset: usize,
    trim_next: &mut bool,
    lines: &LineMap,
) {
    let text = if *trim_next {
        text.trim_start_matches(is_go_space)
    } else {
        text
    };
    *trim_next = false;
    if text.is_empty() {
        return;
    }
    tokens.push(Token {
        kind: TokenKind::Text(text.to_owned()),
        position: lines.position(offset),
    });
}

fn trim_previous_text(tokens: &mut [Token]) {
    if let Some(Token {
        kind: TokenKind::Text(text),
        ..
    }) = tokens.last_mut()
    {
        let trimmed = text.trim_end_matches(is_go_space).len();
        text.truncate(trimmed);
    }
}

fn scan_comment(
    template: &str,
    comment_open: usize,
    position: Position,
) -> Result<(bool, usize), LangError> {
    let after_open = comment_open + 2;
    let Some(close_relative) = template[after_open..].find("*/") else {
        return Err(error_at(position, "unterminated template comment"));
    };
    let mut index = after_open + close_relative + 2;
    let bytes = template.as_bytes();
    while index < bytes.len() && is_go_space_byte(bytes[index]) {
        index += 1;
    }
    if template[index..].starts_with("-}}") {
        Ok((true, index + 3))
    } else if template[index..].starts_with("}}") {
        Ok((false, index + 2))
    } else {
        Err(error_at(position, "expected `}}` after template comment"))
    }
}

fn scan_action(
    template: &str,
    content_start: usize,
    position: Position,
) -> Result<ScannedAction, LangError> {
    let bytes = template.as_bytes();
    let mut index = content_start;
    let mut quote: Option<u8> = None;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if open != b'`' && byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => {
                quote = Some(byte);
                index += 1;
            }
            b'}' if bytes.get(index + 1) == Some(&b'}') => {
                let mut end = index;
                let mut trim_right = false;
                if end > content_start && bytes[end - 1] == b'-' && is_go_space_byte(bytes[end - 2])
                {
                    trim_right = true;
                    end -= 1;
                }
                return Ok(ScannedAction {
                    content: template[content_start..end].to_owned(),
                    trim_right,
                    next: index + 2,
                });
            }
            _ => index += 1,
        }
    }

    let message = if quote.is_some() {
        "unterminated quoted string in template action"
    } else {
        "unterminated `{{ ... }}` action"
    };
    Err(error_at(position, message))
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Node {
    Text(String),
    Output(Pipeline),
    LegacyPath {
        path: String,
        position: Position,
    },
    If {
        condition: Pipeline,
        body: Vec<Node>,
        otherwise: Vec<Node>,
    },
    Range {
        expression: Pipeline,
        body: Vec<Node>,
        otherwise: Vec<Node>,
        position: Position,
    },
    With {
        expression: Pipeline,
        body: Vec<Node>,
        otherwise: Vec<Node>,
    },
}

#[derive(Debug)]
enum Stop {
    Eof,
    End(Position),
    Else(String, Position),
}

fn parse_nodes(tokens: &[Token]) -> Result<Vec<Node>, LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        parse_nodes_impl(tokens)
    })
}

fn parse_nodes_impl(tokens: &[Token]) -> Result<Vec<Node>, LangError> {
    let mut index = 0usize;
    let (nodes, stop) = parse_block(tokens, &mut index, 0)?;
    match stop {
        Stop::Eof => Ok(nodes),
        Stop::End(position) => Err(error_at(
            position,
            "unexpected `{{end}}` without an open `if`, `range` or `with`",
        )),
        Stop::Else(_, position) => Err(error_at(
            position,
            "unexpected `{{else}}` without an open `if`, `range` or `with`",
        )),
    }
}

fn parse_block(
    tokens: &[Token],
    index: &mut usize,
    depth: usize,
) -> Result<(Vec<Node>, Stop), LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        parse_block_impl(tokens, index, depth)
    })
}

fn parse_block_impl(
    tokens: &[Token],
    index: &mut usize,
    depth: usize,
) -> Result<(Vec<Node>, Stop), LangError> {
    let mut nodes = Vec::new();
    while let Some(token) = tokens.get(*index) {
        match &token.kind {
            TokenKind::Text(text) => {
                nodes.push(Node::Text(text.clone()));
                *index += 1;
            }
            TokenKind::Action(content) => {
                let content = content.trim();
                let (keyword, rest) = split_keyword(content);
                match keyword {
                    "end" => {
                        if !rest.is_empty() {
                            return Err(error_at(token.position, "unexpected content after `end`"));
                        }
                        *index += 1;
                        return Ok((nodes, Stop::End(token.position)));
                    }
                    "else" => {
                        *index += 1;
                        return Ok((nodes, Stop::Else(rest.to_owned(), token.position)));
                    }
                    "if" => {
                        *index += 1;
                        nodes.push(parse_if(rest, tokens, index, token.position, depth)?);
                    }
                    "range" => {
                        *index += 1;
                        nodes.push(parse_range(rest, tokens, index, token.position, depth)?);
                    }
                    "with" => {
                        *index += 1;
                        nodes.push(parse_with(rest, tokens, index, token.position, depth)?);
                    }
                    _ => {
                        *index += 1;
                        match parse_pipeline(content, token.position) {
                            Ok(pipeline) => nodes.push(Node::Output(pipeline)),
                            Err(error) => {
                                if let Some(path) = legacy_path(content) {
                                    nodes.push(Node::LegacyPath {
                                        path: path.to_owned(),
                                        position: token.position,
                                    });
                                } else {
                                    return Err(error);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((nodes, Stop::Eof))
}

fn parse_if(
    condition: &str,
    tokens: &[Token],
    index: &mut usize,
    position: Position,
    depth: usize,
) -> Result<Node, LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        parse_if_impl(condition, tokens, index, position, depth)
    })
}

fn parse_if_impl(
    condition: &str,
    tokens: &[Token],
    index: &mut usize,
    position: Position,
    depth: usize,
) -> Result<Node, LangError> {
    ensure_nesting(depth, position)?;
    if condition.is_empty() {
        return Err(error_at(position, "`if` requires a pipeline"));
    }
    let condition = parse_pipeline(condition, position)?;
    let (body, stop) = parse_block(tokens, index, depth + 1)?;
    let otherwise = match stop {
        Stop::Eof => return Err(error_at(position, "missing `{{end}}` for `if`")),
        Stop::End(_) => Vec::new(),
        Stop::Else(rest, else_position) => {
            let rest = rest.trim();
            if rest.is_empty() {
                let (otherwise, stop) = parse_block(tokens, index, depth + 1)?;
                match stop {
                    Stop::End(_) => otherwise,
                    Stop::Eof => return Err(error_at(position, "missing `{{end}}` for `if`")),
                    Stop::Else(_, second) => {
                        return Err(error_at(second, "unexpected second `{{else}}` in `if`"));
                    }
                }
            } else if let Some(nested_condition) = strip_keyword(rest, "if") {
                if nested_condition.is_empty() {
                    return Err(error_at(else_position, "`else if` requires a pipeline"));
                }
                vec![parse_if(
                    nested_condition,
                    tokens,
                    index,
                    else_position,
                    depth + 1,
                )?]
            } else {
                return Err(error_at(
                    else_position,
                    "unexpected content after `{{else}}`; use `{{else if ...}}` or `{{else}}`",
                ));
            }
        }
    };
    Ok(Node::If {
        condition,
        body,
        otherwise,
    })
}

fn parse_range(
    expression: &str,
    tokens: &[Token],
    index: &mut usize,
    position: Position,
    depth: usize,
) -> Result<Node, LangError> {
    ensure_nesting(depth, position)?;
    if expression.is_empty() {
        return Err(error_at(position, "`range` requires a pipeline"));
    }
    let expression = parse_pipeline(expression, position)?;
    let (body, stop) = parse_block(tokens, index, depth + 1)?;
    let otherwise = parse_control_else(stop, tokens, index, position, "range", depth + 1)?;
    Ok(Node::Range {
        expression,
        body,
        otherwise,
        position,
    })
}

fn parse_with(
    expression: &str,
    tokens: &[Token],
    index: &mut usize,
    position: Position,
    depth: usize,
) -> Result<Node, LangError> {
    ensure_nesting(depth, position)?;
    if expression.is_empty() {
        return Err(error_at(position, "`with` requires a pipeline"));
    }
    let expression = parse_pipeline(expression, position)?;
    let (body, stop) = parse_block(tokens, index, depth + 1)?;
    let otherwise = parse_control_else(stop, tokens, index, position, "with", depth + 1)?;
    Ok(Node::With {
        expression,
        body,
        otherwise,
    })
}

fn parse_control_else(
    stop: Stop,
    tokens: &[Token],
    index: &mut usize,
    position: Position,
    keyword: &str,
    depth: usize,
) -> Result<Vec<Node>, LangError> {
    match stop {
        Stop::Eof => Err(error_at(
            position,
            format!("missing `{{{{end}}}}` for `{keyword}`"),
        )),
        Stop::End(_) => Ok(Vec::new()),
        Stop::Else(rest, else_position) => {
            if !rest.trim().is_empty() {
                return Err(error_at(
                    else_position,
                    format!("`{keyword}` does not support `else if`; use a nested `if`"),
                ));
            }
            let (otherwise, stop) = parse_block(tokens, index, depth)?;
            match stop {
                Stop::End(_) => Ok(otherwise),
                Stop::Eof => Err(error_at(
                    position,
                    format!("missing `{{{{end}}}}` for `{keyword}`"),
                )),
                Stop::Else(_, second) => Err(error_at(
                    second,
                    format!("unexpected second `{{{{else}}}}` in `{keyword}`"),
                )),
            }
        }
    }
}

fn ensure_nesting(depth: usize, position: Position) -> Result<(), LangError> {
    if depth >= MAX_TEMPLATE_NESTING {
        Err(error_at(
            position,
            format!("template nesting exceeds the limit of {MAX_TEMPLATE_NESTING}"),
        ))
    } else {
        Ok(())
    }
}

fn legacy_path(content: &str) -> Option<&str> {
    let content = content.trim();
    if content.is_empty() || !content.contains('.') || !content.chars().any(char::is_whitespace) {
        return None;
    }
    let has_operator = content
        .chars()
        .any(|character| matches!(character, '|' | '(' | ')' | '"' | '`' | '\''));
    if has_operator { None } else { Some(content) }
}

fn split_keyword(content: &str) -> (&str, &str) {
    match content.find(char::is_whitespace) {
        Some(index) => (&content[..index], content[index..].trim_start()),
        None => (content, ""),
    }
}

fn strip_keyword<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(keyword)?;
    if rest.is_empty() {
        Some("")
    } else if rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Pipeline parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Pipeline {
    commands: Vec<Command>,
    position: Position,
}

#[derive(Debug, Clone)]
struct Command {
    callee: Operand,
    arguments: Vec<Operand>,
}

#[derive(Debug, Clone)]
enum Operand {
    Literal(Json),
    Path {
        from_root: bool,
        segments: Vec<String>,
        display: String,
    },
    Group(Box<Pipeline>),
    Postfix {
        base: Box<Operand>,
        segments: Vec<String>,
        display: String,
    },
    Function(String),
}

#[derive(Debug, Clone)]
enum Lexeme {
    Word(String),
    /// Adjacent postfix access such as `).name`; whitespace before the dot
    /// makes it a regular dot-relative `Word` instead.
    Postfix(String),
    Quoted(String),
    LeftParen,
    RightParen,
    Pipe,
}

const FUNCTIONS: &[&str] = &[
    "eq", "ne", "lt", "le", "gt", "ge", "and", "or", "not", "len", "index", "print", "printf",
];

fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

fn parse_pipeline(source: &str, position: Position) -> Result<Pipeline, LangError> {
    let lexemes = lex_expression(source, position)?;
    if lexemes.is_empty() {
        return Err(error_at(position, "empty template action"));
    }
    let mut parser = PipelineParser {
        lexemes,
        index: 0,
        position,
        depth: 0,
    };
    let pipeline = parser.parse()?;
    if parser.peek().is_some() {
        return Err(error_at(
            position,
            "unexpected token after template pipeline",
        ));
    }
    Ok(pipeline)
}

struct PipelineParser {
    lexemes: Vec<Lexeme>,
    index: usize,
    position: Position,
    depth: usize,
}

impl PipelineParser {
    fn peek(&self) -> Option<&Lexeme> {
        self.lexemes.get(self.index)
    }

    fn next(&mut self) -> Option<Lexeme> {
        let lexeme = self.lexemes.get(self.index).cloned();
        if lexeme.is_some() {
            self.index += 1;
        }
        lexeme
    }

    fn parse(&mut self) -> Result<Pipeline, LangError> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || self.parse_impl())
    }

    fn parse_impl(&mut self) -> Result<Pipeline, LangError> {
        let mut commands = vec![self.parse_command()?];
        while matches!(self.peek(), Some(Lexeme::Pipe)) {
            self.index += 1;
            if self.peek().is_none() {
                return Err(error_at(
                    self.position,
                    "expected a command after `|` in template pipeline",
                ));
            }
            commands.push(self.parse_command()?);
        }
        Ok(Pipeline {
            commands,
            position: self.position,
        })
    }

    fn parse_command(&mut self) -> Result<Command, LangError> {
        let callee = self.parse_operand(true)?;
        if let Operand::Function(name) = &callee {
            let mut arguments = Vec::new();
            while !matches!(
                self.peek(),
                None | Some(Lexeme::Pipe) | Some(Lexeme::RightParen)
            ) {
                arguments.push(self.parse_operand(false)?);
            }
            return Ok(Command {
                callee: Operand::Function(name.clone()),
                arguments,
            });
        }
        if !matches!(
            self.peek(),
            None | Some(Lexeme::Pipe) | Some(Lexeme::RightParen)
        ) {
            if let Some(name) = bare_function_name(&callee) {
                let mut arguments = Vec::new();
                while !matches!(
                    self.peek(),
                    None | Some(Lexeme::Pipe) | Some(Lexeme::RightParen)
                ) {
                    arguments.push(self.parse_operand(false)?);
                }
                return Ok(Command {
                    callee: Operand::Function(name),
                    arguments,
                });
            }
            return Err(error_at(
                self.position,
                "unexpected argument after template value",
            ));
        }
        Ok(Command {
            callee,
            arguments: Vec::new(),
        })
    }

    fn parse_operand(&mut self, allow_function: bool) -> Result<Operand, LangError> {
        let Some(lexeme) = self.next() else {
            return Err(error_at(self.position, "expected a template value"));
        };
        match lexeme {
            Lexeme::Quoted(value) => Ok(Operand::Literal(Json::String(value))),
            Lexeme::LeftParen => {
                ensure_nesting(self.depth, self.position)?;
                self.depth += 1;
                let pipeline = self.parse();
                self.depth -= 1;
                let pipeline = pipeline?;
                let mut operand = match self.next() {
                    Some(Lexeme::RightParen) => Operand::Group(Box::new(pipeline)),
                    _ => return Err(error_at(self.position, "missing `)` in template pipeline")),
                };
                while let Some(Lexeme::Postfix(word)) = self.peek() {
                    let word = word.clone();
                    self.index += 1;
                    let segments: Vec<String> = word[1..].split('.').map(str::to_owned).collect();
                    if segments.iter().any(String::is_empty) {
                        return Err(error_at(
                            self.position,
                            format!("invalid postfix path `{word}`"),
                        ));
                    }
                    operand = Operand::Postfix {
                        base: Box::new(operand),
                        segments,
                        display: format!("(...){word}"),
                    };
                }
                Ok(operand)
            }
            Lexeme::RightParen => Err(error_at(self.position, "unexpected `)`")),
            Lexeme::Pipe => Err(error_at(self.position, "unexpected `|`")),
            Lexeme::Postfix(word) => Err(error_at(
                self.position,
                format!("postfix path `{word}` must follow a parenthesized pipeline"),
            )),
            Lexeme::Word(word) => self.word_operand(&word, allow_function),
        }
    }

    fn word_operand(&self, word: &str, allow_function: bool) -> Result<Operand, LangError> {
        match word {
            "true" => return Ok(Operand::Literal(Json::Bool(true))),
            "false" => return Ok(Operand::Literal(Json::Bool(false))),
            "null" | "nil" => return Ok(Operand::Literal(Json::Null)),
            _ => {}
        }
        if let Some(number) = parse_number(word) {
            return Ok(Operand::Literal(number));
        }
        if is_function(word) {
            if allow_function {
                return Ok(Operand::Function(word.to_owned()));
            }
            return Err(error_at(
                self.position,
                format!("function `{word}` cannot be used as a value; wrap it in parentheses"),
            ));
        }
        parse_path(word).map_err(|message| error_at(self.position, message))
    }
}

fn bare_function_name(operand: &Operand) -> Option<String> {
    if let Operand::Path {
        from_root: true,
        segments,
        display,
    } = operand
        && segments.len() == 1
        && !display.contains('.')
        && !display.starts_with('$')
    {
        return Some(display.clone());
    }
    None
}

fn parse_number(word: &str) -> Option<Json> {
    let bytes = word.as_bytes();
    let mut index = 0usize;
    if matches!(bytes.first(), Some(b'-' | b'+')) {
        index += 1;
    }
    let starts_with_digit = bytes.get(index).is_some_and(u8::is_ascii_digit);
    let starts_with_fraction =
        bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit);
    if !starts_with_digit && !starts_with_fraction {
        return None;
    }
    if let Ok(value) = word.parse::<i64>() {
        return Some(Json::from(value));
    }
    if let Ok(value) = word.parse::<u64>() {
        return Some(Json::from(value));
    }
    if let Ok(Json::Number(value)) = serde_json::from_str::<Json>(word) {
        if integer_parts(&value.to_string()).is_some() {
            return Some(Json::Number(value));
        }
        if let Some(float) = value.as_f64()
            && let Some(float) = serde_json::Number::from_f64(float)
        {
            match (
                exact_decimal_integer(&value.to_string()),
                exact_decimal_integer(&float.to_string()),
            ) {
                (Some(exact), Some(rounded))
                    if rounded.negative == exact.negative && rounded.digits == exact.digits =>
                {
                    return Some(Json::Number(float));
                }
                (None, None) => return Some(Json::Number(float)),
                _ => {}
            }
        }
        return Some(Json::Number(value));
    }
    word.parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(Json::Number)
}

fn parse_path(word: &str) -> Result<Operand, String> {
    if word == "$" {
        return Ok(Operand::Path {
            from_root: true,
            segments: Vec::new(),
            display: word.to_owned(),
        });
    }
    if word == "." {
        return Ok(Operand::Path {
            from_root: false,
            segments: Vec::new(),
            display: word.to_owned(),
        });
    }
    if let Some(rest) = word.strip_prefix("$.") {
        return path_operand(rest, true, word);
    }
    if let Some(rest) = word.strip_prefix('.') {
        return path_operand(rest, false, word);
    }
    if word.starts_with('$') {
        return Err(format!(
            "unsupported template variable `{word}`; only `$` (the root) is available"
        ));
    }
    path_operand(word, true, word)
}

fn path_operand(rest: &str, from_root: bool, display: &str) -> Result<Operand, String> {
    if rest.is_empty() {
        return Err(format!("invalid template path `{display}`"));
    }
    let segments: Vec<String> = rest.split('.').map(str::to_owned).collect();
    if segments.iter().any(String::is_empty) {
        return Err(format!("invalid template path `{display}`"));
    }
    Ok(Operand::Path {
        from_root,
        segments,
        display: display.to_owned(),
    })
}

fn lex_expression(source: &str, position: Position) -> Result<Vec<Lexeme>, LangError> {
    let characters: Vec<char> = source.chars().collect();
    let mut lexemes = Vec::new();
    let mut index = 0usize;
    while let Some(&character) = characters.get(index) {
        if is_go_space(character) {
            index += 1;
            continue;
        }
        match character {
            '(' => {
                lexemes.push(Lexeme::LeftParen);
                index += 1;
            }
            ')' => {
                lexemes.push(Lexeme::RightParen);
                index += 1;
            }
            '|' => {
                lexemes.push(Lexeme::Pipe);
                index += 1;
            }
            '"' => {
                let (value, next) = parse_quoted(&characters, index, position)?;
                lexemes.push(Lexeme::Quoted(value));
                index = next;
            }
            '`' => {
                let (value, next) = parse_raw_quoted(&characters, index, position)?;
                lexemes.push(Lexeme::Quoted(value));
                index = next;
            }
            '\'' => {
                return Err(error_at(
                    position,
                    "rune literals are not supported in templates; use a quoted string",
                ));
            }
            _ => {
                let start = index;
                while let Some(&current) = characters.get(index) {
                    if is_go_space(current) || matches!(current, '(' | ')' | '|' | '"' | '`' | '\'')
                    {
                        break;
                    }
                    index += 1;
                }
                if start == index {
                    return Err(error_at(
                        position,
                        format!("unexpected character `{character}` in template action"),
                    ));
                }
                let word: String = characters[start..index].iter().collect();
                if character == '.' && start > 0 && characters[start - 1] == ')' {
                    lexemes.push(Lexeme::Postfix(word));
                } else {
                    lexemes.push(Lexeme::Word(word));
                }
            }
        }
    }
    Ok(lexemes)
}

fn parse_quoted(
    characters: &[char],
    start: usize,
    position: Position,
) -> Result<(String, usize), LangError> {
    let mut value = String::new();
    let mut index = start + 1;
    while let Some(&character) = characters.get(index) {
        match character {
            '"' => return Ok((value, index + 1)),
            '\\' => {
                let (escaped, next) = parse_escape(characters, index, position)?;
                value.push(escaped);
                index = next;
            }
            _ => {
                value.push(character);
                index += 1;
            }
        }
    }
    Err(error_at(position, "unterminated string in template action"))
}

fn parse_raw_quoted(
    characters: &[char],
    start: usize,
    position: Position,
) -> Result<(String, usize), LangError> {
    let mut value = String::new();
    let mut index = start + 1;
    while let Some(&character) = characters.get(index) {
        if character == '`' {
            return Ok((value, index + 1));
        }
        value.push(character);
        index += 1;
    }
    Err(error_at(
        position,
        "unterminated raw string in template action",
    ))
}

fn parse_escape(
    characters: &[char],
    backslash: usize,
    position: Position,
) -> Result<(char, usize), LangError> {
    let Some(&escape) = characters.get(backslash + 1) else {
        return Err(error_at(
            position,
            "unterminated escape sequence in template string",
        ));
    };
    let simple = match escape {
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\u{b}'),
        '\\' => Some('\\'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '0' => Some('\0'),
        _ => None,
    };
    if let Some(value) = simple {
        return Ok((value, backslash + 2));
    }
    let digits = match escape {
        'x' => 2,
        'u' => 4,
        'U' => 8,
        _ => {
            return Err(error_at(
                position,
                format!("unknown escape sequence `\\{escape}` in template string"),
            ));
        }
    };
    let start = backslash + 2;
    let end = start + digits;
    if end > characters.len() {
        return Err(error_at(
            position,
            "unterminated escape sequence in template string",
        ));
    }
    let hex: String = characters[start..end].iter().collect();
    let code = u32::from_str_radix(&hex, 16).map_err(|_| {
        error_at(
            position,
            format!("invalid hexadecimal escape `\\{escape}{hex}` in template string"),
        )
    })?;
    let character = char::from_u32(code).ok_or_else(|| {
        error_at(
            position,
            format!("invalid Unicode escape `\\{escape}{hex}` in template string"),
        )
    })?;
    Ok((character, end))
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

fn lookup_segments<'a>(
    context: &'a Json,
    segments: &[String],
    display: &str,
) -> Result<&'a Json, LangError> {
    let mut current = context;
    for segment in segments {
        current = match current {
            Json::Object(map) => map.get(segment.as_str()).ok_or_else(|| {
                LangError::Template(format!("unknown template reference `{display}`"))
            })?,
            Json::Array(list) => {
                let index = segment.parse::<usize>().map_err(|_| {
                    LangError::Template(format!(
                        "`{segment}` is not a valid array index in `{display}`"
                    ))
                })?;
                list.get(index).ok_or_else(|| {
                    LangError::Template(format!("array index out of range in `{display}`"))
                })?
            }
            _ => {
                return Err(LangError::Template(format!(
                    "cannot descend into `{segment}` in `{display}`"
                )));
            }
        };
    }
    Ok(current)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

struct Engine<'a> {
    root: &'a Json,
}

impl Engine<'_> {
    fn render_nodes(
        &self,
        nodes: &[Node],
        dot: &Json,
        output: &mut String,
    ) -> Result<(), LangError> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
            self.render_nodes_impl(nodes, dot, output)
        })
    }

    fn render_nodes_impl(
        &self,
        nodes: &[Node],
        dot: &Json,
        output: &mut String,
    ) -> Result<(), LangError> {
        for node in nodes {
            match node {
                Node::Text(text) => output.push_str(text),
                Node::Output(pipeline) => {
                    let value = self.eval_pipeline(pipeline, dot)?;
                    output.push_str(&stringify(&value));
                }
                Node::LegacyPath { path, position } => {
                    let segments: Vec<String> = path.split('.').map(str::to_owned).collect();
                    let value = lookup_segments(self.root, &segments, path)
                        .map_err(|error| locate_error(error, *position))?;
                    output.push_str(&stringify(value));
                }
                Node::If {
                    condition,
                    body,
                    otherwise,
                } => {
                    let value = self.eval_pipeline(condition, dot)?;
                    if truthy(&value) {
                        self.render_nodes(body, dot, output)?;
                    } else {
                        self.render_nodes(otherwise, dot, output)?;
                    }
                }
                Node::Range {
                    expression,
                    body,
                    otherwise,
                    position,
                } => {
                    let value = self.eval_pipeline(expression, dot)?;
                    match &value {
                        Json::Array(items) => {
                            if items.is_empty() {
                                self.render_nodes(otherwise, dot, output)?;
                            } else {
                                for item in items {
                                    self.render_nodes(body, item, output)?;
                                }
                            }
                        }
                        Json::Object(map) => {
                            if map.is_empty() {
                                self.render_nodes(otherwise, dot, output)?;
                            } else {
                                let mut keys: Vec<&String> = map.keys().collect();
                                keys.sort();
                                for key in keys {
                                    if let Some(item) = map.get(key.as_str()) {
                                        self.render_nodes(body, item, output)?;
                                    }
                                }
                            }
                        }
                        Json::Null => self.render_nodes(otherwise, dot, output)?,
                        other => {
                            return Err(error_at(
                                *position,
                                format!("cannot range over {} value", type_name(other)),
                            ));
                        }
                    }
                }
                Node::With {
                    expression,
                    body,
                    otherwise,
                } => {
                    let value = self.eval_pipeline(expression, dot)?;
                    if truthy(&value) {
                        self.render_nodes(body, &value, output)?;
                    } else {
                        self.render_nodes(otherwise, dot, output)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn eval_pipeline(&self, pipeline: &Pipeline, dot: &Json) -> Result<Json, LangError> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
            self.eval_pipeline_impl(pipeline, dot)
        })
    }

    fn eval_pipeline_impl(&self, pipeline: &Pipeline, dot: &Json) -> Result<Json, LangError> {
        let mut value: Option<Json> = None;
        for command in &pipeline.commands {
            value = Some(self.eval_command(command, dot, value, pipeline.position)?);
        }
        value.ok_or_else(|| error_at(pipeline.position, "empty template pipeline"))
    }

    fn eval_command(
        &self,
        command: &Command,
        dot: &Json,
        piped: Option<Json>,
        position: Position,
    ) -> Result<Json, LangError> {
        if let Operand::Function(name) = &command.callee {
            if name == "and" || name == "or" {
                return self.eval_short_circuit(name, command, dot, piped, position);
            }
            let extra = if piped.is_some() { 1 } else { 0 };
            let mut arguments = Vec::with_capacity(command.arguments.len() + extra);
            for argument in &command.arguments {
                arguments.push(self.eval_operand(argument, dot, position)?);
            }
            if let Some(value) = piped {
                arguments.push(value);
            }
            return call_function(name, &arguments, position);
        }
        if piped.is_some() {
            return Err(error_at(
                position,
                "cannot pipe a value into a template value; pipe into a function instead",
            ));
        }
        self.eval_operand(&command.callee, dot, position)
    }

    fn eval_short_circuit(
        &self,
        name: &str,
        command: &Command,
        dot: &Json,
        piped: Option<Json>,
        position: Position,
    ) -> Result<Json, LangError> {
        let mut last: Option<Json> = None;
        for argument in &command.arguments {
            let value = self.eval_operand(argument, dot, position)?;
            let continue_evaluating = if name == "and" {
                truthy(&value)
            } else {
                !truthy(&value)
            };
            if !continue_evaluating {
                return Ok(value);
            }
            last = Some(value);
        }
        if let Some(value) = piped {
            let continue_evaluating = if name == "and" {
                truthy(&value)
            } else {
                !truthy(&value)
            };
            if !continue_evaluating {
                return Ok(value);
            }
            last = Some(value);
        }
        last.ok_or_else(|| {
            error_at(
                position,
                format!("function `{name}` expects at least 1 argument(s), got 0"),
            )
        })
    }

    fn eval_operand(
        &self,
        operand: &Operand,
        dot: &Json,
        position: Position,
    ) -> Result<Json, LangError> {
        match operand {
            Operand::Literal(value) => Ok(value.clone()),
            Operand::Group(pipeline) => self.eval_pipeline(pipeline, dot),
            Operand::Postfix {
                base,
                segments,
                display,
            } => {
                let value = self.eval_operand(base, dot, position)?;
                lookup_segments(&value, segments, display)
                    .cloned()
                    .map_err(|error| locate_error(error, position))
            }
            Operand::Function(name) => Err(error_at(
                position,
                format!("function `{name}` used as a value"),
            )),
            Operand::Path {
                from_root,
                segments,
                display,
            } => {
                let base = if *from_root { self.root } else { dot };
                lookup_segments(base, segments, display)
                    .cloned()
                    .map_err(|error| locate_error(error, position))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

fn call_function(name: &str, arguments: &[Json], position: Position) -> Result<Json, LangError> {
    match name {
        "eq" => {
            require_minimum(name, arguments, 2, position)?;
            let first = &arguments[0];
            let equal = arguments[1..].iter().any(|other| json_equal(first, other));
            Ok(Json::Bool(equal))
        }
        "ne" => {
            require_exact(name, arguments, 2, position)?;
            Ok(Json::Bool(!json_equal(&arguments[0], &arguments[1])))
        }
        "lt" | "le" | "gt" | "ge" => {
            require_exact(name, arguments, 2, position)?;
            let ordering = compare(&arguments[0], &arguments[1]).ok_or_else(|| {
                error_at(
                    position,
                    format!(
                        "cannot compare {} and {} with `{name}`",
                        type_name(&arguments[0]),
                        type_name(&arguments[1])
                    ),
                )
            })?;
            let result = match name {
                "lt" => ordering == Ordering::Less,
                "le" => ordering != Ordering::Greater,
                "gt" => ordering == Ordering::Greater,
                _ => ordering != Ordering::Less,
            };
            Ok(Json::Bool(result))
        }
        "and" => {
            require_minimum(name, arguments, 1, position)?;
            for argument in arguments {
                if !truthy(argument) {
                    return Ok(argument.clone());
                }
            }
            Ok(arguments.last().cloned().unwrap_or(Json::Bool(true)))
        }
        "or" => {
            require_minimum(name, arguments, 1, position)?;
            for argument in arguments {
                if truthy(argument) {
                    return Ok(argument.clone());
                }
            }
            Ok(arguments.last().cloned().unwrap_or(Json::Bool(false)))
        }
        "not" => {
            require_exact(name, arguments, 1, position)?;
            Ok(Json::Bool(!truthy(&arguments[0])))
        }
        "len" => {
            require_exact(name, arguments, 1, position)?;
            let length = match &arguments[0] {
                Json::String(value) => value.len(),
                Json::Array(values) => values.len(),
                Json::Object(values) => values.len(),
                other => {
                    return Err(error_at(
                        position,
                        format!("cannot take len of {} value", type_name(other)),
                    ));
                }
            };
            Ok(Json::from(length))
        }
        "index" => {
            require_minimum(name, arguments, 1, position)?;
            let mut current = &arguments[0];
            if arguments.len() == 1 {
                return Ok(current.clone());
            }
            for index in &arguments[1..] {
                current = match current {
                    Json::Array(values) => {
                        let position_index = exact_integer_index(index)
                            .map_err(|message| error_at(position, message))?;
                        values.get(position_index).ok_or_else(|| {
                            error_at(
                                position,
                                format!("array index {position_index} out of range"),
                            )
                        })?
                    }
                    Json::Object(values) => {
                        let Json::String(key) = index else {
                            return Err(error_at(
                                position,
                                format!("object index must be a string, got {}", type_name(index)),
                            ));
                        };
                        values.get(key.as_str()).unwrap_or(&Json::Null)
                    }
                    other => {
                        return Err(error_at(
                            position,
                            format!("cannot index into {} value", type_name(other)),
                        ));
                    }
                };
            }
            Ok(current.clone())
        }
        "print" => Ok(Json::String(go_print(arguments))),
        "printf" => {
            require_minimum(name, arguments, 1, position)?;
            let Json::String(format) = &arguments[0] else {
                return Err(error_at(position, "printf requires a string format"));
            };
            go_printf(format, &arguments[1..], position).map(Json::String)
        }
        _ => Err(error_at(
            position,
            format!("unknown template function `{name}`"),
        )),
    }
}

fn require_exact(
    name: &str,
    arguments: &[Json],
    expected: usize,
    position: Position,
) -> Result<(), LangError> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(error_at(
            position,
            format!(
                "function `{name}` expects {expected} argument(s), got {}",
                arguments.len()
            ),
        ))
    }
}

fn require_minimum(
    name: &str,
    arguments: &[Json],
    minimum: usize,
    position: Position,
) -> Result<(), LangError> {
    if arguments.len() >= minimum {
        Ok(())
    } else {
        Err(error_at(
            position,
            format!(
                "function `{name}` expects at least {minimum} argument(s), got {}",
                arguments.len()
            ),
        ))
    }
}

fn json_equal(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Number(left), Json::Number(right)) => {
            if left == right {
                return true;
            }
            if let Some(ordering) = exact_integer_ordering(left, right) {
                return ordering == Ordering::Equal;
            }
            match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        _ => left == right,
    }
}

fn compare(left: &Json, right: &Json) -> Option<Ordering> {
    match (left, right) {
        (Json::Number(left), Json::Number(right)) => compare_numbers(left, right),
        (Json::String(left), Json::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_numbers(left: &serde_json::Number, right: &serde_json::Number) -> Option<Ordering> {
    if let Some(ordering) = exact_integer_ordering(left, right) {
        return Some(ordering);
    }
    left.as_f64()?.partial_cmp(&right.as_f64()?)
}

#[derive(Debug)]
struct IntegerParts {
    negative: bool,
    digits: String,
}

fn exact_integer_ordering(
    left: &serde_json::Number,
    right: &serde_json::Number,
) -> Option<Ordering> {
    let left = exact_decimal_integer(&left.to_string())?;
    let right = exact_decimal_integer(&right.to_string())?;
    Some(compare_integer_parts(&left, &right))
}

fn integer_parts(text: &str) -> Option<IntegerParts> {
    if text.contains(['.', 'e', 'E']) {
        return None;
    }
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let digits = rest.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    Some(IntegerParts {
        negative: negative && digits != "0",
        digits: digits.to_owned(),
    })
}

fn exact_integer(value: &Json) -> Option<IntegerParts> {
    let Json::Number(number) = value else {
        return None;
    };
    let text = number.to_string();
    if let Some(parts) = integer_parts(&text) {
        return Some(parts);
    }
    exact_decimal_integer(&text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecimalIntegerError {
    NotInteger,
    TooLarge,
}

fn exact_decimal_integer(text: &str) -> Option<IntegerParts> {
    exact_decimal_integer_result(text).ok()
}

fn exact_decimal_integer_result(text: &str) -> Result<IntegerParts, DecimalIntegerError> {
    let decimal = text
        .parse::<BigDecimal>()
        .map_err(|_| DecimalIntegerError::NotInteger)?;
    let (bigint, exponent) = decimal.as_bigint_and_exponent();
    if bigint == BigInt::from(0) {
        return Ok(IntegerParts {
            negative: false,
            digits: "0".to_owned(),
        });
    }
    let scale =
        usize::try_from(exponent.unsigned_abs()).map_err(|_| DecimalIntegerError::TooLarge)?;
    if exponent <= 0 && scale > MAX_TEMPLATE_INTEGER_DIGITS {
        return Err(DecimalIntegerError::TooLarge);
    }
    let digits = bigint.to_string();
    let digits = digits.trim_start_matches('-');
    if exponent <= 0 {
        let expanded_len = digits
            .len()
            .checked_add(scale)
            .ok_or(DecimalIntegerError::TooLarge)?;
        if expanded_len > MAX_TEMPLATE_INTEGER_DIGITS {
            return Err(DecimalIntegerError::TooLarge);
        }
        let power = BigInt::from(10u32)
            .pow(u32::try_from(scale).map_err(|_| DecimalIntegerError::TooLarge)?);
        integer_parts(&(bigint * power).to_string()).ok_or(DecimalIntegerError::NotInteger)
    } else {
        if digits.len() < scale {
            return Err(DecimalIntegerError::NotInteger);
        }
        let power = BigInt::from(10u32)
            .pow(u32::try_from(scale).map_err(|_| DecimalIntegerError::TooLarge)?);
        if (&bigint % &power) != BigInt::from(0) {
            return Err(DecimalIntegerError::NotInteger);
        }
        integer_parts(&(bigint / power).to_string()).ok_or(DecimalIntegerError::NotInteger)
    }
}

fn exact_integer_checked(
    value: &Json,
    verb: char,
    position: Position,
) -> Result<IntegerParts, LangError> {
    let Json::Number(number) = value else {
        return Err(error_at(
            position,
            format!(
                "printf: `%{verb}` requires an integer, got {}",
                type_name(value)
            ),
        ));
    };
    let text = number.to_string();
    if let Some(parts) = integer_parts(&text) {
        return Ok(parts);
    }
    exact_decimal_integer_result(&text).map_err(|error| match error {
        DecimalIntegerError::NotInteger => error_at(
            position,
            format!(
                "printf: `%{verb}` requires an integer, got a non-integral number"
            ),
        ),
        DecimalIntegerError::TooLarge => error_at(
            position,
            format!(
                "printf: integer expansion exceeds the maximum of {MAX_TEMPLATE_INTEGER_DIGITS} digits"
            ),
        ),
    })
}

fn exact_integer_index(value: &Json) -> Result<usize, &'static str> {
    let parts = exact_integer(value).ok_or("array index must be an integer")?;
    if parts.negative {
        return Err("array index must be non-negative");
    }
    parts
        .digits
        .parse::<usize>()
        .map_err(|_| "array index is out of range")
}

fn compare_integer_parts(left: &IntegerParts, right: &IntegerParts) -> Ordering {
    if left.negative != right.negative {
        return if left.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let ordering = match left.digits.len().cmp(&right.digits.len()) {
        Ordering::Equal => left.digits.cmp(&right.digits),
        other => other,
    };
    if left.negative {
        ordering.reverse()
    } else {
        ordering
    }
}

fn float_of(value: &Json) -> Option<f64> {
    match value {
        Json::Number(number) => number.as_f64(),
        Json::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn go_print(arguments: &[Json]) -> String {
    let mut output = String::new();
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0
            && !matches!(arguments[index - 1], Json::String(_))
            && !matches!(argument, Json::String(_))
        {
            output.push(' ');
        }
        output.push_str(&stringify(argument));
    }
    output
}

#[derive(Debug, Default, Clone, Copy)]
struct PrintfFlags {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alternate: bool,
}

#[derive(Debug)]
struct FormattedValue {
    text: String,
    sign_len: usize,
    base_len: usize,
    numeric: bool,
    zero_pad: bool,
}

fn go_printf(format: &str, arguments: &[Json], position: Position) -> Result<String, LangError> {
    let mut output = String::new();
    let mut characters = format.chars().peekable();
    let mut argument_index = 0usize;

    while let Some(character) = characters.next() {
        if character != '%' {
            output.push(character);
            continue;
        }
        if characters.peek() == Some(&'%') {
            characters.next();
            output.push('%');
            continue;
        }
        let mut flags = PrintfFlags::default();
        while let Some(&flag) = characters.peek() {
            match flag {
                '-' => flags.left = true,
                '+' => flags.plus = true,
                ' ' => flags.space = true,
                '0' => flags.zero = true,
                '#' => flags.alternate = true,
                _ => break,
            }
            characters.next();
        }
        let width_from_argument = characters.peek() == Some(&'*');
        let mut width_digits = String::new();
        if width_from_argument {
            characters.next();
        } else {
            while let Some(&digit) = characters.peek() {
                if digit.is_ascii_digit() {
                    width_digits.push(digit);
                    characters.next();
                } else {
                    break;
                }
            }
        }
        let mut precision_digits: Option<String> = None;
        let mut precision_from_argument = false;
        if characters.peek() == Some(&'.') {
            characters.next();
            if characters.peek() == Some(&'*') {
                characters.next();
                precision_from_argument = true;
            } else {
                let mut digits = String::new();
                while let Some(&digit) = characters.peek() {
                    if digit.is_ascii_digit() {
                        digits.push(digit);
                        characters.next();
                    } else {
                        break;
                    }
                }
                precision_digits = Some(digits);
            }
        }
        let Some(verb) = characters.next() else {
            return Err(error_at(position, "printf: trailing `%` in format string"));
        };

        let mut width = if width_from_argument {
            let value = next_printf_argument(arguments, &mut argument_index, position)?;
            let value = exact_integer(value)
                .ok_or_else(|| error_at(position, "printf: width argument must be an integer"))?;
            if value.negative {
                flags.left = true;
                Some(checked_printf_size(
                    integer_parts_u128(&value, "width", position)?,
                    "width",
                    position,
                )?)
            } else {
                Some(checked_printf_size(
                    integer_parts_u128(&value, "width", position)?,
                    "width",
                    position,
                )?)
            }
        } else {
            fixed_printf_size(&width_digits, "width", position)?
        };

        let precision = if precision_from_argument {
            let value = next_printf_argument(arguments, &mut argument_index, position)?;
            let value = exact_integer(value).ok_or_else(|| {
                error_at(position, "printf: precision argument must be an integer")
            })?;
            if value.negative {
                None
            } else {
                Some(checked_printf_size(
                    integer_parts_u128(&value, "precision", position)?,
                    "precision",
                    position,
                )?)
            }
        } else {
            match precision_digits {
                Some(digits) => {
                    Some(fixed_printf_size(&digits, "precision", position)?.unwrap_or(0))
                }
                None => None,
            }
        };

        let argument = next_printf_argument(arguments, &mut argument_index, position)?;
        let formatted = format_value(verb, argument, &flags, precision, position)?;
        if let Some(width) = width.take() {
            output.push_str(&apply_width(formatted, width, &flags));
        } else {
            output.push_str(&formatted.text);
        }
    }

    if argument_index < arguments.len() {
        return Err(error_at(
            position,
            format!(
                "printf: {} unused argument(s)",
                arguments.len() - argument_index
            ),
        ));
    }
    Ok(output)
}

fn fixed_printf_size(
    text: &str,
    label: &str,
    position: Position,
) -> Result<Option<usize>, LangError> {
    if text.is_empty() {
        return Ok(None);
    }
    let value = text.parse::<u128>().map_err(|_| {
        error_at(
            position,
            format!("printf: invalid {label} `{text}` in format string"),
        )
    })?;
    checked_printf_size(value, label, position).map(Some)
}

fn next_printf_argument<'a>(
    arguments: &'a [Json],
    index: &mut usize,
    position: Position,
) -> Result<&'a Json, LangError> {
    let Some(argument) = arguments.get(*index) else {
        return Err(error_at(position, "printf: not enough arguments"));
    };
    *index += 1;
    Ok(argument)
}

fn integer_parts_u128(
    parts: &IntegerParts,
    label: &str,
    position: Position,
) -> Result<u128, LangError> {
    parts.digits.parse::<u128>().map_err(|_| {
        error_at(
            position,
            format!("printf: {label} {} is too large", parts.digits),
        )
    })
}

fn checked_printf_size(value: u128, label: &str, position: Position) -> Result<usize, LangError> {
    if value > MAX_PRINTF_FIELD as u128 {
        return Err(error_at(
            position,
            format!("printf: {label} {value} exceeds the maximum of {MAX_PRINTF_FIELD}"),
        ));
    }
    usize::try_from(value)
        .map_err(|_| error_at(position, format!("printf: {label} {value} is too large")))
}

fn apply_width(value: FormattedValue, width: usize, flags: &PrintfFlags) -> String {
    let length = value.text.chars().count();
    if length >= width {
        return value.text;
    }
    let padding = width.saturating_sub(length);
    if flags.left {
        let mut text = value.text;
        text.push_str(&" ".repeat(padding));
        text
    } else if flags.zero && value.numeric && value.zero_pad {
        let sign_len = value.sign_len.min(value.text.len());
        let base_len = value.base_len.min(value.text.len() - sign_len);
        let digits_len = value.text.len().saturating_sub(sign_len + base_len);
        let zero_count = width.saturating_sub(sign_len + digits_len);
        if zero_count == 0 {
            return value.text;
        }
        let (sign, rest) = value.text.split_at(sign_len);
        let (base, digits) = rest.split_at(base_len);
        format!("{sign}{base}{}{digits}", "0".repeat(zero_count))
    } else {
        format!("{}{}", " ".repeat(padding), value.text)
    }
}

fn format_value(
    verb: char,
    argument: &Json,
    flags: &PrintfFlags,
    precision: Option<usize>,
    position: Position,
) -> Result<FormattedValue, LangError> {
    match verb {
        'v' => Ok(FormattedValue {
            text: stringify(argument),
            sign_len: 0,
            base_len: 0,
            numeric: false,
            zero_pad: false,
        }),
        's' => {
            let mut text = stringify(argument);
            if let Some(limit) = precision {
                text = text.chars().take(limit).collect();
            }
            Ok(FormattedValue {
                text,
                sign_len: 0,
                base_len: 0,
                numeric: false,
                zero_pad: false,
            })
        }
        'q' => Ok(FormattedValue {
            text: go_quote(argument, position)?,
            sign_len: 0,
            base_len: 0,
            numeric: false,
            zero_pad: false,
        }),
        't' => match argument {
            Json::Bool(value) => Ok(FormattedValue {
                text: value.to_string(),
                sign_len: 0,
                base_len: 0,
                numeric: false,
                zero_pad: false,
            }),
            other => Err(error_at(
                position,
                format!("printf: `%t` requires a boolean, got {}", type_name(other)),
            )),
        },
        'd' | 'i' | 'x' | 'X' | 'o' | 'b' => {
            let value = exact_integer_checked(argument, verb, position)?;
            format_integer(verb, &value, flags, precision, position)
        }
        'f' | 'e' | 'g' => {
            let value = float_of(argument).ok_or_else(|| {
                error_at(
                    position,
                    format!(
                        "printf: `%{verb}` requires a number, got {}",
                        type_name(argument)
                    ),
                )
            })?;
            Ok(format_float(verb, value, flags, precision))
        }
        _ => Err(error_at(
            position,
            format!("printf: unsupported verb `%{verb}`"),
        )),
    }
}

fn format_integer(
    verb: char,
    value: &IntegerParts,
    flags: &PrintfFlags,
    precision: Option<usize>,
    position: Position,
) -> Result<FormattedValue, LangError> {
    let mut digits = match verb {
        'd' | 'i' => value.digits.clone(),
        _ => {
            let magnitude = BigUint::parse_bytes(value.digits.as_bytes(), 10)
                .ok_or_else(|| error_at(position, "printf: invalid integer value"))?;
            match verb {
                'x' => magnitude.to_str_radix(16),
                'X' => magnitude.to_str_radix(16).to_uppercase(),
                'o' => magnitude.to_str_radix(8),
                _ => magnitude.to_str_radix(2),
            }
        }
    };
    if let Some(precision) = precision {
        if precision == 0 && value.digits == "0" {
            digits.clear();
        } else if digits.len() < precision {
            digits = format!("{}{digits}", "0".repeat(precision - digits.len()));
        }
    }
    let mut sign = String::new();
    if value.negative {
        sign.push('-');
    } else if flags.plus {
        sign.push('+');
    } else if flags.space {
        sign.push(' ');
    }
    let mut base = String::new();
    if flags.alternate && !digits.is_empty() {
        match verb {
            'x' => base.push_str("0x"),
            'X' => base.push_str("0X"),
            'o' if !digits.starts_with('0') => digits.insert(0, '0'),
            'b' => base.push_str("0b"),
            _ => {}
        }
    }
    Ok(FormattedValue {
        text: format!("{sign}{base}{digits}"),
        sign_len: sign.len(),
        base_len: base.len(),
        numeric: true,
        zero_pad: precision.is_none(),
    })
}

fn format_float(
    verb: char,
    value: f64,
    flags: &PrintfFlags,
    precision: Option<usize>,
) -> FormattedValue {
    let negative = value.is_sign_negative();
    let magnitude = value.abs();
    let mut sign = String::new();
    if negative {
        sign.push('-');
    } else if flags.plus {
        sign.push('+');
    } else if flags.space {
        sign.push(' ');
    }
    let body = match verb {
        'f' => {
            let places = precision.unwrap_or(6);
            let mut body = format!("{magnitude:.places$}");
            if flags.alternate && places == 0 {
                body.push('.');
            }
            body
        }
        'e' => {
            let places = precision.unwrap_or(6);
            let mut body = go_exponent(magnitude, places);
            if flags.alternate {
                body = ensure_decimal_point_in_exponent(body);
            }
            body
        }
        _ => go_general(magnitude, precision, flags.alternate),
    };
    FormattedValue {
        text: format!("{sign}{body}"),
        sign_len: sign.len(),
        base_len: 0,
        numeric: true,
        zero_pad: true,
    }
}

fn ensure_decimal_point_in_exponent(text: String) -> String {
    match text.split_once('e') {
        Some((mantissa, exponent)) if !mantissa.contains('.') => {
            format!("{mantissa}.e{exponent}")
        }
        _ => text,
    }
}

fn go_exponent(magnitude: f64, precision: usize) -> String {
    let formatted = format!("{magnitude:.precision$e}");
    match formatted.split_once('e') {
        Some((mantissa, exponent)) => {
            let exponent = exponent.parse::<i32>().unwrap_or(0);
            format!(
                "{mantissa}e{}{:02}",
                if exponent < 0 { '-' } else { '+' },
                exponent.unsigned_abs()
            )
        }
        None => formatted,
    }
}

fn go_general(magnitude: f64, precision: Option<usize>, alternate: bool) -> String {
    if magnitude == 0.0 {
        return match (precision, alternate) {
            (Some(places), true) => format!("0.{}", "0".repeat(places.saturating_sub(1))),
            (Some(_), false) | (None, false) => "0".to_owned(),
            (None, true) => "0.00000".to_owned(),
        };
    }
    match precision {
        Some(places) => go_general_explicit(magnitude, places, alternate),
        None if alternate => apply_sharp_default(&shortest_general(magnitude)),
        None => shortest_general(magnitude),
    }
}

fn shortest_general(magnitude: f64) -> String {
    let (mantissa, exponent) = shortest_exponent(magnitude);
    if !(-4..6).contains(&exponent) {
        format!(
            "{mantissa}e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.unsigned_abs()
        )
    } else {
        format!("{magnitude}")
    }
}

fn apply_sharp_default(text: &str) -> String {
    let (mantissa, exponent) = match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (text, None),
    };
    let mut remaining = 6i32;
    let mut saw_nonzero = false;
    let mut has_decimal_point = false;
    for character in mantissa.chars() {
        match character {
            '.' => has_decimal_point = true,
            '0'..='9' => {
                if character != '0' {
                    saw_nonzero = true;
                }
                if saw_nonzero {
                    remaining -= 1;
                }
            }
            _ => {}
        }
    }
    let mut output = mantissa.to_owned();
    if !has_decimal_point {
        if mantissa == "0" {
            remaining -= 1;
        }
        output.push('.');
    }
    if remaining > 0 {
        output.push_str(&"0".repeat(remaining as usize));
    }
    if let Some(exponent) = exponent {
        output.push('e');
        output.push_str(exponent);
    }
    output
}

fn shortest_exponent(magnitude: f64) -> (String, i32) {
    let text = format!("{magnitude:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa.to_owned(), exponent.parse::<i32>().unwrap_or(0)),
        None => (text, 0),
    }
}

fn go_general_explicit(magnitude: f64, precision: usize, alternate: bool) -> String {
    let effective = if precision == 0 { 1 } else { precision };
    let places = effective.saturating_sub(1);
    let scientific = format!("{magnitude:.places$e}");
    let exponent = scientific
        .split_once('e')
        .and_then(|(_, exponent)| exponent.parse::<i32>().ok())
        .unwrap_or(0);
    if exponent < -4 || exponent >= effective as i32 {
        let mut output = go_exponent(magnitude, places);
        if alternate {
            output = ensure_decimal_point_in_exponent(output);
        } else {
            output = trim_exponent_zeros(&output);
        }
        output
    } else {
        let fractional = (effective as i32 - exponent - 1).max(0) as usize;
        let mut output = format!("{magnitude:.fractional$}");
        if alternate {
            if !output.contains('.') {
                output.push('.');
            }
        } else {
            output = trim_decimal_zeros(&output);
        }
        output
    }
}

fn trim_exponent_zeros(text: &str) -> String {
    match text.split_once('e') {
        Some((mantissa, exponent)) => format!("{}e{exponent}", trim_decimal_zeros(mantissa)),
        None => text.to_owned(),
    }
}

fn trim_decimal_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_owned();
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn go_quote(value: &Json, position: Position) -> Result<String, LangError> {
    match value {
        Json::String(text) => Ok(quote_go(text, '"')),
        Json::Number(_) => {
            let parts = exact_integer_checked(value, 'q', position)?;
            let character = if parts.negative {
                '\u{fffd}'
            } else {
                parts
                    .digits
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\u{fffd}')
            };
            Ok(quote_go(&character.to_string(), '\''))
        }
        other => Err(error_at(
            position,
            format!("printf: `%q` does not support {} values", type_name(other)),
        )),
    }
}

fn quote_go(value: &str, quote: char) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push(quote);
    for character in value.chars() {
        if character == quote {
            output.push('\\');
            output.push(quote);
            continue;
        }
        match character {
            '\\' => output.push_str("\\\\"),
            '\u{7}' => output.push_str("\\a"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{b}' => output.push_str("\\v"),
            character if needs_go_escape(character) => {
                let code = character as u32;
                if code < 0x20 || code == 0x7f {
                    output.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    output.push_str(&format!("\\u{code:04x}"));
                } else {
                    output.push_str(&format!("\\U{code:08x}"));
                }
            }
            character => output.push(character),
        }
    }
    output.push(quote);
    output
}

fn needs_go_escape(character: char) -> bool {
    if character == ' ' {
        return false;
    }
    !matches!(
        get_general_category(character),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
            | GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Number(number) => match number.as_f64() {
            Some(value) => value != 0.0,
            None => true,
        },
        Json::String(value) => !value.is_empty(),
        Json::Array(values) => !values.is_empty(),
        Json::Object(values) => !values.is_empty(),
    }
}

fn type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

fn stringify(value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::String(value) => value.clone(),
        Json::Bool(value) => value.to_string(),
        Json::Number(value) => value.to_string(),
        Json::Array(_) | Json::Object(_) => value.to_string(),
    }
}
