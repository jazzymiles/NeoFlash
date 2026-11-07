//! Script compilation for the editor and HTML5 publisher.
//!
//! JavaScript and TypeScript support static type checking (`ts_check`), ES module imports/exports
//! and topological bundling (`modules`), JSX/TSX transformation (`__nfJsx`), structured compiler
//! warnings with quick fixes (`warnings`), and Source Map v3 + step debugging (`sourcemap`).
//! ActionScript uses a bounded, explicit frontend; see `docs/COMPATIBILITY.md`.

mod actionscript;
pub mod language_service;
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
    OriginalLocation, RuntimeSourceLocation, SourceMap, SourceMappingSegment,
    build_bundle_source_map, build_source_map, debug_runtime_js, decode_vlq, encode_vlq,
    instrument_script_for_debug,
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
    /// Heap-backed metadata keeps recursive parser Result values small. Serialization stays flat.
    #[serde(flatten)]
    pub details: Box<DiagnosticDetails>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticDetails {
    #[serde(default)]
    pub end_line: usize,
    #[serde(default)]
    pub end_column: usize,
    #[serde(default)]
    pub severity: DiagnosticSeverity,
    /// A stable lint identifier or a Flash-compatible diagnostic number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// UTF-8 byte offsets; `end` is exclusive. Positions above use Unicode characters.
    #[serde(default)]
    pub span: (usize, usize),
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<QuickFix>,
}

impl std::ops::Deref for Diagnostic {
    type Target = DiagnosticDetails;
    fn deref(&self) -> &Self::Target {
        &self.details
    }
}

impl std::ops::DerefMut for Diagnostic {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.details
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path
            && !self.message.starts_with(&format!("{path}: "))
            && !self.message.starts_with(&format!("#include {path}: "))
        {
            write!(f, "{path}: ")?;
        }
        if let Some(code) = &self.code
            && !self.message.contains(&format!("[{code}]"))
        {
            write!(f, "[{code}] ")?;
        }
        write!(
            f,
            "{:?} {}:{}: {}",
            self.language, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

impl Diagnostic {
    pub(crate) fn new(
        language: ScriptLanguage,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let code = if message.starts_with("Expected ") {
            Some("syntax".into())
        } else if message.contains("Unresolved or inaccessible import '") {
            Some("1172".into())
        } else if message.contains("Unknown base class '") {
            Some("1017".into())
        } else if message.contains("Cannot find name '") {
            Some("unresolved-name".into())
        } else if message.contains("is not assignable to type '") {
            Some("type-mismatch".into())
        } else if message.contains("unsupported") || message.contains("not supported") {
            Some("unsupported-feature".into())
        } else {
            None
        };
        Self {
            language,
            line,
            column,
            details: Box::new(DiagnosticDetails {
                end_line: line,
                end_column: column,
                severity: DiagnosticSeverity::Error,
                code,
                path: None,
                span: (0, 0),
                fixes: Vec::new(),
            }),
            message,
        }
    }

    pub(crate) fn at(script: &Script, offset: usize, message: impl Into<String>) -> Self {
        let (line, column) = byte_offset_to_line_col(&script.source, offset);
        Self::new(script.language, line, column, message).with_source(&script.source)
    }

    pub(crate) fn at_span(
        script: &Script,
        span: (usize, usize),
        message: impl Into<String>,
    ) -> Self {
        let mut result = Self::at(script, span.0, message);
        let end = span.1.min(script.source.len()).max(result.span.0);
        if script.source.is_char_boundary(end) {
            result.span.1 = end;
            (result.end_line, result.end_column) = byte_offset_to_line_col(&script.source, end);
        }
        result.fixes = warnings::quick_fixes_for_diagnostic(&result, &script.source);
        result
    }

    pub(crate) fn with_source(mut self, source: &str) -> Self {
        let start = line_col_to_byte_offset(source, self.line, self.column);
        let mut end = start;
        for ch in source[start..].chars() {
            if end > start && !(ch.is_alphanumeric() || ch == '_' || ch == '$') {
                break;
            }
            end += ch.len_utf8();
            if !(ch.is_alphanumeric() || ch == '_' || ch == '$') {
                break;
            }
        }
        self.span = (start, end);
        (self.end_line, self.end_column) = byte_offset_to_line_col(source, end);
        self.fixes = warnings::quick_fixes_for_diagnostic(&self, source);
        self
    }

    pub(crate) fn with_path(mut self, path: &str) -> Self {
        if !path.is_empty() {
            self.path = Some(path.to_owned());
        }
        self
    }

    /// Compute actionable quick-fix suggestions for this diagnostic against `source`.
    pub fn quick_fixes(&self, source: &str) -> Vec<QuickFix> {
        if self.fixes.is_empty() {
            warnings::quick_fixes_for_diagnostic(self, source)
        } else {
            self.fixes.clone()
        }
    }
}

#[derive(Clone, Copy)]
struct LogicalLineBounds {
    start: usize,
    end: usize,
    next_start: usize,
}

/// Scan logical lines, excluding their line endings from each returned range.
fn logical_lines(source: &str) -> impl Iterator<Item = LogicalLineBounds> + '_ {
    let mut chars = source.char_indices().peekable();
    let mut line_start = 0usize;
    let mut finished = false;

    std::iter::from_fn(move || {
        if finished {
            return None;
        }

        while let Some((offset, character)) = chars.next() {
            let mut next_start = match character {
                '\n' | '\r' => offset + 1,
                '\u{2028}' | '\u{2029}' => offset + character.len_utf8(),
                _ => continue,
            };
            if character == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
                chars.next();
                next_start += 1;
            }

            let bounds = LogicalLineBounds {
                start: line_start,
                end: offset,
                next_start,
            };
            line_start = next_start;
            return Some(bounds);
        }

        finished = true;
        Some(LogicalLineBounds {
            start: line_start,
            end: source.len(),
            next_start: source.len(),
        })
    })
}

