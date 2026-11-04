use crate::ast::{
    Attribute, BinOp, BoundOp, BytePart, CompClause, Expr, Field, File, ImportDecl, Label, Pattern,
    Span, StrPart, StructItem, UnOp,
};
use crate::error::LangError;
use crate::lexer::{LexBytePart, Tok, Token, lex};

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

    fn at_separator_or(&self, closer: &Tok) -> bool {
        matches!(
            self.peek_tok(),
            Tok::Newline | Tok::Comma | Tok::Semi | Tok::At | Tok::Eof
        ) || self.peek_tok() == closer
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
        let mut imports = Vec::new();
        loop {
            self.skip_separators();
            if !self.eat(&Tok::Import) {
                break;
            }
            if self.eat(&Tok::LParen) {
                loop {
                    self.skip_separators();
                    if self.eat(&Tok::RParen) {
                        break;
                    }
                    if matches!(self.peek_tok(), Tok::Eof) {
                        return Err(self.error("unterminated import block"));
                    }
                    imports.push(self.parse_import_decl()?);
                    if !self.at_separator_or(&Tok::RParen) {
                        return Err(self.error("expected `,`, newline or `)` after import"));
                    }
                }
            } else {
                imports.push(self.parse_import_decl()?);
            }
        }
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if matches!(self.peek_tok(), Tok::Eof) {
                break;
            }
            if let Some(item) = self.parse_struct_item()? {
                items.push(item);
                if !self.at_separator_or(&Tok::Eof) {
                    return Err(self.error("expected `,`, newline or end of file after field"));
                }
            }
        }
        Ok(File {
            package,
            imports,
            items,
        })
    }

    fn parse_import_decl(&mut self) -> Result<ImportDecl, LangError> {
        let span = self.span();
        let mut alias = None;
        if let Tok::Ident(name) = self.peek_tok().clone()
            && matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.tok),
                Some(Tok::Str(_))
            )
        {
            self.bump();
            alias = Some(name);
        }
        let Tok::Str(path) = self.peek_tok().clone() else {
            return Err(self.error("expected import path string"));
        };
        self.bump();
        let alias = alias.unwrap_or_else(|| {
            let segment = path.rsplit('/').next().unwrap_or(path.as_str());
            segment.split('@').next().unwrap_or(segment).to_owned()
        });
        Ok(ImportDecl { alias, path, span })
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
                if !self.at_separator_or(&Tok::RBrace) {
                    return Err(self.error("expected `,`, newline or `}` after field"));
                }
            }
        }
        Ok(items)
    }

    fn parse_attribute(&mut self) -> Result<Attribute, LangError> {
        let span = self.span();
        self.bump(); // '@'
        let name = match self.peek_tok().clone() {
            Tok::Ident(name) => {
                self.bump();
                name
            }
            Tok::If => {
                self.bump();
                "if".to_owned()
            }
            Tok::For => {
                self.bump();
                "for".to_owned()
            }
            Tok::In => {
                self.bump();
                "in".to_owned()
            }
            Tok::Let => {
                self.bump();
                "let".to_owned()
            }
            _ => return Err(self.error("attribute name expected")),
        };
        let mut args = Vec::new();
        if self.eat(&Tok::LParen) {
            let mut current = String::new();
            let mut depth = 1usize;
            loop {
                match self.peek_tok().clone() {
                    Tok::LParen => {
                        depth += 1;
                        current.push('(');
                        self.bump();
                    }
                    Tok::RParen => {
                        depth -= 1;
                        self.bump();
                        if depth == 0 {
                            if !current.trim().is_empty() {
                                args.push(current.trim().to_owned());
                            }
                            break;
                        }
                        current.push(')');
                    }
                    Tok::Comma if depth == 1 => {
                        if !current.trim().is_empty() {
                            args.push(current.trim().to_owned());
                        }
                        current.clear();
                        self.bump();
                    }
                    Tok::Eof => return Err(self.error("unterminated attribute")),
                    ref other => {
                        current.push_str(&token_text(other));
                        self.bump();
                    }
                }
            }
        }
        Ok(Attribute { name, args, span })
    }

    fn parse_struct_item(&mut self) -> Result<Option<StructItem>, LangError> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
            self.parse_struct_item_impl()
        })
    }

    fn parse_struct_item_impl(&mut self) -> Result<Option<StructItem>, LangError> {
        let mut attributes = Vec::new();
        while matches!(self.peek_tok(), Tok::At) {
            attributes.push(self.parse_attribute()?);
            self.skip_newlines();
        }
        if matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            return Ok(None);
        }
        if matches!(self.peek_tok(), Tok::LBracket) {
            let saved = self.pos;
            if let Some(pattern) = self.try_parse_pattern()? {
                self.expect(&Tok::Colon, "`:`")?;
                let value = self.parse_field_value()?;
                return Ok(Some(StructItem::Pattern {
                    pattern,
                    value,
                    span: self.span(),
                }));
            }
            self.pos = saved;
        }
        let span = self.span();
        match self.peek_tok().clone() {
            Tok::Let => {
                self.bump();
                let name = self.expect_ident()?;
                self.expect(&Tok::Equals, "`=`")?;
                let value = self.parse_expr()?;
                Ok(Some(StructItem::Let { name, value, span }))
            }
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
                let body = self.parse_comprehension_body(span)?;
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
                let body = self.parse_comprehension_body(span)?;
                Ok(Some(StructItem::If { cond, body, span }))
            }
            Tok::Ellipsis => {
                self.bump();
                Ok(Some(StructItem::Ellipsis(span)))
            }
            _ => {
                let alias = if let Tok::Ident(alias) = self.peek_tok().clone()
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|token| &token.tok),
                        Some(Tok::Equals)
                    ) {
                    self.bump();
                    self.bump();
                    Some(alias)
                } else {
                    None
                };
                let quoted = matches!(self.peek_tok(), Tok::Str(_) | Tok::Bytes(_));
                let bytes_label_runes = self.bytes_label_runes();
                let definition = matches!(
                    self.peek_tok(),
                    Tok::Ident(name) if name.starts_with('#')
                );
                let label = self.parse_label()?;
                let optional = self.eat(&Tok::Question);
                self.expect(&Tok::Colon, "`:`")?;
                let value = self.parse_field_value()?;
                while matches!(self.peek_tok(), Tok::At) {
                    attributes.push(self.parse_attribute()?);
                }
                Ok(Some(StructItem::Field(Field {
                    id: 0,
                    label,
                    value,
                    optional,
                    definition,
                    alias,
                    quoted,
                    bytes_label_runes,
                    attributes,
                    span,
                })))
            }
        }
    }

    fn try_parse_pattern(&mut self) -> Result<Option<Pattern>, LangError> {
        self.expect(&Tok::LBracket, "`[`")?;
        let pattern = match self.peek_tok().clone() {
            Tok::Ident(name) if name == "string" => {
                self.bump();
                Pattern::Any
            }
            Tok::Match => {
                self.bump();
                let Tok::Str(regex) = self.peek_tok().clone() else {
                    return Err(self.error("expected regex string after `=~`"));
                };
                self.bump();
                let parts = parse_str_parts(&regex, self.span())?;
                let mut text = String::new();
                for part in parts {
                    match part {
                        StrPart::Text(value) => text.push_str(&value),
                        StrPart::Interp(_) => {
                            return Err(self.error("pattern regex cannot interpolate"));
                        }
                    }
                }
                Pattern::Regex(text)
            }
            _ => return Ok(None),
        };
        self.expect(&Tok::RBracket, "`]`")?;
        Ok(Some(pattern))
    }

    fn bytes_label_runes(&self) -> Option<usize> {
        let Tok::Bytes(parts) = self.peek_tok() else {
            return None;
        };
        let mut bytes = Vec::new();
        for part in parts {
            match part {
                LexBytePart::Text(value) => bytes.extend_from_slice(value),
                LexBytePart::Interp(_) => return None,
            }
        }
        Some(String::from_utf8_lossy(&bytes).chars().count())
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
                let parts = parse_str_parts(&raw, span)?;
                let has_interp = parts.iter().any(|part| matches!(part, StrPart::Interp(_)));
                if has_interp {
                    Ok(Label::Dynamic(Box::new(Expr::Str(parts, span)), span))
                } else {
                    let text: String = parts
                        .into_iter()
                        .filter_map(|part| match part {
                            StrPart::Text(value) => Some(value),
                            StrPart::Interp(_) => None,
                        })
                        .collect();
                    Ok(Label::Name(text, span))
                }
            }
            Tok::Bytes(raw) => {
                self.bump();
                let mut bytes = Vec::new();
                for part in raw {
                    match part {
                        LexBytePart::Text(value) => bytes.extend_from_slice(&value),
                        LexBytePart::Interp(_) => {
                            return Err(self.error("bytes label cannot interpolate"));
                        }
                    }
                }
                Ok(Label::Name(
                    String::from_utf8_lossy(&bytes).into_owned(),
                    span,
                ))
            }
            Tok::LBracket => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(&Tok::RBracket, "`]`")?;
                Ok(Label::Dynamic(Box::new(expr), span))
            }
            Tok::LParen => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(Label::Dynamic(Box::new(expr), span))
            }
            other => Err(self.error(format!("expected field label, found {other:?}"))),
        }
    }

    fn parse_comprehension_clauses(&mut self) -> Result<Vec<CompClause>, LangError> {
        let mut clauses = Vec::new();
        loop {
            self.skip_newlines();
            if self.eat(&Tok::For) {
                let first = self.expect_ident()?;
                let (key, value) = if self.eat(&Tok::Comma) {
                    let second = self.expect_ident()?;
                    (Some(first), second)
                } else {
                    (None, first)
                };
                self.expect(&Tok::In, "`in`")?;
                let source = self.parse_expr()?;
                clauses.push(CompClause::For {
                    key,
                    value,
                    source,
                    span: self.span(),
                });
            } else if self.eat(&Tok::If) {
                let cond = self.parse_expr()?;
                clauses.push(CompClause::If {
                    cond,
                    span: self.span(),
                });
            } else {
                break;
            }
        }
        Ok(clauses)
    }

    fn parse_comprehension_body(&mut self, span: Span) -> Result<Vec<StructItem>, LangError> {
        self.skip_newlines();
        if self.eat(&Tok::If) {
            let cond = self.parse_expr()?;
            let body = self.parse_comprehension_body(span)?;
            return Ok(vec![StructItem::If { cond, body, span }]);
        }
        if self.eat(&Tok::For) {
            let first = self.expect_ident()?;
            let (key, value) = if self.eat(&Tok::Comma) {
                let second = self.expect_ident()?;
                (Some(first), second)
            } else {
                (None, first)
            };
            self.expect(&Tok::In, "`in`")?;
            let source = self.parse_expr()?;
            let body = self.parse_comprehension_body(span)?;
            return Ok(vec![StructItem::For {
                key,
                value,
                source,
                body,
                span,
            }]);
        }
        self.expect(&Tok::LBrace, "`{`")?;
        self.parse_struct_body()
    }

    fn starts_struct_item(&self) -> bool {
        match self.peek_tok() {
            Tok::Let | Tok::For | Tok::If | Tok::Ellipsis => true,
            Tok::LBracket => self.bracket_closes_a_label(),
            Tok::Ident(_) | Tok::Str(_) | Tok::Bytes(_) => matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.tok),
                Some(Tok::Colon | Tok::Equals | Tok::Question)
            ),
            _ => false,
        }
    }

    /// A leading `[` starts a struct item only when the bracket closes a
    /// pattern/dynamic label followed by `:`; otherwise it starts a list.
    fn bracket_closes_a_label(&self) -> bool {
        let mut depth = 0usize;
        let mut index = self.pos;
        while let Some(token) = self.tokens.get(index) {
            match &token.tok {
                Tok::LBracket => depth += 1,
                Tok::RBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.tok),
                            Some(Tok::Colon)
                        );
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    fn expect_rbrace(&mut self) -> Result<(), LangError> {
        self.expect(&Tok::RBrace, "`}`")
    }

    fn parse_field_value(&mut self) -> Result<Expr, LangError> {
        self.skip_newlines();
        let alias = if let Tok::Ident(alias) = self.peek_tok().clone()
            && matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.tok),
                Some(Tok::Equals)
            ) {
            self.bump();
            self.bump();
            Some(alias)
        } else {
            None
        };
        let nested = alias.is_some()
            || (matches!(self.peek_tok(), Tok::Ident(_) | Tok::Str(_) | Tok::Bytes(_))
                && matches!(
                    self.tokens.get(self.pos + 1).map(|token| &token.tok),
                    Some(Tok::Colon)
                ));
        if nested {
            let span = self.span();
            let quoted = matches!(self.peek_tok(), Tok::Str(_) | Tok::Bytes(_));
            let bytes_label_runes = self.bytes_label_runes();
            let definition = matches!(
                self.peek_tok(),
                Tok::Ident(name) if name.starts_with('#')
            );
            let label = self.parse_label()?;
            let optional = self.eat(&Tok::Question);
            self.expect(&Tok::Colon, "`:`")?;
            let value = self.parse_field_value()?;
            let mut attributes = Vec::new();
            while matches!(self.peek_tok(), Tok::At) {
                attributes.push(self.parse_attribute()?);
            }
            Ok(Expr::Struct(
                vec![StructItem::Field(Field {
                    id: 0,
                    label,
                    value,
                    optional,
                    definition,
                    alias,
                    quoted,
                    bytes_label_runes,
                    attributes,
                    span,
                })],
                span,
            ))
        } else {
            self.parse_expr()
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, LangError> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || self.parse_expr_impl())
    }

    fn parse_expr_impl(&mut self) -> Result<Expr, LangError> {
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
            Tok::Ge | Tok::Gt | Tok::Le | Tok::Lt | Tok::NotEq | Tok::Match | Tok::BangMatch => {
                let op = match self.bump().tok {
                    Tok::Ge => BoundOp::Ge,
                    Tok::Gt => BoundOp::Gt,
                    Tok::Le => BoundOp::Le,
                    Tok::Lt => BoundOp::Lt,
                    Tok::NotEq => BoundOp::Ne,
                    Tok::Match => BoundOp::Match,
                    Tok::BangMatch => BoundOp::NotMatch,
                    _ => unreachable!("bound operator"),
                };
                let operand = self.parse_unary()?;
                Ok(Expr::Bound(op, Box::new(operand), span))
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
                    let (name, quoted) = match self.peek_tok().clone() {
                        Tok::Ident(name) => {
                            self.bump();
                            (name, false)
                        }
                        Tok::Str(raw) => {
                            self.bump();
                            let parts = parse_str_parts(&raw, span)?;
                            let mut text = String::new();
                            for part in parts {
                                match part {
                                    StrPart::Text(value) => text.push_str(&value),
                                    StrPart::Interp(_) => {
                                        return Err(
                                            self.error("quoted selector cannot interpolate")
                                        );
                                    }
                                }
                            }
                            (text, true)
                        }
                        _ => return Err(self.error("expected selector")),
                    };
                    expr = Expr::Select(Box::new(expr), name, quoted, span);
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.parse_expr()?;
                    self.expect(&Tok::RBracket, "`]`")?;
                    expr = Expr::Index(Box::new(expr), Box::new(index), span);
                }
                Tok::LParen => {
                    self.bump();
                    let mut arguments = Vec::new();
                    loop {
                        self.skip_separators();
                        if self.eat(&Tok::RParen) {
                            break;
                        }
                        arguments.push(self.parse_expr()?);
                        self.skip_separators();
                    }
                    expr = Expr::Call(Box::new(expr), arguments, span);
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
            Tok::BigInt(value) => {
                self.bump();
                Ok(Expr::BigInt(value, span))
            }
            Tok::Float(value) => {
                self.bump();
                Ok(Expr::Float(value, span))
            }
            Tok::Str(raw) => {
                self.bump();
                Ok(Expr::Str(parse_str_parts(&raw, span)?, span))
            }
            Tok::Bytes(raw) => {
                self.bump();
                let mut parts = Vec::new();
                for part in raw {
                    match part {
                        LexBytePart::Text(bytes) => parts.push(BytePart::Text(bytes)),
                        LexBytePart::Interp(source) => {
                            let expr = parse_expr_fragment(&source)?;
                            parts.push(BytePart::Interp(expr));
                        }
                    }
                }
                match parts.as_slice() {
                    [BytePart::Text(bytes)] => Ok(Expr::Bytes(bytes.clone(), span)),
                    _ => Ok(Expr::BytesInterp(parts, span)),
                }
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
                self.skip_separators();
                if matches!(self.peek_tok(), Tok::For | Tok::If) {
                    let clauses = self.parse_comprehension_clauses()?;
                    self.skip_newlines();
                    self.expect(&Tok::LBrace, "`{`")?;
                    self.skip_newlines();
                    let body = if self.starts_struct_item() {
                        Expr::Struct(self.parse_struct_body()?, span)
                    } else {
                        let body = self.parse_expr()?;
                        self.skip_newlines();
                        self.expect_rbrace()?;
                        body
                    };
                    self.skip_newlines();
                    self.expect(&Tok::RBracket, "`]`")?;
                    return Ok(Expr::ListComp {
                        clauses,
                        body: Box::new(body),
                        span,
                    });
                }
                if matches!(self.peek_tok(), Tok::Ellipsis) {
                    self.bump();
                    self.skip_newlines();
                    let element = if matches!(self.peek_tok(), Tok::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.parse_expr()?))
                    };
                    self.expect(&Tok::RBracket, "`]`")?;
                    return Ok(Expr::OpenList(element, span));
                }
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
                        self.skip_newlines();
                        let tail = if matches!(self.peek_tok(), Tok::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.expect(&Tok::RBracket, "`]`")?;
                        return Ok(Expr::OpenListPrefix(items, tail, span));
                    }
                    items.push(self.parse_expr()?);
                    if !self.at_separator_or(&Tok::RBracket) {
                        return Err(self.error("expected `,`, newline or `]` after list element"));
                    }
                    self.skip_separators();
                }
                Ok(Expr::List(items, span))
            }
            Tok::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(Expr::Paren(Box::new(inner), span))
            }
            other => Err(self.error(format!("unexpected token in expression: {other:?}"))),
        }
    }
}

