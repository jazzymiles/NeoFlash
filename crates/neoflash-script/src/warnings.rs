//! Structured compiler warnings, static lint checks, and automated quick-fix suggestions
//! across ActionScript 1.0/2.0/3.0, JavaScript, and TypeScript.

use crate::ts_check::suggest_identifier;
use crate::{Diagnostic, byte_offset_to_line_col, line_col_to_byte_offset};
use neoflash_core::{Project, Script, ScriptLanguage};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPatternKind, Declaration, Expression, FunctionBody, Statement, VariableDeclaration,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Severity of a compiler diagnostic or warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// An actionable code transformation that fixes a compiler diagnostic or warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickFix {
    /// Human-readable description of the fix (e.g., `"Replace 'trce' with 'trace'"`).
    pub title: String,
    /// Replacement text to substitute for `source[span.0..span.1]`.
    pub replacement: String,
    /// Byte offset range `(start, end)` in the source string.
    pub span: (usize, usize),
    /// 1-based line of the start of the edit.
    pub line: usize,
    /// 1-based column of the start of the edit.
    pub column: usize,
}

impl QuickFix {
    /// Apply this quick fix to `source`, returning the updated source text.
    pub fn apply(&self, source: &str) -> String {
        let start = self.span.0.min(source.len());
        let end = self.span.1.min(source.len()).max(start);
        let mut out = String::with_capacity(source.len() + self.replacement.len());
        out.push_str(&source[..start]);
        out.push_str(&self.replacement);
        out.push_str(&source[end..]);
        out
    }
}

/// Apply a `QuickFix` to `source`.
pub fn apply_quick_fix(source: &str, fix: &QuickFix) -> String {
    fix.apply(source)
}

/// A structured compiler warning with associated quick fixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerWarning {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub language: ScriptLanguage,
    pub path: Option<String>,
    pub line: usize,
    pub column: usize,
    pub span: (usize, usize),
    pub message: String,
    pub quick_fixes: Vec<QuickFix>,
}

impl CompilerWarning {
    /// Convert this warning into a standard `Diagnostic`.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let prefix = self
            .path
            .as_ref()
            .map(|p| format!("{p}: "))
            .unwrap_or_default();
        Diagnostic {
            language: self.language,
            line: self.line,
            column: self.column,
            message: format!("{prefix}[{}] {}", self.code, self.message),
        }
    }
}

const COMMON_BUILTINS: &[&str] = &[
    "trace",
    "play",
    "stop",
    "nextFrame",
    "prevFrame",
    "gotoAndPlay",
    "gotoAndStop",
    "stopAllSounds",
    "getURL",
    "fscommand",
    "setInterval",
    "clearInterval",
    "setTimeout",
    "clearTimeout",
    "getTimer",
    "updateAfterEvent",
    "MovieClip",
    "Sprite",
    "Shape",
    "SimpleButton",
    "TextField",
    "TextFormat",
    "Bitmap",
    "BitmapData",
    "Stage",
    "Sound",
    "Color",
    "Key",
    "Mouse",
    "Selection",
    "System",
    "Stage",
    "Matrix",
    "Point",
    "Rectangle",
    "ColorTransform",
    "Event",
    "MouseEvent",
    "KeyboardEvent",
    "Timer",
    "TimerEvent",
    "Loader",
    "URLRequest",
    "URLLoader",
    "XML",
    "XMLList",
    "RegExp",
    "Math",
    "Number",
    "String",
    "Boolean",
    "Array",
    "Object",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "JSON",
    "Map",
    "Set",
    "Promise",
    "console",
    "window",
    "document",
    "globalThis",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "_root",
    "_parent",
    "_global",
    "_level0",
    "this",
    "super",
    "arguments",
    "undefined",
    "null",
    "NaN",
    "Infinity",
    "addEventListener",
    "removeEventListener",
    "dispatchEvent",
    "addChild",
    "removeChild",
    "getChildByName",
    "onEnterFrame",
    "onRelease",
    "onPress",
    "onRollOver",
    "onRollOut",
    "onLoad",
];

