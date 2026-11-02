//! Script compilation for the editor and HTML5 publisher.
//!
//! JavaScript and TypeScript support static type checking (`ts_check`), ES module imports/exports
//! and topological bundling (`modules`), JSX/TSX transformation (`__nfJsx`), structured compiler
//! warnings with quick fixes (`warnings`), and Source Map v3 + step debugging (`sourcemap`).
//! ActionScript uses a bounded, explicit frontend; see `docs/COMPATIBILITY.md`.

mod actionscript;
pub mod modules;
pub mod sourcemap;
pub mod ts_check;
pub mod warnings;

pub use modules::{
    CompiledModule, CompiledModuleGraph, ModuleImport, compile_web_modules, jsx_inline_preamble_js,
    normalize_module_path,
};
pub use sourcemap::{
    CompiledDebugScript, CompiledWithSourceMap, DebuggerSession, MappedStackFrame,
    OriginalLocation, SourceMap, SourceMappingSegment, build_bundle_source_map, build_source_map,
    debug_runtime_js, decode_vlq, encode_vlq, instrument_script_for_debug,
};
pub use ts_check::{
    ModuleTypeExports, TsCheckOutput, TsFunctionSig, TsPropertySig, TsType, TypeChecker,
    check_typescript_program, levenshtein_distance, suggest_identifier,
};
pub use warnings::{
    CompilerWarning, DiagnosticSeverity, QuickFix, apply_quick_fix, check_project_warnings,
    check_warnings, check_warnings_with_path, quick_fixes_for_diagnostic,
};

use neoflash_core::{Project, Script, ScriptLanguage, SourceUnit};
use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Statement};
use oxc_codegen::Codegen;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, path::Path};

/// Alias for `Diagnostic` used across script tooling.
pub type ScriptDiagnostic = Diagnostic;

/// Line and column are one-based Unicode character positions in the user's source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub language: ScriptLanguage,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} {}:{}: {}",
            self.language, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

impl Diagnostic {
    pub(crate) fn at(script: &Script, offset: usize, message: impl Into<String>) -> Self {
        let (line, column) = byte_offset_to_line_col(&script.source, offset);
        Self {
            language: script.language,
            line,
            column,
            message: message.into(),
        }
    }

    /// Compute actionable quick-fix suggestions for this diagnostic against `source`.
    pub fn quick_fixes(&self, source: &str) -> Vec<QuickFix> {
        warnings::quick_fixes_for_diagnostic(self, source)
    }
}

/// Convert a 0-based byte offset in `source` to 1-based `(line, column)` Unicode character positions.
pub fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &source[..offset];
    let line = prefix.chars().filter(|c| *c == '\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

/// Convert a 1-based `(line, column)` position in `source` to a 0-based byte offset.
pub fn line_col_to_byte_offset(source: &str, line: usize, column: usize) -> usize {
    let target_line = line.saturating_sub(1);
    let target_col = column.saturating_sub(1);
    let mut byte_offset = 0usize;
    for (idx, l) in source.split_inclusive('\n').enumerate() {
        if idx == target_line {
            let col_bytes: usize = l.chars().take(target_col).map(char::len_utf8).sum();
            return (byte_offset + col_bytes).min(source.len());
        }
        byte_offset += l.len();
    }
    source.len()
}

/// Compiled project program combining ActionScript classes and JavaScript/TypeScript modules.
pub struct ProjectProgram {
    pub(crate) inner: actionscript::project::ProjectProgram,
    pub code: String,
    pub class_names: Vec<String>,
    pub module_names: Vec<String>,
    pub web_modules: CompiledModuleGraph,
    pub source_maps: BTreeMap<String, SourceMap>,
    pub bundle_source_map: Option<SourceMap>,
}

impl fmt::Debug for ProjectProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectProgram")
            .field("class_names", &self.class_names)
            .field("module_names", &self.module_names)
            .finish()
    }
}

impl PartialEq for ProjectProgram {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.class_names == other.class_names
            && self.module_names == other.module_names
    }
}
impl Eq for ProjectProgram {}

impl PartialEq for CompiledModuleGraph {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order && self.bundled_code == other.bundled_code
    }
}
impl Eq for CompiledModuleGraph {}

