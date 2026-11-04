//! A tokenizing/recursive-descent + Pratt frontend, not a text substitution pass.
pub(crate) mod analysis;
mod includes;

use crate::Diagnostic;
use neoflash_core::{Script, ScriptLanguage};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
mod completion;
#[cfg(test)]
mod control_tests;
pub(crate) mod diagnostics;
pub(crate) mod project;
mod scope;
mod xml_literals;

#[derive(Clone, Default)]
struct CompileContext {
    legacy_case: bool,
    mapping_prefix: Option<String>,
    types: std::collections::BTreeMap<String, String>,
    intrinsic_types: std::collections::BTreeSet<String>,
    functions: std::collections::BTreeMap<String, String>,
    interfaces: std::collections::BTreeSet<String>,
    namespaces: std::collections::BTreeMap<String, String>,
    open_namespaces: Vec<String>,
    namespace_members: std::collections::BTreeSet<(String, String)>,
    members: std::collections::BTreeMap<String, Option<String>>,
    static_members: std::collections::BTreeMap<String, String>,
    as2_private: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    static_method: bool,
    class_name: Option<String>,
    method_id: Option<usize>,
    project: bool,
}

impl CompileContext {
    fn intrinsic_type(&self, name: &str, as3: bool) -> bool {
        (matches!(name, "Date" | "XML")
            || (as3
                && matches!(
                    name,
                    "RegExp" | "JSON" | "Vector" | "XMLList" | "Namespace" | "QName"
                ))
            || (!as3 && name == "XMLNode"))
            && (!self.project || self.intrinsic_types.contains(name))
    }
    fn core_type(&self, name: &str) -> bool {
        (matches!(
            name,
            "Number"
                | "String"
                | "Boolean"
                | "int"
                | "uint"
                | "Function"
                | "Class"
                | "Object"
                | "Array"
        ) || is_error_type(name))
            && self.types.get(name).is_none_or(|resolved| resolved == name)
    }
    fn utility_function(&self, name: &str, as3: bool) -> bool {
        ((as3
            && matches!(
                name,
                "encodeURI"
                    | "encodeURIComponent"
                    | "decodeURI"
                    | "decodeURIComponent"
                    | "escape"
                    | "unescape"
                    | "isXMLName"
            ))
            || (!as3
                && matches!(
                    name,
                    "random"
                        | "escape"
                        | "unescape"
                        | "chr"
                        | "ord"
                        | "length"
                        | "substring"
                        | "mbchr"
                        | "mbord"
                        | "mblength"
                        | "mbsubstring"
                )))
            && !self.types.contains_key(name)
            && !self.functions.contains_key(name)
    }
    fn numeric_conversion(&self, name: &str, as3: bool) -> bool {
        (name == "int" || (as3 && name == "uint")) && !self.types.contains_key(name)
    }
    fn canonical_type(&self, name: &str) -> String {
        if let Some(element) = vector_element(name) {
            format!("Vector.<{}>", self.canonical_type(element))
        } else {
            self.types.get(name).cloned().unwrap_or_else(|| name.into())
        }
    }
}

fn vector_element(name: &str) -> Option<&str> {
    name.strip_prefix("Vector.<")
        .and_then(|name| name.strip_suffix('>'))
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Ident(String),
    Number(String),
    String(String),
    Punct(&'static str),
    End,
    // Rich diagnostics must not enlarge every successful token on recursive parser frames.
    Invalid(Box<Diagnostic>),
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    offset: usize,
    end: usize,
}

struct Lexer<'a> {
    script: &'a Script,
    offset: usize,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.script.source[self.offset..]
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.rest().chars().next()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn error(&self, offset: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(self.script, offset, message)
    }
    fn next_token(&mut self) -> Result<Token, Diagnostic> {
        while !self.rest().is_empty() {
            if self.rest().starts_with(char::is_whitespace) {
                self.bump();
                continue;
            }
            if self.rest().starts_with("//") {
                while self
                    .bump()
                    .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                {
                }
                continue;
            }
            if self.rest().starts_with("/*") {
                let start = self.offset;
                self.offset += 2;
                let Some(end) = self.rest().find("*/") else {
                    return Err(self.error(start, "Unterminated block comment."));
                };
                self.offset += end + 2;
                continue;
            }
            let start = self.offset;
            let c = self.bump().unwrap();
            let kind = if c.is_alphabetic() || c == '_' || c == '$' {
                while self
                    .rest()
                    .starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
                {
                    self.bump();
                }
                Kind::Ident(self.script.source[start..self.offset].into())
            } else if c.is_ascii_digit()
                || (c == '.' && self.rest().starts_with(|c: char| c.is_ascii_digit()))
            {
                if c == '0' && (self.rest().starts_with('x') || self.rest().starts_with('X')) {
                    self.bump();
                    let digits = self.offset;
                    while self.rest().starts_with(|c: char| c.is_ascii_hexdigit()) {
                        self.bump();
                    }
                    if self.offset == digits {
                        return Err(self.error(start, "Expected hexadecimal digits after 0x."));
                    }
                } else {
                    while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                        self.bump();
                    }
                    if c != '.' && self.rest().starts_with('.') {
                        self.bump();
                        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                            self.bump();
                        }
                    }
                    if self.rest().starts_with(['e', 'E']) {
                        self.bump();
                        if self.rest().starts_with(['+', '-']) {
                            self.bump();
                        }
                        let digits = self.offset;
                        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                            self.bump();
                        }
                        if self.offset == digits {
                            return Err(self.error(start, "Expected digits in numeric exponent."));
                        }
                    }
                }
                if self
                    .rest()
                    .starts_with(|c: char| c.is_alphabetic() || c == '_')
                {
                    return Err(self.error(start, "Invalid number literal."));
                }
                let literal = &self.script.source[start..self.offset];
                if literal.len() > 1
                    && literal.starts_with('0')
                    && literal.as_bytes()[1].is_ascii_digit()
                {
                    return Err(self.error(start, "Legacy leading-zero numeric literals are not supported; use decimal or 0x hexadecimal."));
                }
                Kind::Number(literal.into())
            } else if c == '\'' || c == '"' {
                let mut closed = false;
                while let Some(next) = self.bump() {
                    if next == c {
                        closed = true;
                        break;
                    }
                    if matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                        return Err(self.error(start, "Unterminated string literal."));
                    }
                    if next == '\\' && self.bump().is_none() {
                        break;
                    }
                }
                if !closed {
                    return Err(self.error(start, "Unterminated string literal."));
                }
                Kind::String(self.script.source[start..self.offset].into())
            } else if c == '#' && self.rest().starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                while self.rest().starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                    self.bump();
                }
                match &self.script.source[start..self.offset] {
                    "#initclip" => Kind::Punct("#initclip"),
                    "#endinitclip" => Kind::Punct("#endinitclip"),
                    "#include" => {
                        return Err(self.error(
                            start,
                            "#include requires compile_with_program and bundled .as project resources.",
                        ));
                    }
                    directive => {
                        return Err(self.error(
                            start,
                            format!(
                                "Unsupported compiler directive '{directive}' in ActionScript."
                            ),
                        ));
                    }
                }
            } else {
                self.offset = start;
                const OPERATORS: &[&str] = &[
                    ">>>=", "===", "!==", ">>>", "...", "<<=", ">>=", "&&=", "||=", "++", "--",
                    "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
                    "&=", "|=", "^=", "<>", "::", "..", "{", "}", "(", ")", "[", "]", ";", ",",
                    ".", ":", "?", "+", "-", "*", "/", "%", "=", "!", "~", "<", ">", "&", "|", "^",
                    "@",
                ];
                let Some(operator) = OPERATORS.iter().find(|operator| {
                    self.rest().starts_with(**operator)
                        && (**operator != "<>"
                            || self.script.language == ScriptLanguage::ActionScript2)
                }) else {
                    return Err(self.error(
                        start,
                        format!("Unsupported character {c:?} in ActionScript."),
                    ));
                };
                self.offset += operator.len();
                Kind::Punct(operator)
            };
            return Ok(Token {
                kind,
                offset: start,
                end: self.offset,
            });
        }
        Ok(Token {
            kind: Kind::End,
            offset: self.offset,
            end: self.offset,
        })
    }
}

// Buffered on demand: the parser decides when '<' starts an XML literal.
// XML content is never lexed as ActionScript ahead of that decision.
#[derive(Clone, Default)]
struct TokenCursor {
    buffer: RefCell<Vec<Token>>,
    offset: Cell<usize>,
    produced: Cell<usize>,
}
impl TokenCursor {
    fn get(&self, script: &Script, index: usize) -> Option<Token> {
        let mut buffer = self.buffer.borrow_mut();
        while buffer.len() <= index {
            if buffer
                .last()
                .is_some_and(|t| matches!(t.kind, Kind::End | Kind::Invalid(_)))
            {
                return None;
            }
            let mut lexer = Lexer {
                script,
                offset: self.offset.get(),
            };
            let result = if self.produced.get() >= 100_000 {
                Err(Diagnostic::at(
                    script,
                    lexer.offset,
                    "Script exceeds the 100,000-token limit.",
                ))
            } else {
                lexer.next_token()
            };
            self.produced.set(self.produced.get() + 1);
            let token = result.unwrap_or_else(|error| Token {
                kind: Kind::Invalid(Box::new(error)),
                offset: lexer.offset,
                end: lexer.offset,
            });
            self.offset.set(lexer.offset);
            buffer.push(token);
        }
        buffer.get(index).cloned()
    }
    fn split(&self, index: usize, kind: Kind) {
        let mut buffer = self.buffer.borrow_mut();
        buffer[index].kind = kind;
        buffer[index].offset += 1;
    }
    fn jump(&self, index: usize, offset: usize) {
        self.buffer.borrow_mut().truncate(index);
        self.offset.set(offset);
    }
    fn literal(&self, index: usize, start: usize, end: usize) {
        self.jump(index, end);
        self.buffer.borrow_mut().push(Token {
            kind: Kind::String(String::new()),
            offset: start,
            end,
        });
    }
}

