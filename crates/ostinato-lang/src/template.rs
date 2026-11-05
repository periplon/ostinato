use std::cmp::Ordering;

use serde_json::Value as Json;

use crate::error::LangError;

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

fn position_at(source: &str, offset: usize) -> Position {
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = match before.rfind('\n') {
        Some(index) => before[index + 1..].chars().count() + 1,
        None => before.chars().count() + 1,
    };
    Position { line, column }
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

    while cursor < template.len() {
        let Some(relative) = template[cursor..].find("{{") else {
            push_text(
                &mut tokens,
                &template[cursor..],
                cursor,
                &mut trim_next,
                template,
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
                template,
            );
        }
        let position = position_at(template, start);
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
    template: &str,
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
        position: position_at(template, offset),
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
    let mut index = 0usize;
    let (nodes, stop) = parse_block(tokens, &mut index)?;
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

fn parse_block(tokens: &[Token], index: &mut usize) -> Result<(Vec<Node>, Stop), LangError> {
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
                        nodes.push(parse_if(rest, tokens, index, token.position)?);
                    }
                    "range" => {
                        *index += 1;
                        nodes.push(parse_range(rest, tokens, index, token.position)?);
                    }
                    "with" => {
                        *index += 1;
                        nodes.push(parse_with(rest, tokens, index, token.position)?);
                    }
                    _ => {
                        *index += 1;
                        nodes.push(Node::Output(parse_pipeline(content, token.position)?));
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
) -> Result<Node, LangError> {
    if condition.is_empty() {
        return Err(error_at(position, "`if` requires a pipeline"));
    }
    let condition = parse_pipeline(condition, position)?;
    let (body, stop) = parse_block(tokens, index)?;
    let otherwise = match stop {
        Stop::Eof => return Err(error_at(position, "missing `{{end}}` for `if`")),
        Stop::End(_) => Vec::new(),
        Stop::Else(rest, else_position) => {
            let rest = rest.trim();
            if rest.is_empty() {
                let (otherwise, stop) = parse_block(tokens, index)?;
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
                vec![parse_if(nested_condition, tokens, index, else_position)?]
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
) -> Result<Node, LangError> {
    if expression.is_empty() {
        return Err(error_at(position, "`range` requires a pipeline"));
    }
    let expression = parse_pipeline(expression, position)?;
    let (body, stop) = parse_block(tokens, index)?;
    let otherwise = parse_control_else(stop, tokens, index, position, "range")?;
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
) -> Result<Node, LangError> {
    if expression.is_empty() {
        return Err(error_at(position, "`with` requires a pipeline"));
    }
    let expression = parse_pipeline(expression, position)?;
    let (body, stop) = parse_block(tokens, index)?;
    let otherwise = parse_control_else(stop, tokens, index, position, "with")?;
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
) -> Result<Vec<Node>, LangError> {
    match stop {
        Stop::Eof => Err(error_at(
            position,
            format!("missing `{{end}}` for `{keyword}`"),
        )),
        Stop::End(_) => Ok(Vec::new()),
        Stop::Else(rest, else_position) => {
            if !rest.trim().is_empty() {
                return Err(error_at(
                    else_position,
                    format!("`{keyword}` does not support `else if`; use a nested `if`"),
                ));
            }
            let (otherwise, stop) = parse_block(tokens, index)?;
            match stop {
                Stop::End(_) => Ok(otherwise),
                Stop::Eof => Err(error_at(
                    position,
                    format!("missing `{{end}}` for `{keyword}`"),
                )),
                Stop::Else(_, second) => Err(error_at(
                    second,
                    format!("unexpected second `{{else}}` in `{keyword}`"),
                )),
            }
        }
    }
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
    Function(String),
}

#[derive(Debug, Clone)]
enum Lexeme {
    Word(String),
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
                let pipeline = self.parse()?;
                match self.next() {
                    Some(Lexeme::RightParen) => Ok(Operand::Group(Box::new(pipeline))),
                    _ => Err(error_at(self.position, "missing `)` in template pipeline")),
                }
            }
            Lexeme::RightParen => Err(error_at(self.position, "unexpected `)`")),
            Lexeme::Pipe => Err(error_at(self.position, "unexpected `|`")),
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
    let mut characters = word.chars();
    let first = characters.next()?;
    if !first.is_ascii_digit() && first != '-' && first != '+' {
        return None;
    }
    if (first == '-' || first == '+') && !characters.next()?.is_ascii_digit() {
        return None;
    }
    if let Ok(value) = word.parse::<i64>() {
        return Some(Json::from(value));
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
                lexemes.push(Lexeme::Word(characters[start..index].iter().collect()));
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
        for node in nodes {
            match node {
                Node::Text(text) => output.push_str(text),
                Node::Output(pipeline) => {
                    let value = self.eval_pipeline(pipeline, dot)?;
                    output.push_str(&stringify(&value));
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
                        let position_index = integer_of(index).ok_or_else(|| {
                            error_at(
                                position,
                                format!("index must be an integer, got {}", type_name(index)),
                            )
                        })?;
                        let Ok(position_index) = usize::try_from(position_index) else {
                            return Err(error_at(
                                position,
                                "array index must be a non-negative integer",
                            ));
                        };
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
        (Json::Number(left), Json::Number(right)) => left.as_f64()?.partial_cmp(&right.as_f64()?),
        (Json::String(left), Json::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn integer_of(value: &Json) -> Option<i64> {
    match value {
        Json::Number(number) => {
            if let Some(value) = number.as_i64() {
                Some(value)
            } else if let Some(value) = number.as_u64() {
                i64::try_from(value).ok()
            } else {
                number.as_f64().map(|value| value as i64)
            }
        }
        Json::Bool(value) => Some(i64::from(*value)),
        _ => None,
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
        let mut flags = String::new();
        while let Some(&flag) = characters.peek() {
            if matches!(flag, '-' | '+' | ' ' | '0' | '#') {
                flags.push(flag);
                characters.next();
            } else {
                break;
            }
        }
        let mut width = String::new();
        while let Some(&digit) = characters.peek() {
            if digit.is_ascii_digit() {
                width.push(digit);
                characters.next();
            } else {
                break;
            }
        }
        let mut precision = String::new();
        if characters.peek() == Some(&'.') {
            characters.next();
            while let Some(&digit) = characters.peek() {
                if digit.is_ascii_digit() {
                    precision.push(digit);
                    characters.next();
                } else {
                    break;
                }
            }
        }
        let Some(verb) = characters.next() else {
            return Err(error_at(position, "printf: trailing `%` in format string"));
        };
        let Some(argument) = arguments.get(argument_index) else {
            return Err(error_at(
                position,
                format!("printf: not enough arguments for `%{verb}`"),
            ));
        };
        argument_index += 1;
        let mut rendered = format_value(verb, argument, &precision, position)?;
        if let Ok(width) = width.parse::<usize>() {
            let length = rendered.chars().count();
            if length < width {
                let padding = width - length;
                if flags.contains('-') {
                    rendered.push_str(&" ".repeat(padding));
                } else if flags.contains('0')
                    && matches!(verb, 'd' | 'i' | 'f' | 'e' | 'g' | 'x' | 'X' | 'o' | 'b')
                {
                    rendered = format!("{}{rendered}", "0".repeat(padding));
                } else {
                    rendered = format!("{}{rendered}", " ".repeat(padding));
                }
            }
        }
        output.push_str(&rendered);
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

fn format_value(
    verb: char,
    argument: &Json,
    precision: &str,
    position: Position,
) -> Result<String, LangError> {
    match verb {
        'v' => Ok(stringify(argument)),
        's' => {
            let mut value = stringify(argument);
            if !precision.is_empty() {
                let limit = precision.parse::<usize>().unwrap_or(0);
                value = value.chars().take(limit).collect();
            }
            Ok(value)
        }
        'q' => Ok(format!("{:?}", stringify(argument))),
        't' => match argument {
            Json::Bool(value) => Ok(value.to_string()),
            other => Err(error_at(
                position,
                format!("printf: `%t` requires a boolean, got {}", type_name(other)),
            )),
        },
        'd' | 'i' => {
            let value = integer_of(argument).ok_or_else(|| {
                error_at(
                    position,
                    format!(
                        "printf: `%{verb}` requires a number, got {}",
                        type_name(argument)
                    ),
                )
            })?;
            Ok(value.to_string())
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
            let digits = if precision.is_empty() {
                6
            } else {
                precision.parse::<usize>().map_or(6, |value| value.min(100))
            };
            Ok(match verb {
                'f' => format!("{value:.digits$}"),
                'e' => format!("{value:.digits$e}"),
                _ => format!("{value}"),
            })
        }
        'x' | 'X' | 'o' | 'b' => {
            let value = integer_of(argument).ok_or_else(|| {
                error_at(
                    position,
                    format!(
                        "printf: `%{verb}` requires a number, got {}",
                        type_name(argument)
                    ),
                )
            })?;
            Ok(match verb {
                'x' => format!("{value:x}"),
                'X' => format!("{value:X}"),
                'o' => format!("{value:o}"),
                _ => format!("{value:b}"),
            })
        }
        _ => Err(error_at(
            position,
            format!("printf: unsupported verb `%{verb}`"),
        )),
    }
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
