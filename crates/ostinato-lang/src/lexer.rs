use std::str::FromStr;

use bigdecimal::BigDecimal;
use num_bigint::BigInt;

use crate::error::LangError;

/// One piece of a lexed bytes literal.
#[derive(Debug, Clone, PartialEq)]
pub enum LexBytePart {
    Text(Vec<u8>),
    Interp(String),
}

/// Lexical tokens. Newlines are significant as field separators and are
/// normalised by the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    BigInt(BigInt),
    Float(BigDecimal),
    Str(String),
    Bytes(Vec<LexBytePart>, Option<usize>, bool),
    Package,
    Import,
    For,
    In,
    If,
    Let,
    True,
    False,
    Null,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Comma,
    Semi,
    Dot,
    Question,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Amp,
    Pipe,
    EqEq,
    Match,
    Equals,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    BangMatch,
    Ellipsis,
    At,
    Newline,
    Eof,
}

impl Tok {
    fn is_ident_start(c: char) -> bool {
        c.is_alphabetic() || c == '_' || c == '#' || c == '$'
    }

    fn is_ident_continue(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }
}

fn append_code_point(out: &mut Vec<u8>, value: u32, line: u32, col: u32) -> Result<(), LangError> {
    let character = char::from_u32(value)
        .ok_or_else(|| LangError::parse(line, col, "invalid unicode escape"))?;
    let mut buffer = [0u8; 4];
    out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    Ok(())
}

/// Token with source position.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