fn token_text(token: &Tok) -> String {
    match token {
        Tok::Ident(name) => name.clone(),
        Tok::Int(value) => value.to_string(),
        Tok::BigInt(value) => value.to_string(),
        Tok::Float(value) => value.to_string(),
        Tok::Str(value) => value.clone(),
        Tok::Equals => "=".to_owned(),
        Tok::Comma => ",".to_owned(),
        Tok::Dot => ".".to_owned(),
        Tok::Colon => ":".to_owned(),
        Tok::Minus => "-".to_owned(),
        other => format!("{other:?}"),
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
            let Some(end) = crate::lexer::interpolation_end(&chars, start) else {
                return Err(LangError::parse(
                    span.line,
                    span.col,
                    "unterminated interpolation",
                ));
            };
            let inner: String = chars[start..end].iter().collect();
            let expr = parse_expr_fragment(&inner)?;
            parts.push(StrPart::Interp(expr));
            index = end + 1;
        } else if chars[index] == '\\' {
            let Some(escaped) = chars.get(index + 1).copied() else {
                return Err(LangError::parse(
                    span.line,
                    span.col,
                    "unterminated escape sequence",
                ));
            };
            match escaped {
                'a' => {
                    text.push('\u{7}');
                    index += 2;
                }
                'b' => {
                    text.push('\u{8}');
                    index += 2;
                }
                'f' => {
                    text.push('\u{c}');
                    index += 2;
                }
                'n' => {
                    text.push('\n');
                    index += 2;
                }
                'r' => {
                    text.push('\r');
                    index += 2;
                }
                't' => {
                    text.push('\t');
                    index += 2;
                }
                'v' => {
                    text.push('\u{b}');
                    index += 2;
                }
                '\\' => {
                    text.push('\\');
                    index += 2;
                }
                '/' => {
                    text.push('/');
                    index += 2;
                }
                '"' => {
                    text.push('"');
                    index += 2;
                }
                'u' | 'U' => {
                    let count = if escaped == 'u' { 4 } else { 8 };
                    let start = index + 2;
                    if start + count > chars.len() {
                        return Err(LangError::parse(
                            span.line,
                            span.col,
                            "invalid unicode escape",
                        ));
                    }
                    let digits: String = chars[start..start + count].iter().collect();
                    let value = u32::from_str_radix(&digits, 16).map_err(|_| {
                        LangError::parse(span.line, span.col, "invalid unicode escape")
                    })?;
                    let mut next = start + count;
                    let scalar = if (0xD800..=0xDBFF).contains(&value) {
                        if escaped != 'u'
                            || chars.get(next) != Some(&'\\')
                            || chars.get(next + 1) != Some(&'u')
                        {
                            return Err(LangError::parse(
                                span.line,
                                span.col,
                                "invalid unicode escape",
                            ));
                        }
                        let low_start = next + 2;
                        if low_start + 4 > chars.len() {
                            return Err(LangError::parse(
                                span.line,
                                span.col,
                                "invalid unicode escape",
                            ));
                        }
                        let low_digits: String = chars[low_start..low_start + 4].iter().collect();
                        let low = u32::from_str_radix(&low_digits, 16).map_err(|_| {
                            LangError::parse(span.line, span.col, "invalid unicode escape")
                        })?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(LangError::parse(
                                span.line,
                                span.col,
                                "invalid unicode escape",
                            ));
                        }
                        next = low_start + 4;
                        0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00)
                    } else if (0xDC00..=0xDFFF).contains(&value) {
                        return Err(LangError::parse(
                            span.line,
                            span.col,
                            "invalid unicode escape",
                        ));
                    } else {
                        value
                    };
                    let character = char::from_u32(scalar).ok_or_else(|| {
                        LangError::parse(span.line, span.col, "invalid unicode escape")
                    })?;
                    text.push(character);
                    index = next;
                }
                other => {
                    return Err(LangError::parse(
                        span.line,
                        span.col,
                        format!("unknown escape sequence `\\{other}`"),
                    ));
                }
            }
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