#[derive(Clone, Debug)]
enum Expr {
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
struct PropertyKey {
    /// Preserve the original quoting/escaping when emitting JavaScript.
    source: String,
    /// Match member access using the decoded property name.
    name: String,
}

#[derive(Clone, Debug)]
struct Function {
    id: usize,
    offset: usize,
    name: Option<String>,
    params: Vec<(String, Option<String>)>,
    defaults: Vec<Option<Expr>>,
    rest: Option<String>,
    returns: Option<String>,
    body: Vec<Stmt>,
    declaration: bool,
}

fn constant_default(expr: &Expr) -> bool {
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
struct CatchClause {
    id: usize,
    offset: usize,
    name: String,
    ty: Option<String>,
    body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
struct Case {
    test: Option<Expr>,
    body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
struct ControlTarget {
    label: Option<String>,
    iteration: bool,
    breakable: bool,
}

impl Expr {
    fn depth(&self) -> usize {
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
            Self::XmlWithAttr { local, namespace } => local
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
    fn assignable(&self) -> bool {
        match self {
            // Writes to globals are checked after lexical scope resolution.
            Self::Name(name) => name != "this",
            Self::Member(_, _) | Self::Index(_, _) | Self::XmlWithAttr { .. } => true,
            Self::XmlSelect { descendant, .. } => !descendant,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LegacyTrigger {
    clip_event: bool,
    event: &'static str,
    key_press: Option<String>,
}

#[derive(Clone, Debug)]
enum Stmt {
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
    fn unlocated(&self) -> &Self {
        match self {
            Self::Located(_, statement) => statement.unlocated(),
            _ => self,
        }
    }
}

struct Frontend<'a> {
    script: &'a Script,
    tokens: TokenCursor,
    pos: usize,
    depth: usize,
    controls: Vec<ControlTarget>,
    function_depth: usize,
    next_function: usize,
    context: CompileContext,
    no_in: bool,
    with_depth: usize,
    in_initclip: bool,
}

impl<'a> Frontend<'a> {
    fn peek(&self, index: usize) -> Option<Token> {
        self.tokens.get(self.script, index)
    }
    fn token(&self) -> Token {
        self.peek(self.pos).expect("parser does not consume EOF")
    }
    fn is(&self, text: &str) -> bool {
        match &self.token().kind {
            Kind::Ident(s) => s == text,
            Kind::Punct(s) => *s == text,
            _ => false,
        }
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.is(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> Result<(), Diagnostic> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{text}'.")))
        }
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        let token = self.token();
        if let Kind::Invalid(error) = token.kind {
            *error
        } else {
            Diagnostic::at(self.script, token.offset, message)
        }
    }
    fn ident(&mut self) -> Result<String, Diagnostic> {
        if let Kind::Ident(name) = &self.token().kind {
            let name = name.clone();
            self.pos += 1;
            Ok(name)
        } else {
            Err(self.error("Expected an identifier."))
        }
    }
    fn enter(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > 48 {
            return Err(self.error("ActionScript nesting exceeds 48 levels."));
        }
        Ok(())
    }
    fn semicolon(&mut self) -> Result<(), Diagnostic> {
        if self.eat(";") || self.is("}") || self.token().kind == Kind::End {
            return Ok(());
        }
        if self.line_break_before_token() {
            return Ok(());
        }
        Err(self.error("Expected ';' or a line break after the statement."))
    }
    fn line_break_before_token(&self) -> bool {
        self.pos > 0
            && self.script.source[self.peek(self.pos - 1).unwrap().end..self.token().offset]
                .contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    }
    fn statements(&mut self, block: bool) -> Result<Vec<Stmt>, Diagnostic> {
        let mut statements = Vec::new();
        while self.token().kind != Kind::End && !(block && self.is("}")) {
            statements.push(self.statement()?);
        }
        if block {
            self.expect("}")?;
        }
        Ok(statements)
    }
    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        self.enter()?;
        let offset = self.token().offset;
        let statement = self.statement_inner()?;
        self.depth -= 1;
        Ok(Stmt::Located(offset, Box::new(statement)))
    }
    #[inline(never)]
    fn parse_if(&mut self) -> Result<Stmt, Diagnostic> {
        let mut branches = Vec::new();
        let mut tail = None;
        let mut close_braces = 0usize;
        loop {
            if branches.len() >= 1024 {
                return Err(self.error("ActionScript if/else-if chain exceeds 1024 branches."));
            }
            let offset = self.peek(self.pos - 1).unwrap().offset;
            self.expect("(")?;
            let condition = self.sequence_expression()?;
            self.expect(")")?;
            let yes = Box::new(self.statement()?);
            branches.push((offset, condition, yes));
            if self.eat("else") {
                if self.eat("if") {
                    continue;
                }
                let saved_pos = self.pos;
                if self.eat("{") {
                    if self.eat("if") {
                        close_braces += 1;
                        continue;
                    }
                    self.pos = saved_pos;
                }
                tail = Some(Box::new(self.statement()?));
            }
            break;
        }
        for _ in 0..close_braces {
            self.expect("}")?;
        }
        let mut current = tail;
        for (offset, condition, yes) in branches.into_iter().rev() {
            current = Some(Box::new(Stmt::Located(
                offset,
                Box::new(Stmt::If(condition, yes, current)),
            )));
        }
        Ok(*current.unwrap())
    }
    #[inline(never)]
    fn parse_initclip(&mut self) -> Result<Stmt, Diagnostic> {
        let offset = self.token().offset;
        if self.script.language != ScriptLanguage::ActionScript2 {
            return Err(self.error("#initclip is supported only in ActionScript 2."));
        }
        if self.function_depth != 0
            || self.context.class_name.is_some()
            || self.depth != 1
            || self.with_depth != 0
            || self.in_initclip
        {
            return Err(
                self.error("#initclip is allowed only at the top level of an AS2 frame script.")
            );
        }
        self.expect("#initclip")?;
        let order = if !self.line_break_before_token()
            && let Kind::Number(raw) = &self.token().kind
        {
            let raw = raw.clone();
            let order = raw.parse::<u32>().map_err(|_| {
                self.error("#initclip order requires a non-negative 32-bit integer.")
            })?;
            self.pos += 1;
            Some(order)
        } else {
            None
        };
        if !self.eat(";") && !self.is("#endinitclip") && !self.line_break_before_token() {
            return Err(self.error("Expected a line break or ';' after #initclip."));
        }
        self.in_initclip = true;
        let mut body = Vec::new();
        while self.token().kind != Kind::End && !self.is("#endinitclip") {
            body.push(self.statement()?);
        }
        self.in_initclip = false;
        if !self.eat("#endinitclip") {
            return Err(Diagnostic::at(
                self.script,
                offset,
                "Unterminated #initclip block; expected #endinitclip.",
            ));
        }
        let _ = self.eat(";");
        Ok(Stmt::InitClip(order, body, offset))
    }
    #[inline(never)]
    fn parse_tell_target(&mut self) -> Result<Stmt, Diagnostic> {
        let offset = self.token().offset;
        self.expect("tellTarget")?;
        self.expect("(")?;
        let target = self.expression(0)?;
        self.expect(")")?;
        self.with_depth += 1;
        let body = self.statement()?;
        self.with_depth -= 1;
        if !scope::tell_target_body_supported(&body) {
            return Err(self.error("Function definitions and legacy event blocks inside tellTarget require captured scope-chain semantics outside this profile."));
        }
        Ok(Stmt::TellTarget(target, Box::new(body), offset))
    }
    #[inline(never)]
    fn parse_if_frame_loaded(&mut self) -> Result<Stmt, Diagnostic> {
        self.expect("ifFrameLoaded")?;
        self.expect("(")?;
        if self.is(")") {
            return Err(self.error("ifFrameLoaded requires (frame) or (scene, frame)."));
        }
        let mut args = vec![self.expression(0)?];
        if self.eat(",") {
            args.push(self.expression(0)?);
        }
        if self.eat(",") {
            return Err(self.error("ifFrameLoaded accepts (frame) or (scene, frame)."));
        }
        self.expect(")")?;
        let body = self.statement()?;
        Ok(Stmt::IfFrameLoaded(args, Box::new(body)))
    }
    #[inline(never)]
    fn parse_as3_directive(&mut self) -> Result<Option<Stmt>, Diagnostic> {
        if self.is("default")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "xml"))
            && self
                .peek(self.pos + 2)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            self.expect("default")?;
            self.expect("xml")?;
            self.expect("namespace")?;
            self.expect("=")?;
            let expr = self.expression(0)?;
            self.semicolon()?;
            return Ok(Some(Stmt::DefaultXmlNamespace(expr)));
        }
        if self.is("use")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            self.expect("use")?;
            self.expect("namespace")?;
            loop {
                let ns = self.ident()?;
                let uri = self
                    .context
                    .namespaces
                    .get(&ns)
                    .cloned()
                    .unwrap_or_else(|| ns.clone());
                if !self.context.open_namespaces.contains(&uri) {
                    self.context.open_namespaces.push(uri);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.semicolon()?;
            return Ok(Some(Stmt::Empty));
        }
        if self.is("namespace")
            || ((self.is("public") || self.is("internal"))
                && self
                    .peek(self.pos + 1)
                    .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace")))
        {
            let _ = self.eat("public") || self.eat("internal");
            let offset = self.token().offset;
            self.expect("namespace")?;
            let name = self.ident()?;
            let uri = if self.eat("=") {
                let expr = self.expression(0)?;
                let Some(s) = literal_property_name(&expr) else {
                    return Err(self.error("namespace URI requires a string literal."));
                };
                s
            } else {
                format!("neoflash://namespace/{name}")
            };
            self.semicolon()?;
            self.context.namespaces.insert(name.clone(), uri.clone());
            let uri_lit = serde_json::to_string(&uri).unwrap();
            let init = Expr::Call(
                Box::new(Expr::Name("Namespace".into())),
                vec![Expr::Literal(uri_lit)],
                offset,
            );
            return Ok(Some(Stmt::Var(vec![(name, None, Some(init))])));
        }
        {
            let mut look = self.pos;
            let mut saw_native = false;
            while let Some(t) = self.peek(look) {
                if let Kind::Ident(s) = &t.kind {
                    if s == "native" {
                        saw_native = true;
                        look += 1;
                    } else if matches!(
                        s.as_str(),
                        "public" | "internal" | "private" | "protected" | "static" | "final"
                    ) {
                        look += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if saw_native
                && self
                    .peek(look)
                    .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "function"))
            {
                self.pos = look;
                let (func, _) = self.project_function(true, false, false, true)?;
                return Ok(Some(Stmt::Function(Box::new(func))));
            }
        }
        if self.is("include")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::String(_)))
        {
            return Err(self.error(
                "include requires compile_with_program and bundled .as project resources.",
            ));
        }
        Ok(None)
    }
    fn statement_inner(&mut self) -> Result<Stmt, Diagnostic> {
        if self.is("#endinitclip") {
            return Err(self.error("Unexpected #endinitclip without a matching #initclip."));
        }
        if self.is("#initclip") {
            return self.parse_initclip();
        }
        if self.script.language == ScriptLanguage::ActionScript2
            && self.is("tellTarget")
            && matches!(
                self.peek(self.pos + 1).map(|t| t.kind),
                Some(Kind::Punct("("))
            )
        {
            return self.parse_tell_target();
        }
        if self.script.language == ScriptLanguage::ActionScript2
            && self.is("ifFrameLoaded")
            && matches!(
                self.peek(self.pos + 1).map(|t| t.kind),
                Some(Kind::Punct("("))
            )
        {
            return self.parse_if_frame_loaded();
        }
        if self.is("with") {
            let offset = self.token().offset;
            self.expect("with")?;
            self.expect("(")?;
            let target = self.expression(0)?;
            self.expect(")")?;
            self.with_depth += 1;
            let body = self.statement()?;
            self.with_depth -= 1;
            if !scope::with_body_supported(&body) {
                return Err(self.error("Function definitions and eval/set calls inside with require captured/dynamic scope-chain semantics outside this profile. Define callbacks and resolve named paths outside the with block."));
            }
            return Ok(Stmt::With(target, Box::new(body), offset));
        }
        if self.script.language == ScriptLanguage::ActionScript3
            && let Some(stmt) = self.parse_as3_directive()?
        {
            return Ok(stmt);
        }
        if self.is("on") || self.is("onClipEvent") {
            if self.script.language != ScriptLanguage::ActionScript2
                || self.function_depth != 0
                || self.context.class_name.is_some()
                || self.in_initclip
            {
                return Err(self.error(
                    "Legacy on/onClipEvent blocks belong at the top level of an AS1/AS2 object script.",
                ));
            }
            let clip_event = self.eat("onClipEvent");
            if !clip_event {
                self.expect("on")?;
            }
            let offset = self.token().offset;
            self.expect("(")?;
            let mut triggers = Vec::new();
            loop {
                let name = self.ident()?;
                let Some(event) =
                    resolve_legacy_event_name(clip_event, &name, self.context.legacy_case)
                else {
                    return Err(self.error(format!(
                        "Legacy event '{name}' is not a supported {} event.",
                        if clip_event { "onClipEvent" } else { "on" }
                    )));
                };
                let key_press = if !clip_event && event == "keyPress" {
                    let Kind::String(raw) = self.token().kind.clone() else {
                        return Err(self.error(
                            "Legacy on(keyPress \"<key>\") requires a key string literal immediately after keyPress without a comma (for example on(keyPress \"<Enter>\") or on(keyPress \"a\")).",
                        ));
                    };
                    self.pos += 1;
                    let Some(decoded) = literal_property_name(&Expr::Literal(raw)) else {
                        return Err(self.error("Invalid string literal in on(keyPress)."));
                    };
                    let Some(valid_key) =
                        validate_flash_key_press(&decoded, self.context.legacy_case)
                    else {
                        return Err(self.error(format!(
                            "Invalid on(keyPress) key specifier {decoded:?}; expected a single character or a Flash key token (<Left>, <Right>, <Up>, <Down>, <Enter>, <Space>, <Tab>, <Escape>, <Backspace>, <Delete>, <Home>, <End>, <PageUp>, <PageDown>, <Insert>)."
                        )));
                    };
                    Some(valid_key)
                } else {
                    None
                };
                let trigger = LegacyTrigger {
                    clip_event,
                    event,
                    key_press,
                };
                if !triggers.contains(&trigger) {
                    triggers.push(trigger);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
            self.expect("{")?;
            let id = self.next_function;
            self.next_function += 1;
            let outer_controls = std::mem::take(&mut self.controls);
            let outer_no_in = self.no_in;
            self.no_in = false;
            self.function_depth += 1;
            let body = self.statements(true)?;
            self.function_depth -= 1;
            self.controls = outer_controls;
            self.no_in = outer_no_in;
            let func = Function {
                id,
                offset,
                name: None,
                params: Vec::new(),
                defaults: Vec::new(),
                rest: None,
                returns: None,
                body,
                declaration: false,
            };
            return Ok(Stmt::LegacyEvent(triggers, Box::new(func)));
        }
        if matches!(self.token().kind, Kind::Ident(_))
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| t.kind == Kind::Punct(":"))
        {
            if self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error(
                    "Labels require ActionScript 3; AS2 label semantics are not implemented.",
                ));
            }
            let name = self.ident()?;
            if invalid_binding(&name, true)
                || self
                    .controls
                    .iter()
                    .any(|c| c.label.as_ref() == Some(&name))
            {
                return Err(self.error("Invalid or duplicate active label."));
            }
            self.expect(":")?;
            let mut at = self.pos;
            while matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(_)))
                && self
                    .peek(at + 1)
                    .is_some_and(|t| t.kind == Kind::Punct(":"))
            {
                at += 2;
            }
            let iteration = matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(s)) if matches!(s.as_str(), "for" | "while" | "do"));
            self.controls.push(ControlTarget {
                label: Some(name.clone()),
                iteration,
                breakable: false,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::Label(name, Box::new(body)));
        }
        if self.eat("throw") {
            if self.line_break_before_token()
                || self.is(";")
                || self.is("}")
                || self.token().kind == Kind::End
            {
                return Err(self.error("throw requires an expression on the same line."));
            }
            let value = self.sequence_expression()?;
            self.semicolon()?;
            return Ok(Stmt::Throw(value));
        }
        if self.eat("try") {
            self.expect("{")?;
            let body = self.statements(true)?;
            let mut catches = Vec::new();
            let mut catch_all = false;
            while self.eat("catch") {
                let offset = self.token().offset;
                if catch_all {
                    return Err(self.error("A catch-all clause must be last."));
                }
                self.expect("(")?;
                let name = self.ident()?;
                if invalid_binding(&name, true) {
                    return Err(self.error("Invalid catch parameter name."));
                }
                let ty = if self.eat(":") {
                    let ty = if self.eat("*") {
                        "*".into()
                    } else {
                        let mut name = self.ident()?;
                        while self.script.language == ScriptLanguage::ActionScript3 && self.eat(".")
                        {
                            name.push('.');
                            name.push_str(&self.ident()?);
                        }
                        name
                    };
                    let supported = if self.script.language == ScriptLanguage::ActionScript3 {
                        is_error_type(&ty)
                            || matches!(
                                ty.as_str(),
                                "*" | "Object"
                                    | "Number"
                                    | "String"
                                    | "Boolean"
                                    | "Function"
                                    | "int"
                                    | "uint"
                            )
                            || (self.context.project && self.context.types.contains_key(&ty))
                    } else {
                        ty == "Error"
                    };
                    if !supported {
                        return Err(self.error("Unsupported catch type. AS2 currently accepts Error or an untyped catch; AS3 accepts core error and primitive types, Object, Function or *. Custom classes require project type resolution."));
                    }
                    Some(ty)
                } else {
                    None
                };
                catch_all = ty.as_deref().is_none_or(|t| t == "*");
                self.expect(")")?;
                self.expect("{")?;
                let id = self.next_function;
                self.next_function += 1;
                catches.push(CatchClause {
                    id,
                    offset,
                    name,
                    ty,
                    body: self.statements(true)?,
                });
            }
            let finally = if self.eat("finally") {
                self.expect("{")?;
                Some(self.statements(true)?)
            } else {
                None
            };
            if catches.is_empty() && finally.is_none() {
                return Err(self.error("try requires catch or finally."));
            }
            return Ok(Stmt::Try(body, catches, finally));
        }
        if self.eat("switch") {
            self.expect("(")?;
            let value = self.sequence_expression()?;
            self.expect(")")?;
            self.expect("{")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: false,
                breakable: true,
            });
            let mut cases = Vec::new();
            let mut default = false;
            while !self.eat("}") {
                let test = if self.eat("case") {
                    Some(self.sequence_expression()?)
                } else if self.eat("default") {
                    if default {
                        return Err(self.error("A switch cannot have multiple default clauses."));
                    }
                    default = true;
                    None
                } else {
                    return Err(self.error("Expected case or default in switch."));
                };
                self.expect(":")?;
                let mut body = Vec::new();
                while !self.is("case")
                    && !self.is("default")
                    && !self.is("}")
                    && self.token().kind != Kind::End
                {
                    body.push(self.statement()?);
                }
                cases.push(Case { test, body });
            }
            self.controls.pop();
            return Ok(Stmt::Switch(value, cases));
        }
        if self.eat("do") {
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            self.expect("while")?;
            self.expect("(")?;
            let test = self.sequence_expression()?;
            self.expect(")")?;
            self.eat(";");
            return Ok(Stmt::DoWhile(Box::new(body), test));
        }
        if self.is("function") {
            return Ok(Stmt::Function(Box::new(self.function(true)?)));
        }
        if self.is("return") {
            let offset = self.token().offset;
            if self.function_depth == 0 {
                return Err(self.error("ActionScript return requires a function body."));
            }
            self.pos += 1;
            let value = if self.is(";")
                || self.is("}")
                || self.token().kind == Kind::End
                || self.script.source[offset..self.token().offset]
                    .contains(['\n', '\r', '\u{2028}', '\u{2029}'])
            {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.semicolon()?;
            return Ok(Stmt::Return(value, offset));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if self.eat("{") {
            return Ok(Stmt::Block(self.statements(true)?));
        }
        if self.eat("var") {
            let value = self.variables()?;
            self.semicolon()?;
            return Ok(value);
        }
        if self.eat("if") {
            return self.parse_if();
        }
        if self.eat("while") {
            self.expect("(")?;
            let condition = self.sequence_expression()?;
            self.expect(")")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::While(condition, Box::new(body)));
        }
        if self.eat("for") {
            let values = self.eat("each");
            if values && self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error(
                    "for each..in is an AS3 collection statement; use for..in keys in AS2.",
                ));
            }
            self.expect("(")?;
            let previous_no_in = self.no_in;
            self.no_in = true;
            let init = if self.is(";") {
                Stmt::Empty
            } else if self.eat("var") {
                self.variables()?
            } else {
                Stmt::Expr(self.sequence_expression()?)
            };
            self.no_in = previous_no_in;
            if self.eat("in") {
                let valid = match &init {
                    Stmt::Var(vars) => vars.len() == 1 && vars[0].2.is_none(),
                    Stmt::Expr(value) => value.assignable(),
                    _ => false,
                };
                if !valid {
                    return Err(self.error("for..in requires one uninitialized variable or assignable property target."));
                }
                let collection = self.sequence_expression()?;
                self.expect(")")?;
                self.controls.push(ControlTarget {
                    label: None,
                    iteration: true,
                    breakable: true,
                });
                let body = self.statement()?;
                self.controls.pop();
                return Ok(Stmt::ForIn(
                    Box::new(init),
                    collection,
                    values,
                    Box::new(body),
                ));
            }
            if values {
                return Err(self.error("for each requires an 'in' collection."));
            }
            self.expect(";")?;
            let condition = if self.is(";") {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.expect(";")?;
            let update = if self.is(")") {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.expect(")")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::For(Box::new(init), condition, update, Box::new(body)));
        }
        if self.is("break") || self.is("continue") {
            let is_break = self.eat("break");
            if !is_break {
                self.expect("continue")?;
            }
            let label =
                if !self.line_break_before_token() && matches!(self.token().kind, Kind::Ident(_)) {
                    Some(self.ident()?)
                } else {
                    None
                };
            if label.is_some() && self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error("Labelled jumps require ActionScript 3."));
            }
            if !self.controls.iter().rev().any(|target| match &label {
                Some(name) => target.label.as_ref() == Some(name) && (is_break || target.iteration),
                None => {
                    if is_break {
                        target.breakable
                    } else {
                        target.iteration && target.label.is_none()
                    }
                }
            }) {
                return Err(self.error("Jump has no valid enclosing loop, switch or label target."));
            }
            self.semicolon()?;
            return Ok(if is_break {
                Stmt::Break(label)
            } else {
                Stmt::Continue(label)
            });
        }
        if let Kind::Ident(word) = &self.token().kind
            && self.context.legacy_case
            && matches!(
                word.as_str(),
                "class" | "interface" | "extends" | "implements" | "package" | "import"
            )
        {
            return Err(self.error(format!(
                "ActionScript 1.0 does not support '{word}' declarations; use ActionScript 2.0/3.0 for classes and interfaces, or prototype functions in AS1."
            )));
        }
        if let Kind::Ident(word) = &self.token().kind
            && is_unsupported_keyword(word)
            && !(word == "set" && self.script.language == ScriptLanguage::ActionScript2)
            && !matches!(word.as_str(), "new" | "delete" | "void")
            && !(((self.context.project && self.context.class_name.is_some())
                || self.context.legacy_case)
                && word == "super")
        {
            return Err(self.error(format!("ActionScript '{word}' is not implemented yet. See docs/COMPATIBILITY.md; use a JavaScript/TypeScript handler for this feature.")));
        }
        let expression = self.sequence_expression()?;
        self.semicolon()?;
        Ok(Stmt::Expr(expression))
    }
    fn variables(&mut self) -> Result<Stmt, Diagnostic> {
        let mut variables = Vec::new();
        loop {
            let offset = self.token().offset;
            let name = self.ident()?;
            if invalid_binding(&name, self.function_depth > 0) {
                return Err(Diagnostic::at(
                    self.script,
                    offset,
                    format!("'{name}' cannot be declared in the current ActionScript subset."),
                ));
            }
            let annotation = self.annotation(false)?;
            let value = if self.eat("=") {
                Some(self.expression(0)?)
            } else {
                None
            };
            variables.push((name, annotation, value));
            if !self.eat(",") {
                break;
            }
        }
        Ok(Stmt::Var(variables))
    }
    fn annotation(&mut self, returns: bool) -> Result<Option<String>, Diagnostic> {
        if !self.eat(":") {
            return Ok(None);
        }
        let start = self.token().offset;
        let name = self.type_name()?;
        if !self.supported_type(&name, returns) {
            let end = self
                .peek(self.pos.saturating_sub(1))
                .map_or(start, |token| token.end);
            let mut error = Diagnostic::at_span(
                self.script,
                (start, end),
                format!(
                    "Type '{name}' is not supported in this ActionScript annotation. Use a supported primitive, resolved project type or AS3 Vector.<T>; void/Void is return-only."
                ),
            );
            error.code = Some("1046".into());
            return Err(error);
        }
        Ok(Some(name))
    }
    fn type_name(&mut self) -> Result<String, Diagnostic> {
        self.enter()?;
        let mut name = if self.is("*=") {
            // In an annotation, `value:*=...` is wildcard plus initializer,
            // not multiplication assignment. Leave '=' for its caller.
            self.tokens.split(self.pos, Kind::Punct("="));
            "*".into()
        } else if self.eat("*") {
            "*".into()
        } else {
            self.ident()?
        };
        while self.is(".")
            && (self.context.project
                || self.context.legacy_case
                || (self.script.language == ScriptLanguage::ActionScript2 && name == "TextField")
                || name == "flash"
                || name.starts_with("flash.")
                || matches!(
                    self.peek(self.pos + 1).map(|t| t.kind),
                    Some(Kind::Punct("<"))
                ))
        {
            self.pos += 1;
            if self.eat("<") {
                if name != "Vector"
                    || !self.context.intrinsic_type(
                        "Vector",
                        self.script.language == ScriptLanguage::ActionScript3,
                    )
                {
                    return Err(self.error("Only the unshadowed AS3 Vector.<T> native type supports type parameters in this profile."));
                }
                let element = self.type_name()?;
                if !self.supported_type(&element, false) {
                    return Err(self.error(format!("Unsupported Vector element type '{element}'.")));
                }
                self.type_close()?;
                name = format!("Vector.<{element}>");
                break;
            }
            name.push('.');
            name.push_str(&self.ident()?);
        }
        self.depth -= 1;
        Ok(name)
    }
    fn type_close(&mut self) -> Result<(), Diagnostic> {
        if self.eat(">") {
            return Ok(());
        }
        // Split only while consuming a generic closer; expression shifts and
        // >= remain untouched outside this type grammar.
        let rest = match self.token().kind {
            Kind::Punct(">>") => ">",
            Kind::Punct(">>>") => ">>",
            Kind::Punct(">=") => "=",
            Kind::Punct(">>=") => ">=",
            Kind::Punct(">>>=") => ">>=",
            _ => return Err(self.error("Expected '>' after the Vector element type.")),
        };
        self.tokens.split(self.pos, Kind::Punct(rest));
        Ok(())
    }
    fn supported_type(&self, name: &str, returns: bool) -> bool {
        let as3 = self.script.language == ScriptLanguage::ActionScript3;
        if let Some(element) = vector_element(name) {
            return as3
                && self.context.intrinsic_type("Vector", true)
                && self.supported_type(element, false);
        }
        let supported = matches!(
            name,
            "Number" | "String" | "Boolean" | "Function" | "Array" | "Object"
        ) || (as3 && matches!(name, "int" | "uint" | "Class" | "*"))
            || self.context.intrinsic_type(name, as3)
            || (!as3
                && matches!(
                    name,
                    "MovieClip"
                        | "Button"
                        | "TextField"
                        | "TextField.StyleSheet"
                        | "TextSnapshot"
                        | "LoadVars"
                        | "Color"
                        | "TextFormat"
                        | "Sound"
                        | "MovieClipLoader"
                        | "SharedObject"
                        | "Camera"
                        | "Microphone"
                        | "ContextMenu"
                        | "ContextMenuItem"
                        | "PrintJob"
                        | "LocalConnection"
                        | "XMLSocket"
                        | "NetConnection"
                        | "NetStream"
                        | "Video"
                        | "FileReference"
                        | "FileReferenceList"
                        | "flash.net.FileReference"
                        | "flash.net.FileReferenceList"
                        | "TextRenderer"
                        | "flash.text.TextRenderer"
                ))
            || (as3 && !self.context.project && is_as3_wave7_type(name))
            || (self.context.project && self.context.types.contains_key(name))
            || (is_error_type(name) && (as3 || name == "Error"))
            || (returns && name == if as3 { "void" } else { "Void" });
        supported && !(matches!(name, "JSON" | "Vector") && self.context.intrinsic_type(name, as3))
    }
    fn function(&mut self, declaration: bool) -> Result<Function, Diagnostic> {
        self.project_function(declaration, false, false, false)
            .map(|(function, _)| function)
    }
    fn project_function(
        &mut self,
        declaration: bool,
        member: bool,
        signature_only: bool,
        allow_as2_signature_accessor: bool,
    ) -> Result<(Function, Option<String>), Diagnostic> {
        let offset = self.token().offset;
        self.expect("function")?;
        let accessor = if member
            && (self.is("get") || self.is("set"))
            && matches!(
                self.peek(self.pos + 1).map(|token| token.kind),
                Some(Kind::Ident(_))
            ) {
            let kind = self.ident()?;
            if signature_only
                && !allow_as2_signature_accessor
                && self.script.language == ScriptLanguage::ActionScript2
            {
                return Err(self.error("AS2 interface accessor signatures are outside this profile; declare get/set accessors in a concrete source class."));
            }
            Some(kind)
        } else {
            None
        };
        let name = if declaration || !self.is("(") {
            let name = self.ident()?;
            if invalid_binding(&name, !declaration || self.function_depth > 0) {
                return Err(self.error("Invalid or reserved function name."));
            }
            Some(name)
        } else {
            None
        };
        self.expect("(")?;
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut rest = None;
        if !self.is(")") {
            loop {
                if self.eat("...") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(self.error("Rest parameter syntax requires AS3. AS2 functions receive extra values through arguments."));
                    }
                    let name = self.ident()?;
                    if invalid_binding(&name, true) {
                        return Err(self.error("Invalid or reserved rest parameter name."));
                    }
                    let ty = self.annotation(false)?;
                    if ty.as_deref().is_some_and(|ty| ty != "Array") {
                        return Err(
                            self.error("A rest parameter may be unannotated or typed Array.")
                        );
                    }
                    if !self.is(")") {
                        return Err(self.error(
                            "The rest parameter must be last and cannot have a default value.",
                        ));
                    }
                    rest = Some(name);
                    break;
                }
                let name = self.ident()?;
                if invalid_binding(&name, true) {
                    return Err(self.error("Invalid or reserved parameter name."));
                }
                let ty = self.annotation(false)?;
                let default = if self.eat("=") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(self.error("Default parameter syntax requires AS3. AS2 functions can test omitted values explicitly."));
                    }
                    let value = self.expression(0)?;
                    if !constant_default(&value) {
                        return Err(self.error("Optional parameters currently require primitive literals, undefined/NaN/Infinity or signed numeric literals; computed/class constants are not supported."));
                    }
                    Some(value)
                } else {
                    if defaults.iter().any(Option::is_some) {
                        return Err(
                            self.error("Required parameters cannot follow optional parameters.")
                        );
                    }
                    None
                };
                params.push((name, ty));
                defaults.push(default);
                if params.len() > 256 {
                    return Err(
                        self.error("A source function supports at most 256 normal parameters.")
                    );
                }
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        let returns = self.annotation(true)?;
        let id = self.next_function;
        self.next_function += 1;
        let body = if signature_only {
            self.expect(";")?;
            Vec::new()
        } else if allow_as2_signature_accessor
            && self.script.language == ScriptLanguage::ActionScript3
            && self.eat(";")
        {
            let class_key =
                serde_json::to_string(&self.context.class_name.clone().unwrap_or_default())
                    .unwrap();
            let raw_name = name.clone().unwrap_or_default();
            let full_name = match accessor.as_deref() {
                Some("get") => format!("get:{raw_name}"),
                Some("set") => format!("set:{raw_name}"),
                _ => raw_name,
            };
            let fn_key = serde_json::to_string(&full_name).unwrap();
            let mut arg_parts: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
            if let Some(r) = &rest {
                arg_parts.push(format!("...{r}"));
            }
            let args_js = if arg_parts.is_empty() {
                "__nfActivation.values".to_owned()
            } else {
                format!("[{}]", arg_parts.join(", "))
            };
            let call_js = format!(
                "__nfInvokeNative(typeof __nfFrame !== 'undefined' ? __nfFrame : this, {class_key}, {fn_key}, {args_js})"
            );
            if returns.as_deref() == Some("void") {
                vec![Stmt::Expr(Expr::Literal(call_js))]
            } else {
                vec![Stmt::Return(Some(Expr::Literal(call_js)), offset)]
            }
        } else {
            self.expect("{")?;
            let outer_controls = std::mem::take(&mut self.controls);
            let outer_no_in = self.no_in;
            self.no_in = false;
            self.function_depth += 1;
            let body = self.statements(true)?;
            self.function_depth -= 1;
            self.controls = outer_controls;
            self.no_in = outer_no_in;
            body
        };
        Ok((
            Function {
                id,
                offset,
                name,
                params,
                defaults,
                rest,
                returns,
                body,
                declaration,
            },
            accessor,
        ))
    }
    fn xml_name(&mut self) -> Result<Expr, Diagnostic> {
        if self.eat("[") {
            let name = self.expression(0)?;
            self.expect("]")?;
            Ok(name)
        } else if self.eat("*") {
            Ok(Expr::Literal("\"*\"".into()))
        } else {
            Ok(Expr::Literal(format!("{:?}", self.ident()?)))
        }
    }
    fn xml_selector(&mut self, base: Expr, descendant: bool) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error("E4X selectors require AS3; use AS2 XMLNode DOM methods."));
        }
        let attribute = self.eat("@");
        let first = self.xml_name()?;
        let (local, namespace) = if self.eat("::") {
            let namespace=match &first {
                Expr::Literal(value) if value=="\"*\""=>Expr::Literal("null".into()),
                Expr::Literal(value)=>Expr::Name(serde_json::from_str(value).map_err(|_|self.error("Invalid namespace identifier."))?),
                _=>return Err(self.error("Computed namespace qualifiers are outside this selector profile; use an explicit QName value in brackets.")),
            };
            (self.xml_name()?, Some(Box::new(namespace)))
        } else {
            (first, None)
        };
        Ok(Expr::XmlSelect {
            base: Box::new(base),
            local: Box::new(local),
            namespace,
            attribute,
            descendant,
        })
    }
    fn xml_literal(&mut self) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error("XML literals require AS3; use the AS2 XML constructor."));
        }
        let start = self.token().offset;
        let position = self.pos;
        let script = self.script;
        let mut holes = Vec::new();
        let literal = xml_literals::scan(script, start, |at| {
            self.tokens.jump(self.pos, at);
            let outer_no_in = self.no_in;
            self.no_in = false;
            let value = self.expression(0)?;
            self.no_in = outer_no_in;
            let close = self.token();
            self.expect("}")?;
            holes.push(value);
            Ok(close.end)
        })?;
        let encoded = serde_json::to_string(&literal)
            .map_err(|_| self.error("XML literal template could not be encoded."))?;
        self.tokens.literal(position, start, literal.end);
        self.pos = position + 1;
        Ok(Expr::XmlLiteral(encoded, holes))
    }
    fn regexp_literal(&mut self) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error(
                "Regular expression literals require AS3; AS2 has no native RegExp family.",
            ));
        }
        let start = self.token().offset;
        let source = &self.script.source;
        let mut escaped = false;
        let mut in_class = false;
        let mut close = None;
        for (offset, ch) in source[start + 1..].char_indices() {
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                return Err(self.error(
                    "Unterminated regular expression literal; escape line breaks in the pattern.",
                ));
            }
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '[' => in_class = true,
                ']' => in_class = false,
                '/' if !in_class => {
                    close = Some(start + 1 + offset);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            return Err(
                self.error("Unterminated regular expression literal; expected a closing '/'.")
            );
        };
        let mut end = close + 1;
        while source[end..].starts_with(|ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$') {
            end += source[end..].chars().next().unwrap().len_utf8();
        }
        let flags = &source[close + 1..end];
        if flags
            .chars()
            .any(|ch| !matches!(ch, 'g' | 'i' | 'm' | 's' | 'x'))
        {
            return Err(Diagnostic::at(
                self.script,
                close + 1,
                "Unsupported RegExp literal flag; this profile supports g, i, m, s and x.",
            ));
        }
        let pattern = serde_json::to_string(&source[start + 1..close]).unwrap();
        let flags = serde_json::to_string(flags).unwrap();
        self.tokens.literal(self.pos, start, end);
        self.pos += 1;
        Ok(Expr::RegExpLiteral(pattern, flags))
    }
    fn sequence_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut value = self.expression(0)?;
        while self.eat(",") {
            if value.depth() >= 128 {
                return Err(self.error(
                    "Expression exceeds 128 nested operations; split it into smaller expressions.",
                ));
            }
            value = Expr::Binary(",".into(), Box::new(value), Box::new(self.expression(0)?));
        }
        Ok(value)
    }
    fn expression(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let result = self.expression_inner(min_bp)?;
        self.depth -= 1;
        Ok(result)
    }
    fn expression_inner(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = if self.is("!")
            || self.is("~")
            || self.is("-")
            || self.is("+")
            || self.is("typeof")
            || self.is("void")
            || (self.script.language == ScriptLanguage::ActionScript2 && self.is("not"))
        {
            let operator = self.token_text();
            self.pos += 1;
            Expr::Unary(operator, Box::new(self.expression(25)?))
        } else if self.eat("delete") {
            let offset = self.peek(self.pos - 1).unwrap().offset;
            let operand = self.expression(25)?;
            if !matches!(
                operand,
                Expr::Member(_, _)
                    | Expr::Index(_, _)
                    | Expr::XmlSelect {
                        descendant: false,
                        ..
                    }
            ) && !((self.with_depth > 0
                || self.script.language == ScriptLanguage::ActionScript2)
                && matches!(operand, Expr::Name(_)))
            {
                return Err(self.error("delete requires a property or indexed element; deleting lexical bindings is unsupported."));
            }
            Expr::Delete(Box::new(operand), offset)
        } else if self.is("++") || self.is("--") {
            let operator = self.token_text();
            self.pos += 1;
            let operand = self.expression(25)?;
            if !operand.assignable() {
                return Err(self.error("Increment/decrement requires a variable or property."));
            }
            Expr::Update(operator, Box::new(operand), true)
        } else {
            self.primary()?
        };
        loop {
            if lhs.depth() > 128 {
                return Err(self.error(
                    "Expression exceeds 128 nested operations; split it into smaller expressions.",
                ));
            }
            if min_bp <= 30 && self.eat("..") {
                lhs = self.xml_selector(lhs, true)?;
                continue;
            }
            if min_bp <= 30 && self.eat(".") {
                if self.eat("<") {
                    if !matches!(&lhs,Expr::Name(name) if name=="Vector" && self.context.intrinsic_type(name,self.script.language==ScriptLanguage::ActionScript3))
                    {
                        return Err(self.error("Only the unshadowed AS3 Vector.<T> native type supports type parameters."));
                    }
                    let offset = self.peek(self.pos - 1).unwrap().offset;
                    let element = self.type_name()?;
                    if !self.supported_type(&element, false) {
                        return Err(
                            self.error(format!("Unsupported Vector element type '{element}'."))
                        );
                    }
                    self.type_close()?;
                    lhs = Expr::TypeValue(format!("Vector.<{element}>"), offset);
                    continue;
                }
                if self.eat("(") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(self.error("E4X filters require AS3."));
                    }
                    self.with_depth += 1;
                    let outer_no_in = self.no_in;
                    self.no_in = false;
                    let predicate = self.sequence_expression()?;
                    self.no_in = outer_no_in;
                    self.with_depth -= 1;
                    self.expect(")")?;
                    if !scope::with_body_supported(&Stmt::Expr(predicate.clone())) {
                        return Err(self.error(
                            "Function definitions inside E4X filter predicates are not supported.",
                        ));
                    }
                    let id = self.next_function;
                    self.next_function += 1;
                    lhs = Expr::XmlFilter(Box::new(lhs), Box::new(predicate), id);
                    continue;
                }
                if self.is("@")
                    || self.is("*")
                    || self
                        .peek(self.pos + 1)
                        .is_some_and(|t| t.kind == Kind::Punct("::"))
                {
                    lhs = self.xml_selector(lhs, false)?;
                    continue;
                }
                lhs = Expr::Member(Box::new(lhs), self.ident()?);
                continue;
            }
            if min_bp <= 30 && self.eat("[") {
                let index = self.sequence_expression()?;
                self.expect("]")?;
                lhs = Expr::Index(Box::new(lhs), Box::new(index));
                continue;
            }
            if min_bp <= 30 && self.is("(") {
                let offset = self.token().offset;
                self.pos += 1;
                let mut args = Vec::new();
                if !self.is(")") {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect(")")?;
                lhs = Expr::Call(Box::new(lhs), args, offset);
                continue;
            }
            if min_bp <= 27 && (self.is("++") || self.is("--")) {
                // Postfix operators have a restricted production: a line
                // terminator starts a separate prefix-expression statement.
                if self.line_break_before_token() {
                    break;
                }
                if !lhs.assignable() {
                    return Err(self.error("Increment/decrement requires a variable or property."));
                }
                let operator = self.token_text();
                self.pos += 1;
                lhs = Expr::Update(operator, Box::new(lhs), false);
                continue;
            }
            if min_bp <= 2 && self.eat("?") {
                let yes = self.expression(0)?;
                self.expect(":")?;
                let no = self.expression(1)?;
                lhs = Expr::Conditional(Box::new(lhs), Box::new(yes), Box::new(no));
                continue;
            }
            let operator = self.token_text();
            if matches!(operator.as_str(), "is" | "as") {
                if min_bp > 15 {
                    break;
                }
                if self.script.language != ScriptLanguage::ActionScript3 {
                    return Err(self.error("AS2 does not support the AS3 is/as type operators."));
                }
                let offset = self.token().offset;
                self.pos += 1;
                let name = self.type_name()?;
                if (matches!(name.as_str(), "JSON" | "Vector")
                    && self.context.intrinsic_type(&name, true))
                    || (!self.context.intrinsic_type(&name, true)
                        && !self.context.core_type(&name)
                        && !self.context.types.contains_key(&name)
                        && !(!self.context.project && is_as3_wave7_type(&name))
                        && vector_element(&name).is_none())
                {
                    return Err(self.error("is/as currently requires a supported native type or a resolved project/native type; dynamic class expressions are unsupported."));
                }
                lhs = Expr::TypeTest(operator, Box::new(lhs), name, offset);
                continue;
            }
            if operator == "instanceof" && self.script.language == ScriptLanguage::ActionScript2 {
                if min_bp > 15 {
                    break;
                }
                let offset = self.token().offset;
                self.pos += 1;
                let name = self.type_name()?;
                if !self.context.legacy_case
                    && !self.context.core_type(&name)
                    && !self.context.intrinsic_type(&name, false)
                    && !self.context.types.contains_key(&name)
                    && !is_as2_legacy_instanceof_type(&name)
                {
                    return Err(self.error("AS2 instanceof requires a resolved class or supported native constructor; dynamic constructor expressions are outside this profile."));
                }
                lhs = Expr::TypeTest(operator, Box::new(lhs), name, offset);
                continue;
            }
            if operator == "instanceof" && self.script.language == ScriptLanguage::ActionScript3 {
                if min_bp > 15 {
                    break;
                }
                self.pos += 1;
                let rhs = self.expression(16)?;
                lhs = Expr::InstanceOf(Box::new(lhs), Box::new(rhs));
                continue;
            }
            if operator == "in" && self.no_in {
                break;
            }
            if matches!(operator.as_str(), "instanceof" | "::" | ".." | "@") {
                return Err(self.error(format!("ActionScript operator '{operator}' requires compatibility semantics not implemented yet.")));
            }
            if self.script.language == ScriptLanguage::ActionScript3
                && matches!(
                    operator.as_str(),
                    "add" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "<>" | "and" | "or"
                )
            {
                return Err(self
                    .error("Legacy string operators belong to AS2; use AS3 symbolic operators."));
            }
            let Some((left_bp, right_bp, assignment)) = binding_power(&operator) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            if assignment && !lhs.assignable() {
                return Err(
                    self.error("The left side of an assignment must be a variable or property.")
                );
            }
            self.pos += 1;
            let rhs = self.expression(right_bp)?;
            lhs = if assignment {
                Expr::Assign(operator, Box::new(lhs), Box::new(rhs))
            } else {
                Expr::Binary(operator, Box::new(lhs), Box::new(rhs))
            };
        }
        Ok(lhs)
    }
    fn token_text(&self) -> String {
        match &self.token().kind {
            Kind::Ident(s) | Kind::Number(s) | Kind::String(s) => s.clone(),
            Kind::Punct(s) => s.to_string(),
            Kind::End | Kind::Invalid(_) => String::new(),
        }
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        if self.is("/") || self.is("/=") {
            return self.regexp_literal();
        }
        if self.is("<") {
            return self.xml_literal();
        }
        if self.eat("new") {
            let offset = self.token().offset;
            let name = self.type_name()?;
            if name == "Vector"
                && self
                    .context
                    .intrinsic_type(&name, self.script.language == ScriptLanguage::ActionScript3)
            {
                return Err(self.error(
                    "Vector construction requires an element type: new Vector.<T>(length, fixed).",
                ));
            }
            let class = self.context.types.get(&name).cloned();
            let builtin = matches!(name.as_str(), "Array" | "String")
                || (matches!(
                    name.as_str(),
                    "Object" | "Number" | "Boolean" | "int" | "uint"
                ) && self.context.core_type(&name))
                || self
                    .context
                    .intrinsic_type(&name, self.script.language == ScriptLanguage::ActionScript3)
                || (self.script.language == ScriptLanguage::ActionScript2
                    && class.is_none()
                    && matches!(name.as_str(), "RegExp" | "JSON"))
                || (self.script.language == ScriptLanguage::ActionScript2
                    && matches!(name.as_str(), "LoadVars" | "TextFormat" | "Sound"))
                || (self.script.language == ScriptLanguage::ActionScript2
                    && class.is_none()
                    && matches!(
                        name.as_str(),
                        "MovieClipLoader"
                            | "TextField.StyleSheet"
                            | "Color"
                            | "SharedObject"
                            | "Selection"
                            | "Camera"
                            | "Microphone"
                            | "ContextMenu"
                            | "ContextMenuItem"
                            | "PrintJob"
                            | "LocalConnection"
                            | "XMLSocket"
                            | "NetConnection"
                            | "NetStream"
                            | "Video"
                            | "FileReference"
                            | "FileReferenceList"
                            | "flash.net.FileReference"
                            | "flash.net.FileReferenceList"
                    ))
                || (self.script.language == ScriptLanguage::ActionScript3
                    && !self.context.project
                    && class.is_none()
                    && is_as3_wave7_constructible(&name));
            if !builtin
                && vector_element(&name).is_none()
                && class.is_none()
                && (!is_error_type(&name)
                    || (self.script.language == ScriptLanguage::ActionScript2 && name != "Error"))
                && !self.context.legacy_case
                && (self.script.language != ScriptLanguage::ActionScript3 || name.contains('.'))
            {
                return Err(self.error("This constructor is not a supported native type in this language/profile; user classes require project type resolution."));
            }
            let mut args = Vec::new();
            if self.eat("(") {
                if !self.is(")") {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect(")")?;
            }
            if vector_element(&name).is_some() {
                return Ok(Expr::NewVector(name, args, offset));
            }
            if builtin {
                return Ok(Expr::NewBuiltin(name, args, offset));
            }
            if let Some(name) = class {
                return Ok(Expr::NewClass(name, args, offset));
            }
            if !is_error_type(&name) {
                return Ok(Expr::NewValue(name, args, offset));
            }
            let maximum = if self.script.language == ScriptLanguage::ActionScript3 {
                2
            } else {
                1
            };
            if args.len() > maximum {
                return Err(
                    self.error("Error constructor has too many arguments for this language.")
                );
            }
            return Ok(Expr::NewError(name, args, offset));
        }
        if self.is("function") {
            return Ok(Expr::Function(Box::new(self.function(false)?)));
        }
        if self.script.language == ScriptLanguage::ActionScript3
            && self.with_depth > 0
            && self.eat("@")
        {
            let first = self.xml_name()?;
            let (local, namespace) = if self.eat("::") {
                let namespace = match &first {
                    Expr::Literal(value) if value == "\"*\"" => Expr::Literal("null".into()),
                    Expr::Literal(value) => Expr::Name(
                        serde_json::from_str(value)
                            .map_err(|_| self.error("Invalid namespace identifier."))?,
                    ),
                    _ => {
                        return Err(self.error(
                            "Computed namespace qualifiers are outside this selector profile; use an explicit QName value in brackets.",
                        ))
                    }
                };
                (self.xml_name()?, Some(Box::new(namespace)))
            } else {
                (first, None)
            };
            return Ok(Expr::XmlWithAttr {
                local: Box::new(local),
                namespace,
            });
        }
        let token = self.token().clone();
        match token.kind {
            Kind::Number(value) | Kind::String(value) => { self.pos += 1; Ok(Expr::Literal(value)) }
            Kind::Ident(name) => {
                if (is_unsupported_keyword(&name) && !(name == "super" && (self.context.class_name.is_some() || self.context.legacy_case)) && !(name == "set" && self.script.language == ScriptLanguage::ActionScript2)) || matches!(name.as_str(), "if" | "else" | "while" | "for" | "var" | "break" | "continue" | "return") {
                    return Err(self.error(format!("ActionScript construct '{name}' is not supported in this expression.")));
                }
                self.pos += 1;
                if matches!(name.as_str(), "true" | "false" | "null" | "undefined" | "NaN" | "Infinity") {
                    Ok(Expr::Literal(name))
                } else if self.script.language == ScriptLanguage::ActionScript3 && self.eat("::") {
                    let local = self.xml_name()?;
                    Ok(Expr::XmlSelect {
                        base: Box::new(Expr::Name("this".into())),
                        local: Box::new(local),
                        namespace: Some(Box::new(Expr::Name(name))),
                        attribute: false,
                        descendant: false,
                    })
                } else { Ok(Expr::Name(name)) }
            }
            Kind::Punct("(") => { self.pos += 1; let previous=self.no_in;self.no_in=false; let expr = self.sequence_expression()?;self.no_in=previous; self.expect(")")?; Ok(expr) }
            Kind::Punct("[") => {
                self.pos += 1; let mut values = Vec::new();
                while !self.is("]") {
                    if self.eat(",") { values.push(None); continue; }
                    values.push(Some(self.expression(0)?));
                    if !self.eat(",") { break; }
                }
                self.expect("]")?; Ok(Expr::Array(values))
            }
            Kind::Punct("{") => {
                self.pos += 1; let mut values = Vec::new();
                if !self.is("}") { loop {
                    let key = match self.token().kind.clone() {
                        Kind::Ident(s) | Kind::Number(s) => PropertyKey { source:s.clone(), name:s },
                        Kind::String(s) => self.string_key(s)?,
                        _ => return Err(self.error("Expected a property name in object literal.")),
                    };
                    self.pos += 1; self.expect(":")?; values.push((key, self.expression(0)?));
                    if !self.eat(",") || self.is("}") { break; }
                } }
                self.expect("}")?; Ok(Expr::Object(values))
            }
            Kind::Punct("<" | "@" | "::" | "..") => Err(self.error("XML literals and E4X namespace/attribute operators are unsupported; use XML(string) and explicit traversal methods.")),
            _ => Err(self.error("Expected an expression.")),
        }
    }
    fn string_key(&self, source: String) -> Result<PropertyKey, Diagnostic> {
        // Use the same real parser as output validation, including escaped
        // quotes, hexadecimal and Unicode escapes. Never strip quotes by hand.
        let allocator = oxc_allocator::Allocator::default();
        let value = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::cjs())
            .parse_expression()
            .map_err(|_| self.error("Invalid escaped string property name."))?;
        let oxc_ast::ast::Expression::StringLiteral(literal) = value else {
            return Err(self.error("Expected a string property name."));
        };
        if literal.lone_surrogates {
            return Err(self
                .error("Property names containing lone UTF-16 surrogates are not supported yet."));
        }
        let name = literal.value.to_string();
        Ok(PropertyKey { source, name })
    }
    fn builtin_call(callee: &Expr) -> bool {
        match callee {
            Expr::Name(name) => {
                matches!(
                    name.as_str(),
                    "trace"
                        | "Object"
                        | "Number"
                        | "String"
                        | "Array"
                        | "Boolean"
                        | "parseInt"
                        | "parseFloat"
                        | "escape"
                        | "unescape"
                        | "isXMLName"
                        | "setInterval"
                        | "setTimeout"
                        | "clearInterval"
                        | "clearTimeout"
                        | "chr"
                        | "ord"
                        | "length"
                        | "substring"
                        | "mbchr"
                        | "mbord"
                        | "mblength"
                        | "mbsubstring"
                        | "isNaN"
                        | "isFinite"
                        | "Date"
                        | "RegExp"
                        | "XML"
                        | "XMLList"
                        | "XMLNode"
                        | "Namespace"
                        | "QName"
                ) || is_timeline_method(name)
                    || is_as3_wave7_type(name)
            }
            Expr::Member(base, name) if matches!(base.as_ref(), Expr::Name(n) if n == "Math") => {
                matches!(
                    name.as_str(),
                    "abs"
                        | "acos"
                        | "asin"
                        | "atan"
                        | "atan2"
                        | "ceil"
                        | "cos"
                        | "exp"
                        | "floor"
                        | "log"
                        | "max"
                        | "min"
                        | "pow"
                        | "random"
                        | "round"
                        | "sin"
                        | "sqrt"
                        | "tan"
                )
            }
            Expr::Member(base, name)
                if name == "get"
                    && matches!(
                        base.as_ref(),
                        Expr::Name(n) if matches!(n.as_str(), "Camera" | "Microphone")
                    ) =>
            {
                true
            }
            Expr::Member(_, name) => {
                is_timeline_method(name)
                    || is_family_method(name)
                    || matches!(
                        name.as_str(),
                        "trace"
                            | "appendText"
                            | "replaceText"
                            | "replaceSelectedText"
                            | "setSelection"
                            | "getLineText"
                            | "getLineLength"
                            | "getLineOffset"
                            | "getLineIndexOfChar"
                            | "getTextFormat"
                            | "setTextFormat"
                            | "getNewTextFormat"
                            | "setNewTextFormat"
                            | "createTextField"
                            | "hitTestObject"
                            | "hitTestPoint"
                            | "hitTest"
                            | "startDrag"
                            | "stopDrag"
                            | "getURL"
                            | "loadMovie"
                            | "unloadMovie"
                            | "loadVariables"
                            | "getSWFVersion"
                            | "addCallback"
                            | "send"
                            | "setMode"
                            | "setMotionLevel"
                            | "setRate"
                            | "setGain"
                            | "setSilenceLevel"
                            | "setUseEchoSuppression"
                            | "setBufferTime"
                            | "receiveAudio"
                            | "receiveVideo"
                            | "browse"
                            | "upload"
                            | "download"
                            | "cancel"
                            | "hasMultiChannelAudio"
                            | "createVideoTexture"
                            | "uploadCompressedTextureFromByteArray"
                            | "attachNetStream"
                            | "setProfileLevel"
                            | "setKeyFrameInterval"
                            | "setQuality"
                            | "setDirty"
                            | "setProperty"
                            | "getLocal"
                            | "getRemote"
                            | "connect"
                            | "close"
                            | "setFps"
                            | "flush"
                            | "clear"
                            | "dispatchEvent"
                            | "writeUTFBytes"
                            | "writeByte"
                            | "writeUnsignedInt"
                            | "dispose"
                            | "play2"
                            | "createTexture"
                            | "createCubeTexture"
                            | "clone"
                    )
            }
            Expr::Index(base, index) => literal_property_name(index)
                .is_some_and(|name| Self::builtin_call(&Expr::Member(base.clone(), name))),
            _ => false,
        }
    }
}