/// Tokenise a CUE-compatible source file.
///
/// # Errors
/// Returns a parse error with line/column for unterminated strings or
/// unexpected characters.
pub fn lex(src: &str) -> Result<Vec<Token>, LangError> {
    let mut lexer = Lexer {
        chars: src.chars().collect(),
        pos: 0,
        line: 1,
        col: 1,
    };
    lexer.run()
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn run(&mut self) -> Result<Vec<Token>, LangError> {
        let mut out = Vec::new();
        loop {
            let line = self.line;
            let col = self.col;
            let Some(ch) = self.peek() else {
                out.push(Token {
                    tok: Tok::Eof,
                    line,
                    col,
                });
                return Ok(out);
            };
            match ch {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => {
                    self.bump();
                    out.push(Token {
                        tok: Tok::Newline,
                        line,
                        col,
                    });
                }
                '/' if self.peek_n(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                '/' if self.peek_n(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            Some('*') if self.peek_n(1) == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            Some(_) => {
                                self.bump();
                            }
                            None => {
                                return Err(LangError::parse(
                                    line,
                                    col,
                                    "unterminated block comment",
                                ));
                            }
                        }
                    }
                }
                '"' => {
                    let raw = self.read_string(line, col)?;
                    out.push(Token {
                        tok: Tok::Str(raw),
                        line,
                        col,
                    });
                }
                '\'' => {
                    let (parts, units, multiline) = self.read_bytes(line, col)?;
                    out.push(Token {
                        tok: Tok::Bytes(parts, units, multiline),
                        line,
                        col,
                    });
                }
                c if c.is_ascii_digit()
                    || (c == '.' && self.peek_n(1).is_some_and(|n| n.is_ascii_digit())) =>
                {
                    out.push(Token {
                        tok: self.read_number(line, col)?,
                        line,
                        col,
                    });
                }
                c if Tok::is_ident_start(c) => {
                    let mut ident = String::new();
                    if matches!(self.peek(), Some('#') | Some('$'))
                        && let Some(prefix) = self.bump()
                    {
                        ident.push(prefix);
                    }
                    while let Some(c) = self.peek() {
                        if Tok::is_ident_continue(c) {
                            ident.push(c);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let tok = match ident.as_str() {
                        "package" => Tok::Package,
                        "import" => Tok::Import,
                        "for" => Tok::For,
                        "in" => Tok::In,
                        "if" => Tok::If,
                        "let" => Tok::Let,
                        "true" => Tok::True,
                        "false" => Tok::False,
                        "null" => Tok::Null,
                        _ => Tok::Ident(ident),
                    };
                    out.push(Token { tok, line, col });
                }
                _ => {
                    let tok = self.read_operator(line, col)?;
                    out.push(Token { tok, line, col });
                }
            }
        }
    }

    fn read_operator(&mut self, line: u32, col: u32) -> Result<Tok, LangError> {
        let first = self
            .bump()
            .ok_or_else(|| LangError::parse(line, col, "unexpected end of input"))?;
        let tok = match first {
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ':' => Tok::Colon,
            ',' => Tok::Comma,
            ';' => Tok::Semi,
            '.' => {
                if self.peek() == Some('.') && self.peek_n(1) == Some('.') {
                    self.bump();
                    self.bump();
                    Tok::Ellipsis
                } else {
                    Tok::Dot
                }
            }
            '?' => Tok::Question,
            '*' => Tok::Star,
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '%' => Tok::Percent,
            '&' => {
                if self.peek() == Some('&') {
                    self.bump();
                    Tok::AndAnd
                } else {
                    Tok::Amp
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.bump();
                    Tok::OrOr
                } else {
                    Tok::Pipe
                }
            }
            '=' => {
                if self.peek() == Some('~') {
                    self.bump();
                    Tok::Match
                } else if self.peek() == Some('=') {
                    self.bump();
                    Tok::EqEq
                } else {
                    Tok::Equals
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::NotEq
                } else if self.peek() == Some('~') {
                    self.bump();
                    Tok::BangMatch
                } else {
                    Tok::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::Le
                } else {
                    Tok::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Tok::Ge
                } else {
                    Tok::Gt
                }
            }
            '/' => Tok::Slash,
            '@' => Tok::At,
            other => {
                return Err(LangError::parse(
                    line,
                    col,
                    format!("unexpected character `{other}`"),
                ));
            }
        };
        Ok(tok)
    }

    fn read_number(&mut self, line: u32, col: u32) -> Result<Tok, LangError> {
        let mut text = String::new();
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_n(1).is_some_and(|c| c.is_ascii_digit()) {
            text.push('0');
            is_float = true;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                if c != '_' {
                    text.push(c);
                }
                self.bump();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            let after_dot = self.peek_n(1);
            let selector =
                after_dot.is_some_and(|c| Tok::is_ident_start(c) && !matches!(c, 'e' | 'E'));
            if !selector {
                is_float = true;
                text.push('.');
                self.bump();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '_' {
                        if c != '_' {
                            text.push(c);
                        }
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            text.push('e');
            self.bump();
            if matches!(self.peek(), Some('+' | '-'))
                && let Some(sign) = self.bump()
            {
                text.push(sign);
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    text.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
        }
        if is_float {
            if text.ends_with('.') {
                text.push('0');
            }
            let value = BigDecimal::from_str(&text)
                .map_err(|_| LangError::parse(line, col, format!("invalid float `{text}`")))?;
            return Ok(Tok::Float(value));
        }
        match text.parse::<i64>() {
            Ok(value) => Ok(Tok::Int(value)),
            Err(_) => text
                .parse::<BigInt>()
                .map(Tok::BigInt)
                .map_err(|_| LangError::parse(line, col, format!("invalid integer `{text}`"))),
        }
    }

    fn read_bytes(
        &mut self,
        line: u32,
        col: u32,
    ) -> Result<(Vec<LexBytePart>, Option<usize>, bool), LangError> {
        self.bump(); // opening quote
        let multiline = self.peek() == Some('\'') && self.peek_n(1) == Some('\'');
        if multiline {
            self.bump();
            self.bump();
            match self.peek() {
                Some('\n') => {
                    self.bump();
                }
                Some('\r') if self.peek_n(1) == Some('\n') => {
                    self.bump();
                    self.bump();
                }
                _ => {
                    return Err(LangError::parse(
                        line,
                        col,
                        "expected newline after multiline quote `'''`",
                    ));
                }
            }
            let mut raw = String::new();
            let mut at_line_start = true;
            loop {
                if at_line_start {
                    let mut indent = String::new();
                    while let Some(current) = self.peek() {
                        if matches!(current, ' ' | '\t' | '\r') {
                            indent.push(current);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    if self.peek() == Some('\'')
                        && self.peek_n(1) == Some('\'')
                        && self.peek_n(2) == Some('\'')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        let content = dedent_multiline_literal(&raw, &indent, line, col, "bytes")?;
                        return decode_bytes_parts(&content, line, col)
                            .map(|parts| (parts, None, true));
                    }
                    raw.push_str(&indent);
                    at_line_start = false;
                    continue;
                }
                let Some(current) = self.peek() else {
                    return Err(LangError::parse(line, col, "unterminated bytes literal"));
                };
                self.bump();
                raw.push(current);
                if current == '\\' {
                    if self.peek() == Some('(') {
                        self.bump();
                        raw.push('(');
                        self.scan_interpolation(&mut raw, line, col)?;
                    } else if let Some(escaped) = self.bump() {
                        raw.push(escaped);
                        if escaped == '\n' {
                            at_line_start = true;
                        }
                    }
                    continue;
                }
                if current == '\n' {
                    at_line_start = true;
                }
            }
        }

        let mut raw = String::new();
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated bytes literal"));
            };
            match current {
                '\'' => {
                    self.bump();
                    break;
                }
                '\n' => {
                    return Err(LangError::parse(line, col, "unterminated bytes literal"));
                }
                '\\' => {
                    self.bump();
                    let Some(escaped) = self.bump() else {
                        return Err(LangError::parse(line, col, "unterminated escape sequence"));
                    };
                    raw.push('\\');
                    raw.push(escaped);
                    if escaped == '(' {
                        self.scan_interpolation(&mut raw, line, col)?;
                    }
                }
                _ => {
                    self.bump();
                    raw.push(current);
                }
            }
        }
        let units = byte_rune_units(&raw);
        decode_bytes_parts(&raw, line, col).map(|parts| (parts, Some(units), false))
    }

    /// Consume the remainder of an interpolation started by `\(`.
    ///
    /// Parentheses and quoted strings/bytes literals inside the interpolation
    /// are respected so the enclosing bytes literal ends at the right quote.
    fn scan_interpolation(
        &mut self,
        out: &mut String,
        line: u32,
        col: u32,
    ) -> Result<(), LangError> {
        let mut depth = 1usize;
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated interpolation"));
            };
            match current {
                '(' => {
                    self.bump();
                    out.push('(');
                    depth += 1;
                }
                ')' => {
                    self.bump();
                    out.push(')');
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                '"' | '\'' => {
                    let triple = self.peek_n(1) == Some(current) && self.peek_n(2) == Some(current);
                    self.bump();
                    out.push(current);
                    if triple {
                        self.bump();
                        out.push(current);
                        self.bump();
                        out.push(current);
                        self.scan_multiline_literal(current, out, line, col)?;
                    } else {
                        self.scan_single_literal(current, out, line, col)?;
                    }
                }
                _ => {
                    self.bump();
                    out.push(current);
                }
            }
        }
    }

    fn scan_single_literal(
        &mut self,
        quote: char,
        out: &mut String,
        line: u32,
        col: u32,
    ) -> Result<(), LangError> {
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated string"));
            };
            self.bump();
            if current == '\\' {
                if self.peek() == Some('(') {
                    self.bump();
                    out.push('\\');
                    out.push('(');
                    self.scan_interpolation(out, line, col)?;
                } else if let Some(escaped) = self.bump() {
                    out.push('\\');
                    out.push(escaped);
                } else {
                    return Err(LangError::parse(line, col, "unterminated escape sequence"));
                }
                continue;
            }
            out.push(current);
            if current == quote {
                return Ok(());
            }
            if current == '\n' {
                return Err(LangError::parse(line, col, "unterminated string"));
            }
        }
    }

    fn scan_multiline_literal(
        &mut self,
        quote: char,
        out: &mut String,
        line: u32,
        col: u32,
    ) -> Result<(), LangError> {
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated multiline string"));
            };
            if self.peek() == Some(quote)
                && self.peek_n(1) == Some(quote)
                && self.peek_n(2) == Some(quote)
            {
                self.bump();
                self.bump();
                self.bump();
                out.push(quote);
                out.push(quote);
                out.push(quote);
                return Ok(());
            }
            self.bump();
            if current == '\\' {
                if self.peek() == Some('(') {
                    self.bump();
                    out.push('\\');
                    out.push('(');
                    self.scan_interpolation(out, line, col)?;
                } else if let Some(escaped) = self.bump() {
                    out.push('\\');
                    out.push(escaped);
                } else {
                    return Err(LangError::parse(line, col, "unterminated escape sequence"));
                }
                continue;
            }
            out.push(current);
        }
    }

    fn read_string(&mut self, line: u32, col: u32) -> Result<String, LangError> {
        self.bump(); // opening quote
        let multiline = self.peek() == Some('"') && self.peek_n(1) == Some('"');
        if multiline {
            self.bump();
            self.bump();
            match self.peek() {
                Some('\n') => {
                    self.bump();
                }
                Some('\r') if self.peek_n(1) == Some('\n') => {
                    self.bump();
                    self.bump();
                }
                _ => {
                    return Err(LangError::parse(
                        line,
                        col,
                        "expected newline after multiline quote `\"\"\"`",
                    ));
                }
            }
            let mut raw = String::new();
            let mut at_line_start = true;
            loop {
                if at_line_start {
                    let mut indent = String::new();
                    while let Some(current) = self.peek() {
                        if matches!(current, ' ' | '\t' | '\r') {
                            indent.push(current);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    if self.peek() == Some('"')
                        && self.peek_n(1) == Some('"')
                        && self.peek_n(2) == Some('"')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return dedent_multiline_literal(&raw, &indent, line, col, "strings");
                    }
                    raw.push_str(&indent);
                    at_line_start = false;
                    continue;
                }
                let Some(current) = self.peek() else {
                    return Err(LangError::parse(line, col, "unterminated multiline string"));
                };
                self.bump();
                raw.push(current);
                if current == '\\' {
                    if self.peek() == Some('(') {
                        self.bump();
                        raw.push('(');
                        self.scan_interpolation(&mut raw, line, col)?;
                    } else if let Some(escaped) = self.bump() {
                        raw.push(escaped);
                        if escaped == '\n' {
                            at_line_start = true;
                        }
                    }
                    continue;
                }
                if current == '\n' {
                    at_line_start = true;
                }
            }
        }

        let mut raw = String::new();
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated string"));
            };
            if current == '\n' {
                return Err(LangError::parse(line, col, "unterminated string"));
            }
            match current {
                '"' => {
                    self.bump();
                    return Ok(raw);
                }
                '\\' => {
                    self.bump();
                    let Some(escaped) = self.bump() else {
                        return Err(LangError::parse(line, col, "unterminated escape sequence"));
                    };
                    match escaped {
                        'n' => raw.push('\n'),
                        't' => raw.push('\t'),
                        'r' => raw.push('\r'),
                        '"' => raw.push('"'),
                        '\\' => {
                            raw.push('\\');
                            raw.push('\\');
                        }
                        '(' => {
                            raw.push('\\');
                            raw.push('(');
                            self.scan_interpolation(&mut raw, line, col)?;
                        }
                        other => {
                            raw.push('\\');
                            raw.push(other);
                        }
                    }
                }
                _ => {
                    raw.push(current);
                    self.bump();
                }
            }
        }
    }
}

