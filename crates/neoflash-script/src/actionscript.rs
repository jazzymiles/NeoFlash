//! A tokenizing/recursive-descent + Pratt frontend, not a text substitution pass.

use crate::Diagnostic;
use neoflash_core::{Script, ScriptLanguage};
use std::collections::HashMap;
mod completion;
#[cfg(test)]
mod control_tests;
pub(crate) mod project;
mod scope;

#[derive(Clone, Default)]
struct CompileContext {
    types: std::collections::BTreeMap<String, String>,
    functions: std::collections::BTreeMap<String, String>,
    interfaces: std::collections::BTreeSet<String>,
    members: std::collections::BTreeMap<String, Option<String>>,
    static_members: std::collections::BTreeMap<String, String>,
    static_method: bool,
    class_name: Option<String>,
    method_id: Option<usize>,
    project: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Ident(String),
    Number(String),
    String(String),
    Punct(&'static str),
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    offset: usize,
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
    fn tokens(mut self) -> Result<Vec<Token>, Diagnostic> {
        let mut tokens = Vec::new();
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
            } else {
                self.offset = start;
                const OPERATORS: &[&str] = &[
                    ">>>=", "===", "!==", ">>>", "<<=", ">>=", "&&=", "||=", "++", "--", "+=",
                    "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "&=",
                    "|=", "^=", "::", "..", "{", "}", "(", ")", "[", "]", ";", ",", ".", ":", "?",
                    "+", "-", "*", "/", "%", "=", "!", "~", "<", ">", "&", "|", "^", "@",
                ];
                let Some(operator) = OPERATORS
                    .iter()
                    .find(|operator| self.rest().starts_with(**operator))
                else {
                    return Err(self.error(
                        start,
                        format!("Unsupported character {c:?} in ActionScript."),
                    ));
                };
                self.offset += operator.len();
                Kind::Punct(operator)
            };
            tokens.push(Token {
                kind,
                offset: start,
            });
            if tokens.len() > 100_000 {
                return Err(self.error(start, "Script exceeds the 100,000-token limit."));
            }
        }
        tokens.push(Token {
            kind: Kind::End,
            offset: self.offset,
        });
        Ok(tokens)
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Literal(String),
    Name(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(PropertyKey, Expr)>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Assign(String, Box<Expr>, Box<Expr>),
    Update(String, Box<Expr>, bool),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, usize),
    Function(Function),
    NewError(String, Vec<Expr>, usize),
    NewClass(String, Vec<Expr>, usize),
    NewBuiltin(String, Vec<Expr>, usize),
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
    returns: Option<String>,
    body: Vec<Stmt>,
    declaration: bool,
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
            Self::Literal(_) | Self::Name(_) | Self::Function(_) => 0,
            Self::Member(v, _) | Self::Unary(_, v) | Self::Update(_, v, _) => v.depth(),
            Self::Index(a, b) | Self::Binary(_, a, b) | Self::Assign(_, a, b) => {
                a.depth().max(b.depth())
            }
            Self::Array(v) => v.iter().map(Self::depth).max().unwrap_or(0),
            Self::Object(v) => v.iter().map(|(_, e)| e.depth()).max().unwrap_or(0),
            Self::Conditional(a, b, c) => a.depth().max(b.depth()).max(c.depth()),
            Self::Call(c, args, _) => c
                .depth()
                .max(args.iter().map(Self::depth).max().unwrap_or(0)),
            Self::NewError(_, args, _)
            | Self::NewClass(_, args, _)
            | Self::NewBuiltin(_, args, _) => args.iter().map(Self::depth).max().unwrap_or(0),
        }
    }
    fn assignable(&self) -> bool {
        match self {
            // Writes to globals are checked after lexical scope resolution.
            Self::Name(name) => name != "this",
            Self::Member(_, _) | Self::Index(_, _) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
enum Stmt {
    Empty,
    Expr(Expr),
    Var(Vec<(String, Option<String>, Option<Expr>)>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For(Box<Stmt>, Option<Expr>, Option<Expr>, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Switch(Expr, Vec<Case>),
    Label(String, Box<Stmt>),
    Break(Option<String>),
    Continue(Option<String>),
    Throw(Expr),
    Try(Vec<Stmt>, Vec<CatchClause>, Option<Vec<Stmt>>),
    Function(Function),
    Return(Option<Expr>, usize),
}

struct Frontend<'a> {
    script: &'a Script,
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    controls: Vec<ControlTarget>,
    function_depth: usize,
    next_function: usize,
    context: CompileContext,
}

impl<'a> Frontend<'a> {
    fn token(&self) -> &Token {
        &self.tokens[self.pos]
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
        Diagnostic::at(self.script, self.token().offset, message)
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
            && self.script.source[self.tokens[self.pos - 1].offset..self.token().offset]
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
        let statement = self.statement_inner()?;
        self.depth -= 1;
        Ok(statement)
    }
    fn statement_inner(&mut self) -> Result<Stmt, Diagnostic> {
        if matches!(self.token().kind, Kind::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
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
            while matches!(self.tokens.get(at).map(|t| &t.kind), Some(Kind::Ident(_)))
                && self
                    .tokens
                    .get(at + 1)
                    .is_some_and(|t| t.kind == Kind::Punct(":"))
            {
                at += 2;
            }
            let iteration = matches!(self.tokens.get(at).map(|t| &t.kind), Some(Kind::Ident(s)) if matches!(s.as_str(), "for" | "while" | "do"));
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
            let value = self.expression(0)?;
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
                        self.ident()?
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
            let value = self.expression(0)?;
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
                    Some(self.expression(0)?)
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
            let test = self.expression(0)?;
            self.expect(")")?;
            self.eat(";");
            return Ok(Stmt::DoWhile(Box::new(body), test));
        }
        if self.is("function") {
            return Ok(Stmt::Function(self.function(true)?));
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
                Some(self.expression(0)?)
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
            self.expect("(")?;
            let condition = self.expression(0)?;
            self.expect(")")?;
            let yes = Box::new(self.statement()?);
            let no = if self.eat("else") {
                Some(Box::new(self.statement()?))
            } else {
                None
            };
            return Ok(Stmt::If(condition, yes, no));
        }
        if self.eat("while") {
            self.expect("(")?;
            let condition = self.expression(0)?;
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
            self.expect("(")?;
            let init = if self.is(";") {
                Stmt::Empty
            } else if self.eat("var") {
                self.variables()?
            } else {
                Stmt::Expr(self.expression(0)?)
            };
            self.expect(";")?;
            let condition = if self.is(";") {
                None
            } else {
                Some(self.expression(0)?)
            };
            self.expect(";")?;
            let update = if self.is(")") {
                None
            } else {
                Some(self.expression(0)?)
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
            && is_unsupported_keyword(word)
            && word != "new"
            && !(self.context.project && word == "super" && self.context.class_name.is_some())
        {
            return Err(self.error(format!("ActionScript '{word}' is not implemented yet. See docs/COMPATIBILITY.md; use a JavaScript/TypeScript handler for this feature.")));
        }
        let expression = self.expression(0)?;
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
        let mut name = if self.eat("*") {
            "*".into()
        } else {
            self.ident()?
        };
        while self.context.project && self.eat(".") {
            name.push('.');
            name.push_str(&self.ident()?);
        }
        let as3 = self.script.language == ScriptLanguage::ActionScript3;
        let supported = matches!(
            name.as_str(),
            "Number" | "String" | "Boolean" | "Function" | "Array" | "Object"
        ) || (as3 && matches!(name.as_str(), "int" | "uint" | "*"))
            || (!as3 && name == "LoadVars")
            || (self.context.project && self.context.types.contains_key(&name))
            || (is_error_type(&name) && (as3 || name == "Error"))
            || (returns && name == if as3 { "void" } else { "Void" });
        if !supported {
            return Err(self.error(format!("Type '{name}' is not supported in this ActionScript annotation. Supported types are primitive types and Function; void/Void is return-only.")));
        }
        Ok(Some(name))
    }
    fn function(&mut self, declaration: bool) -> Result<Function, Diagnostic> {
        self.project_function(declaration, false, false)
            .map(|(function, _)| function)
    }
    fn project_function(
        &mut self,
        declaration: bool,
        member: bool,
        signature_only: bool,
    ) -> Result<(Function, Option<String>), Diagnostic> {
        let offset = self.token().offset;
        self.expect("function")?;
        let accessor = if member
            && (self.is("get") || self.is("set"))
            && matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.kind),
                Some(Kind::Ident(_))
            ) {
            let kind = self.ident()?;
            if self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error("Project get/set accessors currently require AS3; AS2 accessor semantics remain unsupported."));
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
        if !self.is(")") {
            loop {
                if self.is("..") {
                    return Err(self.error("Rest parameters are not implemented yet."));
                }
                let name = self.ident()?;
                if invalid_binding(&name, true) {
                    return Err(self.error("Invalid or reserved parameter name."));
                }
                let ty = self.annotation(false)?;
                if self.is("=") {
                    return Err(self.error("Default parameters are not implemented yet."));
                }
                params.push((name, ty));
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
        } else {
            self.expect("{")?;
            let outer_controls = std::mem::take(&mut self.controls);
            self.function_depth += 1;
            let body = self.statements(true)?;
            self.function_depth -= 1;
            self.controls = outer_controls;
            body
        };
        Ok((
            Function {
                id,
                offset,
                name,
                params,
                returns,
                body,
                declaration,
            },
            accessor,
        ))
    }
    fn expression(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let result = self.expression_inner(min_bp)?;
        self.depth -= 1;
        Ok(result)
    }
    fn expression_inner(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs =
            if self.is("!") || self.is("~") || self.is("-") || self.is("+") || self.is("typeof") {
                let operator = self.token_text();
                self.pos += 1;
                Expr::Unary(operator, Box::new(self.expression(25)?))
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
            if min_bp <= 30 && self.eat(".") {
                lhs = Expr::Member(Box::new(lhs), self.ident()?);
                continue;
            }
            if min_bp <= 30 && self.eat("[") {
                let index = self.expression(0)?;
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
            if matches!(
                operator.as_str(),
                "as" | "is" | "in" | "instanceof" | "::" | ".." | "@"
            ) {
                return Err(self.error(format!("ActionScript operator '{operator}' requires compatibility semantics not implemented yet.")));
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
            Kind::End => String::new(),
        }
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        if self.eat("new") {
            let offset = self.token().offset;
            let mut name = self.ident()?;
            while self.context.project && self.eat(".") {
                name.push('.');
                name.push_str(&self.ident()?);
            }
            let class = self.context.types.get(&name).cloned();
            let builtin = matches!(name.as_str(), "Array" | "String")
                || (self.script.language == ScriptLanguage::ActionScript2 && name == "LoadVars");
            if !builtin
                && class.is_none()
                && (!is_error_type(&name)
                    || (self.script.language == ScriptLanguage::ActionScript2 && name != "Error"))
            {
                return Err(self.error("Only Array, String and core Error constructors are supported here; user classes require project type resolution."));
            }
            self.expect("(")?;
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
            if builtin {
                return Ok(Expr::NewBuiltin(name, args, offset));
            }
            if let Some(name) = class {
                return Ok(Expr::NewClass(name, args, offset));
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
            return Ok(Expr::Function(self.function(false)?));
        }
        let token = self.token().clone();
        match token.kind {
            Kind::Number(value) | Kind::String(value) => { self.pos += 1; Ok(Expr::Literal(value)) }
            Kind::Ident(name) => {
                if (is_unsupported_keyword(&name) && !(name == "super" && self.context.class_name.is_some())) || matches!(name.as_str(), "if" | "else" | "while" | "for" | "var" | "break" | "continue" | "return") {
                    return Err(self.error(format!("ActionScript construct '{name}' is not supported in this expression.")));
                }
                self.pos += 1;
                if matches!(name.as_str(), "true" | "false" | "null" | "undefined" | "NaN" | "Infinity") {
                    Ok(Expr::Literal(name))
                } else { Ok(Expr::Name(name)) }
            }
            Kind::Punct("(") => { self.pos += 1; let expr = self.expression(0)?; self.expect(")")?; Ok(expr) }
            Kind::Punct("[") => {
                self.pos += 1; let mut values = Vec::new();
                if !self.is("]") { loop { values.push(self.expression(0)?); if !self.eat(",") || self.is("]") { break; } } }
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
            Kind::Punct("<" | "@" | "::" | "..") => Err(self.error("XML/E4X and namespace operators are not implemented yet.")),
            Kind::Punct("/") => Err(self.error("Regular expression literals are not implemented in ActionScript yet; use a JavaScript handler.")),
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
                        | "Number"
                        | "String"
                        | "Array"
                        | "Boolean"
                        | "parseInt"
                        | "parseFloat"
                        | "isNaN"
                        | "isFinite"
                ) || is_timeline_method(name)
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
            Expr::Member(_, name) => {
                is_timeline_method(name)
                    || is_family_method(name)
                    || matches!(
                        name.as_str(),
                        "trace" | "appendText" | "replaceText" | "getLineText" | "getLineLength"
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

fn is_family_method(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "join"
            | "slice"
            | "concat"
            | "reverse"
            | "splice"
            | "sort"
            | "sortOn"
            | "indexOf"
            | "lastIndexOf"
            | "map"
            | "filter"
            | "forEach"
            | "every"
            | "some"
            | "charAt"
            | "charCodeAt"
            | "substring"
            | "substr"
            | "split"
            | "toLowerCase"
            | "toUpperCase"
            | "replace"
            | "toString"
            | "valueOf"
            | "fromCharCode"
            | "toFixed"
    )
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
            | "arguments"
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
        "||" => (3, 4, false),
        "&&" => (5, 6, false),
        "|" => (7, 8, false),
        "^" => (9, 10, false),
        "&" => (11, 12, false),
        "==" | "!=" | "===" | "!==" => (13, 14, false),
        "<" | "<=" | ">" | ">=" => (15, 16, false),
        "<<" | ">>" | ">>>" => (17, 18, false),
        "+" | "-" => (19, 20, false),
        "*" | "/" | "%" => (21, 22, false),
        _ => return None,
    })
}

struct Emitter<'a> {
    as3: bool,
    analysis: &'a scope::Analysis<'a>,
    scope: usize,
}

impl Emitter<'_> {
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
            Some("Number") => format!("{number}({value})"),
            Some("Boolean") => format!("{boolean}({value})"),
            Some("int") => format!("({number}({value}) | 0)"),
            Some("uint") => format!("({number}({value}) >>> 0)"),
            Some("String") => {
                format!("((__nfValue) => __nfValue == null ? null : {string}(__nfValue))({value})")
            }
            Some("Function") => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (typeof __nfValue !== 'function') throw new __nfTypeError('AS3 Function value required'); return __nfValue; }})({value})"
            ),
            Some("Object") => {
                format!("((__nfValue) => __nfValue == null ? null : __nfValue)({value})")
            }
            Some("Array") => format!(
                "((__nfValue) => {{ if (__nfValue == null || __nfArrayIsArray(__nfValue)) return __nfValue == null ? null : __nfValue; throw new __nfTypeError('AS3 Array value required'); }})({value})"
            ),
            Some(ty) if is_error_type(ty) => format!(
                "((__nfValue) => {{ if (__nfValue == null) return null; if (__nfCatchType(__nfValue, {ty:?})) return __nfValue; throw new __nfTypeError('AS3 {ty} value required'); }})({value})"
            ),
            Some(ty) if self.analysis.context.types.contains_key(ty) => format!(
                "__nfProgram.cast({:?}, {value})",
                self.analysis.context.types[ty]
            ),
            _ => value,
        }
    }
    fn intrinsic(&self, name: &str) -> String {
        format!("__nf{name}")
    }
    fn typed(&self, name: &str) -> bool {
        self.analysis
            .resolve(self.scope, name)
            .is_some_and(|(_, b)| b.ty.is_some())
    }
    fn name(&self, name: &str) -> String {
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
            && matches!(name, "Key" | "Mouse" | "Stage" | "LoadVars")
        {
            return format!("__nfLegacy.{name}");
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, binding)| id == 0 && binding.kind == scope::BindingKind::Field)
            && let Some(owner) = self.analysis.context.static_members.get(name)
        {
            return format!("__nfProgram.type({owner:?}).{name}");
        }
        if self.analysis.resolve(self.scope, name).is_none() && matches!(name, "Array" | "String") {
            return format!("__nf{name}Class({})", self.as3);
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
            return format!("__nfProgram.type({qname:?})");
        }
        if name == "parseInt" && self.analysis.resolve(self.scope, name).is_none() {
            return if self.as3 {
                "__nfParseInt3"
            } else {
                "__nfParseInt2"
            }
            .into();
        }
        if self
            .analysis
            .resolve(self.scope, name)
            .is_some_and(|(id, _)| id != 0)
        {
            return name.into();
        }
        if name == "this" || (is_global(name) && !is_timeline_method(name) && name != "trace") {
            name.into()
        } else if name == "trace" {
            "__nfTrace".into()
        } else {
            format!(
                "{}.{name}",
                if self.scope == 0 { "this" } else { "__nfFrame" }
            )
        }
    }
    fn default_value(&self, ty: Option<&str>) -> &'static str {
        if !self.as3 {
            return "undefined";
        }
        match ty {
            Some("Number") => "NaN",
            Some("Boolean") => "false",
            Some("String" | "Function" | "Object" | "Array") => "null",
            Some(ty) if is_error_type(ty) => "null",
            Some("int" | "uint") => "0",
            Some(ty) if self.analysis.context.types.contains_key(ty) => "null",
            _ => "undefined",
        }
    }
    fn function(&self, f: &Function) -> String {
        let inner = Emitter {
            as3: self.as3,
            analysis: self.analysis,
            scope: f.id,
        };
        let mut prologue = String::new();
        if self.analysis.context.method_id == Some(f.id) {
            prologue.push_str("const __nfFrame = this;\n");
        }
        if self.as3 {
            prologue.push_str(&format!("if (arguments.length !== {}) throw new __nfTypeError('AS3 function expects {} arguments');\n",f.params.len(),f.params.len()));
        }
        for (name, ty) in &f.params {
            if self.as3 && ty.is_some() {
                prologue.push_str(&format!("{name} = {};\n", inner.coerce(name, name.clone())));
            }
        }
        for (name, binding) in &self.analysis.scopes[&f.id].bindings {
            if binding.kind == scope::BindingKind::Variable {
                prologue.push_str(&format!(
                    "var {name} = {};\n",
                    inner.default_value(binding.ty.as_deref())
                ));
            }
        }
        let name = if f.declaration && self.scope == 0 {
            ""
        } else {
            f.name.as_deref().unwrap_or("")
        };
        format!(
            "function {name}({}) {{\n{prologue}{}\n}}",
            f.params
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            inner.body(&f.body)
        )
    }
    fn body(&self, body: &[Stmt]) -> String {
        let mut output = String::new();
        for statement in body {
            if let Stmt::Function(f) = statement {
                let code = self.function(f);
                if self.scope == 0 {
                    output.push_str(&format!(
                        "this.{} = ({code}){};\n",
                        f.name.as_deref().unwrap(),
                        if self.as3 { ".bind(__nfFrame)" } else { "" }
                    ));
                } else {
                    output.push_str(&format!("{code}\n"));
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
            && self
                .analysis
                .resolve(self.scope, name.split('.').next().unwrap())
                .is_none()
            && let Some(qname) = self
                .analysis
                .context
                .types
                .get(&name)
                .or_else(|| self.analysis.context.functions.get(&name))
        {
            return format!("__nfProgram.type({qname:?})");
        }
        match expr {
            Expr::Literal(value) => value.clone(),
            Expr::Name(name) => self.name(name),
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
            Expr::NewBuiltin(name, args, _) if name == "LoadVars" => format!(
                "new __nfLegacy.LoadVars({})",
                args.iter()
                    .map(|arg| self.expression(arg))
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
                format!(
                    "__nfFamilyPropertyValue({}, {name:?}, {})",
                    self.expression(base),
                    self.as3
                )
            }
            Expr::Index(base, index) => {
                format!(
                    "__nfFamilyPropertyValue({}, {}, {})",
                    self.expression(base),
                    self.expression(index),
                    self.as3
                )
            }
            Expr::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(|v| self.expression(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Object(values) => format!(
                "({{{}}})",
                values
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.source, self.expression(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Unary(op, value) => format!("({op} {})", self.expression(value)),
            Expr::Binary(op, left, right) => format!(
                "({} {op} {})",
                self.expression(left),
                self.expression(right)
            ),
            Expr::Assign(op, left, right) => {
                let target = self.lvalue(left);
                let value = self.expression(right);
                if !self.as3 && op == "=" {
                    let callback = match left.as_ref() {
                        Expr::Name(name)
                            if matches!(
                                name.as_str(),
                                "onEnterFrame" | "onPress" | "onRelease"
                            ) && self.analysis.resolve(self.scope, name).is_none() =>
                        {
                            Some(("__nfFrame".into(), name))
                        }
                        Expr::Member(base, name)
                            if matches!(
                                name.as_str(),
                                "onEnterFrame" | "onPress" | "onRelease"
                            ) =>
                        {
                            Some((self.expression(base), name))
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
                        format!("({target} {} {value})", &op[..op.len() - 1])
                    };
                    return format!("({target} = {})", self.coerce(name, value));
                }
                let assignment = format!("({target} {op} {value})");
                if self.as3 && matches!(op.as_str(), "&&=" | "||=") {
                    // A short-circuited indexed ||= may itself extract the old
                    // function value without passing through an ordinary read.
                    format!("__nfFamilyValue({assignment}, true)")
                } else {
                    assignment
                }
            }
            Expr::Update(op, value, prefix) => {
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
                if let Some(name) = qualified_name(callee)
                    && self
                        .analysis
                        .resolve(self.scope, name.split('.').next().unwrap())
                        .is_none()
                    && (self.analysis.context.functions.contains_key(&name)
                        || (!self.as3 && name == "getTimer"))
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
                if let Expr::Name(name) = callee.as_ref()
                    && self.analysis.resolve(self.scope, name).is_none()
                {
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
                    if name == "super"
                        && let Some(class) = &self.analysis.context.class_name
                    {
                        return format!(
                            "__nfProgram.superConstruct({class:?}, __nfFrame, [{}])",
                            args.iter()
                                .map(|a| self.expression(a))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
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
                }
                let member = match callee.as_ref() {
                    Expr::Member(base, name) => Some((base.as_ref(), name.clone())),
                    Expr::Index(base, index) => {
                        literal_property_name(index).map(|name| (base.as_ref(), name))
                    }
                    _ => None,
                };
                if let Some((base, name)) = &member
                    && is_family_method(name)
                {
                    return format!(
                        "__nfMemberMethod({}, {name:?}, {})({})",
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
                                && (!is_global(name)
                                    || matches!(name.as_str(), "gotoAndPlay" | "gotoAndStop")) =>
                        {
                            Some((
                                if self.scope == 0 {
                                    "this".into()
                                } else {
                                    "__nfFrame".into()
                                },
                                name,
                            ))
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
                            && let Expr::Member(base, name) = callee.as_ref()
                        {
                            format!("({}).{name}", self.expression(base))
                        } else if self.as3
                            && let Some((base, name)) = &member
                        {
                            format!("({})[{name:?}]", self.expression(base))
                        } else {
                            self.expression(callee)
                        }
                    },
                    |(receiver, name)| format!("__nfAs2Method({receiver}, {name:?})"),
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
            Stmt::Empty => ";".into(),
            Stmt::Expr(expr) => format!("{};", self.expression(expr)),
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
                        if initialized {
                            format!("{target} = {value}")
                        } else {
                            format!("({target} === undefined ? ({target} = {value}) : {target})")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Stmt::Block(body) => format!("{{\n{}\n}}", self.statements(body)),
            Stmt::If(condition, yes, no) => {
                let mut output = format!(
                    "if ({}) {{\n{}\n}}",
                    self.expression(condition),
                    self.statement(yes)
                );
                if let Some(no) = no {
                    output.push_str(&format!(" else {{\n{}\n}}", self.statement(no)));
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
                self.expression(value),
                cases
                    .iter()
                    .map(|c| format!(
                        "{}:\n{}",
                        c.test.as_ref().map_or_else(
                            || "default".into(),
                            |e| format!("case {}", self.expression(e))
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
                        };
                        let condition = catch.ty.as_deref().filter(|t| *t != "*").map_or_else(
                            || "true".into(),
                            |ty| format!("__nfCatchType({value}, {ty:?})"),
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

// One helper profile for standalone handlers and project methods. Helpers are
// lexically captured per compiled unit; JavaScript/TypeScript remain untouched.
fn embedded_helpers() -> String {
    [
        include_str!("actionscript/exceptions.js"),
        include_str!("actionscript/intrinsics.js"),
        include_str!("actionscript/collections.js"),
        include_str!("actionscript/strings.js"),
        r#"
function __nfFamilyValue(value, as3) {
    value = __nfArrayMethodValue(value);
    value = __nfStringMethodValue(value);
    return as3 ? __nfMethodValue(value) : value;
}
function __nfFamilyPropertyValue(receiver, key, as3) {
    return __nfFamilyValue(receiver[key], as3);
}
function __nfMemberMethod(receiver, key, as3) {
    if (__nfArrayIsArray(receiver)) return __nfArrayMethod(receiver, key, as3);
    if (typeof receiver === 'string') return __nfStringMethod(receiver, key, as3);
    if (as3 && key === 'toFixed') return __nfFixedMethod(receiver);
    // No family lookup has read this property. Preserve one getter invocation,
    // the original receiver and argument evaluation order for custom methods.
    const method = receiver[key];
    return (...args) => __nfApply(method, receiver, args);
}
"#,
    ]
    .join("\n")
}

fn compile_context(script: &Script, context: CompileContext) -> Result<String, Diagnostic> {
    let tokens = Lexer { script, offset: 0 }.tokens()?;
    let mut parser = Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context: context.clone(),
    };
    let statements = parser.statements(false)?;
    let analysis = scope::Analysis::with_context(script, &statements, context)?;
    let body = Emitter {
        as3: script.language == ScriptLanguage::ActionScript3,
        analysis: &analysis,
        scope: 0,
    }
    .body(&statements);
    let output = format!(
        "const __nfFrame = this, __nfNumber = Number, __nfString = String, __nfBoolean = Boolean, __nfTypeError = TypeError;\nconst __nfTrace = trace;\n{}\n{body}",
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
        assert!(output.contains("(this.child).gotoAndPlay(2)"));
        assert!(output.contains("this.stop()"));
    }
    #[test]
    fn operators_preserve_precedence() {
        let output = compile_as("x = 2 + 3 * 4; y = (2 + 3) * 4; flag = x > 3 && y < 50;").unwrap();
        assert!(output.contains("this.x = (2 + (3 * 4))"));
        assert!(output.contains("this.y = ((2 + 3) * 4)"));
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
        assert!(output.contains("this._x += this.speed"));
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
    }
    #[test]
    fn loops_and_math_compile() {
        let output =
            compile_as("for (var i:int = 0; i < 10; i++) { x += Math.sin(i); if (x > 4) break; }")
                .unwrap();
        assert!(output.contains("for ("));
        assert!(output.contains("(Math).sin(this.i)"));
    }
    #[test]
    fn missing_compatibility_features_fail_explicitly() {
        for source in [
            "var x:XML = <a/>;",
            "x = y is String;",
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
            "function f(a:int = 2):int { return a; }",
            "function f(...args) {}",
            "function f(a, a) {}",
            "function f():void { return 1; }",
            "function f():int { return; }",
            "function f(x:int):int { if (x > 0) return x; }",
            "function f(){ return this.x; } var callback = function(){ return this; };",
            "while (true) { var f = function(){ break; }; }",
            "if (true) { function f() {} }",
            "var f = function named(){ named = 2; };",
            "function f(){ return arguments[0]; }",
            "function f(){ var fn:Number = 2; fn(); }",
            "var n:Number = function() {}; n();",
            "function f(){ return 1; } child.f();",
            "var obj = {}; obj.unavailable();",
            "var f = function() {}; f.call(this);",
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