/// Extract quick fixes for a compiler `Diagnostic` against `source`.
pub fn quick_fixes_for_diagnostic(diag: &Diagnostic, source: &str) -> Vec<QuickFix> {
    let mut fixes = Vec::new();
    let offset = line_col_to_byte_offset(source, diag.line, diag.column);

    // 1. Check if the diagnostic mentions "Did you mean '<suggestion>'?"
    if let Some(did_you_mean_idx) = diag.message.find("Did you mean '") {
        let rest = &diag.message[did_you_mean_idx + "Did you mean '".len()..];
        if let Some(end_quote) = rest.find('\'') {
            let suggestion = &rest[..end_quote];
            let (id_start, id_end, bad_ident) = identifier_at_offset(source, offset);
            if id_end > id_start {
                fixes.push(QuickFix {
                    title: format!("Replace '{bad_ident}' with '{suggestion}'"),
                    replacement: suggestion.to_string(),
                    span: (id_start, id_end),
                    line: diag.line,
                    column: diag.column,
                });
            }
        }
    }

    // 2. Check if the diagnostic is an unknown identifier / property error without an existing suggestion
    if fixes.is_empty()
        && (diag.message.contains("Cannot find name '")
            || diag.message.contains("Unknown identifier '")
            || diag.message.contains("Unresolved identifier '")
            || diag.message.contains("has no property '")
            || diag.message.contains("has no exported member '"))
        && let Some(bad_name) = extract_first_single_quoted(&diag.message)
    {
        let mut candidates: Vec<String> =
            COMMON_BUILTINS.iter().map(|s| (*s).to_string()).collect();
        for ident in collect_source_identifiers(source) {
            if ident != bad_name {
                candidates.push(ident);
            }
        }
        if let Some((suggestion, _)) =
            suggest_identifier(&bad_name, candidates.iter().map(String::as_str))
        {
            let (id_start, id_end, found) =
                find_identifier_near_line(source, diag.line, diag.column, &bad_name);
            if id_end > id_start {
                fixes.push(QuickFix {
                    title: format!("Replace '{found}' with '{suggestion}'"),
                    replacement: suggestion.to_string(),
                    span: (id_start, id_end),
                    line: diag.line,
                    column: diag.column,
                });
            }
        }
    }

    // 3. Type mismatch on variable initialization (`Type 'X' is not assignable to type 'Y'`)
    if diag.message.contains("is not assignable to type '")
        && let Some((actual_ty, declared_ty)) = extract_assignable_types(&diag.message)
    {
        let line_bounds = line_byte_bounds(source, diag.line);
        let line_text = &source[line_bounds.0..line_bounds.1];
        let needle = format!(": {declared_ty}");
        if let Some(rel_idx) = line_text.find(&needle) {
            let abs_start = line_bounds.0 + rel_idx + 2;
            let abs_end = abs_start + declared_ty.len();
            let (l, c) = byte_offset_to_line_col(source, abs_start);
            fixes.push(QuickFix {
                title: format!("Change type annotation from '{declared_ty}' to '{actual_ty}'"),
                replacement: actual_ty,
                span: (abs_start, abs_end),
                line: l,
                column: c,
            });
        }
    }

    fixes
}

/// Analyze a single `Script` for compiler warnings and quick fixes.
pub fn check_warnings(script: &Script) -> Vec<CompilerWarning> {
    check_warnings_with_path(script, None)
}

/// Analyze all scripts and source units in a `Project` for compiler warnings and quick fixes.
pub fn check_project_warnings(project: &Project) -> Vec<CompilerWarning> {
    let mut warnings = Vec::new();
    for unit in &project.sources {
        let script = Script {
            language: unit.language,
            source: unit.source.clone(),
        };
        warnings.extend(check_warnings_with_path(&script, Some(&unit.path)));
    }
    for symbol in &project.symbols {
        for entry in &symbol.timeline.scripts {
            warnings.extend(check_warnings_with_path(
                &entry.script,
                Some(&format!("{}[frame {}]", symbol.name, entry.frame + 1)),
            ));
        }
        for layer in &symbol.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for handler in &el.scripts {
                        warnings.extend(check_warnings_with_path(
                            &handler.script,
                            Some(&format!("{}::{}", symbol.name, el.name)),
                        ));
                    }
                }
            }
        }
    }
    warnings
}

pub fn check_warnings_with_path(script: &Script, path: Option<&str>) -> Vec<CompilerWarning> {
    match script.language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            check_js_ts_warnings(script, path)
        }
        ScriptLanguage::ActionScript1
        | ScriptLanguage::ActionScript2
        | ScriptLanguage::ActionScript3 => check_actionscript_warnings(script, path),
    }
}

fn check_js_ts_warnings(script: &Script, path: Option<&str>) -> Vec<CompilerWarning> {
    let mut warnings = Vec::new();
    let allocator = Allocator::default();
    let source_type = match script.language {
        ScriptLanguage::TypeScript => SourceType::tsx().with_module(true),
        _ => SourceType::jsx().with_module(true),
    };
    let parsed = Parser::new(&allocator, &script.source, source_type).parse();
    if !parsed.errors.is_empty() {
        return warnings;
    }

    let mut collector = JsTsWarningCollector {
        source: &script.source,
        language: script.language,
        path: path.map(str::to_string),
        declared_vars: Vec::new(),
        referenced_idents: BTreeSet::new(),
        known_names: COMMON_BUILTINS.iter().map(|s| (*s).to_string()).collect(),
        unresolved_refs: Vec::new(),
        warnings: Vec::new(),
    };
    collector.collect_program(&parsed.program);
    warnings.extend(collector.finish());
    warnings
}

struct DeclaredVar {
    name: String,
    ident_span: (usize, usize),
    decl_span: (usize, usize),
    is_exported: bool,
    has_type_annotation: bool,
    inferred_type: Option<&'static str>,
}

struct JsTsWarningCollector<'a> {
    source: &'a str,
    language: ScriptLanguage,
    path: Option<String>,
    declared_vars: Vec<DeclaredVar>,
    referenced_idents: BTreeSet<String>,
    known_names: BTreeSet<String>,
    unresolved_refs: Vec<(String, usize, usize)>,
    warnings: Vec<CompilerWarning>,
}