fn is_timeline_method(name: &str) -> bool {
    matches!(
        name,
        "play" | "stop" | "gotoAndPlay" | "gotoAndStop" | "nextFrame" | "prevFrame"
    )
}

fn is_as2_global_prop(name: &str) -> bool {
    matches!(
        name,
        "_quality" | "_highquality" | "_focusrect" | "_soundbuftime"
    )
}

pub(super) fn is_as2_frame_global(name: &str) -> bool {
    matches!(
        name,
        "getURL"
            | "fscommand"
            | "loadMovie"
            | "loadMovieNum"
            | "unloadMovie"
            | "unloadMovieNum"
            | "loadVariables"
            | "loadVariablesNum"
            | "print"
            | "printNum"
            | "printAsBitmap"
            | "printAsBitmapNum"
            | "asfunction"
            | "MMExecute"
    )
}

fn qualified_expr_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(n) => Some(n.clone()),
        Expr::Member(base, n) => Some(format!("{}.{}", qualified_expr_name(base)?, n)),
        _ => None,
    }
}

fn is_as2_legacy_instanceof_type(name: &str) -> bool {
    matches!(
        name,
        "MovieClip"
            | "Button"
            | "TextField"
            | "TextField.StyleSheet"
            | "TextFormat"
            | "TextSnapshot"
            | "Video"
            | "LoadVars"
            | "Color"
            | "Sound"
            | "MovieClipLoader"
            | "SharedObject"
            | "Camera"
            | "Microphone"
            | "ContextMenu"
            | "ContextMenuItem"
            | "PrintJob"
            | "LocalConnection"
            | "XMLSocket"
            | "NetConnection"
            | "NetStream"
            | "FileReference"
            | "FileReferenceList"
            | "flash.net.FileReference"
            | "flash.net.FileReferenceList"
    )
}