/// Compile all project source units (AS2/AS3 classes and JS/TS/JSX/TSX modules) into a `ProjectProgram`.
pub fn compile_project(project: &Project) -> Result<ProjectProgram, Vec<Diagnostic>> {
    let mut web_units: Vec<&SourceUnit> = Vec::new();
    let mut as_units: Vec<SourceUnit> = Vec::new();

    for unit in &project.sources {
        match unit.language {
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => web_units.push(unit),
            _ => as_units.push(unit.clone()),
        }
    }

    let mut all_errors: Vec<Diagnostic> = Vec::new();
    let web_graph = match modules::compile_web_modules(&web_units) {
        Ok(graph) => graph,
        Err(errs) => {
            all_errors.extend(errs);
            CompiledModuleGraph::default()
        }
    };

    let mut as_project = project.clone();
    as_project.sources = as_units;

    // If document_class or a symbol's class_name is satisfied by an exported JS/TS class,
    // don't let the ActionScript linker reject it as an unknown AS class.
    if !web_graph.exported_classes.is_empty() {
        if let Some(doc_cls) = &as_project.document_class
            && web_graph.exported_classes.iter().any(|c| c == doc_cls)
        {
            as_project.document_class = None;
        }
        for sym in &mut as_project.symbols {
            if let Some(cls) = &sym.class_name
                && web_graph.exported_classes.iter().any(|c| c == cls)
            {
                sym.class_name = None;
            }
        }
    }

    let as_program = match actionscript::project::compile_project(&as_project) {
        Ok(prog) => prog,
        Err(errs) => {
            all_errors.extend(errs);
            return Err(all_errors);
        }
    };

    if !all_errors.is_empty() {
        return Err(all_errors);
    }

    let mut code = as_program.code.clone();
    if !web_graph.is_empty() {
        let insertion = format!("\n{}\nreturn __nfProgram;", web_graph.bundled_code);
        if let Some(idx) = code.rfind("return __nfProgram;") {
            code.replace_range(idx..idx + "return __nfProgram;".len(), &insertion);
        } else {
            code.push('\n');
            code.push_str(&web_graph.bundled_code);
        }
    }

    let mut class_names = as_program.class_names.clone();
    for cls in &web_graph.exported_classes {
        if !class_names.contains(cls) {
            class_names.push(cls.clone());
        }
    }

    let mut source_maps = BTreeMap::new();
    let mut bundle_chunks: Vec<(String, String, String)> = Vec::new();
    for id in &web_graph.order {
        if let Some(m) = web_graph.modules.get(id) {
            source_maps.insert(id.clone(), m.source_map.clone());
            bundle_chunks.push((
                id.clone(),
                m.original_source.clone(),
                m.compiled_body.clone(),
            ));
        }
    }
    let bundle_source_map = if !bundle_chunks.is_empty() {
        Some(sourcemap::build_bundle_source_map(
            "bundle.js",
            &bundle_chunks,
            &code,
        ))
    } else {
        None
    };

    Ok(ProjectProgram {
        inner: as_program,
        code,
        class_names,
        module_names: web_graph.order.clone(),
        web_modules: web_graph,
        source_maps,
        bundle_source_map,
    })
}

/// Compile a frame or object handler against a compiled `ProjectProgram`.
pub fn compile_with_program(
    script: &Script,
    program: &ProjectProgram,
) -> Result<String, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    match script.language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            modules::compile_handler_with_modules(script, &program.web_modules)
        }
        _ => actionscript::project::compile_with_program(script, &program.inner),
    }
}

/// Compile one frame/object handler into a JavaScript function body.
///
/// The browser calls this body with `this` bound to the current display object.
/// A failed script is never replaced by an empty handler: publication must stop.
pub fn compile(script: &Script) -> Result<String, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    match script.language {
        ScriptLanguage::ActionScript1 => {
            let legacy = Script {
                language: ScriptLanguage::ActionScript2,
                source: script.source.clone(),
            };
            actionscript::compile_legacy(&legacy).map_err(|mut error| {
                error.language = ScriptLanguage::ActionScript1;
                vec![error]
            })
        }
        ScriptLanguage::JavaScript => compile_web(script, false),
        ScriptLanguage::TypeScript => compile_web(script, true),
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
            actionscript::compile(script).map_err(|e| vec![e])
        }
    }
}