impl<'a> JsTsWarningCollector<'a> {
    fn collect_program(&mut self, program: &oxc_ast::ast::Program<'a>) {
        // First pass: collect top-level declarations into known_names
        for stmt in &program.body {
            self.precollect_stmt_names(stmt);
        }
        // Check top-level block for unreachable statements and walk AST
        self.check_statement_list(&program.body);
    }

    fn precollect_stmt_names(&mut self, stmt: &Statement<'a>) {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                for d in &var_decl.declarations {
                    if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                        self.known_names.insert(id.name.as_str().to_string());
                    }
                }
            }
            Statement::FunctionDeclaration(fn_decl) => {
                if let Some(id) = &fn_decl.id {
                    self.known_names.insert(id.name.as_str().to_string());
                }
            }
            Statement::ClassDeclaration(cls_decl) => {
                if let Some(id) = &cls_decl.id {
                    self.known_names.insert(id.name.as_str().to_string());
                }
            }
            Statement::ImportDeclaration(imp) => {
                if let Some(specs) = &imp.specifiers {
                    for s in specs {
                        match s {
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(sp) => {
                                self.known_names.insert(sp.local.name.as_str().to_string());
                            }
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(
                                sp,
                            ) => {
                                self.known_names.insert(sp.local.name.as_str().to_string());
                            }
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                                sp,
                            ) => {
                                self.known_names.insert(sp.local.name.as_str().to_string());
                            }
                        }
                    }
                }
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(Declaration::FunctionDeclaration(f)) = &exp.declaration
                    && let Some(id) = &f.id
                {
                    self.known_names.insert(id.name.as_str().to_string());
                } else if let Some(Declaration::ClassDeclaration(c)) = &exp.declaration
                    && let Some(id) = &c.id
                {
                    self.known_names.insert(id.name.as_str().to_string());
                } else if let Some(Declaration::VariableDeclaration(v)) = &exp.declaration {
                    for d in &v.declarations {
                        if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                            self.known_names.insert(id.name.as_str().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_statement_list(&mut self, stmts: &[Statement<'a>]) {
        let mut terminated_by: Option<&'static str> = None;
        let mut unreachable_reported = false;

        for stmt in stmts {
            if let Some(term_kw) = terminated_by
                && !unreachable_reported
                && !matches!(
                    stmt,
                    Statement::FunctionDeclaration(_)
                        | Statement::EmptyStatement(_)
                        | Statement::TSInterfaceDeclaration(_)
                        | Statement::TSTypeAliasDeclaration(_)
                )
            {
                unreachable_reported = true;
                let start = stmt.span().start as usize;
                let end = stmts
                    .last()
                    .map(|s| s.span().end as usize)
                    .unwrap_or(stmt.span().end as usize);
                let (line, column) = byte_offset_to_line_col(self.source, start);
                self.warnings.push(CompilerWarning {
                    code: "unreachable-code".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language: self.language,
                    path: self.path.clone(),
                    line,
                    column,
                    span: (start, end),
                    message: format!("Unreachable code after '{term_kw}' statement."),
                    quick_fixes: vec![QuickFix {
                        title: "Remove unreachable code".to_string(),
                        replacement: String::new(),
                        span: (start, end),
                        line,
                        column,
                    }],
                });
            }

            if terminated_by.is_none() {
                terminated_by = match stmt {
                    Statement::ReturnStatement(_) => Some("return"),
                    Statement::ThrowStatement(_) => Some("throw"),
                    Statement::BreakStatement(_) => Some("break"),
                    Statement::ContinueStatement(_) => Some("continue"),
                    _ => None,
                };
            }

            self.visit_statement(stmt, false);
        }
    }

    fn visit_statement(&mut self, stmt: &Statement<'a>, is_exported: bool) {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                self.visit_var_decl(var_decl, is_exported);
            }
            Statement::FunctionDeclaration(fn_decl) => {
                if let Some(id) = &fn_decl.id {
                    self.known_names.insert(id.name.as_str().to_string());
                }
                for param in &fn_decl.params.items {
                    if let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind {
                        self.known_names.insert(id.name.as_str().to_string());
                    }
                }
                if let Some(body) = &fn_decl.body {
                    self.visit_function_body(body);
                }
            }
            Statement::ClassDeclaration(cls_decl) => {
                if let Some(id) = &cls_decl.id {
                    self.known_names.insert(id.name.as_str().to_string());
                }
                for elem in &cls_decl.body.body {
                    match elem {
                        oxc_ast::ast::ClassElement::MethodDefinition(m) => {
                            for param in &m.value.params.items {
                                if let BindingPatternKind::BindingIdentifier(id) =
                                    &param.pattern.kind
                                {
                                    self.known_names.insert(id.name.as_str().to_string());
                                }
                            }
                            if let Some(body) = &m.value.body {
                                self.visit_function_body(body);
                            }
                        }
                        oxc_ast::ast::ClassElement::PropertyDefinition(p) => {
                            if let Some(val) = &p.value {
                                self.visit_expression(val);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Statement::BlockStatement(block) => {
                self.check_statement_list(&block.body);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg);
                }
            }
            Statement::ThrowStatement(thr) => {
                self.visit_expression(&thr.argument);
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test);
                self.visit_statement(&if_stmt.consequent, false);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt, false);
                }
            }
            Statement::WhileStatement(w) => {
                self.visit_expression(&w.test);
                self.visit_statement(&w.body, false);
            }
            Statement::DoWhileStatement(dw) => {
                self.visit_statement(&dw.body, false);
                self.visit_expression(&dw.test);
            }
            Statement::ForStatement(f) => {
                if let Some(init) = &f.init {
                    match init {
                        oxc_ast::ast::ForStatementInit::VariableDeclaration(vd) => {
                            self.visit_var_decl(vd, true); // loop counters are often used or intentional
                        }
                        _ => {
                            if let Some(expr) = init.as_expression() {
                                self.visit_expression(expr);
                            }
                        }
                    }
                }
                if let Some(test) = &f.test {
                    self.visit_expression(test);
                }
                if let Some(update) = &f.update {
                    self.visit_expression(update);
                }
                self.visit_statement(&f.body, false);
            }
            Statement::ForInStatement(fi) => {
                self.visit_expression(&fi.right);
                self.visit_statement(&fi.body, false);
            }
            Statement::ForOfStatement(fo) => {
                self.visit_expression(&fo.right);
                self.visit_statement(&fo.body, false);
            }
            Statement::SwitchStatement(sw) => {
                self.visit_expression(&sw.discriminant);
                for case in &sw.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test);
                    }
                    self.check_statement_list(&case.consequent);
                }
            }
            Statement::TryStatement(tr) => {
                self.check_statement_list(&tr.block.body);
                if let Some(handler) = &tr.handler {
                    if let Some(param) = &handler.param
                        && let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind
                    {
                        self.known_names.insert(id.name.as_str().to_string());
                    }
                    self.check_statement_list(&handler.body.body);
                }
                if let Some(fin) = &tr.finalizer {
                    self.check_statement_list(&fin.body);
                }
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(decl) = &exp.declaration {
                    match decl {
                        Declaration::VariableDeclaration(vd) => self.visit_var_decl(vd, true),
                        Declaration::FunctionDeclaration(fd) => {
                            if let Some(body) = &fd.body {
                                self.visit_function_body(body);
                            }
                        }
                        Declaration::ClassDeclaration(cd) => {
                            for elem in &cd.body.body {
                                if let oxc_ast::ast::ClassElement::MethodDefinition(m) = elem
                                    && let Some(body) = &m.value.body
                                {
                                    self.visit_function_body(body);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                for spec in &exp.specifiers {
                    let local = match &spec.local {
                        oxc_ast::ast::ModuleExportName::IdentifierName(id) => id.name.as_str(),
                        oxc_ast::ast::ModuleExportName::IdentifierReference(id) => id.name.as_str(),
                        oxc_ast::ast::ModuleExportName::StringLiteral(s) => s.value.as_str(),
                    };
                    self.referenced_idents.insert(local.to_string());
                }
            }
            Statement::ExportDefaultDeclaration(exp) => {
                if let Some(expr) = exp.declaration.as_expression() {
                    self.visit_expression(expr);
                }
            }
            _ => {}
        }
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        for stmt in &body.statements {
            self.precollect_stmt_names(stmt);
        }
        self.check_statement_list(&body.statements);
    }

    fn visit_var_decl(&mut self, var_decl: &VariableDeclaration<'a>, is_exported: bool) {
        let multi = var_decl.declarations.len() > 1;
        for decl in &var_decl.declarations {
            if let Some(init) = &decl.init {
                self.visit_expression(init);
            }
            if let BindingPatternKind::BindingIdentifier(ident) = &decl.id.kind {
                let name = ident.name.as_str().to_string();
                self.known_names.insert(name.clone());
                let ident_span = (ident.span.start as usize, ident.span.end as usize);
                let decl_span = if multi {
                    (decl.span.start as usize, decl.span.end as usize)
                } else {
                    (var_decl.span.start as usize, var_decl.span.end as usize)
                };
                let inferred_type = decl.init.as_ref().and_then(infer_simple_literal_type);
                self.declared_vars.push(DeclaredVar {
                    name,
                    ident_span,
                    decl_span,
                    is_exported,
                    has_type_annotation: decl.id.type_annotation.is_some(),
                    inferred_type,
                });
            }
        }
    }

    fn visit_expression(&mut self, expr: &Expression<'a>) {
        match expr {
            Expression::Identifier(ident) => {
                let name = ident.name.as_str().to_string();
                self.referenced_idents.insert(name.clone());
                if !self.known_names.contains(&name) {
                    self.unresolved_refs.push((
                        name,
                        ident.span.start as usize,
                        ident.span.end as usize,
                    ));
                }
            }
            Expression::BinaryExpression(bin) => {
                self.visit_expression(&bin.left);
                self.visit_expression(&bin.right);
            }
            Expression::LogicalExpression(log) => {
                self.visit_expression(&log.left);
                self.visit_expression(&log.right);
            }
            Expression::UnaryExpression(un) => {
                self.visit_expression(&un.argument);
            }
            Expression::UpdateExpression(up) => {
                if let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) =
                    &up.argument
                {
                    self.referenced_idents.insert(id.name.as_str().to_string());
                }
            }
            Expression::AssignmentExpression(assign) => {
                if let Some(simple) = assign.left.as_simple_assignment_target()
                    && let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) =
                        simple
                {
                    let name = id.name.as_str().to_string();
                    self.referenced_idents.insert(name.clone());
                    if !self.known_names.contains(&name) {
                        self.unresolved_refs.push((
                            name,
                            id.span.start as usize,
                            id.span.end as usize,
                        ));
                    }
                }
                self.visit_expression(&assign.right);
            }
            Expression::CallExpression(call) => {
                self.visit_expression(&call.callee);
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::NewExpression(new_expr) => {
                self.visit_expression(&new_expr.callee);
                for arg in &new_expr.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::StaticMemberExpression(mem) => {
                self.visit_expression(&mem.object);
            }
            Expression::ComputedMemberExpression(mem) => {
                self.visit_expression(&mem.object);
                self.visit_expression(&mem.expression);
            }
            Expression::ConditionalExpression(cond) => {
                self.visit_expression(&cond.test);
                self.visit_expression(&cond.consequent);
                self.visit_expression(&cond.alternate);
            }
            Expression::ArrayExpression(arr) => {
                for elem in &arr.elements {
                    if let Some(e) = elem.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop {
                        self.visit_expression(&p.value);
                    }
                }
            }
            Expression::TemplateLiteral(tpl) => {
                for e in &tpl.expressions {
                    self.visit_expression(e);
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                for param in &arrow.params.items {
                    if let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind {
                        self.known_names.insert(id.name.as_str().to_string());
                    }
                }
                self.visit_function_body(&arrow.body);
            }
            Expression::FunctionExpression(fn_expr) => {
                for param in &fn_expr.params.items {
                    if let BindingPatternKind::BindingIdentifier(id) = &param.pattern.kind {
                        self.known_names.insert(id.name.as_str().to_string());
                    }
                }
                if let Some(body) = &fn_expr.body {
                    self.visit_function_body(body);
                }
            }
            Expression::ParenthesizedExpression(p) => {
                self.visit_expression(&p.expression);
            }
            Expression::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression);
            }
            Expression::JSXElement(el) => {
                self.visit_jsx_element(el);
            }
            Expression::JSXFragment(frag) => {
                for child in &frag.children {
                    self.visit_jsx_child(child);
                }
            }
            _ => {}
        }
    }

    fn visit_jsx_element(&mut self, el: &oxc_ast::ast::JSXElement<'a>) {
        if let oxc_ast::ast::JSXElementName::IdentifierReference(id) = &el.opening_element.name {
            self.referenced_idents.insert(id.name.as_str().to_string());
        }
        for attr in &el.opening_element.attributes {
            if let oxc_ast::ast::JSXAttributeItem::Attribute(a) = attr
                && let Some(oxc_ast::ast::JSXAttributeValue::ExpressionContainer(c)) = &a.value
                && let Some(e) = c.expression.as_expression()
            {
                self.visit_expression(e);
            }
        }
        for child in &el.children {
            self.visit_jsx_child(child);
        }
    }

    fn visit_jsx_child(&mut self, child: &oxc_ast::ast::JSXChild<'a>) {
        match child {
            oxc_ast::ast::JSXChild::Element(el) => self.visit_jsx_element(el),
            oxc_ast::ast::JSXChild::ExpressionContainer(c) => {
                if let Some(e) = c.expression.as_expression() {
                    self.visit_expression(e);
                }
            }
            oxc_ast::ast::JSXChild::Fragment(frag) => {
                for ch in &frag.children {
                    self.visit_jsx_child(ch);
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<CompilerWarning> {
        for var in &self.declared_vars {
            if !var.is_exported
                && !var.name.starts_with('_')
                && !self.referenced_idents.contains(&var.name)
            {
                let (line, column) = byte_offset_to_line_col(self.source, var.ident_span.0);
                self.warnings.push(CompilerWarning {
                    code: "unused-variable".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language: self.language,
                    path: self.path.clone(),
                    line,
                    column,
                    span: var.ident_span,
                    message: format!("Variable '{}' is declared but never used.", var.name),
                    quick_fixes: vec![
                        QuickFix {
                            title: format!("Prefix unused variable '{}' with '_'", var.name),
                            replacement: format!("_{}", var.name),
                            span: var.ident_span,
                            line,
                            column,
                        },
                        QuickFix {
                            title: format!("Remove unused variable '{}'", var.name),
                            replacement: String::new(),
                            span: var.decl_span,
                            line,
                            column,
                        },
                    ],
                });
            }

            if self.language == ScriptLanguage::TypeScript
                && !var.has_type_annotation
                && let Some(inferred) = var.inferred_type
            {
                let (line, column) = byte_offset_to_line_col(self.source, var.ident_span.0);
                self.warnings.push(CompilerWarning {
                    code: "missing-type-annotation".to_string(),
                    severity: DiagnosticSeverity::Info,
                    language: self.language,
                    path: self.path.clone(),
                    line,
                    column,
                    span: var.ident_span,
                    message: format!(
                        "Variable '{}' is missing an explicit type annotation (inferred '{}').",
                        var.name, inferred
                    ),
                    quick_fixes: vec![QuickFix {
                        title: format!("Add type annotation ': {inferred}' to '{}'", var.name),
                        replacement: format!("{}: {inferred}", var.name),
                        span: var.ident_span,
                        line,
                        column,
                    }],
                });
            }
        }

        for (bad_name, start, end) in &self.unresolved_refs {
            if let Some((suggestion, _)) =
                suggest_identifier(bad_name, self.known_names.iter().map(String::as_str))
            {
                let (line, column) = byte_offset_to_line_col(self.source, *start);
                self.warnings.push(CompilerWarning {
                    code: "misspelled-identifier".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language: self.language,
                    path: self.path.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: format!(
                        "Unknown identifier '{bad_name}'. Did you mean '{suggestion}'?"
                    ),
                    quick_fixes: vec![QuickFix {
                        title: format!("Replace '{bad_name}' with '{suggestion}'"),
                        replacement: suggestion.to_string(),
                        span: (*start, *end),
                        line,
                        column,
                    }],
                });
            }
        }

        self.warnings
    }
}

fn infer_simple_literal_type(expr: &Expression<'_>) -> Option<&'static str> {
    match expr {
        Expression::NumericLiteral(_) => Some("number"),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some("string"),
        Expression::BooleanLiteral(_) => Some("boolean"),
        Expression::ArrayExpression(_) => Some("any[]"),
        _ => None,
    }
}

fn check_actionscript_warnings(script: &Script, path: Option<&str>) -> Vec<CompilerWarning> {
    let mut warnings = Vec::new();
    let src = &script.source;
    let path_str = path.map(str::to_string);

    // 1. Deprecated AS1/AS2 constructs (`tellTarget`, `ifFrameLoaded`, `eq`, `ne`, `and`, `or`, `not`, `add`)
    if matches!(
        script.language,
        ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3
    ) {
        find_deprecated_as_constructs(src, script.language, &path_str, &mut warnings);
    }

    // 2. Unused local variables and missing type annotations (`var x = ...`)
    find_actionscript_var_warnings(src, script.language, &path_str, &mut warnings);

    // 3. Unreachable statements after `return`/`throw`/`break`/`continue` in line-oriented blocks
    find_actionscript_unreachable(src, script.language, &path_str, &mut warnings);

    // 4. Misspelled identifiers against declared variables & built-in Flash globals
    find_actionscript_misspellings(src, script.language, &path_str, &mut warnings);

    warnings
}

fn find_deprecated_as_constructs(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    warnings: &mut Vec<CompilerWarning>,
) {
    let tokens = scan_word_tokens(src);
    for (idx, (word, start, end)) in tokens.iter().enumerate() {
        let (line, column) = byte_offset_to_line_col(src, *start);
        match word.as_str() {
            "tellTarget" => {
                warnings.push(CompilerWarning {
                    code: "deprecated-construct".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: "'tellTarget' is deprecated since Flash 5; use 'with' or direct dot syntax instead.".to_string(),
                    quick_fixes: vec![QuickFix {
                        title: "Replace 'tellTarget' with 'with'".to_string(),
                        replacement: "with".to_string(),
                        span: (*start, *end),
                        line,
                        column,
                    }],
                });
            }
            "ifFrameLoaded" => {
                warnings.push(CompilerWarning {
                    code: "deprecated-construct".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: "'ifFrameLoaded' is deprecated; check '_framesloaded >= _totalframes' instead.".to_string(),
                    quick_fixes: vec![QuickFix {
                        title: "Replace 'ifFrameLoaded' with 'if (_framesloaded >='".to_string(),
                        replacement: "if (_framesloaded >=".to_string(),
                        span: (*start, *end),
                        line,
                        column,
                    }],
                });
            }
            "eq" | "ne" | "and" | "or" | "add" if idx > 0 && idx + 1 < tokens.len() => {
                let (modern, desc) = match word.as_str() {
                    "eq" => ("==", "string equality operator 'eq'"),
                    "ne" => ("!=", "string inequality operator 'ne'"),
                    "and" => ("&&", "logical operator 'and'"),
                    "or" => ("||", "logical operator 'or'"),
                    "add" => ("+", "string concatenation operator 'add'"),
                    _ => unreachable!(),
                };
                warnings.push(CompilerWarning {
                    code: "deprecated-construct".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: format!("Deprecated Flash 4 {desc}; use '{modern}' instead."),
                    quick_fixes: vec![QuickFix {
                        title: format!("Replace deprecated '{word}' with '{modern}'"),
                        replacement: modern.to_string(),
                        span: (*start, *end),
                        line,
                        column,
                    }],
                });
            }
            "not" if idx + 1 < tokens.len() => {
                warnings.push(CompilerWarning {
                    code: "deprecated-construct".to_string(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: "Deprecated Flash 4 logical operator 'not'; use '!' instead."
                        .to_string(),
                    quick_fixes: vec![QuickFix {
                        title: "Replace deprecated 'not' with '!'".to_string(),
                        replacement: "!".to_string(),
                        span: (*start, *end),
                        line,
                        column,
                    }],
                });
            }
            _ => {}
        }
    }
}

fn find_actionscript_var_warnings(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    warnings: &mut Vec<CompilerWarning>,
) {
    let tokens = scan_word_tokens(src);
    let mut var_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (word, _, _) in &tokens {
        *var_counts.entry(word.clone()).or_insert(0) += 1;
    }

    for (idx, (word, var_start, _)) in tokens.iter().enumerate() {
        if word != "var" {
            continue;
        }
        // Avoid class field `public var` / `private var` which may be accessed externally
        if idx > 0
            && matches!(
                tokens[idx - 1].0.as_str(),
                "public" | "private" | "protected" | "internal" | "static"
            )
        {
            continue;
        }
        let Some((var_name, name_start, name_end)) = tokens.get(idx + 1) else {
            continue;
        };
        if is_as_keyword(var_name) {
            continue;
        }
        let after_name = &src[*name_end..];
        let trimmed_after = after_name.trim_start();
        let has_type = trimmed_after.starts_with(':');

        // Unused variable check (appears only once at its declaration)
        if !var_name.starts_with('_') && var_counts.get(var_name).copied().unwrap_or(0) == 1 {
            let (line, column) = byte_offset_to_line_col(src, *name_start);
            let stmt_end = src[*var_start..]
                .find(';')
                .map(|rel| *var_start + rel + 1)
                .unwrap_or(*name_end);
            warnings.push(CompilerWarning {
                code: "unused-variable".to_string(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path.clone(),
                line,
                column,
                span: (*name_start, *name_end),
                message: format!("Variable '{var_name}' is declared but never used."),
                quick_fixes: vec![
                    QuickFix {
                        title: format!("Prefix unused variable '{var_name}' with '_'"),
                        replacement: format!("_{var_name}"),
                        span: (*name_start, *name_end),
                        line,
                        column,
                    },
                    QuickFix {
                        title: format!("Remove unused variable '{var_name}'"),
                        replacement: String::new(),
                        span: (*var_start, stmt_end),
                        line,
                        column,
                    },
                ],
            });
        }

        // Missing type annotation check in AS2 / AS3
        if matches!(
            language,
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3
        ) && !has_type
            && let Some(rest_eq) = trimmed_after.strip_prefix('=')
        {
            let rhs = rest_eq.trim_start();
            let inferred = if rhs.starts_with('"') || rhs.starts_with('\'') {
                Some("String")
            } else if rhs.starts_with("true") || rhs.starts_with("false") {
                Some("Boolean")
            } else if rhs.starts_with('[') {
                Some("Array")
            } else if rhs
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '-')
            {
                Some("Number")
            } else if let Some(after_new) = rhs.strip_prefix("new ") {
                let cls: String = after_new
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                match cls.as_str() {
                    "Array" => Some("Array"),
                    "Object" => Some("Object"),
                    "Date" => Some("Date"),
                    "Sound" => Some("Sound"),
                    "MovieClip" => Some("MovieClip"),
                    "Sprite" => Some("Sprite"),
                    "TextField" => Some("TextField"),
                    _ => None,
                }
            } else {
                None
            };

            if let Some(ty) = inferred {
                let (line, column) = byte_offset_to_line_col(src, *name_start);
                warnings.push(CompilerWarning {
                    code: "missing-type-annotation".to_string(),
                    severity: DiagnosticSeverity::Info,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (*name_start, *name_end),
                    message: format!(
                        "Variable '{var_name}' is missing an explicit type annotation (inferred '{ty}')."
                    ),
                    quick_fixes: vec![QuickFix {
                        title: format!("Add type annotation ':{ty}' to '{var_name}'"),
                        replacement: format!("{var_name}:{ty}"),
                        span: (*name_start, *name_end),
                        line,
                        column,
                    }],
                });
            }
        }
    }
}

fn find_actionscript_unreachable(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    warnings: &mut Vec<CompilerWarning>,
) {
    let mut offset = 0usize;
    let mut after_terminator: Option<&'static str> = None;

    for line_str in src.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line_str.len();
        offset = line_end;

        let trimmed = line_str.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        if trimmed.starts_with('}')
            || trimmed.starts_with("case ")
            || trimmed.starts_with("default:")
            || trimmed.starts_with("else")
            || trimmed.starts_with("catch")
            || trimmed.starts_with("finally")
        {
            after_terminator = None;
            continue;
        }

        if let Some(term_kw) = after_terminator {
            let first_non_ws = line_str.find(|c: char| !c.is_whitespace()).unwrap_or(0);
            let span_start = line_start + first_non_ws;
            let span_end = line_end - if line_str.ends_with('\n') { 1 } else { 0 };
            let (line, column) = byte_offset_to_line_col(src, span_start);
            warnings.push(CompilerWarning {
                code: "unreachable-code".to_string(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path.clone(),
                line,
                column,
                span: (span_start, span_end),
                message: format!("Unreachable code after '{term_kw}' statement."),
                quick_fixes: vec![QuickFix {
                    title: "Remove unreachable code".to_string(),
                    replacement: String::new(),
                    span: (span_start, span_end),
                    line,
                    column,
                }],
            });
            after_terminator = None;
            continue;
        }

        if (trimmed == "return;"
            || trimmed.starts_with("return ")
            || trimmed.starts_with("throw ")
            || trimmed == "break;"
            || trimmed == "continue;")
            && !trimmed.contains('{')
            && !trimmed.contains('}')
        {
            after_terminator = if trimmed.starts_with("return") {
                Some("return")
            } else if trimmed.starts_with("throw") {
                Some("throw")
            } else if trimmed.starts_with("break") {
                Some("break")
            } else {
                Some("continue")
            };
        }
    }
}

fn find_actionscript_misspellings(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    warnings: &mut Vec<CompilerWarning>,
) {
    let tokens = scan_word_tokens(src);
    let mut declared: BTreeSet<String> = COMMON_BUILTINS.iter().map(|s| (*s).to_string()).collect();

    for (idx, (word, _, _)) in tokens.iter().enumerate() {
        if matches!(word.as_str(), "var" | "function" | "class" | "interface")
            && let Some((next_word, _, _)) = tokens.get(idx + 1)
        {
            declared.insert(next_word.clone());
        }
    }

    for (idx, (word, start, end)) in tokens.iter().enumerate() {
        if is_as_keyword(word) || declared.contains(word) {
            continue;
        }
        // Only check standalone calls `word(...)` that are not preceded by `.` or `function`
        let preceded_by_dot = *start > 0 && src[..*start].trim_end().ends_with('.');
        let preceded_by_decl = idx > 0
            && matches!(
                tokens[idx - 1].0.as_str(),
                "var" | "function" | "class" | "interface" | "new" | "import" | "package"
            );
        let followed_by_paren = src[*end..].trim_start().starts_with('(');
        if !preceded_by_dot
            && !preceded_by_decl
            && followed_by_paren
            && let Some((suggestion, dist)) =
                suggest_identifier(word, declared.iter().map(String::as_str))
            && dist <= 2
        {
            let (line, column) = byte_offset_to_line_col(src, *start);
            warnings.push(CompilerWarning {
                code: "misspelled-identifier".to_string(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path.clone(),
                line,
                column,
                span: (*start, *end),
                message: format!("Unknown identifier '{word}'. Did you mean '{suggestion}'?"),
                quick_fixes: vec![QuickFix {
                    title: format!("Replace '{word}' with '{suggestion}'"),
                    replacement: suggestion.to_string(),
                    span: (*start, *end),
                    line,
                    column,
                }],
            });
        }
    }
}

/// Tokenize identifiers/keywords outside of string literals and comments.
pub fn scan_word_tokens(src: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Line comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // String literal
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            if let Ok(word) = std::str::from_utf8(&bytes[start..i]) {
                out.push((word.to_string(), start, i));
            }
            continue;
        }
        i += 1;
    }
    out
}

fn is_as_keyword(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "let"
            | "const"
            | "function"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "new"
            | "delete"
            | "typeof"
            | "instanceof"
            | "in"
            | "with"
            | "class"
            | "interface"
            | "extends"
            | "implements"
            | "package"
            | "import"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "static"
            | "dynamic"
            | "final"
            | "override"
            | "true"
            | "false"
            | "null"
            | "undefined"
            | "void"
            | "this"
            | "super"
            | "on"
            | "onClipEvent"
            | "tellTarget"
            | "ifFrameLoaded"
            | "eq"
            | "ne"
            | "and"
            | "or"
            | "not"
            | "add"
    )
}

fn collect_source_identifiers(source: &str) -> BTreeSet<String> {
    scan_word_tokens(source)
        .into_iter()
        .map(|(w, _, _)| w)
        .filter(|w| !is_as_keyword(w))
        .collect()
}

fn identifier_at_offset(source: &str, offset: usize) -> (usize, usize, String) {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return (offset, offset, String::new());
    }
    let mut start = offset;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'_'
            || bytes[start - 1] == b'$')
    {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$')
    {
        end += 1;
    }
    let text = source.get(start..end).unwrap_or("").to_string();
    (start, end, text)
}

fn find_identifier_near_line(
    source: &str,
    line: usize,
    column: usize,
    target_ident: &str,
) -> (usize, usize, String) {
    let (line_start, line_end) = line_byte_bounds(source, line);
    let line_text = &source[line_start..line_end];
    if let Some(rel_idx) = line_text.find(target_ident) {
        let abs_start = line_start + rel_idx;
        let abs_end = abs_start + target_ident.len();
        return (abs_start, abs_end, target_ident.to_string());
    }
    let offset = line_col_to_byte_offset(source, line, column);
    identifier_at_offset(source, offset)
}

fn line_byte_bounds(source: &str, line: usize) -> (usize, usize) {
    let target = line.saturating_sub(1);
    let mut offset = 0usize;
    for (idx, l) in source.split_inclusive('\n').enumerate() {
        if idx == target {
            return (offset, offset + l.len());
        }
        offset += l.len();
    }
    (source.len(), source.len())
}

fn extract_first_single_quoted(msg: &str) -> Option<String> {
    let first = msg.find('\'')?;
    let rest = &msg[first + 1..];
    let second = rest.find('\'')?;
    Some(rest[..second].to_string())
}

fn extract_assignable_types(msg: &str) -> Option<(String, String)> {
    let prefix = "Type '";
    let start = msg.find(prefix)? + prefix.len();
    let rest = &msg[start..];
    let end_actual = rest.find('\'')?;
    let actual = rest[..end_actual].to_string();
    let mid = "is not assignable to type '";
    let start_decl = rest.find(mid)? + mid.len();
    let rest_decl = &rest[start_decl..];
    let end_decl = rest_decl.find('\'')?;
    let declared = rest_decl[..end_decl].to_string();
    Some((actual, declared))
}
