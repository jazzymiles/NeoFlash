//! A tokenizing/recursive-descent + Pratt frontend, not a text substitution pass.

use crate::Diagnostic;
use neoflash_core::{Script, ScriptLanguage};
use std::collections::HashMap;
mod scope;

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
                while self.bump().is_some_and(|c| c != '\n') {}
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
                    if next == '\n' || next == '\r' {
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
    Break,
    Continue,
    Function(Function),
    Return(Option<Expr>, usize),
}

struct Frontend<'a> {
    script: &'a Script,
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    loop_depth: usize,
    function_depth: usize,
    next_function: usize,
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
        if self.depth > 128 {
            return Err(self.error("ActionScript nesting exceeds 128 levels."));
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
                || self.script.source[offset..self.token().offset].contains(['\n', '\r'])
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
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
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
            self.loop_depth += 1;
            let body = self.statement()?;
            self.loop_depth -= 1;
            return Ok(Stmt::For(Box::new(init), condition, update, Box::new(body)));
        }
        if self.is("break") || self.is("continue") {
            if self.loop_depth == 0 {
                return Err(self.error("break/continue must be inside a loop."));
            }
            let is_break = self.eat("break");
            if !is_break {
                self.expect("continue")?;
            }
            self.semicolon()?;
            return Ok(if is_break {
                Stmt::Break
            } else {
                Stmt::Continue
            });
        }
        if let Kind::Ident(word) = &self.token().kind
            && is_unsupported_keyword(word)
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
        let name = if self.eat("*") {
            "*".into()
        } else {
            self.ident()?
        };
        let as3 = self.script.language == ScriptLanguage::ActionScript3;
        let supported = matches!(name.as_str(), "Number" | "String" | "Boolean" | "Function")
            || (as3 && matches!(name.as_str(), "int" | "uint" | "*"))
            || (returns && name == if as3 { "void" } else { "Void" });
        if !supported {
            return Err(self.error(format!("Type '{name}' is not supported in this ActionScript annotation. Supported types are primitive types and Function; void/Void is return-only.")));
        }
        Ok(Some(name))
    }
    fn function(&mut self, declaration: bool) -> Result<Function, Diagnostic> {
        let offset = self.token().offset;
        self.expect("function")?;
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
        self.expect("{")?;
        let id = self.next_function;
        self.next_function += 1;
        let outer_loop = std::mem::replace(&mut self.loop_depth, 0);
        self.function_depth += 1;
        let body = self.statements(true)?;
        self.function_depth -= 1;
        self.loop_depth = outer_loop;
        Ok(Function {
            id,
            offset,
            name,
            params,
            returns,
            body,
            declaration,
        })
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
        if self.is("function") {
            return Ok(Expr::Function(self.function(false)?));
        }
        let token = self.token().clone();
        match token.kind {
            Kind::Number(value) | Kind::String(value) => { self.pos += 1; Ok(Expr::Literal(value)) }
            Kind::Ident(name) => {
                if is_unsupported_keyword(&name) || matches!(name.as_str(), "if" | "else" | "while" | "for" | "var" | "break" | "continue" | "return") {
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
                    || matches!(
                        name.as_str(),
                        "trace"
                            | "push"
                            | "pop"
                            | "shift"
                            | "unshift"
                            | "join"
                            | "slice"
                            | "indexOf"
                            | "substring"
                            | "charAt"
                            | "charCodeAt"
                            | "toString"
                            | "toFixed"
                            | "split"
                            | "concat"
                            | "reverse"
                    )
            }
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
fn is_global(name: &str) -> bool {
    matches!(
        name,
        "Math"
            | "Number"
            | "String"
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
            _ => value,
        }
    }
    fn intrinsic(&self, name: &str) -> String {
        if self.scope == 0 {
            name.into()
        } else {
            format!("__nf{name}")
        }
    }
    fn typed(&self, name: &str) -> bool {
        self.analysis
            .resolve(self.scope, name)
            .is_some_and(|(_, b)| b.ty.is_some())
    }
    fn name(&self, name: &str) -> String {
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
            Some("String" | "Function") => "null",
            Some("int" | "uint") => "0",
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
    fn expression(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(value) => value.clone(),
            Expr::Name(name) => self.name(name),
            Expr::Function(f) => format!("({})", self.function(f)),
            Expr::Member(base, name) => format!("({}).{name}", self.expression(base)),
            Expr::Index(base, index) => {
                format!("({})[{}]", self.expression(base), self.expression(index))
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
                let target = self.expression(left);
                let value = self.expression(right);
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
                format!("({target} {op} {value})")
            }
            Expr::Update(op, value, prefix) => {
                let target = self.expression(value);
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
            Expr::Call(callee, args, _) => format!(
                "{}({})",
                self.expression(callee),
                args.iter()
                    .map(|v| self.expression(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
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
                        if self.scope != 0 && value.is_none() {
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
            Stmt::Break => "break;".into(),
            Stmt::Continue => "continue;".into(),
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
    let tokens = Lexer { script, offset: 0 }.tokens()?;
    let mut parser = Frontend {
        script,
        tokens,
        pos: 0,
        depth: 0,
        loop_depth: 0,
        function_depth: 0,
        next_function: 1,
    };
    let statements = parser.statements(false)?;
    let analysis = scope::Analysis::new(script, &statements)?;
    let body = Emitter {
        as3: script.language == ScriptLanguage::ActionScript3,
        analysis: &analysis,
        scope: 0,
    }
    .body(&statements);
    let output = format!(
        "const __nfFrame = this, __nfNumber = Number, __nfString = String, __nfBoolean = Boolean, __nfTypeError = TypeError;\nconst __nfTrace = trace;\n{body}"
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
        assert!(output.contains("this.speed = Number(2)"));
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