/// Compile a script and generate a Source Map v3 (`version: 3`, VLQ mappings, and inline `sourceMappingURL`).
pub fn compile_with_source_map(
    script: &Script,
    source_name: &str,
) -> Result<CompiledWithSourceMap, Vec<Diagnostic>> {
    let raw_code = compile(script)?;
    let source_map = sourcemap::build_source_map(source_name, &script.source, &raw_code);
    let code = format!("{raw_code}\n{}", source_map.inline_comment());
    Ok(CompiledWithSourceMap {
        language: script.language,
        source_name: source_name.to_string(),
        code,
        source_map,
    })
}

/// Compile a script with both Source Map v3 and `__nfDebugStep(source, line, locals)` instrumentation.
pub fn compile_debug(
    script: &Script,
    source_name: &str,
) -> Result<CompiledDebugScript, Vec<Diagnostic>> {
    let (instrumented_script, breakable_lines, locals_by_line) =
        sourcemap::instrument_script_for_debug(script, source_name);
    let compiled_body = compile(&instrumented_script)?;
    let source_map = sourcemap::build_source_map(source_name, &script.source, &compiled_body);
    let code = format!(
        "{}\n{}\n{}",
        sourcemap::debug_runtime_js(),
        compiled_body,
        source_map.inline_comment()
    );
    Ok(CompiledDebugScript {
        language: script.language,
        source_name: source_name.to_string(),
        code,
        source_map,
        breakable_lines,
        locals_by_line,
    })
}

fn web_errors(script: &Script, prefix_len: usize, errors: Vec<OxcDiagnostic>) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|error| {
            let offset = error
                .labels
                .as_ref()
                .and_then(|labels| labels.first())
                .map_or(prefix_len, |label| label.offset());
            Diagnostic::at(script, offset.saturating_sub(prefix_len), error.to_string())
        })
        .collect()
}

fn compile_web(script: &Script, typescript: bool) -> Result<String, Vec<Diagnostic>> {
    // Match the runtime's actual function-body context: `return` is legal,
    // module imports/exports and top-level await are not in standalone handlers.
    const PREFIX: &str = "function __neoflash_handler__(){\"use strict\";\n";
    let wrapped = format!("{PREFIX}{}\n}}", script.source);
    let allocator = Allocator::default();
    let source_type = if typescript {
        SourceType::tsx().with_module(false)
    } else {
        SourceType::cjs().with_jsx(true)
    };
    let parsed = Parser::new(&allocator, &wrapped, source_type).parse();
    if !parsed.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), parsed.errors));
    }
    if parsed.program.body.len() != 1 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "The script must be a single function body; an unmatched closing brace escapes the handler.",
        )]);
    }
    let mut program = parsed.program;
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .with_excess_capacity(2.0)
        .build(&program);
    if !semantic.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), semantic.errors));
    }

    let has_jsx = program_contains_jsx(&program);
    if !typescript && !has_jsx {
        return Ok(script.source.clone());
    }

    if typescript {
        let mut checker = TypeChecker::new(script, PREFIX.len());
        checker.check_program(&program);
        if !checker.diagnostics.is_empty() {
            return Err(checker.diagnostics);
        }
    }

    let options = TransformOptions {
        jsx: JsxOptions {
            runtime: JsxRuntime::Classic,
            pragma: Some("__nfJsx".to_string()),
            pragma_frag: Some("__nfFragment".to_string()),
            ..JsxOptions::default()
        },
        ..TransformOptions::default()
    };
    let path_name = if typescript {
        "handler.tsx"
    } else {
        "handler.jsx"
    };
    let transformed = Transformer::new(&allocator, Path::new(path_name), &options)
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if !transformed.errors.is_empty() {
        return Err(web_errors(script, PREFIX.len(), transformed.errors));
    }
    #[allow(deprecated)]
    if !transformed.helpers_used.is_empty() {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "This TypeScript construct requires runtime helpers not yet bundled by NeoFlash. Use equivalent plain JavaScript.",
        )]);
    }
    let code = Codegen::new().build(&program).code;
    if has_jsx || code.contains("__nfJsx") || code.contains("__nfFragment") {
        Ok(format!(
            "{}\n{code}\nreturn __neoflash_handler__.call(this);",
            modules::jsx_inline_preamble_js()
        ))
    } else {
        Ok(format!("{code}\nreturn __neoflash_handler__.call(this);"))
    }
}