enum MultilinePart {
    Text(String),
    Interp(String),
}

/// Split multiline bytes source into literal text and interpolation spans.
pub(crate) fn interpolation_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = start;
    while index < chars.len() {
        let current = chars[index];
        match current {
            '(' => {
                depth += 1;
                index += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            '"' | '\'' => {
                let triple = chars.get(index + 1) == Some(&current)
                    && chars.get(index + 2) == Some(&current);
                index += if triple { 3 } else { 1 };
                index = if triple {
                    skip_multiline_literal(chars, index, current)?
                } else {
                    skip_single_literal(chars, index, current)?
                };
            }
            _ => index += 1,
        }
    }
    None
}

fn skip_single_literal(chars: &[char], mut index: usize, quote: char) -> Option<usize> {
    while index < chars.len() {
        let current = chars[index];
        if current == '\\' {
            if chars.get(index + 1) == Some(&'(') {
                index = interpolation_end(chars, index + 2)? + 1;
            } else {
                index += 2;
            }
            continue;
        }
        if current == quote {
            return Some(index + 1);
        }
        if current == '\n' {
            return None;
        }
        index += 1;
    }
    None
}

fn skip_multiline_literal(chars: &[char], mut index: usize, quote: char) -> Option<usize> {
    while index < chars.len() {
        if chars[index] == quote
            && chars.get(index + 1) == Some(&quote)
            && chars.get(index + 2) == Some(&quote)
        {
            return Some(index + 3);
        }
        if chars[index] == '\\' {
            if chars.get(index + 1) == Some(&'(') {
                index = interpolation_end(chars, index + 2)? + 1;
            } else {
                index += 2;
            }
            continue;
        }
        index += 1;
    }
    None
}

