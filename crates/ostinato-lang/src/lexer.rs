use crate::error::LangError;

/// Lexical tokens. Newlines are significant as field separators and are
/// normalised by the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
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
    Equals,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
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
                c if c.is_ascii_digit() => {
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
                if self.peek() == Some('=') {
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
        if self.peek() == Some('.') && self.peek_n(1).is_some_and(|c| c.is_ascii_digit()) {
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
            let value: f64 = text
                .parse()
                .map_err(|_| LangError::parse(line, col, format!("invalid float `{text}`")))?;
            return Ok(Tok::Float(value));
        }
        let value: i64 = text
            .parse()
            .map_err(|_| LangError::parse(line, col, format!("invalid integer `{text}`")))?;
        Ok(Tok::Int(value))
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
        loop {
            match self.peek() {
                Some('"') => {
                    self.bump();
                    return Ok(raw);
                }
                Some('\\') => {
                    self.bump();
                    let Some(escaped) = self.bump() else {
                        return Err(LangError::parse(line, col, "unterminated escape sequence"));
                    };
                    match escaped {
                        'n' => raw.push('\n'),
                        't' => raw.push('\t'),
                        'r' => raw.push('\r'),
                        '0' => raw.push('\0'),
                        '"' => raw.push('"'),
                        '\\' => raw.push('\\'),
                        '(' => {
                            raw.push('\\');
                            raw.push('(');
                        }
                        other => {
                            raw.push('\\');
                            raw.push(other);
                        }
                    }
                }
                Some(c) => {
                    raw.push(c);
                    self.bump();
                }
                None => {
                    return Err(LangError::parse(line, col, "unterminated string"));
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
