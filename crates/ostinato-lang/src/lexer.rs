use std::str::FromStr;

use bigdecimal::BigDecimal;

use crate::error::LangError;

/// Lexical tokens. Newlines are significant as field separators and are
/// normalised by the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(BigDecimal),
    Str(String),
    Bytes(Vec<u8>),
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
            let (_, scale) = value.as_bigint_and_exponent();
            let value = if scale <= 0 {
                value.with_scale(1)
            } else {
                value
            };
            return Ok(Tok::Float(value));
        }
        let value: i64 = text
            .parse()
            .map_err(|_| LangError::parse(line, col, format!("invalid integer `{text}`")))?;
        Ok(Tok::Int(value))
    }

    fn read_bytes(&mut self, line: u32, col: u32) -> Result<Vec<u8>, LangError> {
        self.bump(); // opening quote
        let mut out = Vec::new();
        loop {
            let Some(current) = self.peek() else {
                return Err(LangError::parse(line, col, "unterminated bytes literal"));
            };
            if current == '\'' {
                self.bump();
                return Ok(out);
            }
            if current == '\\' {
                self.bump();
                let Some(escaped) = self.peek() else {
                    return Err(LangError::parse(line, col, "unterminated escape sequence"));
                };
                self.bump();
                match escaped {
                    'a' => out.push(0x07),
                    'b' => out.push(0x08),
                    'f' => out.push(0x0c),
                    'n' => out.push(b'\n'),
                    'r' => out.push(b'\r'),
                    't' => out.push(b'\t'),
                    'v' => out.push(0x0b),
                    '\\' => out.push(b'\\'),
                    '/' => out.push(b'/'),
                    '\'' => out.push(b'\''),
                    'x' => {
                        let value = self.read_hex_digits(2, line, col)?;
                        out.push(u8::try_from(value).map_err(|_| {
                            LangError::parse(line, col, "invalid hexadecimal escape")
                        })?);
                    }
                    'u' => {
                        let value = self.read_hex_digits(4, line, col)?;
                        if (0xD800..=0xDBFF).contains(&value) {
                            if self.peek() != Some('\\') || self.peek_n(1) != Some('u') {
                                return Err(LangError::parse(line, col, "invalid unicode escape"));
                            }
                            self.bump();
                            self.bump();
                            let low = self.read_hex_digits(4, line, col)?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(LangError::parse(line, col, "invalid unicode escape"));
                            }
                            let scalar = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                            append_code_point(&mut out, scalar, line, col)?;
                        } else if (0xDC00..=0xDFFF).contains(&value) {
                            return Err(LangError::parse(line, col, "invalid unicode escape"));
                        } else {
                            append_code_point(&mut out, value, line, col)?;
                        }
                    }
                    'U' => {
                        let value = self.read_hex_digits(8, line, col)?;
                        append_code_point(&mut out, value, line, col)?;
                    }
                    digit if digit.is_digit(8) => {
                        let mut value = digit.to_digit(8).unwrap_or(0);
                        for _ in 0..2 {
                            match self.peek() {
                                Some(next) if next.is_digit(8) => {
                                    value = value * 8 + next.to_digit(8).unwrap_or(0);
                                    self.bump();
                                }
                                _ => {
                                    return Err(LangError::parse(
                                        line,
                                        col,
                                        "octal escape requires three digits",
                                    ));
                                }
                            }
                        }
                        out.push(u8::try_from(value).map_err(|_| {
                            LangError::parse(line, col, "octal escape out of range")
                        })?);
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
            let text = current.encode_utf8(&mut buffer);
            out.extend_from_slice(text.as_bytes());
            self.bump();
        }
    }

    fn read_hex_digits(&mut self, count: usize, line: u32, col: u32) -> Result<u32, LangError> {
        let mut text = String::new();
        for _ in 0..count {
            match self.peek() {
                Some(value) if value.is_ascii_hexdigit() => {
                    text.push(value);
                    self.bump();
                }
                _ => {
                    return Err(LangError::parse(line, col, "invalid hexadecimal escape"));
                }
            }
        }
        u32::from_str_radix(&text, 16)
            .map_err(|_| LangError::parse(line, col, "invalid hexadecimal escape"))
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