fn split_multiline_parts(source: &str) -> Option<Vec<MultilinePart>> {
    let chars: Vec<char> = source.chars().collect();
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) != Some(&'(') {
            // An escape pair is literal text. Its second character must not be
            // re-examined as a possible `\\(` interpolation start: `\\\\` is a
            // literal backslash followed by a literal parenthesis.
            text.push('\\');
            index += 1;
            if let Some(escaped) = chars.get(index) {
                text.push(*escaped);
                index += 1;
            }
            continue;
        }
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'(') {
            if !text.is_empty() {
                parts.push(MultilinePart::Text(std::mem::take(&mut text)));
            }
            let start = index;
            let end = interpolation_end(&chars, index + 2)?;
            index = end + 1;
            parts.push(MultilinePart::Interp(chars[start..index].iter().collect()));
            continue;
        }
        text.push(chars[index]);
        index += 1;
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(MultilinePart::Text(text));
    }
    Some(parts)
}

fn dedent_multiline_literal(
    raw: &str,
    closing_indent: &str,
    line: u32,
    col: u32,
    kind: &str,
) -> Result<String, LangError> {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    let Some(parts) = split_multiline_parts(body) else {
        return Err(LangError::parse(line, col, "unterminated interpolation"));
    };
    let whitespace_error = || {
        LangError::parse(
            line,
            col,
            format!("non-matching whitespace for multiline {kind}"),
        )
    };

    // The closing indentation must be a prefix of the common leading
    // whitespace of every non-empty literal-text line. Interpolation source is
    // scanned separately and does not create text lines; a line that begins
    // with `\(` contributes an empty leading run.
    let mut view = String::new();
    for part in &parts {
        match part {
            MultilinePart::Text(text) => view.push_str(text),
            MultilinePart::Interp(_) => view.push('x'),
        }
    }
    let mut min_ws: Option<Vec<char>> = None;
    for current in view.split('\n') {
        if current.is_empty() {
            continue;
        }
        let ws: Vec<char> = current
            .chars()
            .take_while(|current| matches!(current, ' ' | '\t' | '\r'))
            .collect();
        min_ws = Some(match min_ws {
            None => ws,
            Some(previous) => {
                let shared = previous
                    .iter()
                    .zip(ws.iter())
                    .take_while(|(left, right)| left == right)
                    .count();
                previous[..shared].to_vec()
            }
        });
    }
    if let Some(ws) = &min_ws {
        let closing: Vec<char> = closing_indent.chars().collect();
        if ws.len() < closing.len() || ws[..closing.len()] != closing[..] {
            return Err(whitespace_error());
        }
    }

    // Unquoting: strip every CR and the closing indentation after newlines.
    // Interpolation source is copied verbatim.
    let indent: Vec<char> = closing_indent.chars().collect();
    let mut out = String::new();
    for (index, part) in parts.iter().enumerate() {
        match part {
            MultilinePart::Interp(source) => out.push_str(source),
            MultilinePart::Text(text) => {
                let chars: Vec<char> = text.chars().collect();
                let mut cursor = 0usize;
                if index == 0 && chars.starts_with(&indent) {
                    cursor = indent.len();
                }
                while cursor < chars.len() {
                    let current = chars[cursor];
                    if current == '\\' && cursor + 1 < chars.len() {
                        // Copy escape pairs opaquely so CR stripping cannot
                        // turn "\\<CR>(" into a synthetic "\\(" interpolation.
                        out.push('\\');
                        out.push(chars[cursor + 1]);
                        cursor += 2;
                        continue;
                    }
                    if current == '\r' {
                        cursor += 1;
                        continue;
                    }
                    if current == '\n' {
                        out.push('\n');
                        cursor += 1;
                        if chars[cursor..].starts_with(&indent) {
                            cursor += indent.len();
                        }
                        continue;
                    }
                    out.push(current);
                    cursor += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Count CUE rune-literal units in single-line bytes source.
///
/// Each raw character and each escape sequence counts as one unit; this is the
/// `strconv.Unquote` rule CUE uses to decide whether a bytes label may carry
/// `@tag`.
fn byte_rune_units(source: &str) -> usize {
    let chars: Vec<char> = source.chars().collect();
    let mut index = 0usize;
    let mut count = 0usize;
    while index < chars.len() {
        if chars[index] == '\\' {
            index += 1;
            if index >= chars.len() {
                break;
            }
            let escape = chars[index];
            index += 1;
            let extra = match escape {
                'x' => 2,
                'u' => 4,
                'U' => 8,
                '0'..='7' => 2,
                _ => 0,
            };
            index = (index + extra).min(chars.len());
        } else {
            index += 1;
        }
        count += 1;
    }
    count
}

fn decode_bytes_parts(source: &str, line: u32, col: u32) -> Result<Vec<LexBytePart>, LangError> {
    let chars: Vec<char> = source.chars().collect();
    let mut parts = Vec::new();
    let mut text = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'(') {
            if !text.is_empty() {
                parts.push(LexBytePart::Text(std::mem::take(&mut text)));
            }
            let start = index + 2;
            let Some(end) = interpolation_end(&chars, start) else {
                return Err(LangError::parse(line, col, "unterminated interpolation"));
            };
            let inner: String = chars[start..end].iter().collect();
            parts.push(LexBytePart::Interp(inner));
            index = end + 1;
            continue;
        }
        if chars[index] == '\\' {
            index += 1;
            let Some(escaped) = chars.get(index).copied() else {
                return Err(LangError::parse(line, col, "unterminated escape sequence"));
            };
            index += 1;
            match escaped {
                'a' => text.push(0x07),
                'b' => text.push(0x08),
                'f' => text.push(0x0c),
                'n' => text.push(b'\n'),
                'r' => text.push(b'\r'),
                't' => text.push(b'\t'),
                'v' => text.push(0x0b),
                '\\' => text.push(b'\\'),
                '/' => text.push(b'/'),
                '\'' => text.push(b'\''),
                'x' => {
                    let mut value = 0u32;
                    for _ in 0..2 {
                        let Some(digit) = chars.get(index).and_then(|c| c.to_digit(16)) else {
                            return Err(LangError::parse(line, col, "invalid hexadecimal escape"));
                        };
                        value = value * 16 + digit;
                        index += 1;
                    }
                    text.push(
                        u8::try_from(value).map_err(|_| {
                            LangError::parse(line, col, "invalid hexadecimal escape")
                        })?,
                    );
                }
                'u' => {
                    let mut value = 0u32;
                    for _ in 0..4 {
                        let Some(digit) = chars.get(index).and_then(|c| c.to_digit(16)) else {
                            return Err(LangError::parse(line, col, "invalid unicode escape"));
                        };
                        value = value * 16 + digit;
                        index += 1;
                    }
                    if (0xD800..=0xDBFF).contains(&value) {
                        if chars.get(index) != Some(&'\\') || chars.get(index + 1) != Some(&'u') {
                            return Err(LangError::parse(line, col, "invalid unicode escape"));
                        }
                        index += 2;
                        let mut low = 0u32;
                        for _ in 0..4 {
                            let Some(digit) = chars.get(index).and_then(|c| c.to_digit(16)) else {
                                return Err(LangError::parse(line, col, "invalid unicode escape"));
                            };
                            low = low * 16 + digit;
                            index += 1;
                        }
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(LangError::parse(line, col, "invalid unicode escape"));
                        }
                        let scalar = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                        append_code_point(&mut text, scalar, line, col)?;
                    } else if (0xDC00..=0xDFFF).contains(&value) {
                        return Err(LangError::parse(line, col, "invalid unicode escape"));
                    } else {
                        append_code_point(&mut text, value, line, col)?;
                    }
                }
                'U' => {
                    let mut value = 0u32;
                    for _ in 0..8 {
                        let Some(digit) = chars.get(index).and_then(|c| c.to_digit(16)) else {
                            return Err(LangError::parse(line, col, "invalid unicode escape"));
                        };
                        value = value * 16 + digit;
                        index += 1;
                    }
                    append_code_point(&mut text, value, line, col)?;
                }
                digit if digit.is_digit(8) => {
                    let mut value = digit.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        match chars.get(index).and_then(|c| c.to_digit(8)) {
                            Some(next) => {
                                value = value * 8 + next;
                                index += 1;
                            }
                            None => {
                                return Err(LangError::parse(
                                    line,
                                    col,
                                    "octal escape requires three digits",
                                ));
                            }
                        }
                    }
                    text.push(
                        u8::try_from(value).map_err(|_| {
                            LangError::parse(line, col, "octal escape out of range")
                        })?,
                    );
                }
                _ => {
                    return Err(LangError::parse(
                        line,
                        col,
                        format!("unknown escape sequence `\\{escaped}`"),
                    ));
                }
            }
            continue;
        }
        let mut buffer = [0u8; 4];
        let encoded = chars[index].encode_utf8(&mut buffer);
        text.extend_from_slice(encoded.as_bytes());
        index += 1;
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(LexBytePart::Text(text));
    }
    Ok(parts)
}
