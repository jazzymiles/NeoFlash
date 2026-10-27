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
    Catch,
    Field,
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
    pub function: usize,
    members: HashSet<Vec<String>>,
}
pub(super) struct Analysis<'a> {
    pub scopes: HashMap<usize, Scope>,
    script: &'a Script,
    as2_catch_names: HashMap<String, usize>,
    pub context: CompileContext,
}
impl<'a> Analysis<'a> {
    pub fn with_context(
        script: &'a Script,
        body: &[Stmt],
        context: CompileContext,
    ) -> Result<Self, Diagnostic> {
        let mut result = Self {
            scopes: HashMap::new(),
            script,
            as2_catch_names: HashMap::new(),
            context,
        };
        result.scopes.insert(
            0,
            Scope {
                parent: None,
                bindings: BTreeMap::new(),
                returns: None,
                allow_this: true,
                offset: 0,
                function: 0,
                members: HashSet::new(),
            },
        );
        for (name, ty) in result.context.members.clone() {
            result.scopes.get_mut(&0).unwrap().bindings.insert(
                name,
                Binding {
                    ty,
                    kind: BindingKind::Field,
                },
            );
        }
        result.collect_catch_names(body);
        result.body(0, body)?;
        Ok(result)
    }
    fn collect_catch_names(&mut self, body: &[Stmt]) {
        for stmt in body {
            if let Stmt::Try(_, catches, _) = stmt {
                for catch in catches {
                    self.as2_catch_names
                        .insert(catch.name.clone(), catch.offset);
                }
            }
            for child in statements(stmt) {
                self.collect_catch_names(std::slice::from_ref(child));
            }
            if let Stmt::Function(f) = stmt {
                self.collect_catch_names(&f.body);
            }
            for expr in expressions(stmt) {
                self.collect_expr_catches(expr);
            }
        }
    }
    fn collect_expr_catches(&mut self, expr: &Expr) {
        if let Expr::Function(f) = expr {
            self.collect_catch_names(&f.body);
        }
        for child in children(expr) {
            self.collect_expr_catches(child);
        }
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
            Stmt::DoWhile(body, _) | Stmt::Label(_, body) => {
                self.declarations(scope, body, false)?
            }
            Stmt::Switch(_, cases) => {
                for case in cases {
                    for stmt in &case.body {
                        self.declarations(scope, stmt, false)?;
                    }
                }
            }
            Stmt::Try(body, catches, finally) => {
                for stmt in body
                    .iter()
                    .chain(catches.iter().flat_map(|c| &c.body))
                    .chain(finally.iter().flatten())
                {
                    self.declarations(scope, stmt, false)?;
                }
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
        if self.script.language == ScriptLanguage::ActionScript2
            && self.scopes[&parent].function != parent
        {
            return Err(self.error(f.offset, "AS2 functions inside catch require verified catch-register closure semantics and are not supported yet."));
        }
        let allow_this = self.script.language == ScriptLanguage::ActionScript2
            || (f.declaration
                && parent == 0
                && self.script.language == ScriptLanguage::ActionScript3)
            || self.context.method_id == Some(f.id);
        self.scopes.insert(
            f.id,
            Scope {
                parent: Some(parent),
                bindings: BTreeMap::new(),
                returns: f.returns.clone(),
                allow_this,
                offset: f.offset,
                function: f.id,
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
            && !completion::never_falls_through(&f.body)
        {
            return Err(self.error(f.offset,"An explicitly typed AS3 function must return a value or throw on every path. Unproven normal fallthrough is rejected."));
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
            let function = self.scopes[&scope].function;
            self.scopes.get_mut(&function).unwrap().members.insert(path);
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
        if let Stmt::Try(body, catches, finally) = stmt {
            for child in body.iter().chain(finally.iter().flatten()) {
                self.definitions(scope, child);
            }
            for catch in catches {
                let enclosing = &self.scopes[&scope];
                self.scopes.insert(
                    catch.id,
                    Scope {
                        parent: Some(scope),
                        bindings: [(
                            catch.name.clone(),
                            Binding {
                                ty: catch.ty.clone(),
                                kind: BindingKind::Catch,
                            },
                        )]
                        .into(),
                        returns: enclosing.returns.clone(),
                        allow_this: enclosing.allow_this,
                        offset: catch.offset,
                        function: enclosing.function,
                        members: HashSet::new(),
                    },
                );
                for child in &catch.body {
                    self.definitions(catch.id, child);
                }
            }
            return;
        }
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
        self.expression_context(scope, expr, false)
    }
    fn expression_context(
        &mut self,
        scope: usize,
        expr: &Expr,
        direct_callee: bool,
    ) -> Result<(), Diagnostic> {
        if self.script.language == ScriptLanguage::ActionScript2 && !direct_callee {
            let timeline_alias = match expr {
                Expr::Name(name) => {
                    matches!(name.as_str(), "gotoAndPlay" | "gotoAndStop")
                        && self.resolve(scope, name).is_none()
                }
                Expr::Member(_, name) => {
                    matches!(name.as_str(), "gotoAndPlay" | "gotoAndStop")
                        && !self.known_member(scope, expr)
                }
                _ => false,
            };
            if timeline_alias {
                return Err(self.error(self.scopes[&scope].offset, "AS2 goto method values and aliases need receiver semantics not supported yet. Call gotoAndPlay/gotoAndStop directly on the target timeline."));
            }
        }
        match expr {
            Expr::Name(name) if self.script.language==ScriptLanguage::ActionScript2 && name=="AsBroadcaster" && self.resolve(scope,name).is_none() => return Err(self.error(self.scopes[&scope].offset,"AS2 AsBroadcaster is not implemented yet; Key/Mouse/Stage support listener objects directly.")),
            Expr::Name(name) if self.script.language==ScriptLanguage::ActionScript2 && self.context.static_method && self.resolve(scope,name).is_some_and(|(id,b)|id==0&&b.kind==BindingKind::Field) && !self.context.static_members.contains_key(name) => return Err(self.error(self.scopes[&scope].offset,"An AS2 static method cannot access an instance member without an explicit object receiver.")),
            Expr::Member(base,_) if self.script.language==ScriptLanguage::ActionScript2 && matches!(base.as_ref(),Expr::Name(n)if n=="super") && !direct_callee => return Err(self.error(self.scopes[&scope].offset,"AS2 super property values and method extraction are outside this profile; call super.method(...) directly.")),
            Expr::Name(name) if is_error_type(name) && self.resolve(scope, name).is_none() => return Err(self.error(self.scopes[&scope].offset, "Core Error class values and constructor aliases are not supported; use an explicit new Error-family constructor.")),
            Expr::NewError(name, _, offset) if self.resolve(scope, name).is_some() => return Err(self.error(*offset, "A shadowed Error constructor cannot be resolved as a core type; arbitrary constructor calls are unsupported.")),
            Expr::NewBuiltin(name, _, offset) if self.resolve(scope, name).is_some() => return Err(self.error(*offset, "A shadowed Array/String constructor requires arbitrary user-constructor semantics not supported here. Ordinary calls to the local binding remain supported.")),
            Expr::NewClass(name, _, offset) if name == "flash.events.EventPhase" => return Err(self.error(*offset,"EventPhase contains constants and is not constructible.")),
            Expr::NewClass(name, _, offset) if name == "flash.utils.Endian" => return Err(self.error(*offset,"Endian contains byte-order constants and is not constructible.")),
            Expr::NewClass(name, _, offset) if name == "flash.display.Graphics" => return Err(self.error(*offset,"Graphics is supplied by a compatible display object's graphics property and is not directly constructible.")),
            Expr::NewClass(name, args, offset) if name == "flash.utils.ByteArray" && !args.is_empty() => return Err(self.error(*offset,"ByteArray construction takes no arguments in the bounded binary-buffer profile.")),
            Expr::Name(name) if self.script.language == ScriptLanguage::ActionScript2 && self.resolve(scope, name).is_none() && self.as2_catch_names.contains_key(name) && !is_global(name) => return Err(self.error(self.as2_catch_names[name], "AS2 catch parameters used outside their catch need verified variable/register scope semantics; this form is unsupported.")),
            Expr::Function(f) => {self.function(scope,f)?; return Ok(());},
            Expr::Name(name) if name=="this" && !self.scopes[&scope].allow_this => return Err(self.error(self.scopes[&scope].offset,"Explicit this in an AS2 or nested/expression function needs receiver/global semantics not implemented yet. Use lexical locals or bare timeline fields; AS3 top-level declared methods support bound this.")),
            Expr::Call(callee,args,offset) => {
                let binding=if let Expr::Name(name)=callee.as_ref() {self.resolve(scope,name)}else{None};
                if self.script.language==ScriptLanguage::ActionScript2 && binding.is_none()
                    && matches!(callee.as_ref(),Expr::Name(name)if self.context.types.contains_key(name)&&!matches!(name.as_str(),"Array"|"String")) {
                    return Err(self.error(*offset,"AS2 class casts and class-as-function calls are not implemented in this profile; construct with new or use the existing instance directly."));
                }
                if self.script.language == ScriptLanguage::ActionScript3 && binding.is_none()
                    && matches!(callee.as_ref(), Expr::Name(name) if name == "parseInt") && args.len() > 2 {
                    return Err(self.error(*offset, "AS3 parseInt excess-argument semantics are not verified. Pass at most two arguments."));
                }
                if self.script.language == ScriptLanguage::ActionScript3
                    && binding.is_some_and(|(_,b)| b.ty.as_deref().is_some_and(|t|matches!(t,"Number"|"String"|"Boolean"|"int"|"uint"))) {
                    return Err(self.error(*offset,"An AS3 primitive binding cannot be called as a function."));
                }
                let user_binding=binding.is_some_and(|(_,b)| self.script.language==ScriptLanguage::ActionScript2 || !b.ty.as_deref().is_some_and(|t|matches!(t,"Number"|"String"|"Boolean"|"int"|"uint")));
                let shadowed_math=matches!(callee.as_ref(),Expr::Member(base,_) if matches!(base.as_ref(),Expr::Name(name) if name=="Math" && self.resolve(scope,name).is_some()));
                let project_call = self.context.project && match callee.as_ref() {
                    Expr::Name(name) => self.context.types.contains_key(name) || self.context.functions.contains_key(name) || project::is_host_method(name) || (name == "super" && self.context.class_name.is_some()),
                    Expr::Member(_, _) => true,
                    _ => false,
                };
                let legacy_api=self.script.language==ScriptLanguage::ActionScript2 && matches!(callee.as_ref(),Expr::Member(_,name) if matches!(name.as_str(),"addListener"|"removeListener"|"getCode"|"getAscii"|"isDown"|"hide"|"show"|"call"|"apply"));
                let builtin=binding.is_none() && !shadowed_math && (Frontend::builtin_call(callee)||legacy_api);
                // AS2 permits a later frame or a parent script to install a
                // function on this timeline. The host checks the actual value
                // when the call executes; arbitrary member APIs stay blocked.
                let dynamic_timeline = self.script.language == ScriptLanguage::ActionScript2
                    && binding.is_none() && matches!(callee.as_ref(), Expr::Name(name) if !is_global(name));
                if !builtin && !project_call && !user_binding && !dynamic_timeline && !matches!(callee.as_ref(),Expr::Function(_)) && !self.known_member(scope,callee) {
                    return Err(self.error(*offset,"This ActionScript call is not a supported API or a declared user function/value. Unknown member APIs and dynamic computed calls remain unsupported; source is not discarded."));
                }
            }
            Expr::Assign(_,left,_) | Expr::Update(_,left,_) => {
                if let Expr::Name(name)=left.as_ref() && self.resolve(scope,name).is_some_and(|(_,b)|b.kind==BindingKind::SelfName) { return Err(self.error(self.scopes[&scope].offset,"A named function expression's self binding cannot be assigned.")); }
                if let Expr::Name(name)=left.as_ref() && is_global(name) && self.resolve(scope,name).is_none() {return Err(self.error(self.scopes[&scope].offset,"Cannot assign to an unshadowed built-in name in this ActionScript subset."));}
            }
            _=>{}
        }
        if let Expr::Call(callee, arguments, _) = expr {
            self.expression_context(scope, callee, true)?;
            for argument in arguments {
                self.expression(scope, argument)?;
            }
        } else {
            for child in children(expr) {
                self.expression(scope, child)?;
            }
        }
        Ok(())
    }
    fn statement(&mut self, scope: usize, stmt: &Stmt) -> Result<(), Diagnostic> {
        if let Stmt::Try(body, catches, finally) = stmt {
            for stmt in body {
                self.statement(scope, stmt)?;
            }
            for catch in catches {
                if self.script.language == ScriptLanguage::ActionScript2
                    && (self.resolve(scope, &catch.name).is_some() || is_global(&catch.name))
                {
                    return Err(self.error(catch.offset, "AS2 catch parameter shadowing needs verified variable/register scope semantics; choose a distinct catch name."));
                }
                for stmt in &catch.body {
                    self.statement(catch.id, stmt)?;
                }
            }
            if let Some(finally) = finally {
                for stmt in finally {
                    self.statement(scope, stmt)?;
                }
            }
            return Ok(());
        }
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
fn expressions(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::Expr(e) => vec![e],
        Stmt::Var(v) => v.iter().filter_map(|(_, _, v)| v.as_ref()).collect(),
        Stmt::If(e, _, _) | Stmt::While(e, _) => vec![e],
        Stmt::For(_, c, u, _) => c.iter().chain(u.iter()).collect(),
        Stmt::Return(e, _) => e.iter().collect(),
        Stmt::Throw(e) | Stmt::DoWhile(_, e) => vec![e],
        Stmt::Switch(e, cases) => std::iter::once(e)
            .chain(cases.iter().filter_map(|c| c.test.as_ref()))
            .collect(),
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
        Stmt::DoWhile(b, _) | Stmt::Label(_, b) => vec![b],
        Stmt::Switch(_, cases) => cases.iter().flat_map(|c| &c.body).collect(),
        Stmt::Try(b, catches, finally) => b
            .iter()
            .chain(catches.iter().flat_map(|c| &c.body))
            .chain(finally.iter().flatten())
            .collect(),
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
        Expr::NewError(_, args, _) | Expr::NewClass(_, args, _) | Expr::NewBuiltin(_, args, _) => {
            args.iter().collect()
        }
        _ => vec![],
    }
}
