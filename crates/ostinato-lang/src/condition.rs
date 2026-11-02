use serde_json::{Number, Value as Json};

use crate::error::LangError;

/// Evaluate a boolean-ish condition such as `steps.review.score >= 0.9`.
///
/// # Errors
/// Returns [`LangError::Condition`] for syntax errors or missing references.
pub fn eval_condition(expr: &str, context: &Json) -> Result<bool, LangError> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        expr,
        strict: true,
    };
    let value = parser.parse_or(context)?;
    if !matches!(parser.peek(), CTok::Eof) {
        return Err(LangError::Condition {
            expr: expr.to_owned(),
            message: "unexpected token after expression".to_owned(),
        });
    }
    Ok(truthy(&value))
}

/// Validate condition syntax and reference roots without executing it.
///
/// Missing nested output fields (`steps.x.json.any`, `provider`, ...) are
/// accepted because they only exist at runtime; unknown step or parameter
/// names are rejected.
///
/// # Errors
/// Returns [`LangError::Condition`] for syntax errors or unknown roots.
pub fn validate_condition(expr: &str, context: &Json) -> Result<(), LangError> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        expr,
        strict: false,
    };
    parser.parse_or(context)?;
    if !matches!(parser.peek(), CTok::Eof) {
        return Err(LangError::Condition {
            expr: expr.to_owned(),
            message: "unexpected token after expression".to_owned(),
        });
    }
    Ok(())
}

fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Json::String(value) => !value.is_empty(),
        Json::Array(values) => !values.is_empty(),
        Json::Object(values) => !values.is_empty(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CTok {
    Ident(String),
    Number(f64),
    Str(String),
    True,
    False,
    Null,
    Dot,
    LParen,
    RParen,
    Minus,
    Not,
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

fn tokenize(expr: &str) -> Result<Vec<CTok>, LangError> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            c if c.is_whitespace() => index += 1,
            '(' => {
                out.push(CTok::LParen);
                index += 1;
            }
            ')' => {
                out.push(CTok::RParen);
                index += 1;
            }
            '.' => {
                out.push(CTok::Dot);
                index += 1;
            }
            '-' => {
                out.push(CTok::Minus);
                index += 1;
            }
            '"' => {
                index += 1;
                let mut text = String::new();
                while index < chars.len() && chars[index] != '"' {
                    if chars[index] == '\\' && index + 1 < chars.len() {
                        index += 1;
                        text.push(match chars[index] {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        text.push(chars[index]);
                    }
                    index += 1;
                }
                if index >= chars.len() {
                    return Err(cond_error(expr, "unterminated string"));
                }
                index += 1;
                out.push(CTok::Str(text));
            }
            c if c.is_ascii_digit() => {
                let start = index;
                while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.')
                {
                    index += 1;
                }
                let text: String = chars[start..index].iter().collect();
                let value = text
                    .parse::<f64>()
                    .map_err(|_| cond_error(expr, &format!("invalid number `{text}`")))?;
                out.push(CTok::Number(value));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_alphanumeric()
                        || chars[index] == '_'
                        || chars[index] == '-')
                {
                    index += 1;
                }
                let text: String = chars[start..index].iter().collect();
                out.push(match text.as_str() {
                    "true" => CTok::True,
                    "false" => CTok::False,
                    "null" => CTok::Null,
                    "and" => CTok::And,
                    "or" => CTok::Or,
                    "not" => CTok::Not,
                    _ => CTok::Ident(text),
                });
            }
            '!' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(CTok::Ne);
                    index += 2;
                } else {
                    out.push(CTok::Not);
                    index += 1;
                }
            }
            '=' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(CTok::Eq);
                    index += 2;
                } else {
                    return Err(cond_error(expr, "use `==` for equality"));
                }
            }
            '<' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(CTok::Le);
                    index += 2;
                } else {
                    out.push(CTok::Lt);
                    index += 1;
                }
            }
            '>' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(CTok::Ge);
                    index += 2;
                } else {
                    out.push(CTok::Gt);
                    index += 1;
                }
            }
            '&' => {
                if chars.get(index + 1) == Some(&'&') {
                    out.push(CTok::And);
                    index += 2;
                } else {
                    return Err(cond_error(expr, "use `&&` for and"));
                }
            }
            '|' => {
                if chars.get(index + 1) == Some(&'|') {
                    out.push(CTok::Or);
                    index += 2;
                } else {
                    return Err(cond_error(expr, "use `||` for or"));
                }
            }
            other => {
                return Err(cond_error(expr, &format!("unexpected character `{other}`")));
            }
        }
    }
    out.push(CTok::Eof);
    Ok(out)
}

fn cond_error(expr: &str, message: &str) -> LangError {
    LangError::Condition {
        expr: expr.to_owned(),
        message: message.to_owned(),
    }
}

struct Parser<'a> {
    tokens: Vec<CTok>,
    pos: usize,
    expr: &'a str,
    strict: bool,
}