fn is_as3_wave7_type(name: &str) -> bool {
    matches!(
        name,
        "LoaderContext"
            | "flash.system.LoaderContext"
            | "JPEGLoaderContext"
            | "flash.system.JPEGLoaderContext"
            | "ApplicationDomain"
            | "flash.system.ApplicationDomain"
            | "SecurityDomain"
            | "flash.system.SecurityDomain"
            | "Capabilities"
            | "flash.system.Capabilities"
            | "Context3D"
            | "flash.display3D.Context3D"
            | "VideoTexture"
            | "flash.display3D.textures.VideoTexture"
            | "Texture"
            | "flash.display3D.textures.Texture"
            | "CubeTexture"
            | "flash.display3D.textures.CubeTexture"
            | "VideoStreamSettings"
            | "flash.media.VideoStreamSettings"
            | "H264VideoStreamSettings"
            | "flash.media.H264VideoStreamSettings"
            | "VP6VideoStreamSettings"
            | "flash.media.VP6VideoStreamSettings"
            | "H264Level"
            | "flash.media.H264Level"
            | "H264Profile"
            | "flash.media.H264Profile"
            | "VideoCodec"
            | "flash.media.VideoCodec"
            | "AudioDecoder"
            | "flash.media.AudioDecoder"
            | "AudioDeviceManager"
            | "flash.media.AudioDeviceManager"
            | "AudioOutputChangeReason"
            | "flash.media.AudioOutputChangeReason"
            | "AudioOutputChangeEvent"
            | "flash.events.AudioOutputChangeEvent"
            | "Responder"
            | "flash.net.Responder"
            | "NetConnection"
            | "flash.net.NetConnection"
            | "NetStream"
            | "flash.net.NetStream"
            | "NetStreamPlayOptions"
            | "flash.net.NetStreamPlayOptions"
            | "NetStreamPlayTransitions"
            | "flash.net.NetStreamPlayTransitions"
            | "SharedObject"
            | "flash.net.SharedObject"
            | "ColorCorrection"
            | "flash.display.ColorCorrection"
            | "ColorCorrectionSupport"
            | "flash.display.ColorCorrectionSupport"
            | "AVM1Movie"
            | "flash.display.AVM1Movie"
            | "InteractiveObject"
            | "flash.display.InteractiveObject"
            | "Stage"
            | "flash.display.Stage"
            | "Rectangle"
            | "flash.geom.Rectangle"
            | "ByteArray"
            | "flash.utils.ByteArray"
    )
}

fn is_as3_wave7_constructible(name: &str) -> bool {
    matches!(
        name,
        "LoaderContext"
            | "flash.system.LoaderContext"
            | "JPEGLoaderContext"
            | "flash.system.JPEGLoaderContext"
            | "ApplicationDomain"
            | "flash.system.ApplicationDomain"
            | "SecurityDomain"
            | "flash.system.SecurityDomain"
            | "Context3D"
            | "flash.display3D.Context3D"
            | "VideoTexture"
            | "flash.display3D.textures.VideoTexture"
            | "Texture"
            | "flash.display3D.textures.Texture"
            | "CubeTexture"
            | "flash.display3D.textures.CubeTexture"
            | "VideoStreamSettings"
            | "flash.media.VideoStreamSettings"
            | "H264VideoStreamSettings"
            | "flash.media.H264VideoStreamSettings"
            | "VP6VideoStreamSettings"
            | "flash.media.VP6VideoStreamSettings"
            | "AudioOutputChangeEvent"
            | "flash.events.AudioOutputChangeEvent"
            | "Responder"
            | "flash.net.Responder"
            | "NetConnection"
            | "flash.net.NetConnection"
            | "NetStream"
            | "flash.net.NetStream"
            | "NetStreamPlayOptions"
            | "flash.net.NetStreamPlayOptions"
            | "AVM1Movie"
            | "flash.display.AVM1Movie"
            | "InteractiveObject"
            | "flash.display.InteractiveObject"
            | "Rectangle"
            | "flash.geom.Rectangle"
            | "ByteArray"
            | "flash.utils.ByteArray"
    )
}

pub(super) fn parse_as2_level(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("_level")?;
    if rest == "0" {
        return Some(0);
    }
    if rest.is_empty() || rest.starts_with('0') || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u32>().ok()
}

pub(super) fn fnv1a64(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

const FAMILY_METHODS: &[&str] = &[
    "call",
    "apply",
    "push",
    "pop",
    "shift",
    "unshift",
    "join",
    "slice",
    "concat",
    "reverse",
    "splice",
    "sort",
    "sortOn",
    "indexOf",
    "lastIndexOf",
    "map",
    "filter",
    "forEach",
    "every",
    "some",
    "charAt",
    "charCodeAt",
    "substring",
    "substr",
    "split",
    "toLowerCase",
    "toUpperCase",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "localeCompare",
    "replace",
    "toString",
    "valueOf",
    "fromCharCode",
    "toFixed",
    "toExponential",
    "toPrecision",
    "match",
    "search",
    "exec",
    "test",
    "UTC",
    "parse",
    "stringify",
    "getDate",
    "getDay",
    "getFullYear",
    "getHours",
    "getMilliseconds",
    "getMinutes",
    "getMonth",
    "getSeconds",
    "getTime",
    "getTimezoneOffset",
    "getUTCDate",
    "getUTCDay",
    "getUTCFullYear",
    "getUTCHours",
    "getUTCMilliseconds",
    "getUTCMinutes",
    "getUTCMonth",
    "getUTCSeconds",
    "getUTCYear",
    "getYear",
    "setDate",
    "setFullYear",
    "setHours",
    "setMilliseconds",
    "setMinutes",
    "setMonth",
    "setSeconds",
    "setTime",
    "setUTCDate",
    "setUTCFullYear",
    "setUTCHours",
    "setUTCMilliseconds",
    "setUTCMinutes",
    "setUTCMonth",
    "setUTCSeconds",
    "setYear",
    "toDateString",
    "toTimeString",
    "toUTCString",
    "toLocaleDateString",
    "toLocaleTimeString",
    "toLocaleString",
    "toJSON",
    "length",
    "child",
    "children",
    "elements",
    "descendants",
    "attribute",
    "attributes",
    "text",
    "parent",
    "localName",
    "nodeKind",
    "hasSimpleContent",
    "hasComplexContent",
    "copy",
    "appendChild",
    "prependChild",
    "setChildren",
    "normalize",
    "toXMLString",
    "settings",
    "setSettings",
    "defaultSettings",
    "parseXML",
    "insertAt",
    "removeAt",
    "getStackTrace",
    "hasOwnProperty",
    "propertyIsEnumerable",
    "setPropertyIsEnumerable",
    "isPrototypeOf",
    "childIndex",
    "comments",
    "processingInstructions",
    "addNamespace",
    "removeNamespace",
    "inScopeNamespaces",
    "namespaceDeclarations",
    "namespace",
    "name",
    "setLocalName",
    "setName",
    "setNamespace",
    "insertChildAfter",
    "insertChildBefore",
    "contains",
    "createElement",
    "createTextNode",
    "hasChildNodes",
    "insertBefore",
    "removeNode",
    "cloneNode",
    "load",
    "getBytesLoaded",
    "getBytesTotal",
];

fn is_family_method(name: &str) -> bool {
    FAMILY_METHODS.contains(&name)
}

fn is_family_method_for(name: &str, legacy_case: bool) -> bool {
    is_family_method(name)
        || (legacy_case && FAMILY_METHODS.iter().any(|m| m.eq_ignore_ascii_case(name)))
}

fn resolve_legacy_event_name(
    clip_event: bool,
    name: &str,
    legacy_case: bool,
) -> Option<&'static str> {
    const CLIP_EVENTS: &[&str] = &[
        "load",
        "enterFrame",
        "unload",
        "data",
        "mouseDown",
        "mouseMove",
        "mouseUp",
        "keyDown",
        "keyUp",
    ];
    const BUTTON_EVENTS: &[&str] = &[
        "press",
        "release",
        "releaseOutside",
        "rollOver",
        "rollOut",
        "dragOver",
        "dragOut",
        "keyPress",
    ];
    let table = if clip_event {
        CLIP_EVENTS
    } else {
        BUTTON_EVENTS
    };
    table.iter().copied().find(|candidate| {
        if legacy_case {
            candidate.eq_ignore_ascii_case(name)
        } else {
            *candidate == name
        }
    })
}

fn validate_flash_key_press(spec: &str, legacy_case: bool) -> Option<String> {
    const SPECIAL_KEYS: &[&str] = &[
        "<Left>",
        "<Right>",
        "<Up>",
        "<Down>",
        "<Enter>",
        "<Space>",
        "<Tab>",
        "<Escape>",
        "<Backspace>",
        "<Delete>",
        "<Home>",
        "<End>",
        "<PageUp>",
        "<PageDown>",
        "<Insert>",
    ];
    if let Some(&canonical) = SPECIAL_KEYS.iter().find(|token| {
        if legacy_case {
            token.eq_ignore_ascii_case(spec)
        } else {
            **token == spec
        }
    }) {
        return Some(canonical.to_owned());
    }
    if spec.len() >= 2 && spec.starts_with('<') && spec.ends_with('>') {
        return None;
    }
    if spec.chars().count() == 1 {
        Some(spec.to_owned())
    } else {
        None
    }
}

fn flash4_property_index(name: &str, legacy_case: bool) -> Option<u32> {
    (0..22).find(|&index| {
        neoflash_core::as2_strings::property_name(index).is_some_and(|prop| {
            if legacy_case {
                prop.eq_ignore_ascii_case(name)
            } else {
                prop == name
            }
        })
    })
}

fn canonical_legacy_callback(name: &str, legacy_case: bool) -> Option<&'static str> {
    const LEGACY_CALLBACKS: &[&str] = &[
        "onEnterFrame",
        "onPress",
        "onRelease",
        "onReleaseOutside",
        "onRollOver",
        "onRollOut",
        "onMouseDown",
        "onMouseMove",
        "onMouseUp",
        "onKeyDown",
        "onKeyUp",
        "onDragOver",
        "onDragOut",
        "onUnload",
    ];
    LEGACY_CALLBACKS.iter().copied().find(|candidate| {
        if legacy_case {
            candidate.eq_ignore_ascii_case(name)
        } else {
            *candidate == name
        }
    })
}

fn literal_property_name(expr: &Expr) -> Option<String> {
    let Expr::Literal(source) = expr else {
        return None;
    };
    let allocator = oxc_allocator::Allocator::default();
    let value = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs())
        .parse_expression()
        .ok()?;
    match value {
        oxc_ast::ast::Expression::StringLiteral(literal) if !literal.lone_surrogates => {
            Some(literal.value.to_string())
        }
        _ => None,
    }
}

fn is_global(name: &str) -> bool {
    matches!(
        name,
        "Math"
            | "Number"
            | "String"
            | "Array"
            | "Boolean"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "NaN"
            | "Infinity"
            | "undefined"
            | "true"
            | "false"
            | "null"
            | "trace"
    ) || is_timeline_method(name)
        || is_error_type(name)
}
fn is_error_type(name: &str) -> bool {
    matches!(
        name,
        "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "EvalError"
            | "URIError"
            | "ArgumentError"
            | "DefinitionError"
            | "SecurityError"
            | "VerifyError"
    )
}
fn is_unsupported_keyword(name: &str) -> bool {
    matches!(
        name,
        "package"
            | "class"
            | "import"
            | "interface"
            | "extends"
            | "implements"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "namespace"
            | "use"
            | "with"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "switch"
            | "case"
            | "default"
            | "new"
            | "delete"
            | "void"
            | "is"
            | "as"
            | "instanceof"
            | "in"
            | "each"
            | "on"
            | "onClipEvent"
            | "const"
            | "let"
            | "do"
            | "super"
            | "get"
            | "set"
    )
}

fn invalid_binding(name: &str, allow_globals: bool) -> bool {
    (is_global(name)
        && (!allow_globals
            || matches!(
                name,
                "NaN" | "Infinity" | "undefined" | "true" | "false" | "null"
            )))
        || is_unsupported_keyword(name)
        || name.starts_with("__nf")
        || matches!(
            name,
            "this"
                | "function"
                | "return"
                | "var"
                | "if"
                | "else"
                | "while"
                | "for"
                | "break"
                | "continue"
                | "arguments"
                | "eval"
                | "await"
                | "yield"
        )
}

fn binding_power(operator: &str) -> Option<(u8, u8, bool)> {
    Some(match operator {
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>="
        | "&&=" | "||=" => (1, 1, true),
        "||" | "or" => (3, 4, false),
        "&&" | "and" => (5, 6, false),
        "|" => (7, 8, false),
        "^" => (9, 10, false),
        "&" => (11, 12, false),
        "==" | "!=" | "===" | "!==" | "eq" | "ne" | "<>" => (13, 14, false),
        "<" | "<=" | ">" | ">=" | "in" | "lt" | "le" | "gt" | "ge" => (15, 16, false),
        "<<" | ">>" | ">>>" => (17, 18, false),
        "+" | "-" | "add" => (19, 20, false),
        "*" | "/" | "%" => (21, 22, false),
        _ => return None,
    })
}

struct Emitter<'a> {
    with_targets: Vec<String>,
    tell_target: Option<String>,
    as3: bool,
    analysis: &'a scope::Analysis<'a>,
    scope: usize,
}

