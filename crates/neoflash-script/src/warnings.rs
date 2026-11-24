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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    #[default]
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
    /// Reject out-of-bounds and non-character-boundary edits without modifying text.
    pub fn try_apply(&self, source: &str) -> Option<String> {
        let (start, end) = self.span;
        if start > end || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return None;
        }
        let mut out = String::with_capacity(source.len() + self.replacement.len());
        out.push_str(&source[..start]);
        out.push_str(&self.replacement);
        out.push_str(&source[end..]);
        Some(out)
    }

    /// Apply this quick fix to `source`, returning the updated source text.
    pub fn apply(&self, source: &str) -> String {
        self.try_apply(source).unwrap_or_else(|| source.to_owned())
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
        let mut diagnostic =
            Diagnostic::new(self.language, self.line, self.column, self.message.clone());
        diagnostic.severity = self.severity;
        diagnostic.code = Some(self.code.clone());
        diagnostic.path = self.path.clone();
        diagnostic.span = self.span;
        diagnostic.fixes = self.quick_fixes.clone();
        diagnostic
    }

    /// Preserve Unicode/multiline range information when the authored source is available.
    pub fn to_diagnostic_with_source(&self, source: &str) -> Diagnostic {
        let mut diagnostic = self.to_diagnostic();
        (diagnostic.end_line, diagnostic.end_column) = byte_offset_to_line_col(source, self.span.1);
        diagnostic
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
    "SwiftHost",
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

    // Resolve AS type imports from the shared generated support index. Do not guess between
    // equally named classes or edit a user-written class name to a different type.
    if matches!(
        diag.language,
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3
    ) && diag.message.starts_with("Type '")
        && diag
            .message
            .contains("is not supported in this ActionScript annotation")
        && let Some(name) = extract_first_single_quoted(&diag.message)
        && let Some(api) = crate::language_service::api(diag.language)
    {
        let candidates: Vec<_> = api
            .classes
            .keys()
            .filter(|qualified| {
                qualified.contains('.') && crate::language_service::short_name(qualified) == name
            })
            .collect();
        if let [qualified] = candidates.as_slice()
            && crate::language_service::missing_import(source, qualified).is_some()
        {
            let start = crate::actionscript::diagnostics::import_insertion(source, diag.language);
            let (line, column) = byte_offset_to_line_col(source, start);
            fixes.push(QuickFix {
                title: format!("Import {qualified}"),
                replacement: format!("\nimport {qualified};\n"),
                span: (start, start),
                line,
                column,
            });
        }
    }

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
            if id_end > id_start && found == bad_name {
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

    // A missing standalone assignment target can be declared without guessing its type.
    // Expressions, member writes and calls do not qualify.
    if matches!(
        diag.language,
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
    ) && diag.message.contains("Cannot find name '")
    {
        let (start, end, name) = identifier_at_offset(source, offset);
        let prefix = source[..start].trim_end();
        let suffix = source[end..].trim_start();
        if !name.is_empty()
            && (prefix.is_empty() || prefix.ends_with([';', '{', '}']))
            && suffix.starts_with('=')
            && !suffix.starts_with("==")
            && !suffix.starts_with("=>")
        {
            fixes.push(QuickFix {
                title: format!("Declare variable '{name}'"),
                replacement: format!("let {name}"),
                span: (start, end),
                line: diag.line,
                column: diag.column,
            });
        }
    }

    // 3. Type mismatch on variable initialization (`Type 'X' is not assignable to type 'Y'`)
    if diag.message.contains("is not assignable to type '")
        && let Some((actual_ty, declared_ty)) = extract_assignable_types(&diag.message)
    {
        let line_bounds = line_byte_bounds(source, diag.line);
        let line_text = &source[line_bounds.0..offset.max(line_bounds.0).min(line_bounds.1)];
        let needle = format!(": {declared_ty}");
        if let Some(rel_idx) = line_text.rfind(&needle)
            && !line_text[rel_idx..].contains(';')
        {
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
    let xcode_swift = project.publish_settings().xcode_swift_target;
    for unit in &project.sources {
        let script = Script {
            language: unit.language,
            source: unit.source.clone(),
        };
        warnings.extend(check_warnings_with_path(&script, Some(&unit.path)));
        if xcode_swift {
            warnings.extend(check_xcode_swift_target_warnings(
                &script,
                Some(&unit.path),
                true,
            ));
        }
    }
    for symbol in &project.symbols {
        for entry in &symbol.timeline.scripts {
            let loc = format!("{}[frame {}]", symbol.name, entry.frame + 1);
            warnings.extend(check_warnings_with_path(&entry.script, Some(&loc)));
            if xcode_swift {
                warnings.extend(check_xcode_swift_target_warnings(
                    &entry.script,
                    Some(&loc),
                    false,
                ));
            }
        }
        for layer in &symbol.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for handler in &el.scripts {
                        let loc = format!("{}::{}", symbol.name, el.name);
                        warnings.extend(check_warnings_with_path(&handler.script, Some(&loc)));
                        if xcode_swift {
                            warnings.extend(check_xcode_swift_target_warnings(
                                &handler.script,
                                Some(&loc),
                                false,
                            ));
                        }
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
    if script.source.len() > 1_048_576 || crate::check_web_nesting_bounds(&script.source).is_err() {
        return warnings;
    }
    let allocator = Allocator::default();
    let source_type = match script.language {
        ScriptLanguage::TypeScript => SourceType::tsx().with_module(true),
        _ => SourceType::jsx().with_module(true),
    };
    let parsed = Parser::new(&allocator, &script.source, source_type).parse();
    if !parsed.errors.is_empty()
        || crate::oxc_program_exceeds_depth(&parsed.program, crate::MAX_WEB_AST_DEPTH).is_some()
    {
        return warnings;
    }

    let mut collector = JsTsWarningCollector {
        source: &script.source,
        language: script.language,
        path: path.map(str::to_string),
        declared_vars: Vec::new(),
        function_depth: 0,
        expr_depth: 0,
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
    local: bool,
    ident_span: (usize, usize),
    is_exported: bool,
    has_type_annotation: bool,
    inferred_type: Option<&'static str>,
}

struct JsTsWarningCollector<'a> {
    function_depth: usize,
    expr_depth: usize,
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
                    quick_fixes: Vec::new(),
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
        self.function_depth += 1;
        for stmt in &body.statements {
            self.precollect_stmt_names(stmt);
        }
        self.check_statement_list(&body.statements);
        self.function_depth -= 1;
    }

    fn visit_var_decl(&mut self, var_decl: &VariableDeclaration<'a>, is_exported: bool) {
        for decl in &var_decl.declarations {
            if let Some(init) = &decl.init {
                self.visit_expression(init);
            }
            if let BindingPatternKind::BindingIdentifier(ident) = &decl.id.kind {
                let name = ident.name.as_str().to_string();
                self.known_names.insert(name.clone());
                let start = ident.span.start as usize;
                let ident_span = (start, (start + name.len()).min(self.source.len()));
                let inferred_type = decl.init.as_ref().and_then(infer_simple_literal_type);
                self.declared_vars.push(DeclaredVar {
                    local: self.function_depth > 0,
                    name,
                    ident_span,
                    is_exported,
                    has_type_annotation: decl.id.type_annotation.is_some(),
                    inferred_type,
                });
            }
        }
    }

    fn visit_expression(&mut self, expr: &Expression<'a>) {
        if self.expr_depth > crate::MAX_WEB_AST_DEPTH {
            return;
        }
        self.expr_depth += 1;
        self.visit_expression_inner(expr);
        self.expr_depth -= 1;
    }

    fn visit_expression_inner(&mut self, expr: &Expression<'a>) {
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
                    quick_fixes: if var.local
                        && !self.known_names.contains(&format!("_{}", var.name))
                    {
                        vec![QuickFix {
                            title: format!("Prefix unused variable '{}' with '_'", var.name),
                            replacement: format!("_{}", var.name),
                            span: var.ident_span,
                            line,
                            column,
                        }]
                    } else {
                        Vec::new()
                    },
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
    let tokens = crate::actionscript::diagnostics::word_tokens(script);

    // 1. Deprecated AS1/AS2 constructs (`tellTarget`, `ifFrameLoaded`, `eq`, `ne`, `and`, `or`, `not`, `add`)
    if matches!(
        script.language,
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
    ) {
        find_deprecated_as_constructs(src, script.language, &path_str, &tokens, &mut warnings);
    }

    // 2. Unused local variables and missing type annotations (`var x = ...`)
    find_actionscript_var_warnings(
        src,
        script.language,
        &path_str,
        &tokens,
        &crate::actionscript::diagnostics::local_variable_names(script),
        &mut warnings,
    );

    // 3. Unreachable statements after `return`/`throw`/`break`/`continue` in line-oriented blocks
    warnings.extend(crate::actionscript::diagnostics::unreachable_warnings(
        script, path,
    ));

    // 4. Misspelled identifiers against declared variables & built-in Flash globals
    find_actionscript_misspellings(src, script.language, &path_str, &tokens, &mut warnings);

    find_unused_imports(script, path, &tokens, &mut warnings);
    warnings.extend(
        crate::language_service::partial_api_uses(script)
            .into_iter()
            .map(|usage| CompilerWarning {
                code: "partial-api".into(),
                severity: DiagnosticSeverity::Warning,
                language: script.language,
                path: path.map(str::to_owned),
                line: usage.line,
                column: usage.column,
                span: usage.span,
                message: format!("{}.{}: {}", usage.owner, usage.member, usage.limitation),
                quick_fixes: Vec::new(),
            }),
    );

    // 5. `SwiftHost` native proxy contract validation (`SwiftHost.as` or `class SwiftHost`)
    let looks_like_swift_host = path.is_some_and(|p| {
        let norm = p.replace('\\', "/");
        norm == "SwiftHost.as" || norm.ends_with("/SwiftHost.as")
    }) || src.contains("class SwiftHost");
    if looks_like_swift_host {
        let unit = neoflash_core::SourceUnit {
            id: "swift-host-lint".into(),
            path: path.unwrap_or("SwiftHost.as").to_owned(),
            language: script.language,
            source: src.clone(),
        };
        if let Ok(parsed) = crate::convert::hir::parse_class_unit(&unit, &BTreeMap::new())
            && crate::convert::swift_contract::is_swift_host_class(path, &parsed)
        {
            let (_, host_warnings) = crate::convert::swift_contract::inspect_swift_host_class(
                &parsed,
                script.language,
                path,
            );
            warnings.extend(host_warnings);
        }
    }

    warnings
}

fn find_unused_imports(
    script: &Script,
    path: Option<&str>,
    tokens: &[(String, usize, usize)],
    warnings: &mut Vec<CompilerWarning>,
) {
    for (index, (word, start, _)) in tokens.iter().enumerate() {
        if word != "import" {
            continue;
        }
        let Some(mut last) = tokens.get(index + 1) else {
            continue;
        };
        for next in tokens.iter().skip(index + 2) {
            if script.source[last.2..next.1].trim() != "." {
                break;
            }
            last = next;
        }
        let suffix = &script.source[last.2..];
        if !suffix.trim_start().starts_with(';')
            || tokens.iter().filter(|(name, _, _)| name == &last.0).count() != 1
        {
            continue;
        }
        let end = last.2 + suffix.find(';').unwrap() + 1;
        let (line, column) = byte_offset_to_line_col(&script.source, *start);
        warnings.push(CompilerWarning {
            code: "unused-import".into(),
            severity: DiagnosticSeverity::Warning,
            language: script.language,
            path: path.map(str::to_owned),
            line,
            column,
            span: (*start, end),
            message: format!("Import '{}' is not used in this source unit.", last.0),
            quick_fixes: vec![QuickFix {
                title: format!("Remove unused import '{}'", last.0),
                replacement: String::new(),
                span: (*start, end),
                line,
                column,
            }],
        });
    }
}

fn find_deprecated_as_constructs(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    tokens: &[(String, usize, usize)],
    warnings: &mut Vec<CompilerWarning>,
) {
    for (idx, (word, start, end)) in tokens.iter().enumerate() {
        // Member names and declaration names are not legacy operators.
        if src[..*start].trim_end().ends_with('.')
            || idx > 0 && matches!(tokens[idx - 1].0.as_str(), "var" | "function" | "const")
        {
            continue;
        }
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
                    quick_fixes: vec![],
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
                    quick_fixes: vec![],
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
                    quick_fixes: if matches!(word.as_str(), "eq" | "ne" | "add") {
                        vec![]
                    } else {
                        vec![QuickFix {
                            title: format!("Replace deprecated '{word}' with '{modern}'"),
                            replacement: modern.to_string(),
                            span: (*start, *end),
                            line,
                            column,
                        }]
                    },
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
    tokens: &[(String, usize, usize)],
    local_names: &BTreeSet<String>,
    warnings: &mut Vec<CompilerWarning>,
) {
    let fold = |name: &str| {
        if language == ScriptLanguage::ActionScript1 {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }
    };
    let mut var_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (word, _, _) in tokens {
        *var_counts.entry(fold(word)).or_insert(0) += 1;
    }

    for (idx, (word, _, _)) in tokens.iter().enumerate() {
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
        if !var_name.starts_with('_') && var_counts.get(&fold(var_name)).copied().unwrap_or(0) == 1
        {
            let (line, column) = byte_offset_to_line_col(src, *name_start);
            warnings.push(CompilerWarning {
                code: "unused-variable".to_string(),
                severity: DiagnosticSeverity::Warning,
                language,
                path: path.clone(),
                line,
                column,
                span: (*name_start, *name_end),
                message: format!("Variable '{var_name}' is not read in this source unit."),
                quick_fixes: if local_names.contains(var_name)
                    && !var_counts.contains_key(&fold(&format!("_{var_name}")))
                {
                    vec![QuickFix {
                        title: format!("Prefix unused variable '{var_name}' with '_'"),
                        replacement: format!("_{var_name}"),
                        span: (*name_start, *name_end),
                        line,
                        column,
                    }]
                } else {
                    Vec::new()
                },
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
                    quick_fixes: Vec::new(),
                });
            }
        }
    }
}

fn find_actionscript_misspellings(
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    tokens: &[(String, usize, usize)],
    warnings: &mut Vec<CompilerWarning>,
) {
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
    if offset >= source.len() || !source.is_char_boundary(offset) {
        return (
            offset.min(source.len()),
            offset.min(source.len()),
            String::new(),
        );
    }
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$';
    let start = source[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_ident(*ch))
        .last()
        .map_or(offset, |(index, _)| index);
    let length = source[offset..]
        .chars()
        .take_while(|ch| is_ident(*ch))
        .map(char::len_utf8)
        .sum::<usize>();
    let end = offset + length;
    (start, end, source[start..end].to_owned())
}

fn find_identifier_near_line(
    source: &str,
    line: usize,
    column: usize,
    target_ident: &str,
) -> (usize, usize, String) {
    let offset = line_col_to_byte_offset(source, line, column);
    let found = identifier_at_offset(source, offset);
    if found.2 == target_ident {
        found
    } else {
        (offset, offset, String::new())
    }
}

fn line_byte_bounds(source: &str, line: usize) -> (usize, usize) {
    crate::logical_line_byte_bounds(source, line)
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

/// Live Xcode/Swift target guardrails emitted when `project.publish_settings().xcode_swift_target` is `true`.
fn check_xcode_swift_target_warnings(
    script: &Script,
    path: Option<&str>,
    is_class_unit: bool,
) -> Vec<CompilerWarning> {
    let mut out = Vec::new();
    let src = &script.source;
    if src.trim().is_empty() {
        return out;
    }
    let path_owned = path.map(str::to_owned);
    let mut reported_offsets = BTreeSet::new();

    // 1. Word-token checks for `ByteArray`, `Stage3D`/`stage3Ds`/`Context3D`, and `eval(...)`
    let tokens = scan_word_tokens(src);
    for (idx, (word, start, end)) in tokens.iter().enumerate() {
        let prev_is_decl = idx > 0
            && matches!(
                tokens[idx - 1].0.as_str(),
                "var" | "let" | "const" | "function"
            );
        let before_dot = src[..*start].trim_end().ends_with('.');

        match word.as_str() {
            "ByteArray" => {
                if reported_offsets.insert(*start) {
                    let (line, column) = byte_offset_to_line_col(src, *start);
                    out.push(CompilerWarning {
                        code: "xcode-swift-unsupported".into(),
                        severity: DiagnosticSeverity::Warning,
                        language: script.language,
                        path: path_owned.clone(),
                        line,
                        column,
                        span: (*start, *end),
                        message: "'ByteArray' is not supported on the native Xcode/Swift target. Pass portable data types (int, uint, Number, Boolean, String, Array, Object) through SwiftHost instead.".into(),
                        quick_fixes: Vec::new(),
                    });
                }
            }
            "Stage3D" | "Context3D" | "stage3Ds" => {
                if reported_offsets.insert(*start) {
                    let (line, column) = byte_offset_to_line_col(src, *start);
                    out.push(CompilerWarning {
                        code: "xcode-swift-unsupported".into(),
                        severity: DiagnosticSeverity::Warning,
                        language: script.language,
                        path: path_owned.clone(),
                        line,
                        column,
                        span: (*start, *end),
                        message: format!(
                            "'{word}' is not supported on the native Xcode/Swift 2D scene target."
                        ),
                        quick_fixes: Vec::new(),
                    });
                }
            }
            "eval"
                if !prev_is_decl
                    && !before_dot
                    && src[*end..].trim_start().starts_with('(')
                    && reported_offsets.insert(*start) =>
            {
                let (line, column) = byte_offset_to_line_col(src, *start);
                out.push(CompilerWarning {
                    code: "xcode-swift-unsupported".into(),
                    severity: DiagnosticSeverity::Warning,
                    language: script.language,
                    path: path_owned.clone(),
                    line,
                    column,
                    span: (*start, *end),
                    message: "Dynamic 'eval()' is not supported on the native Xcode/Swift ahead-of-time compiled target.".into(),
                    quick_fixes: Vec::new(),
                });
            }
            _ => {}
        }
    }

    // 2. AST walk for E4X XML operators (`XmlLiteral`, `XmlSelect` `..`/`@`, `XmlFilter` `.(...)`)
    if matches!(
        script.language,
        ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3
    ) {
        if is_class_unit {
            let unit = neoflash_core::SourceUnit {
                id: "xcode-lint".into(),
                path: path.unwrap_or("Class.as").to_owned(),
                language: script.language,
                source: src.clone(),
            };
            if let Ok(parsed_cls) = crate::convert::hir::parse_class_unit(&unit, &BTreeMap::new()) {
                for f in &parsed_cls.fields {
                    if let Some(val) = &f.value {
                        collect_e4x_warnings_in_expr(
                            val,
                            src,
                            script.language,
                            &path_owned,
                            &mut reported_offsets,
                            &mut out,
                        );
                    }
                }
                for m in &parsed_cls.methods {
                    collect_e4x_warnings_in_stmts(
                        &m.function.body,
                        src,
                        script.language,
                        &path_owned,
                        &mut reported_offsets,
                        &mut out,
                    );
                }
            }
        } else if let Ok(parsed_script) =
            crate::convert::hir::parse_timeline_script(script, "", &BTreeMap::new())
        {
            collect_e4x_warnings_in_stmts(
                &parsed_script.statements,
                src,
                script.language,
                &path_owned,
                &mut reported_offsets,
                &mut out,
            );
        }
    }

    out
}

fn collect_e4x_warnings_in_stmts(
    stmts: &[crate::actionscript::ast::Stmt],
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    reported_offsets: &mut BTreeSet<usize>,
    out: &mut Vec<CompilerWarning>,
) {
    for stmt in stmts {
        for expr in crate::actionscript::scope::expressions(stmt) {
            collect_e4x_warnings_in_expr(expr, src, language, path, reported_offsets, out);
        }
        for child in crate::actionscript::scope::statements(stmt) {
            collect_e4x_warnings_in_stmts(
                std::slice::from_ref(child),
                src,
                language,
                path,
                reported_offsets,
                out,
            );
        }
    }
}

fn collect_e4x_warnings_in_expr(
    expr: &crate::actionscript::ast::Expr,
    src: &str,
    language: ScriptLanguage,
    path: &Option<String>,
    reported_offsets: &mut BTreeSet<usize>,
    out: &mut Vec<CompilerWarning>,
) {
    use crate::actionscript::ast::Expr;
    match expr {
        Expr::XmlLiteral(raw, _) => {
            let start = src.find(raw.as_str()).unwrap_or(0);
            let end = (start + raw.len()).min(src.len());
            if reported_offsets.insert(start) {
                let (line, column) = byte_offset_to_line_col(src, start);
                out.push(CompilerWarning {
                    code: "xcode-swift-unsupported".into(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (start, end),
                    message: "E4X XML literals are not supported on the native Xcode/Swift target."
                        .into(),
                    quick_fixes: Vec::new(),
                });
            }
        }
        Expr::XmlSelect {
            local,
            attribute,
            descendant,
            ..
        }
        | Expr::XmlWithAttr {
            local,
            attribute,
            descendant,
            ..
        } => {
            let local_name = match local.as_ref() {
                Expr::Literal(s) => s.trim_matches('"').trim_matches('\'').to_owned(),
                Expr::Name(n) => n.clone(),
                _ => "*".into(),
            };
            let desc_sym = if *descendant { ".." } else { "." };
            let attr_sym = if *attribute { "@" } else { "" };
            let needle = format!("{desc_sym}{attr_sym}{local_name}");
            let start = src.find(&needle).unwrap_or(0);
            let end = (start + needle.len()).min(src.len());
            if reported_offsets.insert(start) {
                let (line, column) = byte_offset_to_line_col(src, start);
                out.push(CompilerWarning {
                    code: "xcode-swift-unsupported".into(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (start, end),
                    message: format!(
                        "E4X XML operator '{desc_sym}{attr_sym}{local_name}' is not supported on the native Xcode/Swift target."
                    ),
                    quick_fixes: Vec::new(),
                });
            }
        }
        Expr::XmlFilter(_, _, offset) => {
            let start = (*offset).min(src.len());
            let end = (start + 2).min(src.len());
            if reported_offsets.insert(start) {
                let (line, column) = byte_offset_to_line_col(src, start);
                out.push(CompilerWarning {
                    code: "xcode-swift-unsupported".into(),
                    severity: DiagnosticSeverity::Warning,
                    language,
                    path: path.clone(),
                    line,
                    column,
                    span: (start, end),
                    message:
                        "E4X XML filter operator '.(...)' is not supported on the native Xcode/Swift target."
                            .into(),
                    quick_fixes: Vec::new(),
                });
            }
        }
        _ => {}
    }
    for child in crate::actionscript::scope::children(expr) {
        collect_e4x_warnings_in_expr(child, src, language, path, reported_offsets, out);
    }
}
