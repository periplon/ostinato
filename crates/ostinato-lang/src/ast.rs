/// Source position used in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub label: Label,
    pub value: Expr,
    pub optional: bool,
    pub definition: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructItem {
    Field(Field),
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
            Self::Let { span, .. }
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
pub enum Expr {
    Null(Span),
    Bool(bool, Span),
    Int(i64, Span),
    Float(f64, Span),
    Str(Vec<StrPart>, Span),
    Ident(String, Span),
    Select(Box<Expr>, String, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
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
            | Self::Float(_, span)
            | Self::Str(_, span)
            | Self::Ident(_, span)
            | Self::Select(_, _, span)
            | Self::Index(_, _, span)
            | Self::Call(_, _, span)
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
