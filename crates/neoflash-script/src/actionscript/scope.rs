//! Resolve declarations before calls/emission so lexical scope and hoisting
//! are independent of source order. Unknown external APIs remain diagnostics.
use super::*;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum BindingKind {
    Variable,
    Parameter,
    Declaration,
    SelfName,
}
pub(super) struct Binding {
    pub ty: Option<String>,
    pub kind: BindingKind,
}
pub(super) struct Scope {
    pub parent: Option<usize>,
    pub bindings: BTreeMap<String, Binding>,
    pub returns: Option<String>,
    pub allow_this: bool,
    pub offset: usize,
    members: HashSet<Vec<String>>,
}
pub(super) struct Analysis<'a> {
    pub scopes: HashMap<usize, Scope>,
    script: &'a Script,
}
impl<'a> Analysis<'a> {
    pub fn new(script: &'a Script, body: &[Stmt]) -> Result<Self, Diagnostic> {
        let mut result = Self {
            scopes: HashMap::new(),
            script,
        };
        result.scopes.insert(
            0,
            Scope {
                parent: None,
                bindings: BTreeMap::new(),
                returns: None,
                allow_this: true,
                offset: 0,
                members: HashSet::new(),
            },
        );
        result.body(0, body)?;
        Ok(result)
    }
    pub fn resolve(&self, mut scope: usize, name: &str) -> Option<(usize, &Binding)> {
        loop {
            let current = &self.scopes[&scope];
            if let Some(binding) = current.bindings.get(name) {
                return Some((scope, binding));
            }
            scope = current.parent?;
        }
    }
    fn error(&self, offset: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(self.script, offset, message)
    }
    fn bind(
        &mut self,
        scope: usize,
        name: &str,
        ty: Option<String>,
        kind: BindingKind,
    ) -> Result<(), Diagnostic> {
        let current = self.scopes.get_mut(&scope).unwrap();
        if let Some(previous) = current.bindings.get(name) {
            if kind == BindingKind::Variable && previous.kind == kind && previous.ty == ty {
                return Ok(());
            }
            return Err(self.error(
                self.scopes[&scope].offset,
                format!("Conflicting declaration of '{name}' in one function scope."),
            ));
        }
        current.bindings.insert(name.into(), Binding { ty, kind });
        Ok(())
    }
    fn declarations(&mut self, scope: usize, stmt: &Stmt, direct: bool) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Var(vars) => {
                for (name, ty, _) in vars {
                    self.bind(scope, name, ty.clone(), BindingKind::Variable)?;
                }
            }
            Stmt::Function(f) => {
                if !direct {
                    return Err(self.error(f.offset,"Function declarations inside blocks/conditionals/loops are not supported. Assign a function expression instead."));
                }
                self.bind(
                    scope,
                    f.name.as_deref().unwrap(),
                    Some("Function".into()),
                    BindingKind::Declaration,
                )?;
            }
            Stmt::Block(body) => {
                for stmt in body {
                    self.declarations(scope, stmt, false)?;
                }
            }
            Stmt::If(_, yes, no) => {
                self.declarations(scope, yes, false)?;
                if let Some(no) = no {
                    self.declarations(scope, no, false)?;
                }
            }
            Stmt::While(_, body) => self.declarations(scope, body, false)?,
            Stmt::For(init, _, _, body) => {
                self.declarations(scope, init, false)?;
                self.declarations(scope, body, false)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, scope: usize, body: &[Stmt]) -> Result<(), Diagnostic> {
        for stmt in body {
            self.declarations(scope, stmt, true)?;
        }
        for stmt in body {
            self.definitions(scope, stmt);
        }
        for stmt in body {
            self.statement(scope, stmt)?;
        }
        Ok(())
    }
    fn function(&mut self, parent: usize, f: &Function) -> Result<(), Diagnostic> {
        let allow_this =
            f.declaration && parent == 0 && self.script.language == ScriptLanguage::ActionScript3;
        self.scopes.insert(
            f.id,
            Scope {
                parent: Some(parent),
                bindings: BTreeMap::new(),
                returns: f.returns.clone(),
                allow_this,
                offset: f.offset,
                members: HashSet::new(),
            },
        );
        if !f.declaration
            && let Some(name) = &f.name
        {
            self.bind(f.id, name, Some("Function".into()), BindingKind::SelfName)?;
        }
        for (name, ty) in &f.params {
            self.bind(f.id, name, ty.clone(), BindingKind::Parameter)?;
        }
        self.body(f.id, &f.body)?;
        if self.script.language == ScriptLanguage::ActionScript3
            && f.returns
                .as_deref()
                .is_some_and(|ty| !matches!(ty, "*" | "void"))
            && !definitely_returns(&f.body)
        {
            return Err(self.error(f.offset,"An explicitly typed AS3 function must return a value on every path. This subset proves sequential returns and both branches of if/else; unproven fallthrough is rejected."));
        }
        Ok(())
    }
    fn path(&self, scope: usize, expr: &Expr) -> Option<Vec<String>> {
        match expr {
            Expr::Name(name) if name == "this" && self.scopes[&scope].allow_this => {
                Some(vec!["@frame".into()])
            }
            Expr::Name(name)
                if (!is_global(name) || self.resolve(scope, name).is_some()) && name != "this" =>
            {
                let owner = self.resolve(scope, name).map_or(0, |(id, _)| id);
                Some(vec![
                    if owner == 0 {
                        "@frame".into()
                    } else {
                        format!("@local{owner}")
                    },
                    name.clone(),
                ])
            }
            Expr::Member(base, name) => {
                let mut path = self.path(scope, base)?;
                path.push(name.clone());
                Some(path)
            }
            _ => None,
        }
    }
    fn define(&mut self, scope: usize, target: &Expr, value: &Expr) {
        if matches!(value, Expr::Function(_))
            && let Some(path) = self.path(scope, target)
        {
            self.scopes.get_mut(&scope).unwrap().members.insert(path);
        }
        if let Expr::Object(members) = value {
            for (name, value) in members {
                self.define(
                    scope,
                    &Expr::Member(Box::new(target.clone()), name.name.clone()),
                    value,
                );
            }
        }
    }
    fn definitions_expr(&mut self, scope: usize, expr: &Expr) {
        if let Expr::Assign(op, target, value) = expr
            && op == "="
        {
            self.define(scope, target, value);
        }
        for child in children(expr) {
            self.definitions_expr(scope, child);
        }
    }
    fn definitions(&mut self, scope: usize, stmt: &Stmt) {
        if let Stmt::Var(vars) = stmt {
            for (name, _, value) in vars {
                if let Some(value) = value {
                    self.define(scope, &Expr::Name(name.clone()), value);
                }
            }
        }
        for expr in expressions(stmt) {
            self.definitions_expr(scope, expr);
        }
        for child in statements(stmt) {
            self.definitions(scope, child);
        }
    }
    fn known_member(&self, mut scope: usize, expr: &Expr) -> bool {
        let Some(path) = self.path(scope, expr) else {
            return false;
        };
        if path.len() == 2
            && path[0] == "@frame"
            && self.scopes[&0]
                .bindings
                .get(&path[1])
                .is_some_and(|b| b.kind == BindingKind::Declaration)
        {
            return true;
        }
        loop {
            let current = &self.scopes[&scope];
            if current.members.contains(&path) {
                return true;
            }
            let Some(parent) = current.parent else {
                return false;
            };
            scope = parent;
        }
    }
    fn expression(&mut self, scope: usize, expr: &Expr) -> Result<(), Diagnostic> {
        match expr {
            Expr::Function(f) => {self.function(scope,f)?; return Ok(());},
            Expr::Name(name) if name=="this" && !self.scopes[&scope].allow_this => return Err(self.error(self.scopes[&scope].offset,"Explicit this in an AS2 or nested/expression function needs receiver/global semantics not implemented yet. Use lexical locals or bare timeline fields; AS3 top-level declared methods support bound this.")),
            Expr::Call(callee,_,offset) => {
                let binding=if let Expr::Name(name)=callee.as_ref() {self.resolve(scope,name)}else{None};
                if self.script.language == ScriptLanguage::ActionScript3
                    && binding.is_some_and(|(_,b)| b.ty.as_deref().is_some_and(|t|matches!(t,"Number"|"String"|"Boolean"|"int"|"uint"))) {
                    return Err(self.error(*offset,"An AS3 primitive binding cannot be called as a function."));
                }
                let user_binding=binding.is_some_and(|(_,b)| self.script.language==ScriptLanguage::ActionScript2 || !b.ty.as_deref().is_some_and(|t|matches!(t,"Number"|"String"|"Boolean"|"int"|"uint")));
                let shadowed_math=matches!(callee.as_ref(),Expr::Member(base,_) if matches!(base.as_ref(),Expr::Name(name) if name=="Math" && self.resolve(scope,name).is_some()));
                let builtin=binding.is_none() && !shadowed_math && Frontend::builtin_call(callee);
                if !builtin && !user_binding && !matches!(callee.as_ref(),Expr::Function(_)) && !self.known_member(scope,callee) {
                    return Err(self.error(*offset,"This ActionScript call is not a supported API or a declared user function/value. Unknown member APIs and dynamic computed calls remain unsupported; source is not discarded."));
                }
            }
            Expr::Assign(_,left,_) | Expr::Update(_,left,_) => {
                if let Expr::Name(name)=left.as_ref() && self.resolve(scope,name).is_some_and(|(_,b)|b.kind==BindingKind::SelfName) { return Err(self.error(self.scopes[&scope].offset,"A named function expression's self binding cannot be assigned.")); }
                if let Expr::Name(name)=left.as_ref() && is_global(name) && self.resolve(scope,name).is_none() {return Err(self.error(self.scopes[&scope].offset,"Cannot assign to an unshadowed built-in name in this ActionScript subset."));}
            }
            _=>{}
        }
        for child in children(expr) {
            self.expression(scope, child)?;
        }
        Ok(())
    }
    fn statement(&mut self, scope: usize, stmt: &Stmt) -> Result<(), Diagnostic> {
        if let Stmt::Function(f) = stmt {
            return self.function(scope, f);
        }
        if let Stmt::Return(value, offset) = stmt {
            let ty = self.scopes[&scope].returns.as_deref();
            if value.is_some() && matches!(ty, Some("void" | "Void")) {
                return Err(self.error(*offset, "A void/Void function cannot return a value."));
            }
            if value.is_none()
                && self.script.language == ScriptLanguage::ActionScript3
                && ty.is_some_and(|t| !matches!(t, "void" | "*"))
            {
                return Err(self.error(
                    *offset,
                    "An explicitly typed AS3 function must return a value.",
                ));
            }
        }
        for expr in expressions(stmt) {
            self.expression(scope, expr)?;
        }
        for child in statements(stmt) {
            self.statement(scope, child)?;
        }
        Ok(())
    }
}
fn definitely_returns(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::Return(Some(_), _) => true,
        Stmt::Block(b) => definitely_returns(b),
        Stmt::If(_, yes, Some(no)) => {
            definitely_returns(std::slice::from_ref(yes.as_ref()))
                && definitely_returns(std::slice::from_ref(no.as_ref()))
        }
        _ => false,
    })
}
fn expressions(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Expr(e) => vec![e],
        Stmt::Var(v) => v.iter().filter_map(|(_, _, v)| v.as_ref()).collect(),
        Stmt::If(e, _, _) | Stmt::While(e, _) => vec![e],
        Stmt::For(_, c, u, _) => c.iter().chain(u.iter()).collect(),
        Stmt::Return(e, _) => e.iter().collect(),
        _ => vec![],
    }
}
fn statements(stmt: &Stmt) -> Vec<&Stmt> {
    match stmt {
        Stmt::Block(b) => b.iter().collect(),
        Stmt::If(_, y, n) => std::iter::once(y.as_ref())
            .chain(n.iter().map(Box::as_ref))
            .collect(),
        Stmt::While(_, b) => vec![b],
        Stmt::For(i, _, _, b) => vec![i, b],
        _ => vec![],
    }
}
fn children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Member(v, _) | Expr::Unary(_, v) | Expr::Update(_, v, _) => vec![v],
        Expr::Index(a, b) | Expr::Binary(_, a, b) | Expr::Assign(_, a, b) => vec![a, b],
        Expr::Array(v) => v.iter().collect(),
        Expr::Object(v) => v.iter().map(|(_, e)| e).collect(),
        Expr::Conditional(a, b, c) => vec![a, b, c],
        Expr::Call(c, a, _) => std::iter::once(c.as_ref()).chain(a.iter()).collect(),
        _ => vec![],
    }
}
