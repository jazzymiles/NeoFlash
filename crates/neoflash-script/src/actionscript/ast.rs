//! ActionScript AST nodes and structural predicates.

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Literal(String),
    Name(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    XmlSelect {
        base: Box<Expr>,
        local: Box<Expr>,
        namespace: Option<Box<Expr>>,
        attribute: bool,
        descendant: bool,
    },
    XmlFilter(Box<Expr>, Box<Expr>, usize),
    XmlWithAttr {
        local: Box<Expr>,
        namespace: Option<Box<Expr>>,
        attribute: bool,
        descendant: bool,
    },
    XmlLiteral(String, Vec<Expr>),
    RegExpLiteral(String, String),
    Array(Vec<Option<Expr>>),
    Object(Vec<(PropertyKey, Expr)>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    InstanceOf(Box<Expr>, Box<Expr>),
    TypeTest(String, Box<Expr>, String, usize),
    TypeValue(String, usize),
    Delete(Box<Expr>, usize),
    Assign(String, Box<Expr>, Box<Expr>),
    Update(String, Box<Expr>, bool),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, usize),
    Function(Box<Function>),
    NewError(String, Vec<Expr>, usize),
    NewClass(String, Vec<Expr>, usize),
    NewValue(String, Vec<Expr>, usize),
    NewBuiltin(String, Vec<Expr>, usize),
    NewVector(String, Vec<Expr>, usize),
}

#[derive(Clone, Debug)]
pub(crate) struct PropertyKey {
    /// Preserve the original quoting/escaping when emitting JavaScript.
    pub(crate) source: String,
    /// Match member access using the decoded property name.
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub(crate) id: usize,
    pub(crate) offset: usize,
    pub(crate) name: Option<String>,
    pub(crate) params: Vec<(String, Option<String>)>,
    pub(crate) defaults: Vec<Option<Expr>>,
    pub(crate) rest: Option<String>,
    pub(crate) returns: Option<String>,
    pub(crate) body: Vec<Stmt>,
    pub(crate) declaration: bool,
}

pub(crate) fn constant_default(expr: &Expr) -> bool {
    fn numeric(expr: &Expr) -> bool {
        match expr {
            Expr::Literal(value) => {
                value.starts_with(|ch: char| ch.is_ascii_digit() || ch == '.')
                    || matches!(value.as_str(), "NaN" | "Infinity")
            }
            Expr::Unary(op, value) if matches!(op.as_str(), "+" | "-") => numeric(value),
            _ => false,
        }
    }
    matches!(expr, Expr::Literal(_))
        || numeric(expr)
        || matches!(expr, Expr::Unary(op, value) if op == "void" && constant_default(value))
}

#[derive(Clone, Debug)]
pub(crate) struct CatchClause {
    pub(crate) id: usize,
    pub(crate) offset: usize,
    pub(crate) name: String,
    pub(crate) ty: Option<String>,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(crate) test: Option<Expr>,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub(crate) struct ControlTarget {
    pub(crate) label: Option<String>,
    pub(crate) iteration: bool,
    pub(crate) breakable: bool,
}

impl Expr {
    pub(crate) fn depth(&self) -> usize {
        1 + match self {
            Self::Literal(_)
            | Self::Name(_)
            | Self::Function(_)
            | Self::TypeValue(_, _)
            | Self::RegExpLiteral(_, _) => 0,
            Self::Member(v, _)
            | Self::Unary(_, v)
            | Self::Update(_, v, _)
            | Self::TypeTest(_, v, _, _)
            | Self::Delete(v, _) => v.depth(),
            Self::Index(a, b)
            | Self::Binary(_, a, b)
            | Self::Assign(_, a, b)
            | Self::XmlFilter(a, b, _)
            | Self::InstanceOf(a, b) => a.depth().max(b.depth()),
            Self::XmlWithAttr {
                local, namespace, ..
            } => local
                .depth()
                .max(namespace.as_deref().map_or(0, Self::depth)),
            Self::XmlSelect {
                base,
                local,
                namespace,
                ..
            } => base
                .depth()
                .max(local.depth())
                .max(namespace.as_deref().map_or(0, Self::depth)),
            Self::Array(v) => v.iter().flatten().map(Self::depth).max().unwrap_or(0),
            Self::XmlLiteral(_, v) => v.iter().map(Self::depth).max().unwrap_or(0),
            Self::Object(v) => v.iter().map(|(_, e)| e.depth()).max().unwrap_or(0),
            Self::Conditional(a, b, c) => a.depth().max(b.depth()).max(c.depth()),
            Self::Call(c, args, _) => c
                .depth()
                .max(args.iter().map(Self::depth).max().unwrap_or(0)),
            Self::NewError(_, args, _)
            | Self::NewClass(_, args, _)
            | Self::NewValue(_, args, _)
            | Self::NewBuiltin(_, args, _)
            | Self::NewVector(_, args, _) => args.iter().map(Self::depth).max().unwrap_or(0),
        }
    }
    pub(crate) fn assignable(&self) -> bool {
        match self {
            // Writes to globals are checked after lexical scope resolution.
            Self::Name(name) => name != "this",
            Self::Member(_, _)
            | Self::Index(_, _)
            | Self::XmlWithAttr { .. }
            | Self::XmlSelect { .. } => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyTrigger {
    pub(crate) clip_event: bool,
    pub(crate) event: &'static str,
    pub(crate) key_press: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    Located(usize, Box<Stmt>),
    Empty,
    Expr(Expr),
    DefaultXmlNamespace(Expr),
    Var(Vec<(String, Option<String>, Option<Expr>)>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    With(Expr, Box<Stmt>, usize),
    TellTarget(Expr, Box<Stmt>, usize),
    IfFrameLoaded(Vec<Expr>, Box<Stmt>),
    InitClip(Option<u32>, Vec<Stmt>, usize),
    LegacyEvent(Vec<LegacyTrigger>, Box<Function>),
    For(Box<Stmt>, Option<Expr>, Option<Expr>, Box<Stmt>),
    ForIn(Box<Stmt>, Expr, bool, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Switch(Expr, Vec<Case>),
    Label(String, Box<Stmt>),
    Break(Option<String>),
    Continue(Option<String>),
    Throw(Expr),
    Try(Vec<Stmt>, Vec<CatchClause>, Option<Vec<Stmt>>),
    Function(Box<Function>),
    Return(Option<Expr>, usize),
}

impl Stmt {
    pub(crate) fn unlocated(&self) -> &Self {
        match self {
            Self::Located(_, statement) => statement.unlocated(),
            _ => self,
        }
    }
}