fn program_contains_jsx(program: &oxc_ast::ast::Program<'_>) -> bool {
    program.body.iter().any(stmt_contains_jsx)
}

fn stmt_contains_jsx(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::FunctionDeclaration(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.statements.iter().any(stmt_contains_jsx)),
        Statement::BlockStatement(b) => b.body.iter().any(stmt_contains_jsx),
        Statement::VariableDeclaration(v) => v
            .declarations
            .iter()
            .any(|d| d.init.as_ref().is_some_and(expr_contains_jsx)),
        Statement::ExpressionStatement(e) => expr_contains_jsx(&e.expression),
        Statement::ReturnStatement(r) => r.argument.as_ref().is_some_and(expr_contains_jsx),
        Statement::IfStatement(i) => {
            expr_contains_jsx(&i.test)
                || stmt_contains_jsx(&i.consequent)
                || i.alternate.as_ref().is_some_and(|a| stmt_contains_jsx(a))
        }
        _ => false,
    }
}

fn expr_contains_jsx(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ParenthesizedExpression(p) => expr_contains_jsx(&p.expression),
        Expression::AssignmentExpression(a) => expr_contains_jsx(&a.right),
        Expression::CallExpression(c) => {
            expr_contains_jsx(&c.callee)
                || c.arguments
                    .iter()
                    .any(|arg| arg.as_expression().is_some_and(expr_contains_jsx))
        }
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .any(|el| el.as_expression().is_some_and(expr_contains_jsx)),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|p| match p {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(prop) => {
                expr_contains_jsx(&prop.value)
            }
            _ => false,
        }),
        Expression::ConditionalExpression(c) => {
            expr_contains_jsx(&c.test)
                || expr_contains_jsx(&c.consequent)
                || expr_contains_jsx(&c.alternate)
        }
        Expression::ArrowFunctionExpression(a) => a.body.statements.iter().any(stmt_contains_jsx),
        Expression::FunctionExpression(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.statements.iter().any(stmt_contains_jsx)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(language: ScriptLanguage, source: &str) -> Script {
        Script {
            language,
            source: source.into(),
        }
    }

    #[test]
    fn javascript_preserves_source_and_allows_function_body_return() {
        let source = "if (this.x > 20) return;\nthis.x += 2;";
        assert_eq!(
            compile(&script(ScriptLanguage::JavaScript, source)).unwrap(),
            source
        );
    }

    #[test]
    fn syntax_error_has_original_source_position() {
        let diagnostics =
            compile(&script(ScriptLanguage::JavaScript, "this.play();\nlet = ;")).unwrap_err();
        assert_eq!(diagnostics[0].line, 2);
        assert!(diagnostics[0].column > 1);
    }

    #[test]
    fn javascript_rejects_module_syntax_in_handlers() {
        assert!(compile(&script(ScriptLanguage::JavaScript, "import x from 'x';")).is_err());
        assert!(compile(&script(ScriptLanguage::JavaScript, "break;")).is_err());
        assert!(
            compile(&script(
                ScriptLanguage::JavaScript,
                "with (this) { x = 1; }"
            ))
            .is_err()
        );
        assert!(compile(&script(ScriptLanguage::JavaScript, "let x = 010;")).is_err());
        assert!(
            compile(&script(
                ScriptLanguage::JavaScript,
                "} const escaped = 1; function x(){"
            ))
            .is_err()
        );
    }

    #[test]
    fn typescript_is_transformed_with_real_parser() {
        let output = compile(&script(
            ScriptLanguage::TypeScript,
            "interface Point { x: number; }\nconst p: Point = { x: 4 }; this.x = p.x;",
        ))
        .unwrap();
        assert!(!output.contains("interface Point"));
        assert!(!output.contains(": Point"));
        assert!(output.contains("this.x = p.x"));
        assert!(output.contains(".call(this)"));
    }

    #[test]
    fn diagnostic_counts_unicode_characters() {
        let input = script(ScriptLanguage::ActionScript3, "// привет\n  α + @");
        let error = Diagnostic::at(&input, input.source.find('@').unwrap(), "unsupported");
        assert_eq!((error.line, error.column), (2, 7));
    }
}