/// Convert a 0-based byte offset in `source` to 1-based `(line, column)` Unicode character positions.
pub fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }

    for (line, bounds) in logical_lines(source).enumerate() {
        if offset <= bounds.end || offset < bounds.next_start {
            let logical_offset = offset.min(bounds.end);
            let column = source[bounds.start..logical_offset].chars().count() + 1;
            return (line + 1, column);
        }
    }

    (1, 1)
}

/// Return the byte range of a 1-based logical line, excluding its line ending.
///
/// Line zero addresses the first line, matching the coordinate conversion's
/// existing saturating behavior. Lines beyond the source address its end.
pub(crate) fn logical_line_byte_bounds(source: &str, line: usize) -> (usize, usize) {
    let target_line = line.saturating_sub(1);
    logical_lines(source)
        .nth(target_line)
        .map(|bounds| (bounds.start, bounds.end))
        .unwrap_or((source.len(), source.len()))
}

/// Convert a 1-based `(line, column)` position in `source` to a 0-based byte offset.
pub fn line_col_to_byte_offset(source: &str, line: usize, column: usize) -> usize {
    let (line_start, line_end) = logical_line_byte_bounds(source, line);
    let target_col = column.saturating_sub(1);
    let col_bytes: usize = source[line_start..line_end]
        .chars()
        .take(target_col)
        .map(char::len_utf8)
        .sum();
    line_start + col_bytes
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
    let mut bundle_maps: Vec<SourceMap> = as_program.source_maps.values().cloned().collect();
    if !web_graph.is_empty() {
        let insertion = format!("\n{}\nreturn __nfProgram;", web_graph.bundled_code);
        if let Some(idx) = code.rfind("return __nfProgram;") {
            if let Some(map) = &web_graph.bundle_source_map {
                bundle_maps.push(sourcemap::shift_map(
                    map,
                    code[..idx].bytes().filter(|c| *c == b'\n').count() + 1,
                    0,
                ));
            }
            code.replace_range(idx..idx + "return __nfProgram;".len(), &insertion);
        } else {
            code.push('\n');
            if let Some(map) = &web_graph.bundle_source_map {
                bundle_maps.push(sourcemap::shift_map(
                    map,
                    code.bytes().filter(|c| *c == b'\n').count(),
                    0,
                ));
            }
            code.push_str(&web_graph.bundled_code);
        }
    }

    let mut class_names = as_program.class_names.clone();
    for cls in &web_graph.exported_classes {
        if !class_names.contains(cls) {
            class_names.push(cls.clone());
        }
    }

    let mut source_maps = as_program.source_maps.clone();
    for id in &web_graph.order {
        if let Some(module) = web_graph.modules.get(id) {
            source_maps.insert(id.clone(), module.source_map.clone());
        }
    }
    let bundle_source_map =
        (!bundle_maps.is_empty()).then(|| sourcemap::combine_maps(&bundle_maps));

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

/// Compile and validate all project source units, timeline frame scripts, and element handlers,
/// rejecting any syntax/type errors or unsupported feature diagnostics.
pub fn check_project(project: &Project) -> Result<ProjectProgram, Vec<Diagnostic>> {
    let program = compile_project(project)?;
    let mut all_errors: Vec<Diagnostic> = Vec::new();

    for symbol in &project.symbols {
        for frame in &symbol.timeline.scripts {
            if frame.script.source.trim().is_empty() {
                continue;
            }
            let location = format!("{} / frame {}", symbol.name, frame.frame + 1);
            if let Err(errs) = compile_with_program(&frame.script, &program) {
                all_errors.extend(errs.into_iter().map(|e| e.with_path(&location)));
            }
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        if handler.script.source.trim().is_empty() {
                            continue;
                        }
                        let location = format!(
                            "{} / {} / frame {} / {} / {:?}",
                            symbol.name,
                            layer.name,
                            key.frame + 1,
                            element.name,
                            handler.event
                        );
                        if let Err(errs) = compile_with_program(&handler.script, &program) {
                            all_errors.extend(errs.into_iter().map(|e| e.with_path(&location)));
                        }
                    }
                }
            }
        }
    }

    for warning in warnings::check_project_warnings(project) {
        if warning.code == "unsupported-feature" || warning.severity == DiagnosticSeverity::Error {
            let mut diag = Diagnostic::new(
                warning.language,
                warning.line,
                warning.column,
                warning.message,
            );
            diag.code = Some(warning.code);
            diag.path = warning.path;
            all_errors.push(diag);
        }
    }

    if all_errors.is_empty() {
        Ok(program)
    } else {
        Err(all_errors)
    }
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
            actionscript::compile_legacy(&legacy).map_err(|error| {
                actionscript::diagnostics::collect_errors(&legacy, error)
                    .into_iter()
                    .map(|mut error| {
                        error.language = ScriptLanguage::ActionScript1;
                        error
                    })
                    .collect()
            })
        }
        ScriptLanguage::JavaScript => compile_web(script, false),
        ScriptLanguage::TypeScript => compile_web(script, true),
        ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
            actionscript::compile(script)
                .map_err(|e| actionscript::diagnostics::collect_errors(script, e))
        }
    }
}

