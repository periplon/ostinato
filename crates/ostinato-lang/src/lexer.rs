use std::str::FromStr;

use bigdecimal::BigDecimal;

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
    Float(BigDecimal),
    Str(String),
    Bytes(Vec<LexBytePart>),
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
        c.is_alphabetic() || c == '_' || c == '#'
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
                    let raw = self.read_bytes(line, col)?;
                    out.push(Token {
                        tok: Tok::Bytes(raw),
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
                    if self.peek() == Some('#') {
                        ident.push('#');
                        self.bump();
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
        let value: i64 = text
            .parse()
            .map_err(|_| LangError::parse(line, col, format!("invalid integer `{text}`")))?;
        Ok(Tok::Int(value))
    }

    fn read_bytes(&mut self, line: u32, col: u32) -> Result<Vec<LexBytePart>, LangError> {
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
                        if current == ' ' || current == '\t' {
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
                        let content = dedent_bytes_multiline(&raw, &indent, line, col)?;
                        return decode_bytes_parts(&content, line, col);
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
        decode_bytes_parts(&raw, line, col)
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
        let mut quote: Option<char> = None;
        while let Some(current) = self.peek() {
            self.bump();
            out.push(current);
            if let Some(open) = quote {
                if current == '\\' {
                    if let Some(next) = self.bump() {
                        out.push(next);
                    }
                } else if current == open {
                    quote = None;
                }
                continue;
            }
            match current {
                '"' | '\'' => quote = Some(current),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(LangError::parse(line, col, "unterminated interpolation"))
    }

    fn read_string(&mut self, line: u32, col: u32) -> Result<String, LangError> {
        self.bump(); // opening quote
        let multiline = self.peek() == Some('"') && self.peek_n(1) == Some('"');
        if multiline {
            self.bump();
            self.bump();
            let mut raw = String::new();
            loop {
                match self.peek() {
                    Some('"') if self.peek_n(1) == Some('"') && self.peek_n(2) == Some('"') => {
                        self.bump();
                        self.bump();
                        self.bump();
                        return Ok(dedent_multiline(&raw));
                    }
                    Some(c) => {
                        if c == '\\' && self.peek_n(1) == Some('\\') && self.peek_n(2) == Some('"')
                        {
                            raw.push('"');
                            self.bump();
                            self.bump();
                            self.bump();
                        } else {
                            raw.push(c);
                            self.bump();
                        }
                    }
                    None => {
                        return Err(LangError::parse(line, col, "unterminated multiline string"));
                    }
                }
            }
        }

        let mut raw = String::new();
        let mut interpolation_depth = 0usize;
        let mut in_inner_string = false;
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(
                    line,
                    col,
                    if interpolation_depth > 0 {
                        "unterminated interpolation"
                    } else {
                        "unterminated string"
                    },
                ));
            };

            if interpolation_depth > 0 {
                if in_inner_string {
                    match current {
                        '\\' => {
                            raw.push(current);
                            self.bump();
                            if let Some(escaped) = self.bump() {
                                raw.push(escaped);
                            } else {
                                return Err(LangError::parse(
                                    line,
                                    col,
                                    "unterminated escape sequence",
                                ));
                            }
                        }
                        '"' => {
                            raw.push(current);
                            self.bump();
                            in_inner_string = false;
                        }
                        _ => {
                            raw.push(current);
                            self.bump();
                        }
                    }
                } else {
                    match current {
                        '"' => {
                            raw.push(current);
                            self.bump();
                            in_inner_string = true;
                        }
                        '(' => {
                            interpolation_depth += 1;
                            raw.push(current);
                            self.bump();
                        }
                        ')' => {
                            interpolation_depth -= 1;
                            raw.push(current);
                            self.bump();
                        }
                        _ => {
                            raw.push(current);
                            self.bump();
                        }
                    }
                }
                continue;
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
                            interpolation_depth = 1;
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
fn split_multiline_parts(source: &str) -> Vec<MultilinePart> {
    let chars: Vec<char> = source.chars().collect();
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'(') {
            if !text.is_empty() {
                parts.push(MultilinePart::Text(std::mem::take(&mut text)));
            }
            let start = index;
            index += 2;
            let mut depth = 1usize;
            let mut quote: Option<char> = None;
            while index < chars.len() && depth > 0 {
                let current = chars[index];
                if let Some(open) = quote {
                    if current == '\\' {
                        index += 1;
                    } else if current == open {
                        quote = None;
                    }
                } else {
                    match current {
                        '"' | '\'' => quote = Some(current),
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                index += 1;
            }
            parts.push(MultilinePart::Interp(chars[start..index].iter().collect()));
            continue;
        }
        text.push(chars[index]);
        index += 1;
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(MultilinePart::Text(text));
    }
    parts
}

fn dedent_bytes_multiline(
    raw: &str,
    closing_indent: &str,
    line: u32,
    col: u32,
) -> Result<String, LangError> {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    let parts = split_multiline_parts(body);
    let whitespace_error = || {
        LangError::parse(
            line,
            col,
            "non-matching whitespace for multiline bytes literal",
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
            let mut depth = 1usize;
            let mut end = start;
            let mut quote: Option<char> = None;
            while end < chars.len() {
                let current = chars[end];
                if let Some(open) = quote {
                    if current == '\\' {
                        end += 1;
                    } else if current == open {
                        quote = None;
                    }
                } else {
                    match current {
                        '"' | '\'' => quote = Some(current),
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end += 1;
            }
            if depth != 0 {
                return Err(LangError::parse(line, col, "unterminated interpolation"));
            }
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

fn dedent_multiline(raw: &str) -> String {
    let body = raw.strip_prefix('\n').unwrap_or(raw);
    let mut indent: Option<usize> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let spaces = line.len() - trimmed.len();
        indent = Some(indent.map_or(spaces, |current| current.min(spaces)));
    }
    let indent = indent.unwrap_or(0);
    let mut out = String::new();
    for (offset, line) in body.lines().enumerate() {
        if offset > 0 {
            out.push('\n');
        }
        if line.len() >= indent {
            out.push_str(&line[indent..]);
        } else {
            out.push_str(line.trim_start());
        }
    }
    out
}
