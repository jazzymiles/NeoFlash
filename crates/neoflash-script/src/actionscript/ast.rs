//! ActionScript AST nodes and structural predicates.

#[derive(Clone, Debug)]
pub(super) enum Expr {
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
pub(super) struct PropertyKey {
    /// Preserve the original quoting/escaping when emitting JavaScript.
    pub(super) source: String,
    /// Match member access using the decoded property name.
    pub(super) name: String,
}

#[derive(Clone, Debug)]
pub(super) struct Function {
    pub(super) id: usize,
    pub(super) offset: usize,
    pub(super) name: Option<String>,
    pub(super) params: Vec<(String, Option<String>)>,
    pub(super) defaults: Vec<Option<Expr>>,
    pub(super) rest: Option<String>,
    pub(super) returns: Option<String>,
    pub(super) body: Vec<Stmt>,
    pub(super) declaration: bool,
}

pub(super) fn constant_default(expr: &Expr) -> bool {
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
pub(super) struct CatchClause {
    pub(super) id: usize,
    pub(super) offset: usize,
    pub(super) name: String,
    pub(super) ty: Option<String>,
    pub(super) body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub(super) struct Case {
    pub(super) test: Option<Expr>,
    pub(super) body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub(super) struct ControlTarget {
    pub(super) label: Option<String>,
    pub(super) iteration: bool,
    pub(super) breakable: bool,
}

impl Expr {
    pub(super) fn depth(&self) -> usize {
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
    pub(super) fn assignable(&self) -> bool {
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
pub(super) struct LegacyTrigger {
    pub(super) clip_event: bool,
    pub(super) event: &'static str,
    pub(super) key_press: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) enum Stmt {
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
    pub(super) fn unlocated(&self) -> &Self {
        match self {
            Self::Located(_, statement) => statement.unlocated(),
            _ => self,
        }
    }
}