/// Compile a script and generate a Source Map v3 (`version: 3`, VLQ mappings, and inline `sourceMappingURL`).
pub fn compile_with_source_map(
    script: &Script,
    source_name: &str,
) -> Result<CompiledWithSourceMap, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    let (raw_code, source_map) = match script.language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => compile_web_mapped(
            script,
            script.language == ScriptLanguage::TypeScript,
            source_name,
        )?,
        _ => {
            let prefix = sourcemap::marker_prefix(&script.source);
            let code = actionscript::compile_mapped(script, &prefix).map_err(|e| vec![e])?;
            let map = sourcemap::map_marked_source(source_name, &script.source, &code, &prefix);
            (code, map)
        }
    };
    Ok(CompiledWithSourceMap {
        language: script.language,
        source_name: source_name.to_owned(),
        code: format!("{raw_code}\n{}", source_map.inline_comment()),
        source_map,
    })
}

/// Compile a frame or object handler with the same project bindings as publication and exact
/// compiler-origin mappings. Generated runtime helpers deliberately remain unmapped.
pub fn compile_with_program_source_map(
    script: &Script,
    program: &ProjectProgram,
    source_name: &str,
) -> Result<CompiledWithSourceMap, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    let (code, source_map) = match script.language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            modules::compile_handler_with_modules_mapped(script, &program.web_modules, source_name)?
        }
        _ => {
            actionscript::project::compile_with_program_mapped(script, &program.inner, source_name)?
        }
    };
    Ok(CompiledWithSourceMap {
        language: script.language,
        source_name: source_name.into(),
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
    let compiled = compile_with_source_map(&instrumented_script, source_name)?;
    let compiled_body = compiled
        .code
        .rsplit_once("\n//# sourceMappingURL=")
        .map_or(compiled.code.as_str(), |(body, _)| body);
    let mut source_map = compiled.source_map;
    let segments = source_map
        .decode_segments()
        .into_iter()
        .map(|mut segment| {
            // Debug hooks add text on an authored line. Report the original statement start,
            // never a column in the inserted instrumentation.
            segment.original_column = script
                .source
                .lines()
                .nth(segment.original_line)
                .map(|line| {
                    line.chars()
                        .take_while(|c| c.is_whitespace())
                        .map(char::len_utf16)
                        .sum()
                })
                .unwrap_or(0);
            segment
        })
        .collect();
    source_map = SourceMap::from_segments(
        None,
        vec![source_name.into()],
        vec![script.source.clone()],
        source_map.names,
        segments,
    );
    source_map = sourcemap::shift_map(
        &source_map,
        sourcemap::debug_runtime_js()
            .bytes()
            .filter(|c| *c == b'\n')
            .count()
            + 1,
        0,
    );
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
            let span = error
                .labels
                .as_ref()
                .and_then(|labels| labels.first())
                .map_or((prefix_len, prefix_len), |label| {
                    (label.offset(), label.offset() + label.len())
                });
            Diagnostic::at_span(
                script,
                (
                    span.0.saturating_sub(prefix_len),
                    span.1.saturating_sub(prefix_len),
                ),
                error.to_string(),
            )
        })
        .collect()
}