impl Emitter<'_> {
    fn marker(&self, statement: &Stmt) -> String {
        match (&self.analysis.context.mapping_prefix, statement) {
            (Some(prefix), Stmt::Located(offset, _)) => format!("{prefix}{offset}*/"),
            _ => String::new(),
        }
    }

    fn legacy_case(&self) -> bool {
        self.analysis.context.legacy_case
    }
    fn container_tail(&self) -> String {
        if self.legacy_case() {
            ", undefined, true".into()
        } else {
            self.caller_argument()
        }
    }
    fn index_tail(&self) -> &'static str {
        if self.legacy_case() {
            ", undefined, true"
        } else {
            ""
        }
    }
    fn legacy_receiver(&self) -> String {
        let fallback = if self.analysis.context.class_name.is_some()
            && self.analysis.context.method_id.is_none()
        {
            "this"
        } else {
            "__nfFrame"
        };
        let target = self.tell_target.as_deref().unwrap_or(fallback);
        if self.legacy_case() {
            format!("{{__nfTimeline: {target}, __nfLegacyCase: true}}")
        } else {
            target.to_owned()
        }
    }
    fn dotted_value_expr(&self, name: &str) -> String {
        let mut parts = name.split('.');
        let first = parts.next().expect("non-empty type name");
        let mut expr = Expr::Name(first.to_owned());
        let mut has_dot = false;
        for part in parts {
            has_dot = true;
            expr = Expr::Member(Box::new(expr), part.to_owned());
        }
        let primary = self.expression(&expr);
        if !has_dot
            && self.legacy_case()
            && self
                .analysis
                .resolve(self.scope, first)
                .is_none_or(|(id, _)| id == 0)
        {
            format!(
                "((__nfVal) => __nfVal !== undefined ? __nfVal : (typeof __nfLegacy !== 'undefined' && __nfLegacy?._global ? __nfFamilyPropertyValue(__nfLegacy._global, {first:?}, false, undefined, true) : undefined))({primary})"
            )
        } else {
            primary
        }
    }
    fn active_timeline(&self) -> &str {
        self.tell_target
            .as_deref()
            .unwrap_or(if self.scope == 0 { "this" } else { "__nfFrame" })
    }
    fn coerce(&self, name: &str, value: String) -> String {
        self.coerce_type(
            self.analysis
                .resolve(self.scope, name)
                .and_then(|(_, b)| b.ty.as_deref()),
            value,
        )
    }
    fn coerce_type(&self, ty: Option<&str>, value: String) -> String {
        if !self.as3 {
            return value;
        }
        let number = self.intrinsic("Number");
        let boolean = self.intrinsic("Boolean");
        let string = self.intrinsic("String");
        match ty {
            Some(ty) if vector_element(ty).is_some() => {
                format!("__nfVectorCast({value}, {})", self.vector_type(ty))
            }
            Some("Number") => format!("{number}({value})"),
            Some("Boolean") => format!("{boolean}({value})"),
            Some("int") => format!("({number}({value}) | 0)"),
            Some("uint") => format!("({number}({value}) >>> 0)"),
            Some("String") => {
                format!("((__nfValue) => __nfValue == null ? null : {string}(__nfValue))({value})")
            }
            Some("Function") => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (typeof __nfValue !== 'function' || __nfNumericClassName(__nfValue)!==null) throw new __nfTypeError('AS3 Function value required'); return __nfValue; }})({value})"
            ),
            Some("Class") => format!("__nfClassCast({value})"),
            Some("Object") => {
                format!("((__nfValue) => __nfValue == null ? null : __nfValue)({value})")
            }
            Some("Array") => format!(
                "((__nfValue) => {{ if (__nfValue == null || __nfArrayIsArray(__nfValue)) return __nfValue == null ? null : __nfValue; throw new __nfTypeError('AS3 Array value required'); }})({value})"
            ),
            Some(ty @ ("XML" | "XMLList" | "Namespace" | "QName"))
                if self.analysis.context.intrinsic_type(ty, true) =>
            {
                format!("__nfXMLCast({ty:?}, {value})")
            }
            Some(ty @ ("Date" | "RegExp")) if self.analysis.context.intrinsic_type(ty, true) => {
                format!(
                    "((__nfValue) => {{ if (__nfValue == null) return null; if (__nf{ty}Is(__nfValue)) return __nfValue; throw new __nfTypeError('AS3 {ty} value required'); }})({value})"
                )
            }
            Some(ty) if is_error_type(ty) => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (__nfCatchType(__nfValue, {ty:?})) return __nfValue; throw new __nfTypeError('AS3 {ty} value required'); }})({value})"
            ),
            Some(ty) if self.analysis.context.types.contains_key(ty) => format!(
                "__nfProgram.cast({:?}, {value})",
                self.analysis.context.types[ty]
            ),
            Some(ty) if !self.analysis.context.project && is_as3_wave7_type(ty) => {
                format!("__nfWave7Cast({ty:?}, {value})")
            }
            _ => value,
        }
    }
    fn intrinsic(&self, name: &str) -> String {
        format!("__nf{name}")
    }
    fn type_descriptor(&self, ty: &str) -> String {
        if let Some(element) = vector_element(ty) {
            return format!("__nfVectorDescriptor({})", self.type_descriptor(element));
        }
        if self.analysis.context.types.contains_key(ty)
            && !matches!(ty, "Object" | "Array")
            && !self.analysis.context.intrinsic_type(ty, true)
        {
            return format!(
                "__nfProgram.elementDescriptor({:?})",
                self.analysis.context.types[ty]
            );
        }
        format!("__nfPrimitiveDescriptor({ty:?})")
    }
    fn vector_type(&self, ty: &str) -> String {
        format!(
            "__nfVectorType({})",
            self.type_descriptor(vector_element(ty).expect("Vector type"))
        )
    }
    fn typed(&self, name: &str) -> bool {
        self.analysis
            .resolve(self.scope, name)
            .is_some_and(|(_, b)| b.ty.is_some())
    }
    fn name(&self, name: &str) -> String {
        if let Some(reference) = self.with_reference(name) {
            return format!("({reference}).get()");
        }
        self.unscoped_name(name)
    }
    fn without_with(&self) -> Emitter<'_> {
        Emitter {
            as3: self.as3,
            analysis: self.analysis,
            scope: self.scope,
            with_targets: Vec::new(),
            tell_target: self.tell_target.clone(),
        }
    }
    fn with_reference(&self, name: &str) -> Option<String> {
        if self.with_targets.is_empty()
            || matches!(name, "this" | "super" | "eval" | "set")
            || (!self.as3 && parse_as2_level(name).is_some())
        {
            return None;
        }
        let plain = self.without_with();
        let timeline = plain.active_timeline().to_owned();
        let expression = Expr::Name(name.to_owned());
        let receiver = plain.bare_member_receiver(name).or_else(|| {
            let raw = plain.unscoped_name(name);
            if raw == format!("{timeline}.{name}") {
                Some(timeline)
            } else {
                None
            }
        });
        let fallback = receiver
            .as_ref()
            .map(|owner| {
                format!(
                    "__nfContainerRef({owner},{name:?},{}{})",
                    self.as3,
                    self.container_tail()
                )
            })
            .or_else(|| plain.reference(&expression))
            .unwrap_or_else(|| {
                let value = plain.unscoped_name(name);
                let resolved = self.analysis.resolve(self.scope, name);
                let write = if resolved.is_some()
                    || (!is_global(name) && !self.analysis.context.types.contains_key(name))
                {
                    format!("__nfAssigned=>({value}=__nfAssigned)")
                } else {
                    "()=>{throw new __nfTypeError('Cannot replace this built-in binding.');}".into()
                };
                let has_expr = if resolved.is_some_and(|(id, _)| id != 0)
                    || is_global(name)
                    || self.analysis.context.types.contains_key(name)
                {
                    "true".into()
                } else {
                    format!("({value} !== undefined)")
                };
                format!("{{get:()=>{value},set:{write},delete:()=>false,has:()=>{has_expr}}}")
            });
        let receiver = receiver.unwrap_or_else(|| "undefined".into());
        Some(format!(
            "__nfWithRef([{}],{name:?},()=>({fallback}),{receiver},{}{})",
            self.with_targets
                .iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join(","),
            self.as3,
            self.container_tail()
        ))
    }
    fn unscoped_name(&self, name: &str) -> String {
        if name == "arguments" {
            return "__nfArgumentsValue".into();
        }
        if self.as3
            && matches!(name, "Number" | "int" | "uint" | "Boolean")
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfNumericClass({name:?})");
        }
        if self.as3
            && name == "Class"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfClassMetaclass".into();
        }
        if self.as3
            && name == "Function"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfFunctionClass".into();
        }
        if self.as3
            && is_error_type(name)
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfErrorClass({name:?}, true)");
        }
        if self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(uri) = self.analysis.context.namespaces.get(name)
        {
            return format!("__nfXML.namespace(undefined, {uri:?})");
        }
        if name == "Math"
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.types.contains_key(name)
        {
            return format!("__nfMath({})", self.as3);
        }
        if !self.as3
            && matches!(
                name,
                "setInterval" | "setTimeout" | "clearInterval" | "clearTimeout"
            )
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.functions.contains_key(name)
        {
            return format!("__nfLegacy.{name}");
        }
        if !self.as3
            && is_as2_frame_global(name)
            && self.analysis.resolve(self.scope, name).is_none()
            && !self.analysis.context.types.contains_key(name)
        {
            return format!("__nfLegacy.{name}.bind({})", self.legacy_receiver());
        }
        if self.analysis.resolve(self.scope, name).is_none() && (!self.as3 && name == "getTimer") {
            return "__nfProgram.type(\"flash.utils.getTimer\")".into();
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && let Some(path) = self.analysis.context.functions.get(name)
        {
            return format!("__nfProgram.type({path:?})");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(level) = parse_as2_level(name)
        {
            return format!("__nfLegacy.level({level})");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && is_as2_global_prop(name)
        {
            return format!("__nfLegacy.globalPropRef({name:?}).get()");
        }
        if !self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && (matches!(
                name,
                "Key"
                    | "Mouse"
                    | "Stage"
                    | "LoadVars"
                    | "TextFormat"
                    | "TextField"
                    | "Accessibility"
                    | "_global"
                    | "newline"
                    | "Sound"
                    | "System"
                    | "flash"
                    | "targetPath"
                    | "getProperty"
                    | "setProperty"
                    | "getVersion"
                    | "updateAfterEvent"
                    | "stopAllSounds"
                    | "toggleHighQuality"
                    | "showRedrawRegions"
                    | "duplicateMovieClip"
                    | "removeMovieClip"
                    | "startDrag"
                    | "stopDrag"
            ) || (matches!(
                name,
                "MovieClip"
                    | "Button"
                    | "TextSnapshot"
                    | "Video"
                    | "MovieClipLoader"
                    | "AsBroadcaster"
                    | "Color"
                    | "SharedObject"
                    | "Selection"
                    | "Camera"
                    | "Microphone"
                    | "ContextMenu"
                    | "ContextMenuItem"
                    | "PrintJob"
                    | "LocalConnection"
                    | "XMLSocket"
                    | "NetConnection"
                    | "NetStream"
                    | "FileReference"
                    | "FileReferenceList"
                    | "TextRenderer"
            ) && !self.analysis.context.types.contains_key(name)))
        {
            return format!("__nfLegacy.{name}");
        }
        if self.as3
            && !self.analysis.context.project
            && self.analysis.resolve(self.scope, name).is_none()
            && (is_as3_wave7_type(name) || name == "flash")
        {
            return format!("__nfWave7Class({name:?})");
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, binding)| id == 0 && binding.kind == scope::BindingKind::Field)
            && let Some(owner) = self.analysis.context.static_members.get(name)
        {
            return format!("__nfProgram.type({owner:?}).{name}");
        }
        if !self.as3
            && name == "Boolean"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return "__nfBoolean2".into();
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.utility_function(name, self.as3)
        {
            return if self.as3 {
                format!("__nfUriFunctions.{name}")
            } else {
                match name {
                    "escape" => "__nfEscape2",
                    "unescape" => "__nfUnescape2",
                    "chr" | "ord" | "length" | "substring" | "mbchr" | "mbord" | "mblength"
                    | "mbsubstring" => return format!("__nfLegacyStringGlobals.{name}"),
                    _ => "__nfRandom2",
                }
                .into()
            };
        }
        if name == "Object"
            && self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.core_type(name)
        {
            return format!("__nfObjectClass({})", self.as3);
        }
        if self.analysis.resolve(self.scope, name).is_none() && matches!(name, "Array" | "String") {
            return format!("__nf{name}Class({})", self.as3);
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && self.analysis.context.intrinsic_type(name, self.as3)
        {
            return if matches!(name, "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName") {
                format!("__nfXMLClass({name:?}, {})", self.as3)
            } else if name == "JSON" {
                "__nfJSON".into()
            } else {
                format!("__nf{name}Class({})", self.as3)
            };
        }
        if name == "__nfProgram" && self.analysis.context.project {
            return name.into();
        }
        if name == "super"
            && let Some(class) = &self.analysis.context.class_name
        {
            return format!("__nfProgram.superView({class:?}, __nfFrame)");
        }
        if self.analysis.resolve(self.scope, name).is_none()
            && let Some(qname) = self.analysis.context.types.get(name)
        {
            if !self.as3 && qname == "flash.external.ExternalInterface" {
                return "__nfLegacy.ExternalInterface".into();
            }
            return format!("__nfProgram.type({qname:?})");
        }
        if self.as3
            && self.analysis.resolve(self.scope, name).is_none()
            && let Some(uri) = self.analysis.context.namespaces.get(name)
        {
            return format!("(__nfXML?.namespace?.({name:?}, {uri:?}) ?? {uri:?})");
        }
        if name == "parseInt" && self.analysis.resolve(self.scope, name).is_none() {
            return if self.as3 {
                "__nfParseInt3"
            } else {
                "__nfParseInt2"
            }
            .into();
        }
        if let Some((id, binding)) = self.analysis.resolve(self.scope, name)
            && id != 0
        {
            return binding.canonical.clone();
        }
        if name == "this" || (is_global(name) && !is_timeline_method(name) && name != "trace") {
            name.into()
        } else if name == "trace" {
            "__nfTrace".into()
        } else {
            format!("{}.{name}", self.active_timeline())
        }
    }
    /// Lexical access travels with emitted code, never with an ambient runtime
    /// current-class stack. Computed/indexed access deliberately omits it.
    fn caller_argument(&self) -> String {
        self.analysis
            .context
            .class_name
            .as_ref()
            .map_or_else(String::new, |name| format!(", {name:?}"))
    }
    fn open_namespace_uri_for_member(&self, name: &str) -> Option<&str> {
        if !self.as3 {
            return None;
        }
        for uri in &self.analysis.context.open_namespaces {
            if self
                .analysis
                .context
                .namespace_members
                .contains(&(uri.clone(), name.to_owned()))
            {
                return Some(uri.as_str());
            }
        }
        None
    }
    fn bare_member_receiver(&self, name: &str) -> Option<String> {
        if self.analysis.context.class_name.is_none() && !self.analysis.context.legacy_case {
            return None;
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, b)| id == 0 && b.kind == scope::BindingKind::Field)
        {
            return Some(
                self.analysis
                    .context
                    .static_members
                    .get(name)
                    .map(|owner| format!("__nfProgram.type({owner:?})"))
                    .unwrap_or_else(|| {
                        if self.scope == 0 {
                            "this".into()
                        } else {
                            "__nfFrame".into()
                        }
                    }),
            );
        }
        // Unqualified dynamic/native members also need the sealed-object gate.
        // Locals, built-in globals and type names retain their existing binding.
        let timeline = self.active_timeline();
        let raw = self.unscoped_name(name);
        if raw == format!("{timeline}.{name}") {
            Some(timeline.into())
        } else {
            None
        }
    }
    fn as2_lexical_reference(&self) -> String {
        let mut names = std::collections::BTreeMap::new();
        let mut scope = Some(self.scope);
        while let Some(id) = scope {
            if id != 0 || self.tell_target.is_none() {
                for (key, binding) in &self.analysis.scopes[&id].bindings {
                    names
                        .entry(key.clone())
                        .or_insert_with(|| binding.canonical.clone());
                }
            }
            scope = self.analysis.scopes[&id].parent;
        }
        let cases = names
            .into_iter()
            .map(|(key, canonical)| {
                let expression = Expr::Name(canonical.clone());
                let value = self.name(&canonical);
                let reference = self.reference(&expression).unwrap_or_else(|| {
                    let timeline = self.active_timeline();
                    let delete_fn = if self.unscoped_name(&canonical) == format!("{timeline}.{canonical}") {
                        format!(
                            "()=>__nfContainerRef({timeline}, {canonical:?}, false{}).delete()",
                            self.container_tail()
                        )
                    } else {
                        "()=>false".into()
                    };
                    format!("{{get:()=>{value},set:__nfAssigned=>({value}=__nfAssigned),delete:{delete_fn}}}")
                });
                format!("case {key:?}: return {reference};")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let selector = if self.legacy_case() {
            "__nfAs2Strings().invoke(\"toLowerCase\", __nfString(__nfName), [])"
        } else {
            "__nfName"
        };
        format!("(__nfName)=>{{switch({selector}){{{cases}}}return null;}}")
    }
    fn default_value(&self, ty: Option<&str>) -> &'static str {
        if !self.as3 {
            return "undefined";
        }
        match ty {
            Some(ty) if vector_element(ty).is_some() => "null",
            Some("Number") => "NaN",
            Some("Boolean") => "false",
            Some("String" | "Function" | "Class" | "Object" | "Array") => "null",
            Some(ty @ ("Date" | "RegExp" | "XML" | "XMLList" | "Namespace" | "QName"))
                if self.analysis.context.intrinsic_type(ty, true) =>
            {
                "null"
            }
            Some(ty) if is_error_type(ty) => "null",
            Some("int" | "uint") => "0",
            Some(ty) if self.analysis.context.types.contains_key(ty) => "null",
            Some(ty) if !self.analysis.context.project && is_as3_wave7_type(ty) => "null",
            _ => "undefined",
        }
    }
    fn function(&self, f: &Function) -> String {
        let inner = Emitter {
            as3: self.as3,
            analysis: self.analysis,
            scope: f.id,
            with_targets: Vec::new(),
            tell_target: None,
        };
        let mut prologue = String::new();
        if self.analysis.context.method_id == Some(f.id) {
            prologue.push_str("const __nfFrame = this;\n");
        }
        if !f.declaration
            && let Some(name) = &f.name
        {
            prologue.push_str(&format!("const {name} = __nfActivation.callee;\n"));
        }
        for (index, (name, ty)) in f.params.iter().enumerate() {
            let value = format!("__nfActivation.values[{index}]");
            let value = if let Some(default) = &f.defaults[index] {
                format!(
                    "(__nfActivation.values.length > {index} ? {value} : {})",
                    inner.expression(default)
                )
            } else {
                value
            };
            prologue.push_str(&format!("var {name} = {value};\n"));
            if self.as3 && ty.is_some() {
                prologue.push_str(&format!("{name} = {};\n", inner.coerce(name, name.clone())));
            }
        }
        if let Some(name) = &f.rest {
            prologue.push_str(&format!(
                "var {name} = __nfActivation.values.slice({});\n",
                f.params.len()
            ));
        }
        let uses_arguments = self.analysis.scopes[&f.id].uses_arguments;
        if uses_arguments {
            prologue.push_str(&format!(
                "const __nfArgumentsValue = __nfFunctionArguments(__nfActivation, [{}]);\n",
                f.params
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for binding in self.analysis.scopes[&f.id].bindings.values() {
            if binding.kind == scope::BindingKind::Variable {
                prologue.push_str(&format!(
                    "var {} = {};\n",
                    binding.canonical,
                    inner.default_value(binding.ty.as_deref())
                ));
            }
        }
        let required = f
            .defaults
            .iter()
            .take_while(|value| value.is_none())
            .count();
        let fn_body = if self.as3 && has_default_xml_ns(&f.body) {
            format!(
                "const __nfPrevXmlNs = typeof __nfXML !== 'undefined' && __nfXML?.getDefaultNamespace ? __nfXML.getDefaultNamespace() : '';\ntry {{\n{}\n}} finally {{\nif (typeof __nfXML !== 'undefined' && __nfXML?.setDefaultNamespace) __nfXML.setDefaultNamespace(__nfPrevXmlNs);\n}}",
                inner.body(&f.body)
            )
        } else {
            inner.body(&f.body)
        };
        format!(
            "__nfSourceFunction(function(__nfActivation) {{\n{prologue}{fn_body}\n}}, {{as3:{}, length:{}, required:{required}, allowExtra:{}, defaultReceiver:typeof __nfFrame === 'undefined' ? undefined : __nfFrame}})",
            self.as3,
            f.params.len(),
            !self.as3 || f.rest.is_some() || uses_arguments,
        )
    }
    fn body(&self, body: &[Stmt]) -> String {
        let mut output = String::new();
        for statement in body {
            if let Stmt::Function(f) = statement.unlocated() {
                output.push_str(&self.marker(statement));
                let code = self.function(f);
                let fn_name = f.name.as_deref().unwrap();
                if self.scope == 0 {
                    if self.legacy_case() {
                        output.push_str(&format!(
                            "__nfContainerRef(this, {fn_name:?}, false, undefined, true).set({code});\n"
                        ));
                    } else {
                        output.push_str(&format!(
                            "this.{fn_name} = {};\n",
                            if self.as3 {
                                format!("__nfBindFunction({code}, __nfFrame)")
                            } else {
                                code
                            }
                        ));
                    }
                } else {
                    let canonical = self
                        .analysis
                        .resolve(self.scope, fn_name)
                        .map_or(fn_name, |(_, b)| b.canonical.as_str());
                    output.push_str(&format!("var {canonical} = {code};\n"));
                }
            }
        }
        output.push_str(&self.statements(body));
        output
    }
    fn lvalue(&self, expr: &Expr) -> String {
        match expr {
            Expr::Name(name) => self.name(name),
            Expr::Member(base, name) => format!("({}).{name}", self.expression(base)),
            Expr::Index(base, index) => {
                format!("({})[{}]", self.expression(base), self.expression(index))
            }
            _ => unreachable!("The parser only accepts assignable expression targets"),
        }
    }
    fn as2_var_scroll_ref(&self, base: &Expr, prop: &str) -> Option<String> {
        if self.as3 || !matches!(prop, "scroll" | "maxscroll") {
            return None;
        }
        let caller = self.container_tail();
        match base {
            Expr::Name(var_name)
                if !is_global(var_name)
                    && parse_as2_level(var_name).is_none()
                    && !is_as2_global_prop(var_name)
                    && self
                        .analysis
                        .resolve(self.scope, var_name)
                        .is_none_or(|(id, b)| {
                            id == 0
                                && (b.kind != scope::BindingKind::Field
                                    || !self.analysis.context.static_members.contains_key(var_name))
                        }) =>
            {
                let is_instance_field = self
                    .analysis
                    .resolve(self.scope, var_name)
                    .is_some_and(|(id, b)| id == 0 && b.kind == scope::BindingKind::Field);
                if is_instance_field {
                    Some(format!(
                        "__nfVarScrollRef({}, this, {var_name:?}, {prop:?}{caller})",
                        self.name(var_name)
                    ))
                } else if let Some(wref) = self.with_reference(var_name) {
                    Some(format!(
                        "((__nfRef)=>__nfVarScrollRef(__nfRef.get(),__nfRef.receiver,{var_name:?},{prop:?}{caller}))({wref})"
                    ))
                } else {
                    let read_expr = if self.legacy_case() {
                        format!(
                            "__nfFamilyPropertyValue({}, {var_name:?}, false, undefined, true)",
                            self.active_timeline()
                        )
                    } else {
                        self.unscoped_name(var_name)
                    };
                    Some(format!(
                        "__nfVarScrollRef({read_expr}, {}, {var_name:?}, {prop:?}{caller})",
                        self.active_timeline()
                    ))
                }
            }
            Expr::Member(owner_expr, var_name) => Some(format!(
                "((__nfOwner)=>__nfVarScrollRef(__nfFamilyPropertyValue(__nfOwner,{var_name:?},false{caller}),__nfOwner,{var_name:?},{prop:?}{caller}))({})",
                self.expression(owner_expr)
            )),
            _ => None,
        }
    }
    fn reference(&self, expr: &Expr) -> Option<String> {
        if let Expr::Name(name) = expr
            && let Some(reference) = self.with_reference(name)
        {
            return Some(reference);
        }
        match expr {
            Expr::Name(name)
                if !self.as3
                    && self.analysis.resolve(self.scope, name).is_none()
                    && let Some(level) = parse_as2_level(name) =>
            {
                Some(format!("__nfLegacy.levelRef({level})"))
            }
            Expr::Name(name)
                if !self.as3
                    && self.analysis.resolve(self.scope, name).is_none()
                    && is_as2_global_prop(name) =>
            {
                Some(format!("__nfLegacy.globalPropRef({name:?})"))
            }
            Expr::Name(name) => self.bare_member_receiver(name).map(|receiver| {
                if let Some(uri) = self.open_namespace_uri_for_member(name) {
                    format!(
                        "__nfQualifiedRef({receiver}, {uri:?}, {name:?}, {}{})",
                        self.as3,
                        self.container_tail()
                    )
                } else {
                    format!(
                        "__nfContainerRef({receiver}, {name:?}, {}{})",
                        self.as3,
                        self.container_tail()
                    )
                }
            }),
            Expr::Member(base, name) => self.as2_var_scroll_ref(base, name).or_else(|| {
                Some(format!(
                    "__nfContainerRef({}, {name:?}, {}{})",
                    self.expression(base),
                    self.as3,
                    self.container_tail()
                ))
            }),
            Expr::XmlSelect {
                base,
                local,
                namespace,
                attribute,
                descendant,
            } => {
                if self.as3
                    && !*attribute
                    && !*descendant
                    && let Some(namespace) = namespace
                {
                    return Some(format!(
                        "__nfQualifiedRef({}, {}, {}, {}{})",
                        self.expression(base),
                        self.expression(namespace),
                        self.expression(local),
                        self.as3,
                        self.container_tail()
                    ));
                }
                let selector = if let Some(namespace) = namespace {
                    format!(
                        "((__nfNamespace,__nfLocal)=>__nfXML.selector(__nfLocal,__nfNamespace,{attribute},{descendant}))({}, {})",
                        self.expression(namespace),
                        self.expression(local)
                    )
                } else {
                    format!(
                        "__nfXML.selector({},undefined,{attribute},{descendant})",
                        self.expression(local)
                    )
                };
                Some(format!(
                    "__nfXML.ref({}, {selector}, {})",
                    self.expression(base),
                    self.as3
                ))
            }
            Expr::XmlWithAttr { local, namespace } => {
                let selector = if let Some(namespace) = namespace {
                    format!(
                        "((__nfNamespace,__nfLocal)=>__nfXML.selector(__nfLocal,__nfNamespace,true,false))({}, {})",
                        self.expression(namespace),
                        self.expression(local)
                    )
                } else {
                    format!(
                        "__nfXML.selector({},undefined,true,false)",
                        self.expression(local)
                    )
                };
                Some(format!(
                    "__nfWithXmlAttr([{}], {selector})",
                    self.with_targets
                        .iter()
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",")
                ))
            }
            Expr::Index(base, key) => Some(format!(
                "__nfContainerRef({}, {}, {}{})",
                self.expression(base),
                self.expression(key),
                self.as3,
                self.index_tail()
            )),
            _ => None,
        }
    }
    fn binary(&self, op: &str, left: String, right: String) -> String {
        if !self.as3
            && matches!(
                op,
                "add" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "and" | "or"
            )
        {
            format!("__nfLegacyStringOperator({op:?}, {left}, {right})")
        } else if !self.as3 && op == "<>" {
            format!("__nfLegacyStringOperator(\"unequal\", {left}, {right})")
        } else if self.as3 && matches!(op, "+" | "==" | "!=" | "===" | "!==") {
            format!("__nfXMLBinary({op:?}, {left}, {right})")
        } else {
            format!("({left} {op} {right})")
        }
    }
    fn switch_value(&self, expr: &Expr) -> String {
        let value = self.expression(expr);
        if self.as3 {
            format!("__nfXMLSwitch({value})")
        } else {
            value
        }
    }
    fn expression(&self, expr: &Expr) -> String {
        fn qualified_name(expr: &Expr) -> Option<String> {
            match expr {
                Expr::Name(n) => Some(n.clone()),
                Expr::Member(base, n) => Some(format!("{}.{}", qualified_name(base)?, n)),
                _ => None,
            }
        }
        if self.analysis.context.project
            && let Some(name) = qualified_name(expr)
            && !matches!(name.as_str(), "Array" | "String")
            && !(!self.as3 && name == "Object")
            && !self.analysis.context.intrinsic_type(&name, self.as3)
            && self
                .analysis
                .resolve(self.scope, name.split('.').next().unwrap())
                .is_none()
        {
            if let Some(qname) = self
                .analysis
                .context
                .types
                .get(&name)
                .or_else(|| self.analysis.context.functions.get(&name))
            {
                if !self.as3 && qname == "flash.external.ExternalInterface" {
                    return "__nfLegacy.ExternalInterface".into();
                }
                return format!("__nfProgram.type({qname:?})");
            }
            if self.as3
                && let Some(uri) = self.analysis.context.namespaces.get(&name)
            {
                let short = name.rsplit('.').next().unwrap_or(&name);
                return format!("(__nfXML?.namespace?.({short:?}, {uri:?}) ?? {uri:?})");
            }
        }
        match expr {
            Expr::XmlLiteral(template, holes) => format!(
                "__nfXML.literal({template}, [{}])",
                holes
                    .iter()
                    .map(|hole| format!("()=>({})", self.expression(hole)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::XmlSelect { .. } => format!(
                "({}).get()",
                self.reference(expr).expect("XML selector ref")
            ),
            Expr::XmlWithAttr { .. } => format!(
                "({}).get()",
                self.reference(expr).expect("XML with attribute ref")
            ),
            Expr::XmlFilter(base, predicate, id) => {
                let item = format!("__nfXmlFilter{id}");
                let mut targets = self.with_targets.clone();
                targets.push(item.clone());
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: self.tell_target.clone(),
                };
                format!(
                    "__nfXML.filter({}, ({item}) => Boolean({}))",
                    self.expression(base),
                    inner.expression(predicate)
                )
            }
            Expr::InstanceOf(lhs, rhs) => format!(
                "__nfAs3InstanceOf({}, {})",
                self.expression(lhs),
                self.expression(rhs)
            ),
            Expr::TypeValue(ty, _) => self.vector_type(ty),
            Expr::NewVector(ty, args, _) => format!(
                "new ({})({})",
                self.vector_type(ty),
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Delete(target, _) => {
                if let Some(reference) = self.reference(target) {
                    format!("({reference}).delete()")
                } else if !self.as3
                    && let Expr::Name(name) = target.as_ref()
                {
                    let timeline = self.active_timeline();
                    if self.unscoped_name(name) == format!("{timeline}.{name}") {
                        format!(
                            "__nfContainerRef({timeline}, {name:?}, false{}).delete()",
                            self.container_tail()
                        )
                    } else {
                        "false".into()
                    }
                } else {
                    panic!("delete property")
                }
            }
            Expr::Literal(value) => value.clone(),
            Expr::Name(name) if self.with_reference(name).is_some() => self.name(name),
            Expr::Name(name)
                if !self.as3
                    && name == "scrollRect"
                    && self
                        .analysis
                        .resolve(self.scope, name)
                        .is_none_or(|(scope, _)| scope == 0)
                    && !self.analysis.context.types.contains_key(name)
                    && !self.analysis.context.static_members.contains_key(name) =>
            {
                format!(
                    "__nfContainerRef({}, \"scrollRect\", false{}).get()",
                    if self.scope == 0 { "this" } else { "__nfFrame" },
                    self.index_tail()
                )
            }
            Expr::Name(name) => self.bare_member_receiver(name).map_or_else(
                || self.name(name),
                |receiver| {
                    if let Some(uri) = self.open_namespace_uri_for_member(name) {
                        format!(
                            "__nfQualifiedRef({receiver}, {uri:?}, {name:?}, {}{}).get()",
                            self.as3,
                            self.container_tail()
                        )
                    } else {
                        format!(
                            "__nfFamilyPropertyValue({receiver}, {name:?}, {}{})",
                            self.as3,
                            self.container_tail()
                        )
                    }
                },
            ),
            Expr::RegExpLiteral(pattern, flags) => {
                format!("__nfRegExpCreate(true, [{pattern}, {flags}])")
            }
            Expr::Function(f) => format!("({})", self.function(f)),
            Expr::NewError(name, args, _) => format!(
                "__nfMakeError({name:?}, {}, [{}])",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewClass(name, args, _) => format!(
                "__nfProgram.construct({name:?}, [{}])",
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewValue(name, args, _) => {
                if self.legacy_case() {
                    format!(
                        "__nfLegacyConstruct({}, [{}])",
                        self.dotted_value_expr(name),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!(
                        "__nfClassConstruct({}, [{}])",
                        self.name(name),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(
                    name.as_str(),
                    "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                ) =>
            {
                format!(
                    "new (__nfXMLClass({name:?}, {}))({})",
                    self.as3,
                    args.iter()
                        .map(|a| self.expression(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if self.as3
                    && !self.analysis.context.project
                    && is_as3_wave7_constructible(name) =>
            {
                format!(
                    "new (__nfWave7Class({name:?}))({})",
                    args.iter()
                        .map(|arg| self.expression(arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(
                    name.as_str(),
                    "TextField.StyleSheet"
                        | "LoadVars"
                        | "Color"
                        | "TextFormat"
                        | "Sound"
                        | "MovieClipLoader"
                        | "Camera"
                        | "Microphone"
                        | "ContextMenu"
                        | "ContextMenuItem"
                        | "PrintJob"
                        | "LocalConnection"
                        | "XMLSocket"
                        | "NetConnection"
                        | "NetStream"
                        | "FileReference"
                        | "FileReferenceList"
                        | "flash.net.FileReference"
                        | "flash.net.FileReferenceList"
                ) =>
            {
                format!(
                    "new __nfLegacy.{name}({})",
                    args.iter()
                        .map(|arg| self.expression(arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::NewBuiltin(name, args, _)
                if matches!(name.as_str(), "Number" | "Boolean" | "int" | "uint") =>
            {
                let values = args
                    .iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                if !self.as3 && matches!(name.as_str(), "Number" | "Boolean") {
                    format!("__nfBoxCreate({name:?}, [{values}])")
                } else {
                    format!("__nfScalarCreate({name:?}, [{values}])")
                }
            }
            Expr::NewBuiltin(name, args, _) if name == "Object" => format!(
                "__nfObjectCreate({}, [{}], true)",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::NewBuiltin(name, args, _) => format!(
                "__nf{name}Create({}, [{}]{})",
                self.as3,
                args.iter()
                    .map(|a| self.expression(a))
                    .collect::<Vec<_>>()
                    .join(", "),
                if name == "String" { ", true" } else { "" }
            ),
            Expr::Member(base, name) => {
                if let Some(reference) = self.as2_var_scroll_ref(base, name) {
                    return format!("({reference}).get()");
                }
                format!(
                    "__nfFamilyPropertyValue({}, {name:?}, {}{})",
                    self.expression(base),
                    self.as3,
                    self.container_tail()
                )
            }
            Expr::Index(base, index) => {
                format!(
                    "__nfFamilyPropertyValue({}, {}, {}{})",
                    self.expression(base),
                    self.expression(index),
                    self.as3,
                    self.index_tail()
                )
            }
            Expr::Array(values) => format!(
                "[{}{}]",
                values
                    .iter()
                    .map(|v| v.as_ref().map_or_else(String::new, |v| self.expression(v)))
                    .collect::<Vec<_>>()
                    .join(", "),
                if values.last().is_some_and(Option::is_none) {
                    ","
                } else {
                    ""
                }
            ),
            Expr::Object(values) => format!(
                "({{{}}})",
                values
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.source, self.expression(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Unary(op, value) if op == "not" => format!(
                "__nfLegacyStringOperator(\"not\", {}, undefined)",
                self.expression(value)
            ),
            Expr::Unary(op, value) => format!("({op} {})", self.expression(value)),
            Expr::Binary(op, left, right) if op == "in" => format!(
                "((__nfKey,__nfReceiver)=>__nfContainerRef(__nfReceiver,__nfKey,{}{}).has())({}, {})",
                self.as3,
                self.index_tail(),
                self.expression(left),
                self.expression(right)
            ),
            Expr::Binary(op, left, right) => {
                self.binary(op, self.expression(left), self.expression(right))
            }
            Expr::TypeTest(op, value, ty, _) => {
                let predicate = if self.analysis.context.core_type(ty) {
                    if ty == "Class" {
                        "__nfClassIs(__nfValue)".into()
                    } else if self.as3 && ty == "Function" {
                        "(typeof __nfValue === 'function' && __nfNumericClassName(__nfValue)===null)".into()
                    } else if ty == "Array" {
                        "__nfArrayIsArray(__nfValue)".into()
                    } else if !self.as3
                        && op == "instanceof"
                        && matches!(ty.as_str(), "Number" | "String" | "Boolean")
                    {
                        format!(
                            "Boolean(typeof __nfLegacy !== 'undefined' && __nfLegacy.boxing?.is?.(__nfValue) && __nfLegacy.boxing.kind(__nfValue) === {ty:?})"
                        )
                    } else {
                        format!("__nfCatchType(__nfValue, {ty:?})")
                    }
                } else if vector_element(ty).is_some() {
                    format!("{}.__nfIs(__nfValue)", self.vector_type(ty))
                } else if matches!(
                    ty.as_str(),
                    "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                ) && self.analysis.context.intrinsic_type(ty, self.as3)
                {
                    format!("__nfXMLClass({ty:?}, {}).__nfIs(__nfValue)", self.as3)
                } else if self.analysis.context.intrinsic_type(ty, self.as3) {
                    format!("__nf{ty}Is(__nfValue)")
                } else if !self.as3
                    && !self.analysis.context.types.contains_key(ty)
                    && is_as2_legacy_instanceof_type(ty)
                {
                    format!("Boolean(__nfLegacy.{ty}?.__nfIs?.(__nfValue))")
                } else if self.as3 && !self.analysis.context.project && is_as3_wave7_type(ty) {
                    format!("Boolean(__nfWave7Class({ty:?})?.__nfIs?.(__nfValue))")
                } else if self.legacy_case() && !self.analysis.context.types.contains_key(ty) {
                    format!(
                        "__nfLegacyInstanceOf(__nfValue, {})",
                        self.dotted_value_expr(ty)
                    )
                } else {
                    format!(
                        "__nfProgram.is(__nfValue, {:?})",
                        self.analysis.context.types[ty]
                    )
                };
                format!(
                    "((__nfValue) => {} )({})",
                    if op == "instanceof" {
                        format!(
                            "(__nfValue !== null && (typeof __nfValue === 'object' || typeof __nfValue === 'function') && ({predicate}))"
                        )
                    } else if op == "is" {
                        predicate
                    } else {
                        format!("{predicate} ? __nfValue : null")
                    },
                    self.expression(value)
                )
            }
            Expr::Assign(op, left, right) => {
                let target = if matches!(left.as_ref(), Expr::XmlSelect { .. }) {
                    String::new()
                } else {
                    self.lvalue(left)
                };
                let value = self.expression(right);
                if !self.as3 && op == "=" {
                    let callback = match left.as_ref() {
                        Expr::Name(name)
                            if self.with_targets.is_empty()
                                && self.analysis.resolve(self.scope, name).is_none() =>
                        {
                            canonical_legacy_callback(name, self.legacy_case())
                                .map(|canonical| ("__nfFrame".into(), canonical))
                        }
                        Expr::Member(base, name) => {
                            canonical_legacy_callback(name, self.legacy_case())
                                .map(|canonical| (self.expression(base), canonical))
                        }
                        _ => None,
                    };
                    if let Some((receiver, name)) = callback {
                        return format!(
                            "__nfDisplay.setLegacyCallback({receiver}, {name:?}, {value})"
                        );
                    }
                }
                if let Expr::Name(name) = left.as_ref()
                    && self.as3
                    && self.typed(name)
                {
                    if let Some(reference) = self.reference(left) {
                        if op == "=" {
                            return format!("({reference}).set({})", self.coerce(name, value));
                        }
                        let operator = &op[..op.len() - 1];
                        let next = if matches!(op.as_str(), "&&=" | "||=") {
                            format!(
                                "__nfOld {operator} __nfRef.set({})",
                                self.coerce(name, value)
                            )
                        } else {
                            format!(
                                "__nfRef.set({})",
                                self.coerce(name, self.binary(operator, "__nfOld".into(), value))
                            )
                        };
                        return format!(
                            "((__nfRef)=>{{const __nfOld=__nfRef.get();return {next};}})({reference})"
                        );
                    }
                    if op == "&&=" || op == "||=" {
                        return format!(
                            "({target} {} ({target} = {}))",
                            &op[..2],
                            self.coerce(name, value)
                        );
                    }
                    let value = if op == "=" {
                        value
                    } else {
                        self.binary(&op[..op.len() - 1], target.clone(), value)
                    };
                    return format!("({target} = {})", self.coerce(name, value));
                }
                if let Some(reference) = self.reference(left) {
                    if op == "=" {
                        return format!("({reference}).set({value})");
                    }
                    let operator = &op[..op.len() - 1];
                    let expression = if matches!(op.as_str(), "&&=" | "||=") {
                        format!("__nfOld {operator} __nfRef.set({value})")
                    } else {
                        format!(
                            "__nfRef.set({})",
                            self.binary(operator, "__nfOld".into(), value.clone())
                        )
                    };
                    let assignment = format!(
                        "((__nfRef)=>{{const __nfOld=__nfRef.get();return {expression};}})({reference})"
                    );
                    return if self.as3 && matches!(op.as_str(), "&&=" | "||=") {
                        format!("__nfFamilyValue({assignment},true)")
                    } else {
                        assignment
                    };
                }
                let assignment = if self.as3 && op == "+=" {
                    format!("({target} = {})", self.binary("+", target.clone(), value))
                } else {
                    format!("({target} {op} {value})")
                };
                if self.as3 && matches!(op.as_str(), "&&=" | "||=") {
                    // A short-circuited indexed ||= may itself extract the old
                    // function value without passing through an ordinary read.
                    format!("__nfFamilyValue({assignment}, true)")
                } else {
                    assignment
                }
            }
            Expr::Update(op, value, prefix) => {
                if let Expr::Name(name) = value.as_ref()
                    && self.as3
                    && self.typed(name)
                    && let Some(reference) = self.reference(value)
                {
                    let sign = if op == "++" { "+" } else { "-" };
                    let next = self.coerce(name, format!("(__nfOld {sign} 1)"));
                    return format!(
                        "((__nfRef)=>{{const __nfOld=__nfNumber(__nfRef.get()),__nfNext={next};__nfRef.set(__nfNext);return {};}})({reference})",
                        if *prefix { "__nfNext" } else { "__nfOld" }
                    );
                }
                if let Some(reference) = self.reference(value) {
                    let sign = if op == "++" { "+" } else { "-" };
                    return format!(
                        "((__nfRef)=>{{const __nfOld=__nfNumber(__nfRef.get()),__nfNext=__nfOld {sign} 1;__nfRef.set(__nfNext);return {};}})({reference})",
                        if *prefix { "__nfNext" } else { "__nfOld" }
                    );
                }
                let target = self.lvalue(value);
                if let Expr::Name(name) = value.as_ref()
                    && self.as3
                    && self.typed(name)
                {
                    let sign = if op == "++" { "+" } else { "-" };
                    let number = self.intrinsic("Number");
                    if *prefix {
                        let converted = self.coerce(name, format!("({number}({target}) {sign} 1)"));
                        return format!("({target} = {converted})");
                    }
                    let converted = self.coerce(name, format!("(__nfOld {sign} 1)"));
                    return format!(
                        "((__nfOld) => ({target} = {converted}, __nfOld))({number}({target}))"
                    );
                }
                if *prefix {
                    format!("({op}{target})")
                } else {
                    format!("({target}{op})")
                }
            }
            Expr::Conditional(test, yes, no) => format!(
                "({} ? {} : {})",
                self.expression(test),
                self.expression(yes),
                self.expression(no)
            ),
            Expr::Call(callee, args, _) => {
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && name == "call"
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    return format!(
                        "__nfLegacy.callFrame({}, {})",
                        self.active_timeline(),
                        self.expression(&args[0])
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && matches!(name.as_str(), "eval" | "set")
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    let tail = match (name == "set", self.legacy_case()) {
                        (true, true) => format!(", {}, true", self.expression(&args[1])),
                        (true, false) => format!(", {}", self.expression(&args[1])),
                        (false, true) => ", undefined, true".into(),
                        (false, false) => String::new(),
                    };
                    return format!(
                        "__nfLegacy.objects.resolve({}, {}, {}, {}{tail})",
                        self.active_timeline(),
                        self.expression(&args[0]),
                        self.as2_lexical_reference(),
                        name == "set"
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                    && matches!(name.as_str(), "getProperty" | "setProperty" | "targetPath")
                {
                    let rendered_args = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            if index == 1
                                && matches!(name.as_str(), "getProperty" | "setProperty")
                                && let Expr::Name(prop) = arg
                                && self
                                    .analysis
                                    .resolve(self.scope, prop)
                                    .is_none_or(|(id, _)| id == 0)
                                && let Some(prop_index) =
                                    flash4_property_index(prop, self.legacy_case())
                            {
                                return prop_index.to_string();
                            }
                            self.expression(arg)
                        })
                        .map(|arg| format!(", {arg}"))
                        .collect::<String>();
                    return format!(
                        "__nfLegacy.{name}.call({}{rendered_args})",
                        self.legacy_receiver()
                    );
                }
                if !self.as3
                    && (self.with_targets.is_empty() || self.tell_target.is_some())
                    && let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                    && !self.analysis.context.types.contains_key(name)
                    && (matches!(
                        name.as_str(),
                        "duplicateMovieClip"
                            | "removeMovieClip"
                            | "startDrag"
                            | "stopDrag"
                            | "loadVariables"
                            | "loadVariablesNum"
                    ) || is_as2_frame_global(name))
                    && match name.as_str() {
                        "duplicateMovieClip" => args.len() >= 3 || self.with_targets.is_empty(),
                        "removeMovieClip" | "unloadMovie" | "startDrag" => {
                            !args.is_empty() || self.with_targets.is_empty()
                        }
                        "loadVariables" | "loadMovie" => {
                            args.len() >= 2 || self.with_targets.is_empty()
                        }
                        _ => true,
                    }
                {
                    return format!(
                        "__nfLegacy.{name}.call({}{})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| format!(", {}", self.expression(arg)))
                            .collect::<String>()
                    );
                }
                if let Expr::Name(name) = callee.as_ref()
                    && let Some(reference) = self.with_reference(name)
                {
                    return format!(
                        "__nfWithMethod({reference})({})",
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && matches!(name.as_str(), "loadVariables" | "loadVariablesNum")
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    return format!(
                        "__nfLegacy.{name}.call({}, {})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !self.as3
                    && let Expr::Name(name) = callee.as_ref()
                    && is_as2_frame_global(name)
                    && self.analysis.resolve(self.scope, name).is_none()
                    && !self.analysis.context.types.contains_key(name)
                {
                    return format!(
                        "__nfLegacy.{name}.call({}{})",
                        self.legacy_receiver(),
                        args.iter()
                            .map(|arg| format!(", {}", self.expression(arg)))
                            .collect::<String>()
                    );
                }
                if let Some(name) = qualified_name(callee)
                    && self
                        .analysis
                        .resolve(self.scope, name.split('.').next().unwrap())
                        .is_none()
                    && (self.analysis.context.functions.contains_key(&name)
                        || (!self.as3
                            && matches!(
                                name.as_str(),
                                "getTimer"
                                    | "setInterval"
                                    | "setTimeout"
                                    | "clearInterval"
                                    | "clearTimeout"
                            )))
                {
                    return format!(
                        "({})({})",
                        self.expression(callee),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !self.as3
                    && args.len() == 1
                    && let Some(name) = qualified_expr_name(callee)
                    && self
                        .analysis
                        .resolve(self.scope, name.split('.').next().unwrap())
                        .is_none()
                    && !matches!(name.as_str(), "Array" | "String")
                    && !(matches!(name.as_str(), "Object" | "Number" | "Boolean")
                        && self.analysis.context.core_type(&name))
                    && !(name == "Date" && self.analysis.context.intrinsic_type(&name, false))
                    && !self.analysis.context.utility_function(&name, false)
                    && !self.analysis.context.numeric_conversion(&name, false)
                    && !self.analysis.context.functions.contains_key(&name)
                {
                    if matches!(name.as_str(), "XML" | "XMLNode")
                        && self.analysis.context.intrinsic_type(&name, false)
                    {
                        return format!(
                            "__nfLegacyCast(__nfXMLClass({name:?}, false), {})",
                            self.expression(&args[0])
                        );
                    }
                    if !self.analysis.context.intrinsic_type(&name, false)
                        && let Some(qname) = self.analysis.context.types.get(&name)
                    {
                        return format!(
                            "__nfProgram.as2Cast({qname:?}, {})",
                            self.expression(&args[0])
                        );
                    }
                    if is_as2_legacy_instanceof_type(&name) {
                        let ctor = if matches!(
                            name.as_str(),
                            "TextField.StyleSheet"
                                | "flash.net.FileReference"
                                | "flash.net.FileReferenceList"
                        ) {
                            format!("__nfLegacy.{name}")
                        } else {
                            format!("__nfLegacy?.{name}")
                        };
                        return format!("__nfLegacyCast({ctor}, {})", self.expression(&args[0]));
                    }
                }
                if let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                {
                    if self.analysis.context.utility_function(name, self.as3) {
                        return format!(
                            "{}({})",
                            self.name(name),
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if name == "Object" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfObjectCreate({}, [{}], false)",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && name == "Class" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfClassMetaclass({})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && name == "Function" && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfFunctionClass({})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && is_error_type(name) && self.analysis.context.core_type(name) {
                        return format!(
                            "__nfMakeError({name:?}, true, [{}])",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.analysis.context.numeric_conversion(name, self.as3) {
                        let value = args
                            .first()
                            .map_or_else(|| "0".into(), |value| self.expression(value));
                        return format!(
                            "(__nfNumber({value}) {} 0)",
                            if name == "int" { "|" } else { ">>>" }
                        );
                    }
                    if matches!(
                        name.as_str(),
                        "XML" | "XMLList" | "XMLNode" | "Namespace" | "QName"
                    ) && self.analysis.context.intrinsic_type(name, self.as3)
                    {
                        return format!(
                            "(__nfXMLClass({name:?}, {}))({})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if matches!(name.as_str(), "Date" | "RegExp")
                        && self.analysis.context.intrinsic_type(name, self.as3)
                    {
                        return format!(
                            "__nf{name}Class({})({})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if matches!(name.as_str(), "Array" | "String") {
                        return format!(
                            "__nf{name}Create({}, [{}]{})",
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", "),
                            if name == "String" { ", false" } else { "" }
                        );
                    }
                    if name == "super" {
                        if let Some(class) = &self.analysis.context.class_name {
                            return format!(
                                "__nfProgram.superConstruct({class:?}, __nfFrame, [{}])",
                                args.iter()
                                    .map(|a| self.expression(a))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                        if self.legacy_case() {
                            return format!(
                                "__nfLegacySuperConstruct(this, [{}])",
                                args.iter()
                                    .map(|a| self.expression(a))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                    }
                    if let Some(qname) = self.analysis.context.types.get(name) {
                        return format!(
                            "__nfProgram.cast({qname:?}, {})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.as3 && !self.analysis.context.project && is_as3_wave7_type(name) {
                        return format!(
                            "__nfWave7Cast({name:?}, {})",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
                if let Expr::Name(name) = callee.as_ref()
                    && self.as3
                    && let Some(receiver) = self.bare_member_receiver(name)
                {
                    if let Some(uri) = self.open_namespace_uri_for_member(name) {
                        return format!(
                            "__nfQualifiedMethod({receiver}, {uri:?}, {name:?}, {}{})({})",
                            self.as3,
                            self.caller_argument(),
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    return format!(
                        "__nfContainerMethod({receiver}, {name:?}, {}{})({})",
                        self.as3,
                        self.caller_argument(),
                        args.iter()
                            .map(|a| self.expression(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let member = match callee.as_ref() {
                    Expr::Member(base, name) => Some((base.as_ref(), name.clone())),
                    Expr::Index(base, index) => {
                        literal_property_name(index).map(|name| (base.as_ref(), name))
                    }
                    _ => None,
                };
                if self.legacy_case()
                    && self.analysis.context.class_name.is_none()
                    && let Some((Expr::Name(base_name), method_name)) = &member
                    && base_name == "super"
                {
                    return format!(
                        "__nfLegacySuperMethod(this, {method_name:?}, true)({})",
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if let Some((base, name)) = &member
                    && (matches!(name.as_str(), "call" | "apply")
                        || (self.legacy_case()
                            && (name.eq_ignore_ascii_case("call")
                                || name.eq_ignore_ascii_case("apply"))))
                {
                    let canonical = if name.eq_ignore_ascii_case("call") {
                        "call"
                    } else {
                        "apply"
                    };
                    if self.analysis.context.class_name.is_some() {
                        let caller = if matches!(callee.as_ref(), Expr::Member(_, _)) {
                            self.caller_argument()
                        } else {
                            String::new()
                        };
                        return format!(
                            "__nfContainerMethod({}, {name:?}, {}{caller})({})",
                            self.expression(base),
                            self.as3,
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if self.legacy_case() {
                        return format!(
                            "((__nfRecv)=>__nfFunctionMethod(__nfRecv, {canonical:?}, false, __nfContainerRef(__nfRecv, {name:?}, false, undefined, true).get()))({})({})",
                            self.expression(base),
                            args.iter()
                                .map(|arg| self.expression(arg))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    return format!(
                        "__nfFunctionMethod({}, {name:?}, {})({})",
                        self.expression(base),
                        self.as3,
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if let Some((base, name)) = &member
                    && is_family_method_for(name, self.legacy_case())
                {
                    let tail = if matches!(callee.as_ref(), Expr::Member(_, _)) {
                        self.container_tail()
                    } else {
                        self.index_tail().to_owned()
                    };
                    return format!(
                        "__nfMemberMethod({}, {name:?}, {}{tail})({})",
                        self.expression(base),
                        self.as3,
                        args.iter()
                            .map(|arg| self.expression(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let as2_method = if self.as3 {
                    None
                } else {
                    match callee.as_ref() {
                        Expr::Name(name)
                            if self.analysis.resolve(self.scope, name).is_none()
                                && !matches!(
                                    name.as_str(),
                                    "getProperty"
                                        | "setProperty"
                                        | "targetPath"
                                        | "getVersion"
                                        | "updateAfterEvent"
                                        | "stopAllSounds"
                                        | "toggleHighQuality"
                                        | "showRedrawRegions"
                                        | "duplicateMovieClip"
                                        | "removeMovieClip"
                                        | "startDrag"
                                        | "stopDrag"
                                )
                                && (!is_global(name)
                                    || matches!(name.as_str(), "gotoAndPlay" | "gotoAndStop")) =>
                        {
                            Some((self.active_timeline().to_owned(), name))
                        }
                        Expr::Member(base, name) => Some((self.expression(base), name)),
                        Expr::Index(_, _) => member
                            .as_ref()
                            .map(|(base, name)| (self.expression(base), name)),
                        _ => None,
                    }
                };
                let callable = as2_method.map_or_else(
                    || {
                        if !self.as3
                            && let Expr::Name(name) = callee.as_ref()
                            && let Some((owner, _)) = self.analysis.resolve(self.scope, name)
                        {
                            return format!(
                                "__nfAs2Value({}, {})",
                                self.expression(callee),
                                if owner == 0 {
                                    self.analysis
                                        .context
                                        .static_members
                                        .get(name)
                                        .map(|class| format!("__nfProgram.type({class:?})"))
                                        .unwrap_or_else(|| "__nfFrame".into())
                                } else {
                                    "undefined".into()
                                }
                            );
                        }
                        // A direct member call retains the receiver. Ordinary
                        // value reads use the native-method identity guard, but
                        // calling a helper result here would erase this binding.
                        if self.as3
                            && let Expr::XmlSelect {
                                base,
                                local,
                                namespace: Some(namespace),
                                attribute: false,
                                descendant: false,
                            } = callee.as_ref()
                        {
                            format!(
                                "__nfQualifiedMethod({}, {}, {}, true{})",
                                self.expression(base),
                                self.expression(namespace),
                                self.expression(local),
                                self.container_tail()
                            )
                        } else if self.as3
                            && let Expr::Member(base, name) = callee.as_ref()
                        {
                            format!(
                                "__nfContainerMethod({}, {name:?}, true{})",
                                self.expression(base),
                                self.caller_argument()
                            )
                        } else if self.as3
                            && let Some((base, name)) = &member
                        {
                            format!(
                                "__nfContainerMethod({}, {name:?}, true)",
                                self.expression(base)
                            )
                        } else {
                            self.expression(callee)
                        }
                    },
                    |(receiver, name)| {
                        if self.legacy_case() {
                            format!("__nfAs2Method({receiver}, {name:?}, true)")
                        } else {
                            format!("__nfAs2Method({receiver}, {name:?})")
                        }
                    },
                );
                format!(
                    "{callable}({})",
                    args.iter()
                        .map(|v| self.expression(v))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
    fn statement(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Located(offset, inner) => {
                let code = self.statement(inner);
                if code.is_empty() { return code; }
                self.analysis.context.mapping_prefix.as_ref().map_or(code.clone(), |prefix| {
                    format!("{prefix}{offset}*/{code}")
                })
            }
            Stmt::Empty => ";".into(),
            Stmt::Expr(expr) => format!("{};", self.expression(expr)),
            Stmt::DefaultXmlNamespace(expr) => {
                format!("__nfXML.setDefaultNamespace({});", self.expression(expr))
            }
            Stmt::LegacyEvent(triggers, func) => {
                let code = format!(
                    "(function() {{\nconst __nfFrame = this, _parent = this._parent;\nreturn ({}).apply(this, arguments);\n}})",
                    self.function(func)
                );
                let mut out = format!("{{\nconst __nfEventFn = {code};\n");
                for trigger in triggers {
                    let kind = if trigger.clip_event { "clip" } else { "button" };
                    let key = trigger
                        .key_press
                        .as_ref()
                        .map_or_else(|| "null".into(), |k| format!("{k:?}"));
                    out.push_str(&format!(
                        "__nfDisplay.addLegacyEvent(this, {kind:?}, {:?}, {key}, __nfEventFn);\n",
                        trigger.event
                    ));
                }
                out.push('}');
                out
            }
            Stmt::Var(variables) => format!(
                "{};",
                variables
                    .iter()
                    .map(|(name, ty, value)| {
                        let target = self.name(name);
                        if self.analysis.scopes[&self.scope].function != 0 && value.is_none() {
                            return "undefined".into();
                        }
                        let initialized = value.is_some();
                        let value = value
                            .as_ref()
                            .map(|v| self.coerce(name, self.expression(v)))
                            .unwrap_or_else(|| self.default_value(ty.as_deref()).into());
                        if let Some(reference) = self.with_reference(name) {
                            return if initialized {
                                format!("({reference}).set({value})")
                            } else {
                                "undefined".into()
                            };
                        }
                        if self.analysis.scopes[&self.scope].function == 0 && self.legacy_case() {
                            let cref = format!(
                                "__nfContainerRef({}, {name:?}, false, undefined, true)",
                                self.active_timeline()
                            );
                            return if initialized {
                                format!("{cref}.set({value})")
                            } else {
                                format!(
                                    "((__nfRef)=>__nfRef.get()===undefined?__nfRef.set({value}):__nfRef.get())({cref})"
                                )
                            };
                        }
                        if initialized {
                            format!("{target} = {value}")
                        } else {
                            format!("({target} === undefined ? ({target} = {value}) : {target})")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Stmt::With(target, body, id) => {
                let name = format!("__nfWith{id}");
                let mut targets = self.with_targets.clone();
                targets.push(name.clone());
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: self.tell_target.clone(),
                };
                format!(
                    "{{const {name}=__nfWithTarget({}, {});\n{}\n}}",
                    self.expression(target),
                    self.as3,
                    inner.statement(body)
                )
            }
            Stmt::TellTarget(target, body, id) => {
                let name = format!("__nfWith{id}");
                let mut targets = self.with_targets.clone();
                targets.push(name.clone());
                let inner = Emitter {
                    as3: self.as3,
                    analysis: self.analysis,
                    scope: self.scope,
                    with_targets: targets,
                    tell_target: Some(name.clone()),
                };
                format!(
                    "{{const {name}=__nfTellTarget({}, {}, __nfFrame{});\n{}\n}}",
                    self.active_timeline(),
                    self.expression(target),
                    if self.legacy_case() { ", true" } else { "" },
                    inner.statement(body)
                )
            }
            Stmt::IfFrameLoaded(args, body) => format!(
                "if (__nfLegacy.isFrameLoaded({}, [{}])) {{\n{}\n}}",
                self.active_timeline(),
                args.iter()
                    .map(|arg| self.expression(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.statement(body)
            ),
            Stmt::InitClip(_, body, id) => {
                let key = format!("{:016x}:{id}", fnv1a64(&self.analysis.script.source));
                let clean = Emitter {
                    as3: false,
                    analysis: self.analysis,
                    scope: 0,
                    with_targets: Vec::new(),
                    tell_target: None,
                };
                format!(
                    "__nfRunInitClip({key:?}, (__nfLegacy?.level?.(0) ?? (typeof _root !== 'undefined' ? _root : this)), function(__nfInitRoot){{ const __nfFrame = __nfInitRoot;\n(function(){{\n{}\n}}).call(__nfInitRoot);\n}});",
                    clean.body(body)
                )
            }
            Stmt::Block(body) => format!("{{\n{}\n}}", self.statements(body)),
            Stmt::If(condition, yes, no) => {
                let mut output = format!(
                    "if ({}) {{\n{}\n}}",
                    self.expression(condition),
                    self.statement(yes)
                );
                let mut cur = no.as_deref();
                while let Some(node) = cur {
                    if let Stmt::If(next_cond, next_yes, next_no) = node.unlocated() {
                        output.push_str(&format!(
                            " {}else if ({}) {{\n{}\n}}",
                            self.marker(node),
                            self.expression(next_cond),
                            self.statement(next_yes)
                        ));
                        cur = next_no.as_deref();
                    } else {
                        output.push_str(&format!(" else {{\n{}\n}}", self.statement(node)));
                        break;
                    }
                }
                output
            }
            Stmt::While(condition, body) => format!(
                "while ({}) {{\n{}\n}}",
                self.expression(condition),
                self.statement(body)
            ),
            Stmt::For(init, condition, update, body) => format!(
                "for ({} {}; {}) {{\n{}\n}}",
                self.statement(init),
                condition
                    .as_ref()
                    .map(|x| self.expression(x))
                    .unwrap_or_default(),
                update
                    .as_ref()
                    .map(|x| self.expression(x))
                    .unwrap_or_default(),
                self.statement(body)
            ),
            Stmt::ForIn(binding, collection, values, body) => {
                let (target, initial) = match binding.unlocated() {
                    Stmt::Var(vars) => {
                        (Expr::Name(vars[0].0.clone()), Some(self.statement(binding)))
                    }
                    Stmt::Expr(value) => (value.clone(), None),
                    _ => unreachable!("validated enumeration target"),
                };
                let assign = Expr::Assign(
                    "=".into(),
                    Box::new(target),
                    Box::new(Expr::Literal("__nfItem".into())),
                );
                let iterator = format!(
                    "__nfEnumerate({}, {values}, {}{})",
                    self.expression(collection),
                    self.as3,
                    if self.legacy_case() { ", true" } else { "" }
                );
                let iterator = initial.map_or(iterator.clone(), |initial| {
                    format!("({}, {iterator})", initial.trim_end_matches(';'))
                });
                format!(
                    "for (const __nfItem of {iterator}) {{\n{};\n{}\n}}",
                    self.expression(&assign),
                    self.statement(body)
                )
            }
            Stmt::Break(label) => format!(
                "break{};",
                label.as_ref().map_or(String::new(), |l| format!(" {l}"))
            ),
            Stmt::Continue(label) => format!(
                "continue{};",
                label.as_ref().map_or(String::new(), |l| format!(" {l}"))
            ),
            Stmt::Label(name, body) => format!("{name}: {}", self.statement(body)),
            Stmt::DoWhile(body, test) => format!(
                "do {{\n{}\n}} while ({});",
                self.statement(body),
                self.expression(test)
            ),
            Stmt::Switch(value, cases) => format!(
                "switch ({}) {{\n{}\n}}",
                self.switch_value(value),
                cases
                    .iter()
                    .map(|c| format!(
                        "{}:\n{}",
                        c.test.as_ref().map_or_else(
                            || "default".into(),
                            |e| format!("case {}", self.switch_value(e))
                        ),
                        self.statements(&c.body)
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Stmt::Throw(value) => {
                if self.as3 {
                    format!("throw {};", self.expression(value))
                } else {
                    format!(
                        "throw {{ [__nfThrownBrand]: true, value: {} }};",
                        self.expression(value)
                    )
                }
            }
            Stmt::Try(body, catches, finally) => {
                let mut out = format!("try {{\n{}\n}}", self.statements(body));
                if !catches.is_empty() {
                    // Each nested catch owns its generated temporaries and lexical binding.
                    let id = catches[0].id;
                    let caught = format!("__nfCaught{id}");
                    let value = format!("__nfValue{id}");
                    out.push_str(&format!(" catch ({caught}) {{\n"));
                    if !self.as3 {
                        out.push_str(&format!("if (!__nfIsThrown({caught})) throw {caught};\n"));
                    }
                    out.push_str(&format!(
                        "const {value} = {caught}{};\n",
                        if self.as3 { "" } else { ".value" }
                    ));
                    for (index, catch) in catches.iter().enumerate() {
                        let inner = Emitter {
                            as3: self.as3,
                            analysis: self.analysis,
                            scope: catch.id,
                            with_targets: self.with_targets.clone(),
                            tell_target: self.tell_target.clone(),
                        };
                        let condition = catch.ty.as_deref().filter(|t| *t != "*").map_or_else(
                            || "true".into(),
                            |ty| {
                                if self.as3
                                    && self.analysis.context.project
                                    && !is_error_type(ty)
                                    && !matches!(
                                        ty,
                                        "Object"
                                            | "Number"
                                            | "String"
                                            | "Boolean"
                                            | "Function"
                                            | "int"
                                            | "uint"
                                    )
                                    && let Some(qname) = self.analysis.context.types.get(ty)
                                {
                                    format!("__nfProgram.is({value}, {qname:?})")
                                } else {
                                    format!("__nfCatchType({value}, {ty:?})")
                                }
                            },
                        );
                        out.push_str(&format!(
                            "{}if ({condition}) {{ let {} = {value};\n{}\n}} ",
                            if index == 0 { "" } else { "else " },
                            catch.name,
                            inner.statements(&catch.body)
                        ));
                    }
                    out.push_str(&format!("else {{ throw {caught}; }}\n}}"));
                }
                if let Some(finally) = finally {
                    out.push_str(&format!(" finally {{\n{}\n}}", self.statements(finally)));
                }
                out
            }
            Stmt::Function(_) => String::new(),
            Stmt::Return(value, _) => value.as_ref().map_or_else(
                || "return;".into(),
                |value| {
                    format!(
                        "return {};",
                        self.coerce_type(
                            self.analysis.scopes[&self.scope].returns.as_deref(),
                            self.expression(value)
                        )
                    )
                },
            ),
        }
    }
    fn statements(&self, statements: &[Stmt]) -> String {
        statements
            .iter()
            .map(|s| self.statement(s))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) fn compile(script: &Script) -> Result<String, Diagnostic> {
    compile_context(script, CompileContext::default())
}
pub(crate) fn compile_mapped(script: &Script, prefix: &str) -> Result<String, Diagnostic> {
    let mut script = script.clone();
    let legacy = script.language == ScriptLanguage::ActionScript1;
    if legacy {
        script.language = ScriptLanguage::ActionScript2;
    }
    compile_context(
        &script,
        CompileContext {
            legacy_case: legacy,
            mapping_prefix: Some(prefix.to_owned()),
            ..CompileContext::default()
        },
    )
}
pub(crate) fn compile_legacy(script: &Script) -> Result<String, Diagnostic> {
    compile_context(
        script,
        CompileContext {
            legacy_case: true,
            ..CompileContext::default()
        },
    )
}

// One helper profile for standalone handlers and project methods. Helpers are
// lexically captured per compiled unit; JavaScript/TypeScript remain untouched.
fn embedded_helpers() -> String {
    [
        include_str!("actionscript/exceptions.js"),
        include_str!("actionscript/intrinsics.js"),
        include_str!("actionscript/functions.js"),
        include_str!("actionscript/collections.js"),
        include_str!("actionscript/dates.js"),
        include_str!("actionscript/regexp.js"),
        include_str!("actionscript/strings.js"),
        include_str!("actionscript/vectors.js"),
        include_str!("actionscript/dictionary.js"),
        include_str!("actionscript/containers.js"),
        include_str!("actionscript/json.js"),
        r#"
// XML state/settings belong to the publication host, shared by every source unit.
function __nfXMLClass(name,as3) {
    if(typeof __nfXML==='undefined'||!__nfXML)throw new __nfTypeError('XML service is not available in this publication.');
    const key=!as3&&name==='XML'?'LegacyXML':name;
    const ctor=__nfXML[key];
    if(typeof ctor!=='function')throw new __nfTypeError('Unsupported XML native type '+name);
    return ctor;
}
function __nfXMLCast(name,value) {
    if(value==null)return null;
    if(__nfXMLClass(name,true).__nfIs(value))return value;
    throw new __nfTypeError('AS3 '+name+' value required; use the XML/XMLList constructor for conversion.');
}
function __nfXMLSwitch(value) {
    if(typeof __nfXML!=='undefined'&&__nfXML) {
        const kind=__nfXML.kind(value);
        if(kind==='xml'||kind==='list'||kind==='namespace'||kind==='qname')throw new __nfTypeError('E4X XML switch equality is not implemented; explicitly convert the selector and case values.');
    }
    return value;
}
function __nfXMLBinary(op,left,right) {
    if(typeof __nfXML!=='undefined'&&__nfXML) {
        const a=__nfXML.kind(left),b=__nfXML.kind(right);
        if(op==='+') {
            if((a==='xml'||a==='list')&&(b==='xml'||b==='list'))return __nfXML.concat(left,right);
            if(a==='xml'||a==='list'||b==='xml'||b==='list')return __nfString(left)+__nfString(right);
        } else if(a==='xml'||a==='list'||b==='xml'||b==='list'||a==='namespace'||a==='qname'||b==='namespace'||b==='qname') {
            if(op==='=='||op==='!=') {
                const eq=__nfXML.equals(left,right);
                return op==='=='?eq:!eq;
            }
            if(op==='==='||op==='!==') {
                const eq=__nfXML.strictEquals(left,right);
                return op==='==='?eq:!eq;
            }
        }
    }
    switch(op){case '+':return left+right;case '==':return left==right;case '!=':return left!=right;case '===':return left===right;case '!==':return left!==right;}
    throw new __nfTypeError('Unsupported XML operator.');
}
const __nfPrimitiveTypes = new Map();
const __nfObjectType=Object, __nfFunctionType=Function, __nfArrayType=Array;
function __nfPrimitiveDescriptor(name) {
    if (__nfPrimitiveTypes.has(name)) return __nfPrimitiveTypes.get(name);
    const nullable=value=>value==null?null:value;
    let token,coerce,defaultValue=null;
    switch(name) {
      case 'int':token=Symbol.for('neoflash.vector.element.int');coerce=value=>__nfNumber(value)|0;defaultValue=0;break;
      case 'uint':token=Symbol.for('neoflash.vector.element.uint');coerce=value=>__nfNumber(value)>>>0;defaultValue=0;break;
      case 'Number':token=__nfNumber;coerce=__nfNumber;defaultValue=0;break;
      case 'Boolean':token=__nfBoolean;coerce=__nfBoolean;defaultValue=false;break;
      case 'String':token=__nfString;coerce=value=>value==null?null:__nfString(value);break;
      case '*':token=Symbol.for('neoflash.vector.element.any');coerce=nullable;break;
      case 'Object':token=__nfObjectType;coerce=nullable;break;
      case 'Function':token=__nfFunctionType;coerce=value=>{if(value==null)return null;if(typeof value==='function'&&__nfNumericClassName(value)===null)return value;throw new __nfTypeError('AS3 Function value required');};break;
      case 'Class':token=Symbol.for('neoflash.vector.element.Class');coerce=__nfClassCast;break;
      case 'Array':token=__nfArrayType;coerce=value=>{if(value==null)return null;if(__nfArrayIsArray(value))return value;throw new __nfTypeError('AS3 Array value required');};break;
      case 'XML':case 'XMLList':case 'Namespace':case 'QName':token=__nfXMLClass(name,true);coerce=value=>__nfXMLCast(name,value);break;
      case 'Date':token=__nfDateNative;coerce=__nfDateCast;break;
      case 'RegExp':token=Symbol.for('neoflash.vector.element.RegExp');coerce=value=>{if(value==null)return null;if(__nfRegExpIs(value))return value;throw new __nfTypeError('AS3 RegExp value required');};break;
      default:
        if(!Object.hasOwn(__nfErrorTypes,name))throw new __nfTypeError('Unsupported Vector element type '+name);
        token=Symbol.for('neoflash.vector.element.error.'+name);
        coerce=value=>{if(value==null)return null;if(__nfCatchType(value,name))return value;throw new __nfTypeError('AS3 '+name+' value required');};
    }
    const descriptor=Object.freeze({token,coerce,defaultValue,name});__nfPrimitiveTypes.set(name,descriptor);return descriptor;
}
function __nfVectorDescriptor(element) {
    const token=__nfVectorType(element);
    return Object.freeze({token,coerce:value=>__nfVectorCast(value,token),defaultValue:null,name:'Vector.<'+element.name+'>'});
}
function __nfFamilyValue(value, as3) {
    value = __nfFunctionMethodValue(value);
    value = __nfArrayMethodValue(value);
    value = __nfStringMethodValue(value);
    value = __nfDateMethodValue(value);
    return as3 ? __nfMethodValue(value) : value;
}
function __nfFamilyPropertyValue(receiver, key, as3, caller, legacyCase) {
    return __nfFamilyValue(__nfContainerRef(receiver,key,as3,caller,legacyCase).get(), as3);
}
function __nfMemberMethod(receiver, key, as3, caller, legacyCase) {
    return __nfContainerMethod(receiver,key,as3,caller,legacyCase);
}
"#,
    ]
    .join("\n")
}

fn has_default_xml_ns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s.unlocated() {
        Stmt::DefaultXmlNamespace(_) => true,
        Stmt::Block(body) => has_default_xml_ns(body),
        Stmt::If(_, yes, no) => {
            has_default_xml_ns(std::slice::from_ref(yes))
                || no
                    .as_ref()
                    .is_some_and(|n| has_default_xml_ns(std::slice::from_ref(n)))
        }
        Stmt::While(_, body)
        | Stmt::With(_, body, _)
        | Stmt::TellTarget(_, body, _)
        | Stmt::IfFrameLoaded(_, body)
        | Stmt::For(_, _, _, body)
        | Stmt::ForIn(_, _, _, body)
        | Stmt::DoWhile(body, _)
        | Stmt::Label(_, body) => has_default_xml_ns(std::slice::from_ref(body)),
        Stmt::Switch(_, cases) => cases.iter().any(|c| has_default_xml_ns(&c.body)),
        Stmt::Try(body, catches, finally) => {
            has_default_xml_ns(body)
                || catches.iter().any(|c| has_default_xml_ns(&c.body))
                || finally.as_ref().is_some_and(|f| has_default_xml_ns(f))
        }
        _ => false,
    })
}

fn compile_context(script: &Script, mut context: CompileContext) -> Result<String, Diagnostic> {
    if script.language == ScriptLanguage::ActionScript3 {
        context
            .namespaces
            .entry("AS3".into())
            .or_insert_with(|| "http://adobe.com/AS3/2006/builtin".into());
        context
            .namespaces
            .entry("flash_proxy".into())
            .or_insert_with(|| "http://www.adobe.com/2006/actionscript/flash/proxy".into());
        context
            .namespaces
            .entry("object_proxy".into())
            .or_insert_with(|| "http://www.adobe.com/2006/flex/mx/internal".into());
    }
    let legacy_case = context.legacy_case;
    let tokens = TokenCursor::default();
    let mut parser = Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    };
    let statements = parser.statements(false)?;
    let analysis = scope::Analysis::with_context(script, &statements, parser.context)?;
    let raw_body = Emitter {
        as3: script.language == ScriptLanguage::ActionScript3,
        analysis: &analysis,
        scope: 0,
        with_targets: Vec::new(),
        tell_target: None,
    }
    .body(&statements);
    let body = if script.language == ScriptLanguage::ActionScript3
        && has_default_xml_ns(&statements)
    {
        format!(
            "const __nfPrevXmlNs = typeof __nfXML !== 'undefined' && __nfXML?.getDefaultNamespace ? __nfXML.getDefaultNamespace() : '';\ntry {{\n{raw_body}\n}} finally {{\nif (typeof __nfXML !== 'undefined' && __nfXML?.setDefaultNamespace) __nfXML.setDefaultNamespace(__nfPrevXmlNs);\n}}"
        )
    } else {
        raw_body
    };
    let legacy_prefix = if legacy_case {
        "const __nfLegacyCase = true;\n"
    } else {
        ""
    };
    let output = format!(
        "{legacy_prefix}const __nfFrame = this, __nfNumber = Number, __nfString = String, __nfBoolean = Boolean, __nfTypeError = TypeError;\nconst __nfTrace = trace;\n{}\n{body}",
        embedded_helpers()
    );
    // Validate literal spelling and the generated function body with an independent parser.
    let generated = Script {
        language: ScriptLanguage::JavaScript,
        source: output.clone(),
    };
    if let Err(errors) = crate::compile_web(&generated, false) {
        return Err(Diagnostic::at(
            script,
            0,
            format!(
                "This ActionScript syntax cannot be lowered safely yet: {}",
                errors[0].message
            ),
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compile_as(source: &str) -> Result<String, Diagnostic> {
        compile(&Script {
            language: ScriptLanguage::ActionScript3,
            source: source.into(),
        })
    }

    #[test]
    fn timeline_methods_and_strings_are_distinct_tokens() {
        let output =
            compile_as("// stop();\ntrace('stop() _x'); child.gotoAndPlay(2); stop();").unwrap();
        assert!(output.contains("__nfTrace('stop() _x')"));
        assert!(output.contains("const __nfTrace = trace;"));
        assert!(!output.contains("console.log"));
        assert!(output.contains("__nfContainerMethod(this.child, \"gotoAndPlay\", true)(2)"));
        assert!(output.contains("this.stop()"));
    }
    #[test]
    fn operators_preserve_precedence() {
        let output = compile_as("x = 2 + 3 * 4; y = (2 + 3) * 4; flag = x > 3 && y < 50;").unwrap();
        assert!(output.contains("this.x = __nfXMLBinary(\"+\", 2, (3 * 4))"));
        assert!(output.contains("this.y = (__nfXMLBinary(\"+\", 2, 3) * 4)"));
    }
    #[test]
    fn assignments_are_right_associative_and_require_lvalues() {
        assert!(
            compile_as("x = y = 4;")
                .unwrap()
                .contains("this.x = (this.y = 4)")
        );
        assert!(compile_as("(x + 2) = 4;").is_err());
    }
    #[test]
    fn control_flow_and_persistent_variables() {
        let output = compile_as(
            "var speed:Number = 2; if (_x < 10) { _x += speed; } else { gotoAndStop(1); }",
        )
        .unwrap();
        assert!(output.contains("this.speed = __nfNumber(2)"));
        assert!(output.contains("this._x = __nfXMLBinary(\"+\", this._x, this.speed)"));
        assert!(output.contains("this.gotoAndStop(1)"));
    }
    #[test]
    fn typed_int_assignments_wrap_and_defaults_are_defined() {
        let output = compile_as("var count:int; count = 4294967295; var text:String;").unwrap();
        assert!(output.contains("this.count = 0"));
        assert!(output.contains("Number(4294967295) | 0"));
        assert!(output.contains("this.text = null"));
    }
    #[test]
    fn as2_annotations_are_not_as3_runtime_coercions() {
        let output = compile(&Script {
            language: ScriptLanguage::ActionScript2,
            source: "var n:Number = 2; _x += n;".into(),
        })
        .unwrap();
        assert!(output.contains("this.n = 2"));
        assert!(!output.contains("Number(2)"));
        let events = compile(&Script {language:ScriptLanguage::ActionScript2,source:"on(press,release) { trace(this); } onClipEvent(enterFrame) { this.tick=1; } var a=[]; trace(a instanceof Array);".into()}).unwrap();
        assert!(events.contains("addLegacyEvent"));
        assert!(events.contains("__nfArrayIsArray(__nfValue)"));
    }
    #[test]
    fn loops_and_math_compile() {
        let output =
            compile_as("for (var i:int = 0; i < 10; i++) { x += Math.sin(i); if (x > 4) break; }")
                .unwrap();
        assert!(output.contains("for ("));
        assert!(output.contains("__nfContainerMethod(__nfMath(true), \"sin\", true)(this.i)"));
    }
    #[test]
    fn missing_compatibility_features_fail_explicitly() {
        assert!(compile_as("var x:XML = <a/>; x.(@id==1);").is_ok());
        for source in [
            "x = y is MissingType;",
            "on(release) { play(); }",
            "getURL('https://example.com');",
            "import flash.display.Sprite;",
            "foo.addEventListener('click', handler);",
        ] {
            assert!(compile_as(source).is_err(), "must reject {source}");
        }
    }
    #[test]
    fn errors_point_to_user_line() {
        let error = compile_as("play();\n  package sample {}").unwrap_err();
        assert_eq!((error.line, error.column), (2, 3));
    }
    #[test]
    fn no_silent_remainder_and_unterminated_comments() {
        assert!(compile_as("play(); garbage ???").is_err());
        assert!(compile_as("/* unclosed").is_err());
        assert!(compile_as("trace('unclosed)").is_err());
        assert!(compile_as("x = 1e;").is_err());
    }

    #[test]
    fn rejects_extreme_expression_depth_without_overflowing() {
        let source = format!("x = {};", vec!["1"; 1000].join(" + "));
        let error = compile_as(&source).unwrap_err();
        assert!(error.message.contains("128 nested operations"));
    }

    #[test]
    fn functions_are_resolved_after_declarations_and_locals_are_lexical() {
        let code=compile_as("result = twice(4); function twice(value:int):int { var local:int = value * 2; return local; }").unwrap();
        assert!(code.contains("var local = 0"));
        assert!(!code.contains("this.local"));
        assert!(code.find("this.twice =").unwrap() < code.find("this.result =").unwrap());
        assert!(compile_as("function make(x:int):Function { return function(y:int):int { return x + y; }; } var f:Function = make(2); answer = f(3);").is_ok());
    }

    #[test]
    fn function_semantics_outside_the_supported_boundary_are_diagnostics() {
        for source in [
            "return 1;",
            "function f(a:int = 2, b:int):int { return a+b; }",
            "function f(a:int = Number(2)):int { return a; }",
            "function f(a:Object = {}):Object { return a; }",
            "function f(...args:int) {}",
            "function f(...args, later) {}",
            "function f(...args) { return arguments.length; }",
            "function f(arguments) {}",
            "function f(){ var arguments = []; }",
            "arguments[0];",
            "function f(a, a) {}",
            "function f():void { return 1; }",
            "function f():int { return; }",
            "function f(x:int):int { if (x > 0) return x; }",
            "while (true) { var f = function(){ break; }; }",
            "if (true) { function f() {} }",
            "var f = function named(){ named = 2; };",
            "function f(){ var fn:Number = 2; fn(); }",
            "var n:Number = function() {}; n();",
            "function f(){ return 1; } child.f();",
            "var obj = {}; obj.unavailable();",
            "function f(trace:Number) { trace(); }",
            "function f(Math) { Math.sin(1); }",
            "function __nfFrame() {}",
            "function f(__nfNumber) {}",
            "function f(){ var __nfValue; }",
            "Number = 3;",
            "trace++;",
        ] {
            assert!(compile_as(source).is_err(), "must reject {source}");
        }
        for source in [
            "function f(a:int = 2):int { return a; }",
            "function f(a:String = null,...args:Array):int { return args.length; }",
            "function f(){ return arguments[0]; }",
            "var f=function(){return this.x;}; f.call(this); f['apply'](this,[]);",
            "var f=function(n:int):int {if(n<=1)return 1;return n*arguments.callee(n-1);};",
        ] {
            assert!(compile_as(source).is_ok(), "must accept {source}");
        }
        for source in ["function f(a=2){}", "function f(...rest){}"] {
            assert!(
                compile(&Script {
                    language: ScriptLanguage::ActionScript2,
                    source: source.into()
                })
                .is_err()
            );
        }
        let error = compile_as("function ok() {}\nmissing();").unwrap_err();
        assert_eq!(error.line, 2);
    }

    #[test]
    fn explicit_user_function_members_do_not_unlock_unrelated_apis() {
        assert!(
            compile_as("this.custom = function(x:int):int { return x; }; this.custom(2);").is_ok()
        );
        assert!(compile_as("var tools = { go: function() {} }; tools.go();").is_ok());
        assert!(compile_as("var tools = { go: function() {} }; other.go();").is_err());
        assert!(compile_as(r#"var tools = {"r\u0075n": function() {} }; tools.run();"#).is_ok());
        assert!(compile_as(r#"var tools = {"\uZZZZ": function() {} };"#).is_err());
        assert!(
            compile_as(
                "function a() { var tools = { go: function() {} }; } function b() { tools.go(); }"
            )
            .is_err()
        );
    }

    #[test]
    fn as2_function_types_erase_but_as3_primitive_returns_require_values() {
        assert!(
            compile(&Script {
                language: ScriptLanguage::ActionScript2,
                source: "function f(x:Number):Number { return; } f();".into()
            })
            .is_ok()
        );
        assert!(
            compile(&Script {
                language: ScriptLanguage::ActionScript2,
                source: "function f():Void { return; }".into()
            })
            .is_ok()
        );
        assert!(compile_as("function f():int { return; }").is_err());
        assert!(compile_as("function f():int { if (true) return 1; else return 2; }").is_ok());
    }
}
