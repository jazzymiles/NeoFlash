//! Advisory, read-only type flow over the same AST used by ActionScript compilation.
//! No result from this module changes code generation or rejects a program.
use super::{CompileContext, Expr, Frontend, Function, Kind, Stmt, Token, TokenCursor, completion};
use crate::language_service::{HintContext, Member, ProjectClass, short_name};
use crate::{Diagnostic, DiagnosticSeverity};
use neoflash_core::{Script, ScriptLanguage};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Signature {
    params: Vec<Option<String>>,
    returns: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Unknown,
    Type(String),
    Class(String),
    Function(Signature),
}
impl Value {
    fn ty(&self) -> Option<String> {
        match self {
            Self::Type(ty) => Some(ty.clone()),
            Self::Function(_) => Some("Function".into()),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
struct Binding {
    method: bool,
    declared: Option<String>,
    value: Value,
}
type State = BTreeMap<String, Binding>;
#[derive(Clone, Debug)]
struct FlowScope {
    range: Range<usize>,
    points: Vec<(usize, Option<State>)>,
    saturated: bool,
}

/// A source/context snapshot containing conservative flow facts and advisory diagnostics.
#[derive(Clone, Debug, Default)]
pub struct SemanticAnalysis {
    source: String,
    scopes: Vec<FlowScope>,
    diagnostics: Vec<Diagnostic>,
}
impl SemanticAnalysis {
    pub(crate) fn source(&self) -> &str {
        &self.source
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    fn point_at(&self, offset_chars: usize) -> Option<&Option<State>> {
        let at = self
            .source
            .char_indices()
            .nth(offset_chars)
            .map_or(self.source.len(), |(i, _)| i);
        let scope = self
            .scopes
            .iter()
            .rev()
            .filter(|s| s.range.start <= at && at <= s.range.end)
            .min_by_key(|s| s.range.len())?;
        scope
            .points
            .iter()
            .rev()
            .find(|(offset, _)| *offset <= at)
            .map(|(_, state)| state)
    }
    fn state_at(&self, offset_chars: usize) -> Option<&State> {
        self.point_at(offset_chars)?.as_ref()
    }
    pub fn is_analyzed(&self, offset_chars: usize) -> bool {
        self.point_at(offset_chars).is_some()
    }
    /// Distinguishes an unknown/dynamic binding from an unavailable AST snapshot.
    pub fn has_binding(&self, name: &str, offset_chars: usize) -> bool {
        self.point_at(offset_chars)
            .is_some_and(|state| state.as_ref().is_none_or(|state| state.contains_key(name)))
    }
    pub fn type_at(&self, name: &str, offset_chars: usize) -> Option<String> {
        self.state_at(offset_chars)?.get(name)?.value.ty()
    }
    pub fn return_type(&self, name: &str, offset_chars: usize) -> Option<String> {
        match &self.state_at(offset_chars)?.get(name)?.value {
            Value::Function(sig) => sig.returns.clone(),
            _ => None,
        }
    }
}

fn frontend(script: &Script, context: CompileContext) -> Frontend<'_> {
    Frontend {
        script,
        tokens: TokenCursor::default(),
        pos: 0,
        depth: 0,
        controls: Vec::new(),
        function_depth: 0,
        next_function: 1,
        context,
        no_in: false,
        with_depth: 0,
        in_initclip: false,
    }
}
fn known_classes<'a>(context: &'a HintContext<'_>) -> impl Iterator<Item = &'a String> {
    context
        .api
        .classes
        .keys()
        .chain(context.project.iter().map(|c| &c.qualified))
}
fn resolve_type(
    context: &HintContext<'_>,
    name: &str,
    imports: &[String],
    package: &str,
    builtin: bool,
) -> Option<String> {
    if name.is_empty() || name == "*" {
        return None;
    }
    if let Some(element) = name
        .strip_prefix("Vector.<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return resolve_type(context, element, imports, package, builtin)
            .map(|ty| format!("Vector.<{ty}>"));
    }
    if name.contains('.') {
        return known_classes(context)
            .any(|ty| ty == name)
            .then(|| name.into());
    }
    if !builtin {
        let exact: Vec<_> = imports
            .iter()
            .filter(|ty| !ty.ends_with(".*") && short_name(ty) == name)
            .collect();
        if let [ty] = exact.as_slice() {
            return known_classes(context)
                .any(|known| known == *ty)
                .then(|| (*ty).clone());
        }
        if !exact.is_empty() {
            return None;
        }
        let same_package = format!("{package}.{name}");
        if !package.is_empty()
            && context
                .project
                .iter()
                .any(|class| class.qualified == same_package)
        {
            return Some(same_package);
        }
        let wildcard: Vec<_> = known_classes(context)
            .filter(|ty| {
                short_name(ty) == name
                    && imports.iter().any(|import| {
                        import.strip_suffix(".*").is_some_and(|package| {
                            ty.rsplit_once('.')
                                .is_some_and(|(owner, _)| owner == package)
                        })
                    })
            })
            .collect();
        if let [ty] = wildcard.as_slice() {
            return Some((*ty).clone());
        }
        if !wildcard.is_empty() {
            return None;
        }
    }
    let choices: Vec<_> = known_classes(context)
        .filter(|ty| short_name(ty) == name && (!builtin || context.api.classes.contains_key(*ty)))
        .collect();
    match choices.as_slice() {
        [ty] => Some((*ty).clone()),
        [] if matches!(
            name,
            "Number"
                | "String"
                | "Boolean"
                | "int"
                | "uint"
                | "Object"
                | "Function"
                | "Class"
                | "void"
                | "Void"
                | "Array"
        ) =>
        {
            Some(name.into())
        }
        _ => None,
    }
}
pub(crate) fn project_type(
    context: &HintContext<'_>,
    class: &ProjectClass,
    ty: &str,
) -> Option<String> {
    let package = class
        .qualified
        .rsplit_once('.')
        .map_or("", |(package, _)| package);
    resolve_type(context, ty, &class.imports, package, false)
}
pub(crate) fn member_type(context: &HintContext<'_>, member: &Member, ty: &str) -> Option<String> {
    if let Some(class) = context
        .project
        .iter()
        .find(|class| class.qualified == member.owner)
    {
        project_type(context, class, ty)
    } else {
        resolve_type(context, ty, &[], "", true)
    }
}
fn compiler_context(context: &HintContext<'_>, source: &str) -> CompileContext {
    let analysis = context.analyze(source);
    let imports = analysis.imports();
    let package = analysis.package_name();
    let mut result = CompileContext {
        project: true,
        class_name: Some(context.this_class.clone()),
        ..Default::default()
    };
    for class in known_classes(context) {
        result.types.insert(class.clone(), class.clone());
        if let Some(ty) = resolve_type(context, short_name(class), &imports, &package, false) {
            result.types.insert(short_name(class).into(), ty);
        }
    }
    for ty in [
        "Date",
        "RegExp",
        "XML",
        "XMLList",
        "Vector",
        "Namespace",
        "QName",
        "JSON",
    ] {
        result.intrinsic_types.insert(ty.into());
    }
    for member in context.members(&context.this_class, false) {
        result.members.insert(member.name, Some(member.ty));
    }
    for (name, ty) in &context.instances {
        result.members.insert(name.clone(), Some(ty.clone()));
    }
    result
}

fn parse(script: &Script, context: &HintContext<'_>) -> Option<(Vec<Stmt>, Vec<Token>)> {
    let declarations = context.analyze(&script.source);
    let mut masked = script.clone();
    let mut bytes = masked.source.into_bytes();
    for (_, range) in declarations.import_statements() {
        for byte in &mut bytes[range] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    masked.source = String::from_utf8(bytes).expect("masking preserves UTF-8");
    let mut parser = frontend(&masked, compiler_context(context, &script.source));
    let mut body = Vec::new();
    // Source classes use the compiler's member/function parser. Their surrounding package/class
    // envelope is not itself executable; fields and methods keep their original source offsets.
    let class_source = !context.analyze(&script.source).classes().is_empty();
    if class_source {
        while parser.token().kind != Kind::End && !parser.is("class") && !parser.is("interface") {
            parser.pos += 1;
        }
        if parser.token().kind == Kind::End || parser.is("interface") {
            return None;
        }
        while parser.token().kind != Kind::End && !parser.is("{") {
            parser.pos += 1;
        }
        parser.expect("{").ok()?;
        while !parser.is("}") && parser.token().kind != Kind::End {
            while matches!(
                parser.token_text().as_str(),
                "public"
                    | "private"
                    | "protected"
                    | "internal"
                    | "static"
                    | "final"
                    | "override"
                    | "native"
            ) {
                parser.pos += 1;
            }
            if parser.is("function") {
                let (function, _) = parser.project_function(true, true, false, false).ok()?;
                body.push(Stmt::Located(
                    function.offset,
                    Box::new(Stmt::Function(Box::new(function))),
                ));
            } else if parser.is("var") || parser.is("const") || parser.is(";") {
                body.push(parser.statement().ok()?);
            } else {
                return None;
            }
        }
        parser.expect("}").ok()?;
        while parser.is("}") {
            parser.pos += 1;
        }
        if parser.token().kind != Kind::End {
            return None;
        }
    } else {
        body = parser.statements(false).ok()?;
    }
    let tokens = parser.tokens.buffer.borrow().clone();
    Some((body, tokens))
}

/// Repair only an unfinished member-query token / closing braces for editor completion. The
/// repaired program is never compiled, and its diagnostics are never shown to the user.
fn hint_source(script: &Script) -> Option<Script> {
    let mut lexer = super::Lexer { script, offset: 0 };
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token().ok()?;
        if token.kind == Kind::End {
            tokens.push(token);
            break;
        }
        tokens.push(token);
        if tokens.len() > 100_000 {
            return None;
        }
    }
    let mut bytes = script.source.as_bytes().to_vec();
    let mut changed = false;
    let mut stack = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Punct(".")
                if tokens
                    .get(i + 1)
                    .is_some_and(|t| matches!(t.kind, Kind::End | Kind::Punct("}" | ";"))) =>
            {
                bytes[token.offset] = b' ';
                changed = true;
            }
            Kind::Punct("{") => stack.push("}"),
            Kind::Punct("(") => stack.push(")"),
            Kind::Punct("[") => stack.push("]"),
            Kind::Punct(close @ ("}" | ")" | "]")) if stack.pop() != Some(close) => return None,
            _ => {}
        }
    }
    if stack.iter().any(|&end| end != "}") {
        return None;
    }
    if !stack.is_empty() {
        changed = true;
    }
    if !changed {
        return None;
    }
    let mut source = String::from_utf8(bytes).ok()?;
    // A newline also ends an otherwise unterminated // comment before synthesized braces.
    source.push('\n');
    for end in stack.iter().rev() {
        source.push_str(end);
    }
    Some(Script {
        language: script.language,
        source,
    })
}

pub(crate) fn analyze(context: &HintContext<'_>, source: &str) -> SemanticAnalysis {
    let script = Script {
        language: context.language,
        source: source.into(),
    };
    let mut output = SemanticAnalysis {
        source: source.into(),
        ..Default::default()
    };
    if source.len() > 1_048_576
        || !matches!(
            script.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        )
    {
        return output;
    }
    let mut repaired = false;
    let parsed = parse(&script, context).or_else(|| {
        repaired = true;
        parse(&hint_source(&script)?, context)
    });
    let Some((body, tokens)) = parsed else {
        return output;
    };
    let mut engine = Engine {
        context,
        script: &script,
        tokens: &tokens,
        output: &mut output,
        diagnose: !repaired,
        snapshots: true,
        fact_budget: 4_194_304,
        inference_depth: 0,
        imports: context.analyze(source).imports(),
        package: context.analyze(source).package_name(),
        class_source: !context.analyze(source).classes().is_empty(),
    };
    let mut state = engine.initial(&body, &State::new());
    for (name, ty) in &context.instances {
        state.entry(name.clone()).or_insert(Binding {
            method: false,
            declared: Some(ty.clone()),
            value: Value::Type(ty.clone()),
        });
    }
    let scope = engine.new_scope(0..source.len(), &state);
    let captures = state.keys().cloned().collect();
    engine.body(&body, &mut state, scope, &captures, None);
    output.diagnostics.sort_by_key(|d| (d.span, d.code.clone()));
    output
        .diagnostics
        .dedup_by(|a, b| a.span == b.span && a.code == b.code && a.message == b.message);
    output
}

struct Engine<'a, 'b> {
    context: &'a HintContext<'a>,
    script: &'a Script,
    tokens: &'a [Token],
    output: &'b mut SemanticAnalysis,
    diagnose: bool,
    snapshots: bool,
    fact_budget: usize,
    inference_depth: usize,
    imports: Vec<String>,
    package: String,
    class_source: bool,
}
impl Engine<'_, '_> {
    fn ty(&self, ty: &str) -> Option<String> {
        resolve_type(self.context, ty, &self.imports, &self.package, false)
    }
    fn signature_type(&self, member: &Member, ty: &str) -> Option<String> {
        member_type(self.context, member, ty)
    }
    fn new_scope(&mut self, range: Range<usize>, state: &State) -> usize {
        if !self.snapshots {
            return 0;
        }
        let id = self.output.scopes.len();
        self.output.scopes.push(FlowScope {
            range,
            points: Vec::new(),
            saturated: false,
        });
        self.record(id, self.output.scopes[id].range.start, state);
        id
    }
    fn record(&mut self, id: usize, offset: usize, state: &State) {
        if !self.snapshots || self.output.scopes[id].saturated {
            return;
        }
        let bytes = state.iter().fold(0usize, |bytes, (name, binding)| {
            let value = match &binding.value {
                Value::Type(ty) | Value::Class(ty) => ty.len(),
                Value::Function(signature) => {
                    signature
                        .params
                        .iter()
                        .flatten()
                        .map(String::len)
                        .sum::<usize>()
                        + signature.returns.as_ref().map_or(0, String::len)
                }
                Value::Unknown => 0,
            };
            bytes.saturating_add(
                256 + name.len() + binding.declared.as_ref().map_or(0, String::len) + value,
            )
        });
        let state = if bytes > self.fact_budget {
            self.output.scopes[id].saturated = true;
            None
        } else {
            self.fact_budget -= bytes;
            Some(state.clone())
        };
        let points = &mut self.output.scopes[id].points;
        let offset = offset.min(self.script.source.len());
        let insert = points.partition_point(|(at, _)| *at < offset);
        if points.get(insert).is_some_and(|(at, _)| *at == offset) {
            points[insert] = (offset, state);
        } else {
            points.insert(insert, (offset, state));
        }
    }
    fn initial(&mut self, body: &[Stmt], outer: &State) -> State {
        let mut state: State = outer
            .iter()
            .map(|(name, b)| {
                (
                    name.clone(),
                    Binding {
                        method: b.method,
                        declared: b.declared.clone(),
                        value: if b.method {
                            b.value.clone()
                        } else {
                            b.declared
                                .as_ref()
                                .and_then(|ty| self.ty(ty))
                                .map_or(Value::Unknown, Value::Type)
                        },
                    },
                )
            })
            .collect();
        let mut vars = BTreeMap::new();
        collect_declarations(body, &mut vars);
        for (name, ty) in vars {
            let value = ty
                .as_deref()
                .and_then(|ty| self.ty(ty))
                .map_or(Value::Unknown, Value::Type);
            state.insert(
                name,
                Binding {
                    method: false,
                    declared: ty,
                    value,
                },
            );
        }
        // Forward functions are declarations, so infer their signatures before walking statements.
        for stmt in body {
            if let Stmt::Function(f) = stmt.unlocated()
                && let Some(name) = &f.name
            {
                state.insert(
                    name.clone(),
                    Binding {
                        method: self.class_source && outer.is_empty(),
                        declared: Some("Function".into()),
                        value: Value::Function(self.signature(f, &state)),
                    },
                );
            }
        }
        state
    }
    fn signature(&mut self, f: &Function, outer: &State) -> Signature {
        let params = f
            .params
            .iter()
            .map(|(_, ty)| ty.as_deref().and_then(|ty| self.ty(ty)))
            .collect();
        let returns = if let Some(ty) = &f.returns {
            self.ty(ty)
        } else if self.inference_depth >= 4 {
            None
        } else {
            self.inference_depth += 1;
            let diagnose = self.diagnose;
            let snapshots = self.snapshots;
            self.diagnose = false;
            self.snapshots = false;
            let mut state = self.function_state(f, outer);
            let returns = self.body(&f.body, &mut state, 0, &captured_writes(f), None);
            self.diagnose = diagnose;
            self.snapshots = snapshots;
            self.inference_depth -= 1;
            if completion::never_falls_through(&f.body) && !returns.is_empty() {
                returns
                    .into_iter()
                    .reduce(|a, b| self.join_value(a, b))
                    .and_then(|v| v.ty())
            } else {
                None
            }
        };
        Signature { params, returns }
    }
    fn function_state(&mut self, f: &Function, outer: &State) -> State {
        let mut state = self.initial(&f.body, outer);
        for (name, ty) in &f.params {
            let value = ty
                .as_deref()
                .and_then(|ty| self.ty(ty))
                .map_or(Value::Unknown, Value::Type);
            state.insert(
                name.clone(),
                Binding {
                    method: false,
                    declared: ty.clone(),
                    value,
                },
            );
        }
        if let Some(rest) = &f.rest {
            state.insert(
                rest.clone(),
                Binding {
                    method: false,
                    declared: Some("Array".into()),
                    value: Value::Type("Array".into()),
                },
            );
        }
        state
    }
    fn end(&self, stmt: &Stmt) -> usize {
        if let Stmt::If(_, yes, no) = stmt.unlocated() {
            return no
                .as_deref()
                .map_or_else(|| self.end(yes), |no| self.end(no));
        }
        let offset = located(stmt);
        let Some(start) = self.tokens.iter().position(|t| t.offset == offset) else {
            return offset;
        };
        let mut depth = 0usize;
        for (relative, token) in self.tokens[start..].iter().enumerate() {
            match token.kind {
                Kind::Punct("{" | "(" | "[") => depth += 1,
                Kind::Punct("}" | ")" | "]") => {
                    if depth == 0 {
                        return token.offset;
                    }
                    depth -= 1;
                    if depth == 0 && matches!(token.kind, Kind::Punct("}")) {
                        let continuation = self.tokens.get(start+relative+1).is_some_and(|next| {
                            matches!((&next.kind,stmt.unlocated()),(Kind::Ident(word),Stmt::Try(..)) if matches!(word.as_str(),"catch"|"finally"))
                                || matches!((&next.kind,stmt.unlocated()),(Kind::Ident(word),Stmt::DoWhile(..)) if word=="while")
                        });
                        if !continuation {
                            return token.end;
                        }
                    }
                }
                Kind::Punct(";") if depth == 0 => return token.end,
                Kind::End => return token.offset,
                _ => {}
            }
        }
        self.script.source.len()
    }
    fn function_end(&self, f: &Function) -> usize {
        self.end(&Stmt::Located(
            f.offset,
            Box::new(Stmt::Function(Box::new(f.clone()))),
        ))
        .min(self.script.source.len())
    }
    fn body(
        &mut self,
        body: &[Stmt],
        state: &mut State,
        scope: usize,
        captured: &BTreeSet<String>,
        returns: Option<&str>,
    ) -> Vec<Value> {
        let mut values = Vec::new();
        for stmt in body {
            self.record(scope, located(stmt), state);
            values.extend(self.statement(stmt, state, scope, captured, returns));
            self.record(scope, self.end(stmt), state);
            if completion::never_falls_through(std::slice::from_ref(stmt)) {
                break;
            }
        }
        values
    }
    fn statement(
        &mut self,
        stmt: &Stmt,
        state: &mut State,
        scope: usize,
        captured: &BTreeSet<String>,
        returns: Option<&str>,
    ) -> Vec<Value> {
        let at = located(stmt);
        let mut values = Vec::new();
        match stmt.unlocated() {
            Stmt::Var(vars) => {
                for (name, ty, expr) in vars {
                    if let Some(expr) = expr {
                        let value = self.expr(expr, state, captured, at);
                        self.assign(name, ty.as_deref(), value, state, at);
                    }
                }
            }
            Stmt::Expr(expr) | Stmt::DefaultXmlNamespace(expr) => {
                self.expr(expr, state, captured, at);
            }
            Stmt::Block(body) | Stmt::InitClip(_, body, _) => {
                values.extend(self.body(body, state, scope, captured, returns))
            }
            Stmt::Function(f) | Stmt::LegacyEvent(_, f) => {
                let mut local = self.function_state(f, state);
                let id = self.new_scope(f.offset..self.function_end(f), &local);
                self.body(
                    &f.body,
                    &mut local,
                    id,
                    &captured_writes(f),
                    f.returns.as_deref(),
                );
            }
            Stmt::If(condition, yes, no) => {
                self.expr(condition, state, captured, at);
                let mut a = state.clone();
                let mut b = state.clone();
                values.extend(self.statement(yes, &mut a, scope, captured, returns));
                if let Some(no) = no {
                    values.extend(self.statement(no, &mut b, scope, captured, returns));
                }
                let yes_exits = completion::never_falls_through(std::slice::from_ref(yes));
                let no_exits = no
                    .as_ref()
                    .is_some_and(|no| completion::never_falls_through(std::slice::from_ref(no)));
                *state = match (yes_exits, no_exits) {
                    (true, false) => b,
                    (false, true) => a,
                    _ => self.join_states(&a, &b),
                };
            }
            Stmt::While(condition, body) => {
                self.expr(condition, state, captured, at);
                self.widen_loop(body, state);
                let before = state.clone();
                let mut iteration = state.clone();
                values.extend(self.statement(body, &mut iteration, scope, captured, returns));
                self.expr(condition, &mut iteration, captured, at);
                *state = self.join_states(&before, &iteration);
            }
            Stmt::DoWhile(body, condition) => {
                self.widen_loop(body, state);
                let before = state.clone();
                let mut iteration = state.clone();
                values.extend(self.statement(body, &mut iteration, scope, captured, returns));
                self.expr(condition, &mut iteration, captured, at);
                *state = self.join_states(&before, &iteration);
            }
            Stmt::For(init, condition, update, body) => {
                self.statement(init, state, scope, captured, returns);
                if let Some(condition) = condition {
                    self.expr(condition, state, captured, at);
                }
                self.widen_loop(body, state);
                if let Some(update) = update {
                    let mut names = BTreeSet::new();
                    assignment_names(update, &mut names);
                    invalidate(&names, state, self.script.language);
                }
                let before = state.clone();
                let mut iteration = state.clone();
                values.extend(self.statement(body, &mut iteration, scope, captured, returns));
                if let Some(update) = update {
                    self.expr(update, &mut iteration, captured, at);
                }
                *state = self.join_states(&before, &iteration);
            }
            Stmt::ForIn(init, object, _, body) => {
                self.statement(init, state, scope, captured, returns);
                self.expr(object, state, captured, at);
                let names = match init.unlocated() {
                    Stmt::Var(vars) => vars.iter().map(|(name, _, _)| name.clone()).collect(),
                    Stmt::Expr(Expr::Name(name)) => BTreeSet::from([name.clone()]),
                    _ => BTreeSet::new(),
                };
                invalidate(&names, state, self.script.language);
                self.widen_loop(body, state);
                let before = state.clone();
                let mut iteration = state.clone();
                values.extend(self.statement(body, &mut iteration, scope, captured, returns));
                *state = self.join_states(&before, &iteration);
            }
            Stmt::Return(expr, offset) => {
                let value = expr.as_ref().map_or(Value::Unknown, |expr| {
                    self.expr(expr, state, captured, *offset)
                });
                if let (Some(expected), Some(actual)) = (returns, value.ty()) {
                    self.mismatch(
                        "flow-return-type",
                        *offset,
                        &actual,
                        expected,
                        "Returned value",
                    );
                }
                values.push(value);
            }
            Stmt::Throw(expr) => {
                self.expr(expr, state, captured, at);
            }
            Stmt::With(target, body, _) | Stmt::TellTarget(target, body, _) => {
                self.expr(target, state, captured, at);
                for binding in state.values_mut() {
                    binding.value = Value::Unknown;
                }
                self.record(scope, at, state);
                let diagnose = self.diagnose;
                let snapshots = self.snapshots;
                self.diagnose = false;
                self.snapshots = false;
                values.extend(self.statement(body, state, scope, captured, returns));
                self.diagnose = diagnose;
                self.snapshots = snapshots;
                for binding in state.values_mut() {
                    binding.value = Value::Unknown;
                }
            }
            Stmt::IfFrameLoaded(exprs, body) => {
                for expr in exprs {
                    self.expr(expr, state, captured, at);
                }
                let before = state.clone();
                values.extend(self.statement(body, state, scope, captured, returns));
                *state = self.join_states(&before, state);
            }
            Stmt::Switch(test, cases) => {
                self.expr(test, state, captured, at);
                // A later case can be entered through fallthrough from any earlier case.
                let mut writes = BTreeSet::new();
                for case in cases {
                    for statement in &case.body {
                        statement_writes(statement, &mut writes);
                    }
                }
                invalidate(&writes, state, self.script.language);
                let before = state.clone();
                let mut merged = before.clone();
                for case in cases {
                    let mut branch = before.clone();
                    if let Some(test) = &case.test {
                        self.expr(test, &mut branch, captured, at);
                    }
                    values.extend(self.body(&case.body, &mut branch, scope, captured, returns));
                    merged = self.join_states(&merged, &branch);
                }
                *state = merged;
            }
            Stmt::Try(body, catches, finally) => {
                let before = state.clone();
                values.extend(self.body(body, state, scope, captured, returns));
                let mut merged = state.clone();
                let mut writes = BTreeSet::new();
                for statement in body {
                    statement_writes(statement, &mut writes);
                }
                for catch in catches {
                    let mut branch = before.clone();
                    invalidate(&writes, &mut branch, self.script.language);
                    let shadowed = branch.get(&catch.name).cloned();
                    branch.insert(
                        catch.name.clone(),
                        Binding {
                            method: false,
                            declared: catch.ty.clone(),
                            value: catch
                                .ty
                                .as_deref()
                                .and_then(|ty| self.ty(ty))
                                .map_or(Value::Unknown, Value::Type),
                        },
                    );
                    let catch_statement =
                        Stmt::Located(catch.offset, Box::new(Stmt::Block(catch.body.clone())));
                    let catch_scope =
                        self.new_scope(catch.offset..self.end(&catch_statement), &branch);
                    values.extend(self.body(
                        &catch.body,
                        &mut branch,
                        catch_scope,
                        captured,
                        returns,
                    ));
                    branch.remove(&catch.name);
                    if let Some(binding) = shadowed {
                        branch.insert(catch.name.clone(), binding);
                    }
                    merged = self.join_states(&merged, &branch);
                }
                *state = merged;
                if let Some(finally) = finally {
                    values.extend(self.body(finally, state, scope, captured, returns));
                }
            }
            Stmt::Label(_, body) => {
                values.extend(self.statement(body, state, scope, captured, returns))
            }
            _ => {}
        }
        values
    }
    fn expr(
        &mut self,
        expr: &Expr,
        state: &mut State,
        captured: &BTreeSet<String>,
        at: usize,
    ) -> Value {
        match expr {
            Expr::Literal(value) => {
                let ty = if value.starts_with(['\'', '"']) {
                    "String"
                } else if matches!(value.as_str(), "true" | "false") {
                    "Boolean"
                } else if value.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                    || matches!(value.as_str(), "NaN" | "Infinity")
                {
                    "Number"
                } else {
                    return Value::Unknown;
                };
                Value::Type(ty.into())
            }
            Expr::Name(name) => state.get(name).map(|b| b.value.clone()).unwrap_or_else(|| {
                if name == "this" {
                    Value::Type(self.context.this_class.clone())
                } else if name == "super" {
                    self.context
                        .flow_lineage(&self.context.this_class)
                        .get(1)
                        .cloned()
                        .map_or(Value::Unknown, Value::Type)
                } else if let Some(member) = self
                    .context
                    .members(&self.context.this_class, false)
                    .into_iter()
                    .find(|m| &m.name == name)
                {
                    self.member_value(&member)
                } else {
                    self.ty(name).map_or(Value::Unknown, Value::Class)
                }
            }),
            Expr::NewClass(name, args, _)
            | Expr::NewBuiltin(name, args, _)
            | Expr::NewValue(name, args, _)
            | Expr::NewVector(name, args, _)
            | Expr::NewError(name, args, _) => {
                for arg in args {
                    self.expr(arg, state, captured, at);
                }
                let resolved = self.ty(name);
                if matches!(expr, Expr::NewValue(..))
                    || resolved.as_ref().is_some_and(|ty| {
                        self.context
                            .project
                            .iter()
                            .any(|class| &class.qualified == ty)
                    })
                {
                    invalidate(captured, state, self.script.language);
                }
                resolved.map_or(Value::Unknown, Value::Type)
            }
            Expr::Array(items) => {
                for expr in items.iter().flatten() {
                    self.expr(expr, state, captured, at);
                }
                Value::Type("Array".into())
            }
            Expr::Object(items) => {
                for (_, expr) in items {
                    self.expr(expr, state, captured, at);
                }
                Value::Type("Object".into())
            }
            Expr::RegExpLiteral(..) => Value::Type("RegExp".into()),
            Expr::Member(base, name) => {
                let base = self.expr(base, state, captured, at);
                let (class, statics) = match base {
                    Value::Type(ty) => (ty, false),
                    Value::Class(ty) => (ty, true),
                    _ => return Value::Unknown,
                };
                self.context
                    .members(&class, statics)
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map_or(Value::Unknown, |member| self.member_value(&member))
            }
            Expr::Index(base, index) => {
                let value = self.expr(base, state, captured, at);
                self.expr(index, state, captured, at);
                value
                    .ty()
                    .and_then(|ty| {
                        ty.strip_prefix("Vector.<")
                            .and_then(|ty| ty.strip_suffix('>'))
                            .map(str::to_owned)
                    })
                    .and_then(|ty| self.ty(&ty))
                    .map_or(Value::Unknown, Value::Type)
            }
            Expr::Assign(op, target, expr) => {
                if !matches!(target.as_ref(), Expr::Name(_)) {
                    self.expr(target, state, captured, at);
                }
                let value = self.expr(expr, state, captured, at);
                let value = if op == "=" { value } else { Value::Unknown };
                if let Expr::Name(name) = target.as_ref() {
                    let declared = state.get(name).and_then(|b| b.declared.clone());
                    self.assign(name, declared.as_deref(), value.clone(), state, at);
                }
                value
            }
            Expr::Update(_, target, _) => {
                if !matches!(target.as_ref(), Expr::Name(_)) {
                    self.expr(target, state, captured, at);
                }
                if let Expr::Name(name) = target.as_ref() {
                    let declared = state.get(name).and_then(|b| b.declared.clone());
                    self.assign(
                        name,
                        declared.as_deref(),
                        Value::Type("Number".into()),
                        state,
                        at,
                    );
                }
                Value::Type("Number".into())
            }
            Expr::Call(callee, args, offset) => {
                let callee_value = self.expr(callee, state, captured, *offset);
                let arguments: Vec<_> = args
                    .iter()
                    .map(|arg| self.expr(arg, state, captured, *offset))
                    .collect();
                let result = match callee_value {
                    Value::Function(sig) => {
                        for (i, (expected, actual)) in
                            sig.params.iter().zip(arguments.iter()).enumerate()
                        {
                            if let (Some(expected), Some(actual)) = (expected, actual.ty()) {
                                self.mismatch(
                                    "flow-argument-type",
                                    *offset,
                                    &actual,
                                    expected,
                                    &format!("Argument {}", i + 1),
                                );
                            }
                        }
                        sig.returns.map_or(Value::Unknown, Value::Type)
                    }
                    Value::Class(ty) => Value::Type(ty),
                    _ => Value::Unknown,
                };
                invalidate(captured, state, self.script.language);
                result
            }
            Expr::Function(f) => Value::Function(self.signature(f, state)),
            Expr::Conditional(test, a, b) => {
                self.expr(test, state, captured, at);
                let mut yes = state.clone();
                let mut no = state.clone();
                let a = self.expr(a, &mut yes, captured, at);
                let b = self.expr(b, &mut no, captured, at);
                *state = self.join_states(&yes, &no);
                self.join_value(a, b)
            }
            Expr::Binary(op, a, b) if matches!(op.as_str(), "&&" | "||" | "and" | "or") => {
                self.expr(a, state, captured, at);
                let before = state.clone();
                let mut after = state.clone();
                self.expr(b, &mut after, captured, at);
                *state = self.join_states(&before, &after);
                Value::Unknown
            }
            Expr::Binary(op, a, b) => {
                let a = self.expr(a, state, captured, at);
                let b = self.expr(b, state, captured, at);
                match op.as_str() {
                    "," => b,
                    "+" if a.ty().as_deref() == Some("String")
                        || b.ty().as_deref() == Some("String") =>
                    {
                        Value::Type("String".into())
                    }
                    "+" | "-" | "*" | "/" | "%" | "<<" | ">>" | ">>>" | "&" | "|" | "^" => {
                        Value::Type("Number".into())
                    }
                    "==" | "!=" | "===" | "!==" | "<" | ">" | "<=" | ">=" => {
                        Value::Type("Boolean".into())
                    }
                    _ => Value::Unknown,
                }
            }
            Expr::Unary(op, expr) => {
                self.expr(expr, state, captured, at);
                match op.as_str() {
                    "!" | "not" => Value::Type("Boolean".into()),
                    "typeof" => Value::Type("String".into()),
                    "+" | "-" | "~" => Value::Type("Number".into()),
                    _ => Value::Unknown,
                }
            }
            Expr::TypeTest(op, expr, ty, _) => {
                self.expr(expr, state, captured, at);
                if op == "as" {
                    self.ty(ty).map_or(Value::Unknown, Value::Type)
                } else {
                    Value::Type("Boolean".into())
                }
            }
            Expr::TypeValue(ty, _) => Value::Class(ty.clone()),
            Expr::InstanceOf(a, b) => {
                self.expr(a, state, captured, at);
                self.expr(b, state, captured, at);
                Value::Type("Boolean".into())
            }
            Expr::XmlLiteral(_, holes) => {
                for hole in holes {
                    self.expr(hole, state, captured, at);
                }
                Value::Type("XML".into())
            }
            _ => Value::Unknown,
        }
    }
    fn member_value(&self, member: &Member) -> Value {
        if let Some(params) = &member.params {
            Value::Function(Signature {
                params: if params.contains(['\'', '\"'])
                    || params.contains("/*")
                    || params.contains("//")
                {
                    Vec::new()
                } else {
                    params
                        .split(',')
                        .map(|param| {
                            param.split_once(':').and_then(|(_, ty)| {
                                self.signature_type(
                                    member,
                                    ty.split('=').next().unwrap_or(ty).trim(),
                                )
                            })
                        })
                        .collect()
                },
                returns: self.signature_type(member, &member.ty),
            })
        } else {
            self.signature_type(member, &member.ty)
                .map_or(Value::Unknown, Value::Type)
        }
    }
    fn assign(
        &mut self,
        name: &str,
        declared: Option<&str>,
        value: Value,
        state: &mut State,
        at: usize,
    ) {
        if let (Some(expected), Some(actual)) = (declared, value.ty()) {
            self.mismatch(
                "flow-assignment-type",
                at,
                &actual,
                expected,
                "Assigned value",
            );
        }
        let value = if self.script.language == ScriptLanguage::ActionScript3 {
            declared
                .and_then(|ty| self.ty(ty))
                .filter(|ty| ty != "Function")
                .map_or(value.clone(), Value::Type)
        } else {
            value
        };
        state.insert(
            name.into(),
            Binding {
                method: false,
                declared: declared.map(str::to_owned),
                value,
            },
        );
    }
    fn mismatch(&mut self, code: &str, at: usize, actual: &str, expected: &str, label: &str) {
        if !self.diagnose
            || self.script.language != ScriptLanguage::ActionScript3
            || self.output.diagnostics.len() >= 128
        {
            return;
        }
        let Some(expected) = self.ty(expected) else {
            return;
        };
        let Some(actual) = self.ty(actual) else {
            return;
        };
        if dynamic_or_primitive(&expected) || dynamic_or_primitive(&actual) || expected == actual {
            return;
        }
        if self.context.resolve_class(&expected).is_none()
            || self.context.resolve_class(&actual).is_none()
        {
            return;
        }
        if self.context.flow_lineage(&actual).contains(&expected)
            || self.context.flow_lineage(&expected).contains(&actual)
        {
            return;
        }
        // Interfaces may be structurally represented by host adapters; absence from the inheritance
        // index is not evidence that the value cannot implement one.
        if [&expected, &actual].into_iter().any(|ty| {
            self.context
                .api
                .classes
                .get(ty)
                .is_some_and(|c| c.kind == "interface")
                || self
                    .context
                    .project
                    .iter()
                    .any(|c| &c.qualified == ty && c.is_interface)
        }) {
            return;
        }
        let mut diagnostic = Diagnostic::at(
            self.script,
            at,
            format!(
                "{label} has type '{actual}', while '{expected}' is expected; this may fail ActionScript runtime coercion."
            ),
        );
        diagnostic.code = Some(code.into());
        diagnostic.severity = DiagnosticSeverity::Warning;
        diagnostic.fixes.clear();
        self.output.diagnostics.push(diagnostic);
    }
    fn join_value(&self, a: Value, b: Value) -> Value {
        if a == b {
            return a;
        }
        match (a, b) {
            (Value::Type(a), Value::Type(b)) => {
                if numeric(&a) && numeric(&b) {
                    return Value::Type("Number".into());
                }
                let b_line = self.context.flow_lineage(&b);
                self.context
                    .flow_lineage(&a)
                    .into_iter()
                    .find(|ty| b_line.contains(ty))
                    .map_or(Value::Unknown, Value::Type)
            }
            _ => Value::Unknown,
        }
    }
    fn join_states(&self, a: &State, b: &State) -> State {
        a.iter()
            .map(|(name, binding)| {
                let other = b.get(name);
                (
                    name.clone(),
                    Binding {
                        method: false,
                        declared: binding.declared.clone(),
                        value: other.map_or(Value::Unknown, |other| {
                            self.join_value(binding.value.clone(), other.value.clone())
                        }),
                    },
                )
            })
            .collect()
    }
    fn widen_loop(&self, body: &Stmt, state: &mut State) {
        let mut writes = BTreeSet::new();
        statement_writes(body, &mut writes);
        invalidate(&writes, state, self.script.language);
    }
}
fn numeric(ty: &str) -> bool {
    matches!(ty, "Number" | "int" | "uint")
}
fn dynamic_or_primitive(ty: &str) -> bool {
    matches!(
        ty,
        "" | "*"
            | "Object"
            | "Function"
            | "Class"
            | "Number"
            | "int"
            | "uint"
            | "String"
            | "Boolean"
            | "void"
            | "Void"
            | "Array"
            | "XML"
            | "XMLList"
    )
}
fn located(stmt: &Stmt) -> usize {
    if let Stmt::Located(at, _) = stmt {
        *at
    } else {
        0
    }
}
fn invalidate(names: &BTreeSet<String>, state: &mut State, language: ScriptLanguage) {
    for name in names {
        if let Some(binding) = state.get_mut(name) {
            if language == ScriptLanguage::ActionScript3
                && binding
                    .declared
                    .as_deref()
                    .is_some_and(|t| !matches!(t, "*" | "Function"))
            {
                continue;
            }
            binding.value = Value::Unknown;
        }
    }
}
fn collect_declarations(body: &[Stmt], out: &mut BTreeMap<String, Option<String>>) {
    for stmt in body {
        match stmt.unlocated() {
            Stmt::Var(vars) => {
                for (name, ty, _) in vars {
                    out.insert(name.clone(), ty.clone());
                }
            }
            Stmt::Function(f) => {
                if let Some(name) = &f.name {
                    out.insert(name.clone(), Some("Function".into()));
                }
            }
            Stmt::Block(body) | Stmt::InitClip(_, body, _) => collect_declarations(body, out),
            Stmt::If(_, yes, no) => {
                collect_declarations(std::slice::from_ref(yes), out);
                if let Some(no) = no {
                    collect_declarations(std::slice::from_ref(no), out);
                }
            }
            Stmt::While(_, body)
            | Stmt::DoWhile(body, _)
            | Stmt::Label(_, body)
            | Stmt::With(_, body, _)
            | Stmt::TellTarget(_, body, _)
            | Stmt::IfFrameLoaded(_, body) => collect_declarations(std::slice::from_ref(body), out),
            Stmt::For(init, _, _, body) | Stmt::ForIn(init, _, _, body) => {
                collect_declarations(std::slice::from_ref(init), out);
                collect_declarations(std::slice::from_ref(body), out);
            }
            Stmt::Try(body, catches, finally) => {
                collect_declarations(body, out);
                for catch in catches {
                    collect_declarations(&catch.body, out);
                }
                if let Some(finally) = finally {
                    collect_declarations(finally, out);
                }
            }
            Stmt::Switch(_, cases) => {
                for case in cases {
                    collect_declarations(&case.body, out);
                }
            }
            _ => {}
        }
    }
}
fn assignment_names(expr: &Expr, names: &mut BTreeSet<String>) {
    match expr {
        Expr::Assign(_, target, _) | Expr::Update(_, target, _) => {
            if let Expr::Name(name) = target.as_ref() {
                names.insert(name.clone());
            }
        }
        Expr::Function(_) => return,
        _ => {}
    }
    for child in super::scope::children(expr) {
        assignment_names(child, names);
    }
}
fn statement_writes(stmt: &Stmt, names: &mut BTreeSet<String>) {
    if let Stmt::Function(_) = stmt.unlocated() {
        return;
    }
    if let Stmt::Var(vars) = stmt.unlocated() {
        for (name, _, value) in vars {
            if value.is_some() {
                names.insert(name.clone());
            }
        }
    }
    for expr in super::scope::expressions(stmt) {
        assignment_names(expr, names);
    }
    for child in super::scope::statements(stmt) {
        statement_writes(child, names);
    }
}
fn nested_functions<'a>(expr: &'a Expr, out: &mut Vec<&'a Function>) {
    if let Expr::Function(function) = expr {
        out.push(function);
        return;
    }
    for child in super::scope::children(expr) {
        nested_functions(child, out);
    }
}
fn descendant_functions<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Function>) {
    if let Stmt::Function(function) = stmt.unlocated() {
        out.push(function);
        return;
    }
    for expr in super::scope::expressions(stmt) {
        nested_functions(expr, out);
    }
    for child in super::scope::statements(stmt) {
        descendant_functions(child, out);
    }
}
fn free_writes(f: &Function) -> BTreeSet<String> {
    let mut writes = BTreeSet::new();
    let mut nested = Vec::new();
    for statement in &f.body {
        statement_writes(statement, &mut writes);
        descendant_functions(statement, &mut nested);
    }
    for function in nested {
        writes.extend(free_writes(function));
    }
    let mut local = BTreeMap::new();
    collect_declarations(&f.body, &mut local);
    for (name, _) in &f.params {
        local.insert(name.clone(), None);
    }
    if let Some(rest) = &f.rest {
        local.insert(rest.clone(), None);
    }
    writes.retain(|name| !local.contains_key(name));
    writes
}
fn captured_writes(f: &Function) -> BTreeSet<String> {
    let mut functions = Vec::new();
    for statement in &f.body {
        descendant_functions(statement, &mut functions);
    }
    functions.into_iter().flat_map(free_writes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_service::ThisKind;
    use neoflash_core::Project;

    fn context() -> HintContext<'static> {
        HintContext::new(
            &Project::demo(),
            ScriptLanguage::ActionScript3,
            ThisKind::MovieClip,
        )
        .unwrap()
    }
    fn labels(context: &HintContext<'_>, source: &str) -> Vec<String> {
        context
            .hints(source, source.chars().count(), false)
            .map_or(Vec::new(), |h| {
                h.items.into_iter().map(|h| h.label).collect()
            })
    }
    #[test]
    fn ast_flow_drives_incomplete_receiver_hints_across_assignment_branches_and_calls() {
        let context = context();
        let rectangle = "var value = new Point(); value = new Rectangle(); value.";
        let hints = labels(&context, rectangle);
        assert!(hints.contains(&"width".into()), "{hints:?}");
        assert!(!hints.contains(&"length".into()));
        let point =
            "var value; if (flag) { value=new Point(); } else { value=new Point(2,3); } value.";
        assert!(labels(&context, point).contains(&"length".into()));
        for source in [
            "var value=new Point(); value=unknownValue; value.",
            "var value:* = new Point(); unknownCallback(); value.",
            "var value; if(flag){value=new Point();}else{value=new Rectangle();} value.",
            "var value=new Point(); while(flag){value=new Rectangle();} value.",
            "function f(){ var value=new Point(); function mutate(){value=new Rectangle();} unknownCallback(); value.",
            "function one(){var value=new Point();} function two(){ value.",
            "var value=new Point(); for(value in object){} value.",
            "function f(){var value=new Point(); register(function(){value=new Rectangle();}); value.",
            "function f(){var value=new Point(); with(object){value=new Point(); value.",
            "var value=new Point(); try {value=new Rectangle(); dangerous();}catch(error:Error){} value.",
        ] {
            let hints = labels(&context, source);
            assert!(
                !hints.contains(&"length".into()) && !hints.contains(&"width".into()),
                "stale inferred receiver for {source}: {hints:?}"
            );
        }
        let local = "function f(){var value=new Point(); unknownCallback(); value.";
        assert!(
            labels(&context, local).contains(&"length".into()),
            "uncaptured local cannot be reassigned by an external call"
        );
        let shadow = "function f(){var value=new Point(); function inner(){var value=new Rectangle(); value=unknown;} unknownCallback(); value.";
        assert!(
            labels(&context, shadow).contains(&"length".into()),
            "nested local must not invalidate outer binding"
        );
        let catch = "try {dangerous();} catch(error:Error){ error.";
        assert!(labels(&context, catch).contains(&"message".into()));
        let factory = "var value=make(); function make(){ return new Point(); } value.";
        assert!(labels(&context, factory).contains(&"length".into()));
        let branches = "function make(flag:Boolean){if(flag){return new Point();}return new Point(1,2);} var value=make(true);value.";
        assert!(labels(&context, branches).contains(&"length".into()));
        let fallthrough = "function make(flag:Boolean){if(flag){return new Point();}}var value=make(false);value.";
        assert!(!labels(&context, fallthrough).contains(&"length".into()));
    }

    #[test]
    fn nominal_warnings_are_advisory_and_coercible_or_dynamic_values_remain_unclaimed() {
        let context = context();
        let source = "function consume(value:Point):void {}\nconsume(new Rectangle());\nfunction wrong():Point { return new Rectangle(); }";
        let analysis = context.semantic_analysis(source);
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.code.as_deref() == Some("flow-argument-type") && d.line == 2),
            "{:?}",
            analysis.diagnostics()
        );
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.code.as_deref() == Some("flow-return-type") && d.line == 3)
        );
        assert!(
            analysis
                .diagnostics()
                .iter()
                .all(|d| d.severity == DiagnosticSeverity::Warning && d.fixes.is_empty())
        );
        for source in [
            "function consume(value:Number):void {} consume('3');",
            "function consume(value:Point):void {} consume(dynamicValue);",
            "function consume(value:*):void {} consume(new Rectangle());",
            "function consume(value:Point):void {} consume(null);",
            "var value:DisplayObject=new Sprite(); function consume(value:Sprite):void {} consume(value);",
            "var value:IEventDispatcher=new Sprite(); function consume(value:Sprite):void {} consume(value);",
            "function consume(value:Point):void {} var value; box[(value=new Rectangle(),0)] = (value=new Point()); consume(value);",
            "var value=new Rectangle(); function consume(p:Point):Boolean{return false;} do {value=new Point();} while (consume(value));",
        ] {
            assert!(
                context.semantic_analysis(source).diagnostics().is_empty(),
                "{source}"
            );
        }
        let updates = "function consume(value:Point):void {} var value=new Point(); box[(value=new Rectangle(),0)]++; consume(value);";
        assert!(
            context
                .semantic_analysis(updates)
                .diagnostics()
                .iter()
                .any(|d| d.code.as_deref() == Some("flow-argument-type"))
        );
        let as2 = HintContext::new(
            &Project::demo(),
            ScriptLanguage::ActionScript2,
            ThisKind::MovieClip,
        )
        .unwrap();
        assert!(
            as2.semantic_analysis(
                "function consume(value:Point):Void {} consume(new Rectangle());"
            )
            .diagnostics()
            .is_empty()
        );
    }
    #[test]
    fn semantic_context_resolution_and_budget_are_conservative() {
        let mut project = Project::blank();
        for (id, source) in [
            ("b.Base", "package b { public class Base {} }"),
            ("a.Base", "package a { public class Base {} }"),
            (
                "a.Child",
                "package a { public class Child extends Base {} }",
            ),
            ("demo.Point", "package demo { public class Point {} }"),
            (
                "demo.Factory",
                "package demo { import flash.geom.Point; public class Factory { public function create():Point { return new Point(); } } }",
            ),
        ] {
            project.sources.push(neoflash_core::SourceUnit {
                id: id.into(),
                path: format!("{id}.as"),
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            });
        }
        let context =
            HintContext::new(&project, ScriptLanguage::ActionScript3, ThisKind::MovieClip).unwrap();
        let imported = "import flash.geom.Point; function consume(p:Point):void {} var point:Point=new Point();consume(point);";
        let flow = context.semantic_analysis(imported);
        assert!(flow.is_analyzed(imported.chars().count()));
        assert!(flow.diagnostics().is_empty(), "{:?}", flow.diagnostics());
        let inheritance = "function consume(value:a.Base):void {} consume(new a.Child());";
        assert!(
            context
                .semantic_analysis(inheritance)
                .diagnostics()
                .is_empty()
        );
        let query = "import flash.geom.Point;var value:Point=new Point();value.";
        assert!(labels(&context, query).contains(&"length".into()));
        let factory = "import demo.Factory; var factory:Factory=new Factory();factory.create().";
        assert!(
            labels(&context, factory).contains(&"length".into()),
            "return types resolve in the declaring file"
        );
        let member = "var point:flash.geom.Point=new flash.geom.Point();var value=point.add(new flash.geom.Point());value.";
        assert!(
            labels(&context, member).contains(&"length".into()),
            "builtin signatures cannot resolve to demo.Point"
        );
        let name = "long".repeat(5_000);
        let large = format!(
            "var {name}:flash.geom.Point=new flash.geom.Point();{} {name}.",
            ";".repeat(400)
        );
        let flow = context.semantic_analysis(&large);
        let at = large.chars().count();
        assert!(flow.is_analyzed(at));
        assert!(flow.has_binding(&name, at));
        assert!(
            flow.type_at(&name, at).is_none(),
            "exhausted snapshot budget cannot return an earlier type"
        );
        assert!(
            labels(&context, &large).is_empty(),
            "budget exhaustion cannot trigger lexical fallback"
        );
    }
}