fn compile_web(script: &Script, typescript: bool) -> Result<String, Vec<Diagnostic>> {
    compile_web_output(script, typescript, None).map(|(code, _)| code)
}
fn compile_web_mapped(
    script: &Script,
    typescript: bool,
    name: &str,
) -> Result<(String, SourceMap), Vec<Diagnostic>> {
    compile_web_output(script, typescript, Some(name))
        .map(|(code, map)| (code, map.expect("mapping requested")))
}
fn compile_web_output(
    script: &Script,
    typescript: bool,
    map_name: Option<&str>,
) -> Result<(String, Option<SourceMap>), Vec<Diagnostic>> {
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
        return Ok((
            script.source.clone(),
            map_name.map(|name| sourcemap::identity_source_map(name, &script.source)),
        ));
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
    let generated = Codegen::new()
        .with_options(oxc_codegen::CodegenOptions {
            source_map_path: map_name.map(std::path::PathBuf::from),
            ..oxc_codegen::CodegenOptions::default()
        })
        .build(&program);
    let code = generated.code;
    let preamble = if has_jsx || code.contains("__nfJsx") || code.contains("__nfFragment") {
        format!("{}\n", modules::jsx_inline_preamble_js())
    } else {
        String::new()
    };
    let map = generated.map.map(|map| {
        let map: SourceMap =
            serde_json::from_str(&map.to_json_string()).expect("Oxc source map v3");
        let line_count = script.source.lines().count();
        let segments = map
            .decode_segments()
            .into_iter()
            .filter_map(|mut s| {
                if s.original_line == 0 || s.original_line > line_count {
                    return None;
                }
                s.original_line -= 1;
                s.generated_line += preamble.bytes().filter(|c| *c == b'\n').count();
                Some(s)
            })
            .collect();
        SourceMap::from_segments(
            None,
            vec![map_name.unwrap().into()],
            vec![script.source.clone()],
            map.names,
            segments,
        )
    });
    Ok((
        format!("{preamble}{code}\nreturn __neoflash_handler__.call(this);"),
        map,
    ))
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

    #[test]
    fn line_coordinates_support_all_script_line_terminators() {
        for ending in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let source = format!("α{ending}β");
            let first_end = 'α'.len_utf8();
            let second_start = first_end + ending.len();

            assert_eq!(
                logical_line_byte_bounds(&source, 0),
                (0, first_end),
                "line zero with {ending:?}"
            );
            assert_eq!(
                logical_line_byte_bounds(&source, 1),
                (0, first_end),
                "first line with {ending:?}"
            );
            assert_eq!(
                logical_line_byte_bounds(&source, 2),
                (second_start, source.len()),
                "second line with {ending:?}"
            );
            assert_eq!(
                logical_line_byte_bounds(&source, usize::MAX),
                (source.len(), source.len()),
                "past final line with {ending:?}"
            );

            assert_eq!(line_col_to_byte_offset(&source, 0, 0), 0);
            assert_eq!(line_col_to_byte_offset(&source, 1, usize::MAX), first_end);
            assert_eq!(
                line_col_to_byte_offset(&source, 2, 1),
                second_start,
                "second line start with {ending:?}"
            );
            assert_eq!(
                line_col_to_byte_offset(&source, usize::MAX, 1),
                source.len(),
                "past final line with {ending:?}"
            );

            for offset in first_end..second_start {
                assert_eq!(
                    byte_offset_to_line_col(&source, offset),
                    (1, 2),
                    "offset {offset} inside {ending:?}"
                );
            }
            assert_eq!(
                byte_offset_to_line_col(&source, second_start),
                (2, 1),
                "offset after {ending:?}"
            );
            assert_eq!(
                byte_offset_to_line_col(&source, usize::MAX),
                (2, 2),
                "past source end with {ending:?}"
            );

            for (line, column) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
                let offset = line_col_to_byte_offset(&source, line, column);
                assert_eq!(
                    byte_offset_to_line_col(&source, offset),
                    (line, column),
                    "round trip at {line}:{column} with {ending:?}"
                );
            }
        }
    }

    #[test]
    fn final_empty_line_and_empty_source_have_stable_bounds() {
        assert_eq!(logical_line_byte_bounds("", 1), (0, 0));
        assert_eq!(line_col_to_byte_offset("", 1, usize::MAX), 0);

        for ending in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let source = format!("a{ending}");
            assert_eq!(logical_line_byte_bounds(&source, 1), (0, 1));
            assert_eq!(
                logical_line_byte_bounds(&source, 2),
                (source.len(), source.len())
            );
            assert_eq!(line_col_to_byte_offset(&source, 1, usize::MAX), 1);
            assert_eq!(
                line_col_to_byte_offset(&source, 2, usize::MAX),
                source.len()
            );
            assert_eq!(
                byte_offset_to_line_col(&source, source.len()),
                (2, 1),
                "final empty line after {ending:?}"
            );
        }
    }

    #[test]
    fn out_of_range_diagnostic_quick_fix_stays_on_its_logical_line() {
        for ending in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let source = format!("keep{ending}fo{ending}foo");
            let diagnostic = Diagnostic::new(
                ScriptLanguage::TypeScript,
                2,
                usize::MAX,
                "Cannot find name 'fo'. Did you mean 'foo'?",
            );
            let fixes = diagnostic.quick_fixes(&source);
            let fix = fixes
                .iter()
                .find(|fix| fix.replacement == "foo")
                .expect("spelling correction");
            let (line_start, line_end) = logical_line_byte_bounds(&source, 2);
            assert_eq!(fix.span, (line_start, line_start + 2));
            assert!(fix.span.1 <= line_end);
            assert_eq!(
                warnings::apply_quick_fix(&source, fix),
                source.replacen("fo", "foo", 1),
                "quick fix on line after {ending:?}"
            );
        }
    }
}
