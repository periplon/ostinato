use crate::ast::{BinOp, Expr, Field, File, Label, Span, StrPart, StructItem, UnOp};
use crate::error::LangError;
use crate::lexer::{Tok, Token, lex};

/// Parse a CUE-compatible source file.
///
/// # Errors
/// Returns [`LangError::Parse`] with position information.
pub fn parse(src: &str) -> Result<File, LangError> {
    let tokens = lex(src)?;
    Parser { tokens, pos: 0 }.parse_file()
}

/// Parse a single expression (used for string interpolation).
///
/// # Errors
/// Returns [`LangError::Parse`] on invalid input or trailing tokens.
pub fn parse_expr_fragment(src: &str) -> Result<Expr, LangError> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.skip_newlines();
    let expr = parser.parse_expr()?;
    parser.skip_newlines();
    if !matches!(parser.peek().tok, Tok::Eof) {
        return Err(parser.error("unexpected token after expression"));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        let index = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[index]
    }

    fn peek_tok(&self) -> &Tok {
        &self.peek().tok
    }

    fn bump(&mut self) -> Token {
        let token = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn span(&self) -> Span {
        Span {
            line: self.peek().line,
            col: self.peek().col,
        }
    }

    fn error(&self, message: impl Into<String>) -> LangError {
        let span = self.span();
        LangError::parse(span.line, span.col, message)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_tok(), Tok::Newline) {
            self.bump();
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek_tok(), Tok::Newline | Tok::Comma | Tok::Semi) {
            self.bump();
        }
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek_tok() == tok {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> Result<(), LangError> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(self.error(format!("expected {what}, found {:?}", self.peek_tok())))
        }
    }

    fn expect_ident(&mut self) -> Result<String, LangError> {
        if let Tok::Ident(name) = self.peek_tok().clone() {
            self.bump();
            Ok(name)
        } else {
            Err(self.error("expected identifier"))
        }
    }

    fn parse_file(&mut self) -> Result<File, LangError> {
        self.skip_separators();
        let package = if self.eat(&Tok::Package) {
            let name = self.expect_ident()?;
            self.skip_separators();
            Some(name)
        } else {
            None
        };
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if matches!(self.peek_tok(), Tok::Eof) {
                break;
            }
            if let Some(item) = self.parse_struct_item()? {
                items.push(item);
            }
        }
        Ok(File { package, items })
    }

    fn parse_struct_body(&mut self) -> Result<Vec<StructItem>, LangError> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.eat(&Tok::RBrace) {
                break;
            }
            if matches!(self.peek_tok(), Tok::Eof) {
                return Err(self.error("unterminated struct"));
            }
            if let Some(item) = self.parse_struct_item()? {
                items.push(item);
            }
        }
        Ok(items)
    }

    fn skip_attribute(&mut self) -> Result<(), LangError> {
        self.bump(); // '@'
        if matches!(self.peek_tok(), Tok::Eof | Tok::Newline) {
            return Err(self.error("attribute name expected"));
        }
        self.bump();
        if self.eat(&Tok::LParen) {
            let mut depth = 1usize;
            while depth > 0 {
                match self.peek_tok() {
                    Tok::LParen => {
                        depth += 1;
                        self.bump();
                    }
                    Tok::RParen => {
                        depth -= 1;
                        self.bump();
                    }
                    Tok::Eof => return Err(self.error("unterminated attribute")),
                    _ => {
                        self.bump();
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_struct_item(&mut self) -> Result<Option<StructItem>, LangError> {
        if matches!(self.peek_tok(), Tok::At) {
            self.skip_attribute()?;
            return Ok(None);
        }
        let span = self.span();
        match self.peek_tok().clone() {
            Tok::For => {
                self.bump();
                let first = self.expect_ident()?;
                let (key, value) = if self.eat(&Tok::Comma) {
                    let second = self.expect_ident()?;
                    (Some(first), second)
                } else {
                    (None, first)
                };
                self.expect(&Tok::In, "`in`")?;
                let source = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&Tok::LBrace, "`{`")?;
                let body = self.parse_struct_body()?;
                Ok(Some(StructItem::For {
                    key,
                    value,
                    source,
                    body,
                    span,
                }))
            }
            Tok::If => {
                self.bump();
                let cond = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&Tok::LBrace, "`{`")?;
                let body = self.parse_struct_body()?;
                Ok(Some(StructItem::If { cond, body, span }))
            }
            Tok::Ellipsis => {
                self.bump();
                Ok(Some(StructItem::Ellipsis(span)))
            }
            _ => {
                let label = self.parse_label()?;
                let optional = self.eat(&Tok::Question);
                self.expect(&Tok::Colon, "`:`")?;
                let value = self.parse_expr()?;
                let definition = matches!(&label, Label::Name(name, _) if name.starts_with('#'));
                Ok(Some(StructItem::Field(Field {
                    label,
                    value,
                    optional,
                    definition,
                    span,
                })))
            }
        }
    }

    fn parse_label(&mut self) -> Result<Label, LangError> {
        let span = self.span();
        match self.peek_tok().clone() {
            Tok::Ident(name) => {
                self.bump();
                Ok(Label::Name(name, span))
            }
            Tok::Str(raw) => {
                self.bump();
                Ok(Label::Name(raw, span))
            }
            Tok::LBracket => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(&Tok::RBracket, "`]`")?;
                Ok(Label::Dynamic(Box::new(expr), span))
            }
            other => Err(self.error(format!("expected field label, found {other:?}"))),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, LangError> {
        self.skip_newlines();
        self.parse_disjunction()
    }

    fn parse_disjunction(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_conjunction()?;
        while self.eat(&Tok::Pipe) {
            self.skip_newlines();
            let right = self.parse_conjunction()?;
            let span = left.span();
            left = Expr::Disjunction(Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_conjunction(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_or()?;
        while self.eat(&Tok::Amp) {
            self.skip_newlines();
            let right = self.parse_or()?;
            let span = left.span();
            left = Expr::Unify(Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_and()?;
        while self.eat(&Tok::OrOr) {
            self.skip_newlines();
            let right = self.parse_and()?;
            let span = left.span();
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_comparison()?;
        while self.eat(&Tok::AndAnd) {
            self.skip_newlines();
            let right = self.parse_comparison()?;
            let span = left.span();
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek_tok() {
                Tok::EqEq => BinOp::Eq,
                Tok::NotEq => BinOp::Ne,
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.bump();
            self.skip_newlines();
            let right = self.parse_additive()?;
            let span = left.span();
            left = Expr::Binary(op, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek_tok() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            self.skip_newlines();
            let right = self.parse_multiplicative()?;
            let span = left.span();
            left = Expr::Binary(op, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, LangError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_tok() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.bump();
            self.skip_newlines();
            let right = self.parse_unary()?;
            let span = left.span();
            left = Expr::Binary(op, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, LangError> {
        self.skip_newlines();
        let span = self.span();
        match self.peek_tok() {
            Tok::Bang => {
                self.bump();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?), span))
            }
            Tok::Minus => {
                self.bump();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.parse_unary()?), span))
            }
            Tok::Plus => {
                self.bump();
                self.parse_unary()
            }
            Tok::Star => {
                self.bump();
                Ok(Expr::Default(Box::new(self.parse_unary()?), span))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, LangError> {
        let mut expr = self.parse_primary()?;
        loop {
            let span = self.span();
            match self.peek_tok().clone() {
                Tok::Dot => {
                    self.bump();
                    let name = self.expect_ident()?;
                    expr = Expr::Select(Box::new(expr), name, span);
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.parse_expr()?;
                    self.expect(&Tok::RBracket, "`]`")?;
                    expr = Expr::Index(Box::new(expr), Box::new(index), span);
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, LangError> {
        self.skip_newlines();
        let span = self.span();
        match self.peek_tok().clone() {
            Tok::Null => {
                self.bump();
                Ok(Expr::Null(span))
            }
            Tok::True => {
                self.bump();
                Ok(Expr::Bool(true, span))
            }
            Tok::False => {
                self.bump();
                Ok(Expr::Bool(false, span))
            }
            Tok::Int(value) => {
                self.bump();
                Ok(Expr::Int(value, span))
            }
            Tok::Float(value) => {
                self.bump();
                Ok(Expr::Float(value, span))
            }
            Tok::Str(raw) => {
                self.bump();
                Ok(Expr::Str(parse_str_parts(&raw, span)?, span))
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr::Ident(name, span))
            }
            Tok::LBrace => {
                self.bump();
                let items = self.parse_struct_body()?;
                Ok(Expr::Struct(items, span))
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_separators();
                    if self.eat(&Tok::RBracket) {
                        break;
                    }
                    if matches!(self.peek_tok(), Tok::Eof) {
                        return Err(self.error("unterminated list"));
                    }
                    if matches!(self.peek_tok(), Tok::Ellipsis) {
                        self.bump();
                        continue;
                    }
                    items.push(self.parse_expr()?);
                    self.skip_separators();
                }
                Ok(Expr::List(items, span))
            }
            Tok::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(inner)
            }
            other => Err(self.error(format!("unexpected token in expression: {other:?}"))),
        }
    }
}

fn parse_str_parts(raw: &str, span: Span) -> Result<Vec<StrPart>, LangError> {
    let chars: Vec<char> = raw.chars().collect();
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'(') {
            if !text.is_empty() {
                parts.push(StrPart::Text(std::mem::take(&mut text)));
            }
            let start = index + 2;
            let mut depth = 1usize;
            let mut end = start;
            while end < chars.len() {
                match chars[end] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            if depth != 0 {
                return Err(LangError::parse(
                    span.line,
                    span.col,
                    "unterminated interpolation",
                ));
            }
            let inner: String = chars[start..end].iter().collect();
            let expr = parse_expr_fragment(&inner)?;
            parts.push(StrPart::Interp(expr));
            index = end + 1;
        } else {
            text.push(chars[index]);
            index += 1;
        }
    }
    if !text.is_empty() || parts.is_empty() {
        parts.push(StrPart::Text(text));
    }
    Ok(parts)
}