impl Parser<'_> {
    fn peek(&self) -> &CTok {
        let index = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[index]
    }

    fn bump(&mut self) -> CTok {
        let token = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn parse_or(&mut self, context: &Json) -> Result<Json, LangError> {
        let mut left = self.parse_and(context)?;
        while matches!(self.peek(), CTok::Or) {
            self.bump();
            let right = self.parse_and(context)?;
            left = Json::Bool(truthy(&left) || truthy(&right));
        }
        Ok(left)
    }

    fn parse_and(&mut self, context: &Json) -> Result<Json, LangError> {
        let mut left = self.parse_comparison(context)?;
        while matches!(self.peek(), CTok::And) {
            self.bump();
            let right = self.parse_comparison(context)?;
            left = Json::Bool(truthy(&left) && truthy(&right));
        }
        Ok(left)
    }

    fn parse_comparison(&mut self, context: &Json) -> Result<Json, LangError> {
        let left = self.parse_unary(context)?;
        let op = match self.peek() {
            CTok::Eq => Some(CTok::Eq),
            CTok::Ne => Some(CTok::Ne),
            CTok::Lt => Some(CTok::Lt),
            CTok::Le => Some(CTok::Le),
            CTok::Gt => Some(CTok::Gt),
            CTok::Ge => Some(CTok::Ge),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(left);
        };
        self.bump();
        let right = self.parse_unary(context)?;
        if !self.strict {
            return Ok(Json::Bool(false));
        }
        compare(self.expr, &op, &left, &right)
    }

    fn parse_unary(&mut self, context: &Json) -> Result<Json, LangError> {
        if matches!(self.peek(), CTok::Not) {
            self.bump();
            let value = self.parse_unary(context)?;
            return Ok(Json::Bool(!truthy(&value)));
        }
        if matches!(self.peek(), CTok::Minus) {
            self.bump();
            let value = self.parse_unary(context)?;
            let number = value
                .as_f64()
                .ok_or_else(|| cond_error(self.expr, "unary `-` requires a number"))?;
            return Ok(Number::from_f64(-number).map_or(Json::Null, Json::Number));
        }
        self.parse_primary(context)
    }

    fn parse_primary(&mut self, context: &Json) -> Result<Json, LangError> {
        match self.bump() {
            CTok::Number(value) => Ok(Number::from_f64(value).map_or(Json::Null, Json::Number)),
            CTok::Str(value) => Ok(Json::String(value)),
            CTok::True => Ok(Json::Bool(true)),
            CTok::False => Ok(Json::Bool(false)),
            CTok::Null => Ok(Json::Null),
            CTok::LParen => {
                let value = self.parse_or(context)?;
                match self.bump() {
                    CTok::RParen => Ok(value),
                    _ => Err(cond_error(self.expr, "expected `)`")),
                }
            }
            CTok::Ident(first) => {
                let mut path = first;
                while matches!(self.peek(), CTok::Dot) {
                    self.bump();
                    match self.bump() {
                        CTok::Ident(segment) => {
                            path.push('.');
                            path.push_str(&segment);
                        }
                        _ => return Err(cond_error(self.expr, "expected identifier after `.`")),
                    }
                }
                match resolve(context, &path) {
                    Some(value) => Ok(value.clone()),
                    None if !self.strict && valid_reference(context, &path) => Ok(Json::Null),
                    None => Err(cond_error(
                        self.expr,
                        &format!("unknown reference `{path}`"),
                    )),
                }
            }
            other => Err(cond_error(
                self.expr,
                &format!("unexpected token {other:?}"),
            )),
        }
    }
}

fn valid_reference(context: &Json, path: &str) -> bool {
    let mut segments = path.split('.');
    let root = segments.next().unwrap_or("");
    match root {
        "steps" => segments.next().is_some_and(|step| {
            context
                .get("steps")
                .and_then(|steps| steps.get(step))
                .is_some()
        }),
        "params" => segments.next().is_some_and(|param| {
            context
                .get("params")
                .and_then(|params| params.get(param))
                .is_some()
        }),
        "run_id" | "workflow" | "round" => true,
        _ => false,
    }
}

fn resolve<'a>(context: &'a Json, path: &str) -> Option<&'a Json> {
    let mut current = context;
    for segment in path.split('.') {
        current = match current {
            Json::Object(map) => map.get(segment)?,
            Json::Array(list) => list.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn compare(expr: &str, op: &CTok, left: &Json, right: &Json) -> Result<Json, LangError> {
    let result = match op {
        CTok::Eq | CTok::Ne => {
            let equal = match (left, right) {
                (Json::Number(a), Json::Number(b)) => a.as_f64() == b.as_f64(),
                _ => left == right,
            };
            if matches!(op, CTok::Eq) {
                equal
            } else {
                !equal
            }
        }
        CTok::Lt | CTok::Le | CTok::Gt | CTok::Ge => {
            let ordering = match (left, right) {
                (Json::Number(a), Json::Number(b)) => a.as_f64().partial_cmp(&b.as_f64()),
                (Json::String(a), Json::String(b)) => Some(a.cmp(b)),
                (Json::Bool(a), Json::Bool(b)) => Some(a.cmp(b)),
                _ => {
                    return Err(cond_error(
                        expr,
                        "ordered comparison requires two numbers or two strings",
                    ));
                }
            };
            let Some(ordering) = ordering else {
                return Ok(Json::Bool(false));
            };
            match op {
                CTok::Lt => ordering.is_lt(),
                CTok::Le => ordering.is_le(),
                CTok::Gt => ordering.is_gt(),
                CTok::Ge => ordering.is_ge(),
                _ => false,
            }
        }
        _ => false,
    };
    Ok(Json::Bool(result))
}
