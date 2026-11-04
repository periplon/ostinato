use bigdecimal::BigDecimal;
use num_bigint::BigInt;

/// Source position used in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundOp {
    Ge,
    Gt,
    Le,
    Lt,
    Ne,
    Match,
    NotMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Label {
    Name(String, Span),
    Dynamic(Box<Expr>, Span),
}

impl Label {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Name(_, span) | Self::Dynamic(_, span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Stable identity assigned by the package loader.
    pub id: usize,
    pub label: Label,
    pub value: Expr,
    pub optional: bool,
    pub definition: bool,
    pub alias: Option<String>,
    pub quoted: bool,
    /// Set when the label came from a bytes literal: the rune-literal units.
    pub bytes_label_runes: Option<usize>,
    /// Whether the bytes label used the `'''...'''` form.
    pub bytes_label_multiline: bool,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Any,
    Regex(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructItem {
    Field(Field),
    Pattern {
        pattern: Pattern,
        value: Expr,
        span: Span,
    },
    Let {
        name: String,
        value: Expr,
        span: Span,
    },
    For {
        key: Option<String>,
        value: String,
        source: Expr,
        body: Vec<StructItem>,
        span: Span,
    },
    If {
        cond: Expr,
        body: Vec<StructItem>,
        span: Span,
    },
    Ellipsis(Span),
}

impl StructItem {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Field(field) => field.span,
            Self::Pattern { span, .. }
            | Self::Let { span, .. }
            | Self::For { span, .. }
            | Self::If { span, .. }
            | Self::Ellipsis(span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompClause {
    For {
        key: Option<String>,
        value: String,
        source: Expr,
        span: Span,
    },
    If {
        cond: Expr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Interp(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytePart {
    Text(Vec<u8>),
    Interp(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Null(Span),
    Bool(bool, Span),
    Int(i64, Span),
    BigInt(BigInt, Span),
    Float(BigDecimal, Span),
    Str(Vec<StrPart>, Span),
    Bytes(Vec<u8>, Span),
    BytesInterp(Vec<BytePart>, Span),
    Ident(String, Span),
    Select(Box<Expr>, String, bool, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Bound(BoundOp, Box<Expr>, Span),
    Paren(Box<Expr>, Span),
    Struct(Vec<StructItem>, Span),
    List(Vec<Expr>, Span),
    OpenList(Option<Box<Expr>>, Span),
    OpenListPrefix(Vec<Expr>, Option<Box<Expr>>, Span),
    ListComp {
        clauses: Vec<CompClause>,
        body: Box<Expr>,
        span: Span,
    },
    Unify(Box<Expr>, Box<Expr>, Span),
    Disjunction(Box<Expr>, Box<Expr>, Span),
    Default(Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    Unary(UnOp, Box<Expr>, Span),
}

impl Expr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Null(span)
            | Self::Bool(_, span)
            | Self::Int(_, span)
            | Self::BigInt(_, span)
            | Self::Float(_, span)
            | Self::Bytes(_, span)
            | Self::BytesInterp(_, span)
            | Self::Str(_, span)
            | Self::Ident(_, span)
            | Self::Select(_, _, _, span)
            | Self::Index(_, _, span)
            | Self::Call(_, _, span)
            | Self::Bound(_, _, span)
            | Self::Paren(_, span)
            | Self::Struct(_, span)
            | Self::List(_, span)
            | Self::OpenList(_, span)
            | Self::OpenListPrefix(_, _, span)
            | Self::ListComp { span, .. }
            | Self::Unify(_, _, span)
            | Self::Disjunction(_, _, span)
            | Self::Default(_, span)
            | Self::Binary(_, _, _, span)
            | Self::Unary(_, _, span) => *span,
        }
    }
}

/// An import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub alias: String,
    pub path: String,
    pub span: Span,
}

/// A parsed CUE-compatible file.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub package: Option<String>,
    pub imports: Vec<ImportDecl>,
    pub items: Vec<StructItem>,
}
