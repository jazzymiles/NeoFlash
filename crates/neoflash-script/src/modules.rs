//! JavaScript & TypeScript (`.js`, `.jsx`, `.ts`, `.tsx`) module resolution,
//! topological dependency bundling, cross-module static type checking, and JSX runtime support.

use crate::sourcemap::SourceMap;
use crate::ts_check::{ModuleTypeExports, TsType, check_typescript_program, suggest_identifier};
use crate::{Diagnostic, byte_offset_to_line_col};
use neoflash_core::{Script, ScriptLanguage, SourceUnit};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPatternKind, Declaration, ExportDefaultDeclarationKind, ModuleExportName, Statement,
};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A single import edge extracted from a JS/TS module or handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleImport {
    pub specifier: String,
    pub resolved_path: Option<String>,
    pub line: usize,
    pub column: usize,
    pub is_type_only: bool,
    pub named_imports: Vec<(String, String, bool, usize, usize)>, // (imported, local, is_type, line, col)
    pub default_import: Option<(String, usize, usize)>,           // (local, line, col)
    pub namespace_import: Option<(String, usize, usize)>,         // (local, line, col)
}

/// Metadata and compiled output for a single JS/TS project module.
#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub id: String,
    pub path: String,
    pub language: ScriptLanguage,
    pub original_source: String,
    pub compiled_body: String,
    pub imports: Vec<ModuleImport>,
    pub exported_values: BTreeSet<String>,
    pub exported_types: BTreeSet<String>,
    pub exported_classes: BTreeSet<String>,
    pub has_default_export: bool,
    pub has_star_reexport: bool,
    pub star_reexport_targets: Vec<String>,
    pub type_exports: ModuleTypeExports,
    pub source_map: SourceMap,
}

/// Complete compiled module graph for a project's JS/TS `SourceUnit`s.
#[derive(Debug, Clone, Default)]
pub struct CompiledModuleGraph {
    /// Modules in topological evaluation order.
    pub order: Vec<String>,
    /// Map from canonical module ID (e.g., `"math.ts"` or `"src/utils.js"`) to compiled module.
    pub modules: BTreeMap<String, CompiledModule>,
    /// Lookup table from candidate path/alias to canonical module ID.
    pub path_lookup: BTreeMap<String, String>,
    /// Bundled JavaScript registration and runtime helper code.
    pub bundled_code: String,
    pub bundle_source_map: Option<SourceMap>,
    /// Exported class names across all JS/TS modules.
    pub exported_classes: Vec<String>,
}

impl CompiledModuleGraph {
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Resolve a module specifier imported from `importer_path` (or `""` for timeline/object handlers).
    pub fn resolve_specifier(&self, importer_path: &str, specifier: &str) -> Option<String> {
        resolve_in_lookup(&self.path_lookup, importer_path, specifier)
    }

    /// Build a map of `ModuleTypeExports` keyed by both canonical module ID and any specifier alias.
    pub fn type_exports_for_importer(
        &self,
        importer_path: &str,
        imports: &[ModuleImport],
    ) -> BTreeMap<String, ModuleTypeExports> {
        let mut map = BTreeMap::new();
        for (id, module) in &self.modules {
            map.insert(id.clone(), module.type_exports.clone());
        }
        for imp in imports {
            if let Some(resolved) = &imp.resolved_path
                && let Some(target) = self.modules.get(resolved)
            {
                map.insert(imp.specifier.clone(), target.type_exports.clone());
            }
        }
        let _ = importer_path;
        map
    }
}

/// Normalize a project source path into a canonical forward-slash relative path without leading `./`.
pub fn normalize_module_path(raw: &str) -> String {
    let replaced = raw.trim().replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for segment in replaced.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn strip_known_extension(path: &str) -> &str {
    for ext in &[".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs"] {
        if let Some(stripped) = path.strip_suffix(ext) {
            return stripped;
        }
    }
    path
}

fn build_path_lookup(units: &[&SourceUnit]) -> BTreeMap<String, String> {
    let mut lookup = BTreeMap::new();
    for unit in units {
        let canonical = normalize_module_path(&unit.path);
        if canonical.is_empty() {
            continue;
        }
        lookup.insert(canonical.clone(), canonical.clone());
        let stem = strip_known_extension(&canonical).to_string();
        lookup
            .entry(stem.clone())
            .or_insert_with(|| canonical.clone());
        if let Some(dir_prefix) = stem.strip_suffix("/index") {
            lookup
                .entry(dir_prefix.to_string())
                .or_insert_with(|| canonical.clone());
        } else if stem == "index" {
            lookup
                .entry(String::new())
                .or_insert_with(|| canonical.clone());
        }
    }
    lookup
}

fn resolve_in_lookup(
    lookup: &BTreeMap<String, String>,
    importer_path: &str,
    specifier: &str,
) -> Option<String> {
    let spec = specifier.trim().replace('\\', "/");
    let candidate = if spec.starts_with("./") || spec.starts_with("../") {
        let importer_norm = normalize_module_path(importer_path);
        let dir = match importer_norm.rfind('/') {
            Some(idx) => &importer_norm[..idx],
            None => "",
        };
        if dir.is_empty() {
            normalize_module_path(&spec)
        } else {
            normalize_module_path(&format!("{dir}/{spec}"))
        }
    } else if let Some(stripped) = spec.strip_prefix('/') {
        normalize_module_path(stripped)
    } else {
        normalize_module_path(&spec)
    };

    if let Some(found) = lookup.get(&candidate) {
        return Some(found.clone());
    }
    let stem = strip_known_extension(&candidate);
    if let Some(found) = lookup.get(stem) {
        return Some(found.clone());
    }
    for ext in &[
        ".ts",
        ".tsx",
        ".js",
        ".jsx",
        "/index.ts",
        "/index.tsx",
        "/index.js",
        "/index.jsx",
    ] {
        let with_ext = format!("{stem}{ext}");
        if let Some(found) = lookup.get(&with_ext) {
            return Some(found.clone());
        }
    }
    None
}

/// Determine the Oxc `SourceType` for a JS or TS script/module.
pub fn source_type_for(
    language: ScriptLanguage,
    path_hint: Option<&str>,
    is_module: bool,
) -> SourceType {
    let ext = path_hint
        .and_then(|p| Path::new(p).extension())
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match language {
        ScriptLanguage::TypeScript => {
            let st = if ext.eq_ignore_ascii_case("ts") {
                // Enable TSX everywhere so `<div />` or `<MovieClip />` works in frame/object handlers and `.ts`/`.tsx`
                SourceType::tsx()
            } else {
                SourceType::tsx()
            };
            st.with_module(is_module)
        }
        ScriptLanguage::JavaScript => {
            let _ = ext;
            SourceType::jsx().with_module(is_module)
        }
        _ => SourceType::mjs(),
    }
}

struct ParsedModuleHeader {
    imports: Vec<ModuleImport>,
    exported_values: BTreeSet<String>,
    exported_types: BTreeSet<String>,
    exported_classes: BTreeSet<String>,
    has_default_export: bool,
    has_star_reexport: bool,
    star_reexport_specifiers: Vec<(String, usize, usize)>,
}

fn collect_binding_names(pattern: &BindingPatternKind<'_>, out: &mut BTreeSet<String>) {
    match pattern {
        BindingPatternKind::BindingIdentifier(ident) => {
            out.insert(ident.name.as_str().to_string());
        }
        BindingPatternKind::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_names(&prop.value.kind, out);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_names(&rest.argument.kind, out);
            }
        }
        BindingPatternKind::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_names(&elem.kind, out);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_names(&rest.argument.kind, out);
            }
        }
        BindingPatternKind::AssignmentPattern(assign) => {
            collect_binding_names(&assign.left.kind, out);
        }
    }
}

fn module_export_name_str(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.as_str().to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.as_str().to_string(),
        ModuleExportName::StringLiteral(lit) => lit.value.as_str().to_string(),
    }
}

fn extract_module_header(
    source: &str,
    language: ScriptLanguage,
    path: &str,
    lookup: &BTreeMap<String, String>,
) -> Result<ParsedModuleHeader, Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let source_type = source_type_for(language, Some(path), true);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| {
                let offset = error
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                let (line, column) = byte_offset_to_line_col(source, offset);
                Diagnostic::new(language, line, column, format!("{path}: {error}"))
                    .with_source(source)
                    .with_path(path)
            })
            .collect());
    }

    let mut imports = Vec::new();
    let mut exported_values = BTreeSet::new();
    let mut exported_types = BTreeSet::new();
    let mut exported_classes = BTreeSet::new();
    let mut has_default_export = false;
    let mut has_star_reexport = false;
    let mut star_reexport_specifiers = Vec::new();

    for stmt in &parsed.program.body {
        match stmt {
            Statement::ImportDeclaration(imp) => {
                let spec_str = imp.source.value.as_str().to_string();
                let (line, column) =
                    byte_offset_to_line_col(source, imp.source.span.start as usize);
                let resolved_path = resolve_in_lookup(lookup, path, &spec_str);
                let is_type_only = imp.import_kind.is_type();
                let mut named_imports = Vec::new();
                let mut default_import = None;
                let mut namespace_import = None;

                if let Some(specs) = &imp.specifiers {
                    for spec in specs {
                        match spec {
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                let (sl, sc) =
                                    byte_offset_to_line_col(source, s.span.start as usize);
                                named_imports.push((
                                    module_export_name_str(&s.imported),
                                    s.local.name.as_str().to_string(),
                                    is_type_only || s.import_kind.is_type(),
                                    sl,
                                    sc,
                                ));
                            }
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                let (sl, sc) =
                                    byte_offset_to_line_col(source, s.span.start as usize);
                                default_import = Some((s.local.name.as_str().to_string(), sl, sc));
                            }
                            oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                                s,
                            ) => {
                                let (sl, sc) =
                                    byte_offset_to_line_col(source, s.span.start as usize);
                                namespace_import =
                                    Some((s.local.name.as_str().to_string(), sl, sc));
                            }
                        }
                    }
                }

                imports.push(ModuleImport {
                    specifier: spec_str,
                    resolved_path,
                    line,
                    column,
                    is_type_only,
                    named_imports,
                    default_import,
                    namespace_import,
                });
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(src) = &exp.source {
                    let spec_str = src.value.as_str().to_string();
                    let (line, column) = byte_offset_to_line_col(source, src.span.start as usize);
                    let resolved_path = resolve_in_lookup(lookup, path, &spec_str);
                    let is_type_only = exp.export_kind.is_type();
                    let mut named_imports = Vec::new();
                    for s in &exp.specifiers {
                        let exported = module_export_name_str(&s.exported);
                        let local = module_export_name_str(&s.local);
                        let (sl, sc) = byte_offset_to_line_col(source, s.span.start as usize);
                        let spec_is_type = is_type_only || s.export_kind.is_type();
                        if spec_is_type {
                            exported_types.insert(exported);
                        } else {
                            exported_values.insert(exported);
                        }
                        named_imports.push((local.clone(), local, spec_is_type, sl, sc));
                    }
                    imports.push(ModuleImport {
                        specifier: spec_str,
                        resolved_path,
                        line,
                        column,
                        is_type_only,
                        named_imports,
                        default_import: None,
                        namespace_import: None,
                    });
                } else if let Some(decl) = &exp.declaration {
                    match decl {
                        Declaration::VariableDeclaration(var_decl) => {
                            for d in &var_decl.declarations {
                                collect_binding_names(&d.id.kind, &mut exported_values);
                            }
                        }
                        Declaration::FunctionDeclaration(fn_decl) => {
                            if let Some(id) = &fn_decl.id {
                                exported_values.insert(id.name.as_str().to_string());
                            }
                        }
                        Declaration::ClassDeclaration(cls_decl) => {
                            if let Some(id) = &cls_decl.id {
                                let name = id.name.as_str().to_string();
                                exported_values.insert(name.clone());
                                exported_types.insert(name.clone());
                                exported_classes.insert(name);
                            }
                        }
                        Declaration::TSEnumDeclaration(enum_decl) => {
                            let name = enum_decl.id.name.as_str().to_string();
                            exported_values.insert(name.clone());
                            exported_types.insert(name);
                        }
                        Declaration::TSInterfaceDeclaration(iface_decl) => {
                            exported_types.insert(iface_decl.id.name.as_str().to_string());
                        }
                        Declaration::TSTypeAliasDeclaration(alias_decl) => {
                            exported_types.insert(alias_decl.id.name.as_str().to_string());
                        }
                        Declaration::TSModuleDeclaration(mod_decl) => {
                            exported_values.insert(mod_decl.id.name().as_str().to_string());
                        }
                        Declaration::TSImportEqualsDeclaration(_) => {}
                    }
                } else {
                    for s in &exp.specifiers {
                        let exported = module_export_name_str(&s.exported);
                        if exp.export_kind.is_type() || s.export_kind.is_type() {
                            exported_types.insert(exported);
                        } else {
                            exported_values.insert(exported);
                        }
                    }
                }
            }
            Statement::ExportDefaultDeclaration(exp) => {
                has_default_export = true;
                exported_values.insert("default".to_string());
                if let ExportDefaultDeclarationKind::ClassDeclaration(cls) = &exp.declaration
                    && let Some(id) = &cls.id
                {
                    exported_classes.insert(id.name.as_str().to_string());
                }
            }
            Statement::ExportAllDeclaration(exp) => {
                let spec_str = exp.source.value.as_str().to_string();
                let (line, column) =
                    byte_offset_to_line_col(source, exp.source.span.start as usize);
                let resolved_path = resolve_in_lookup(lookup, path, &spec_str);
                let is_type_only = exp.export_kind.is_type();
                if let Some(exported) = &exp.exported {
                    let name = module_export_name_str(exported);
                    if is_type_only {
                        exported_types.insert(name);
                    } else {
                        exported_values.insert(name);
                    }
                } else {
                    has_star_reexport = true;
                    star_reexport_specifiers.push((spec_str.clone(), line, column));
                }
                imports.push(ModuleImport {
                    specifier: spec_str,
                    resolved_path,
                    line,
                    column,
                    is_type_only,
                    named_imports: Vec::new(),
                    default_import: None,
                    namespace_import: None,
                });
            }
            _ => {}
        }
    }

    Ok(ParsedModuleHeader {
        imports,
        exported_values,
        exported_types,
        exported_classes,
        has_default_export,
        has_star_reexport,
        star_reexport_specifiers,
    })
}

/// Compile and bundle all JavaScript & TypeScript `SourceUnit`s in a project.
pub fn compile_web_modules(units: &[&SourceUnit]) -> Result<CompiledModuleGraph, Vec<Diagnostic>> {
    if units.is_empty() {
        return Ok(CompiledModuleGraph::default());
    }

    let lookup = build_path_lookup(units);
    let mut errors = Vec::new();
    let mut headers: BTreeMap<String, (&SourceUnit, ParsedModuleHeader)> = BTreeMap::new();

    for unit in units {
        let canonical = normalize_module_path(&unit.path);
        if canonical.is_empty() {
            errors.push(
                Diagnostic::new(
                    unit.language,
                    1,
                    1,
                    "Source unit path must not be empty.".to_string(),
                )
                .with_source(&unit.source)
                .with_path(&unit.path),
            );
            continue;
        }
        if unit.source.len() > 1_048_576 {
            errors.push(
                Diagnostic::new(
                    unit.language,
                    1,
                    1,
                    format!("{}: A source unit is limited to 1 MiB.", unit.path),
                )
                .with_source(&unit.source)
                .with_path(&unit.path),
            );
            continue;
        }
        match extract_module_header(&unit.source, unit.language, &canonical, &lookup) {
            Ok(header) => {
                if let std::collections::btree_map::Entry::Vacant(e) =
                    headers.entry(canonical.clone())
                {
                    e.insert((unit, header));
                } else {
                    errors.push(
                        Diagnostic::new(
                            unit.language,
                            1,
                            1,
                            format!("Duplicate JS/TS module path '{canonical}'."),
                        )
                        .with_source(&unit.source)
                        .with_path(&unit.path),
                    );
                }
            }
            Err(diags) => errors.extend(diags),
        }
    }

    // 1. Validate that all imported module specifiers resolve to an existing module.
    for (id, (unit, header)) in &headers {
        for imp in &header.imports {
            if imp.resolved_path.is_none() {
                errors.push(
                    Diagnostic::new(
                        unit.language,
                        imp.line,
                        imp.column,
                        format!(
                            "{id}: Cannot find module '{}' imported from '{id}'.",
                            imp.specifier
                        ),
                    )
                    .with_source(&unit.source)
                    .with_path(&unit.path),
                );
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // 2. Topological sort & cycle detection using DFS.
    // Note: runtime imports (and type imports) form edges; we check runtime edges first for circular dependencies,
    // and also detect cycles across all module imports.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum VisitState {
        Visiting,
        Visited,
    }
    let mut state: BTreeMap<String, VisitState> = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut order: Vec<String> = Vec::new();

    fn dfs(
        current: &str,
        headers: &BTreeMap<String, (&SourceUnit, ParsedModuleHeader)>,
        state: &mut BTreeMap<String, VisitState>,
        stack: &mut Vec<String>,
        order: &mut Vec<String>,
        errors: &mut Vec<Diagnostic>,
    ) {
        state.insert(current.to_string(), VisitState::Visiting);
        stack.push(current.to_string());

        if let Some((unit, header)) = headers.get(current) {
            for imp in &header.imports {
                let Some(dep) = &imp.resolved_path else {
                    continue;
                };
                match state.get(dep).copied() {
                    None => {
                        dfs(dep, headers, state, stack, order, errors);
                    }
                    Some(VisitState::Visiting) => {
                        let cycle_start = stack.iter().position(|m| m == dep).unwrap_or(0);
                        let mut chain: Vec<String> = stack[cycle_start..].to_vec();
                        chain.push(dep.clone());
                        errors.push(
                            Diagnostic::new(
                                unit.language,
                                imp.line,
                                imp.column,
                                format!(
                                    "{current}: Circular module dependency detected: {}",
                                    chain.join(" -> ")
                                ),
                            )
                            .with_source(&unit.source)
                            .with_path(&unit.path),
                        );
                    }
                    Some(VisitState::Visited) => {}
                }
            }
        }

        stack.pop();
        state.insert(current.to_string(), VisitState::Visited);
        order.push(current.to_string());
    }

    for id in headers.keys() {
        if !state.contains_key(id) {
            dfs(
                id,
                &headers,
                &mut state,
                &mut stack,
                &mut order,
                &mut errors,
            );
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // 3. Process modules in topological order:
    //    - Propagate star re-exports (`export * from "./..."`)
    //    - Validate named & default imports against target module exports
    //    - Run static TypeScript type checking (`ts_check`)
    //    - Transform TS + JSX and rewrite ES module syntax into bundled module body
    let mut compiled_modules: BTreeMap<String, CompiledModule> = BTreeMap::new();
    let mut all_exported_classes: Vec<String> = Vec::new();

    for id in &order {
        let (unit, header) = headers.get(id).expect("module in topological order");
        let mut exported_values = header.exported_values.clone();
        let mut exported_types = header.exported_types.clone();
        let exported_classes = header.exported_classes.clone();
        let mut star_targets = Vec::new();

        for (spec_str, _, _) in &header.star_reexport_specifiers {
            if let Some(target_id) = resolve_in_lookup(&lookup, id, spec_str)
                && let Some(target_mod) = compiled_modules.get(&target_id)
            {
                for v in &target_mod.exported_values {
                    if v != "default" {
                        exported_values.insert(v.clone());
                    }
                }
                for t in &target_mod.exported_types {
                    exported_types.insert(t.clone());
                }
                star_targets.push(target_id);
            }
        }

        // Validate named and default imports against already-compiled dependencies
        for imp in &header.imports {
            let Some(target_id) = &imp.resolved_path else {
                continue;
            };
            let Some(target_mod) = compiled_modules.get(target_id) else {
                continue;
            };
            if let Some((local, dl, dc)) = &imp.default_import
                && !imp.is_type_only
                && !target_mod.has_default_export
                && !target_mod.exported_values.contains("default")
            {
                errors.push(Diagnostic::new(unit.language, *dl, *dc, format!(
                        "{id}: Module '{target_id}' has no default export (imported as '{local}')."
                    )).with_source(&unit.source).with_path(&unit.path));
            }
            for (imported_name, _, is_type, sl, sc) in &imp.named_imports {
                let exists_val = target_mod.exported_values.contains(imported_name);
                let exists_ty = target_mod.exported_types.contains(imported_name);
                if *is_type {
                    if !exists_ty && !exists_val && !target_mod.has_star_reexport {
                        let candidates: Vec<&str> = target_mod
                            .exported_types
                            .iter()
                            .chain(target_mod.exported_values.iter())
                            .map(String::as_str)
                            .collect();
                        let hint = suggest_identifier(imported_name, candidates)
                            .map(|(s, _)| format!(" Did you mean '{s}'?"))
                            .unwrap_or_default();
                        errors.push(Diagnostic::new(unit.language, *sl, *sc, format!(
                                "{id}: Module '{target_id}' has no exported type '{imported_name}'.{hint}"
                            )).with_source(&unit.source).with_path(&unit.path));
                    }
                } else if !exists_val && !exists_ty && !target_mod.has_star_reexport {
                    let candidates: Vec<&str> = target_mod
                        .exported_values
                        .iter()
                        .chain(target_mod.exported_types.iter())
                        .map(String::as_str)
                        .collect();
                    let hint = suggest_identifier(imported_name, candidates)
                        .map(|(s, _)| format!(" Did you mean '{s}'?"))
                        .unwrap_or_default();
                    errors.push(Diagnostic::new(unit.language, *sl, *sc, format!(
                            "{id}: Module '{target_id}' has no exported member '{imported_name}'.{hint}"
                        )).with_source(&unit.source).with_path(&unit.path));
                }
            }
        }

        // Build imported_modules map for TypeScript static checker
        let mut imported_type_map: BTreeMap<String, ModuleTypeExports> = BTreeMap::new();
        for (mod_id, compiled) in &compiled_modules {
            imported_type_map.insert(mod_id.clone(), compiled.type_exports.clone());
        }
        for imp in &header.imports {
            if let Some(target_id) = &imp.resolved_path
                && let Some(target_mod) = compiled_modules.get(target_id)
            {
                imported_type_map.insert(imp.specifier.clone(), target_mod.type_exports.clone());
            }
        }

        // Static type checking & export signature extraction
        let type_exports = match analyze_and_typecheck_module(
            &unit.source,
            unit.language,
            id,
            &imported_type_map,
            &exported_values,
            &star_targets,
            &compiled_modules,
        ) {
            Ok(exports) => exports,
            Err(diags) => {
                errors.extend(diags);
                ModuleTypeExports::default()
            }
        };

        // Transform TypeScript & JSX, then rewrite ES module imports/exports
        let (compiled_body, source_map) =
            match transform_and_rewrite_module(&unit.source, unit.language, id, &lookup) {
                Ok(output) => output,
                Err(diags) => {
                    errors.extend(diags);
                    (
                        String::new(),
                        SourceMap::from_segments(
                            None,
                            vec![id.clone()],
                            vec![unit.source.clone()],
                            vec![],
                            vec![],
                        ),
                    )
                }
            };

        for cls in &exported_classes {
            if !all_exported_classes.contains(cls) {
                all_exported_classes.push(cls.clone());
            }
        }

        compiled_modules.insert(
            id.clone(),
            CompiledModule {
                id: id.clone(),
                path: unit.path.clone(),
                language: unit.language,
                original_source: unit.source.clone(),
                compiled_body,
                imports: header.imports.clone(),
                exported_values,
                exported_types,
                exported_classes,
                has_default_export: header.has_default_export,
                has_star_reexport: header.has_star_reexport,
                star_reexport_targets: star_targets,
                type_exports,
                source_map,
            },
        );
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let (bundled_code, bundle_source_map) = emit_bundled_module_runtime(&order, &compiled_modules);

    Ok(CompiledModuleGraph {
        order,
        modules: compiled_modules,
        path_lookup: lookup,
        bundled_code,
        bundle_source_map: Some(bundle_source_map),
        exported_classes: all_exported_classes,
    })
}

fn analyze_and_typecheck_module(
    source: &str,
    language: ScriptLanguage,
    path: &str,
    imported_modules: &BTreeMap<String, ModuleTypeExports>,
    exported_values: &BTreeSet<String>,
    star_targets: &[String],
    compiled_modules: &BTreeMap<String, CompiledModule>,
) -> Result<ModuleTypeExports, Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let source_type = source_type_for(language, Some(path), true);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| {
                let offset = error
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                let (line, column) = byte_offset_to_line_col(source, offset);
                Diagnostic::new(language, line, column, format!("{path}: {error}"))
                    .with_source(source)
                    .with_path(path)
            })
            .collect());
    }

    let mut check_result = check_typescript_program(
        &parsed.program,
        source,
        language,
        Some(path),
        imported_modules,
    );

    if language == ScriptLanguage::TypeScript && !check_result.diagnostics.is_empty() {
        return Err(check_result.diagnostics);
    }

    // For untyped JavaScript exports (or any exported value not yet in type_exports), populate `Any` so TS importers see them.
    for val in exported_values {
        check_result
            .exports
            .values
            .entry(val.clone())
            .or_insert(TsType::Any);
    }
    for target_id in star_targets {
        if let Some(target) = compiled_modules.get(target_id) {
            for (k, v) in &target.type_exports.values {
                if k != "default" {
                    check_result
                        .exports
                        .values
                        .entry(k.clone())
                        .or_insert_with(|| v.clone());
                }
            }
            for (k, v) in &target.type_exports.types {
                check_result
                    .exports
                    .types
                    .entry(k.clone())
                    .or_insert_with(|| v.clone());
            }
            for (k, v) in &target.type_exports.constructors {
                check_result
                    .exports
                    .constructors
                    .entry(k.clone())
                    .or_insert_with(|| v.clone());
            }
        }
    }

    Ok(check_result.exports)
}

/// Transform TypeScript types and JSX syntax with Oxc into JavaScript.
pub fn transform_ts_and_jsx(
    source: &str,
    language: ScriptLanguage,
    path_hint: Option<&str>,
    is_module: bool,
) -> Result<String, Vec<Diagnostic>> {
    transform_ts_and_jsx_mapped(source, language, path_hint, is_module).map(|(code, _)| code)
}
fn transform_ts_and_jsx_mapped(
    source: &str,
    language: ScriptLanguage,
    path_hint: Option<&str>,
    is_module: bool,
) -> Result<(String, SourceMap), Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let source_type = source_type_for(language, path_hint, is_module);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| {
                let offset = error
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                let (line, column) = byte_offset_to_line_col(source, offset);
                let prefix = path_hint.map(|p| format!("{p}: ")).unwrap_or_default();
                Diagnostic::new(language, line, column, format!("{prefix}{error}"))
                    .with_source(source)
                    .with_path(path_hint.unwrap_or(""))
            })
            .collect());
    }

    let mut program = parsed.program;
    let semantic_ret = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    if !semantic_ret.errors.is_empty() {
        return Err(semantic_ret
            .errors
            .into_iter()
            .map(|error| {
                let offset = error
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                let (line, column) = byte_offset_to_line_col(source, offset);
                let prefix = path_hint.map(|p| format!("{p}: ")).unwrap_or_default();
                Diagnostic::new(language, line, column, format!("{prefix}{error}"))
                    .with_source(source)
                    .with_path(path_hint.unwrap_or(""))
            })
            .collect());
    }

    let scoping = semantic_ret.semantic.into_scoping();
    let transform_options = TransformOptions {
        jsx: JsxOptions {
            runtime: JsxRuntime::Classic,
            pragma: Some("__nfJsx".to_string()),
            pragma_frag: Some("__nfFragment".to_string()),
            ..JsxOptions::default()
        },
        ..TransformOptions::default()
    };

    let file_path = Path::new(path_hint.unwrap_or(match language {
        ScriptLanguage::TypeScript => "script.tsx",
        _ => "script.jsx",
    }));
    let transformed = Transformer::new(&allocator, file_path, &transform_options)
        .build_with_scoping(scoping, &mut program);
    if !transformed.errors.is_empty() {
        return Err(transformed
            .errors
            .into_iter()
            .map(|error| {
                let offset = error
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.first())
                    .map(|label| label.offset())
                    .unwrap_or(0);
                let (line, column) = byte_offset_to_line_col(source, offset);
                let prefix = path_hint.map(|p| format!("{p}: ")).unwrap_or_default();
                Diagnostic::new(language, line, column, format!("{prefix}{error}"))
                    .with_source(source)
                    .with_path(path_hint.unwrap_or(""))
            })
            .collect());
    }

    let generated = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(file_path.to_owned()),
            ..CodegenOptions::default()
        })
        .build(&program);
    let map = serde_json::from_str(&generated.map.expect("mapping requested").to_json_string())
        .expect("Oxc source map v3");
    Ok((generated.code, map))
}

/// Transform a JS/TS module and rewrite ES `import`/`export` statements into `__nfRequire` and `__nfExports`.
fn transform_and_rewrite_module(
    source: &str,
    language: ScriptLanguage,
    module_id: &str,
    lookup: &BTreeMap<String, String>,
) -> Result<(String, SourceMap), Vec<Diagnostic>> {
    let (transformed_js, map) =
        transform_ts_and_jsx_mapped(source, language, Some(module_id), true)?;
    let (code, ranges) =
        rewrite_esm_with_ranges(&transformed_js, language, module_id, lookup, true)?;
    let map = crate::sourcemap::relocate_map(&map, &transformed_js, &code, &ranges);
    Ok((code, map))
}

/// Rewrite ES `import` and `export` statements in JavaScript code produced after TS/JSX transform.
#[allow(clippy::type_complexity)]
fn rewrite_esm_with_ranges(
    js_code: &str,
    language: ScriptLanguage,
    importer_path: &str,
    lookup: &BTreeMap<String, String>,
    allow_exports: bool,
) -> Result<(String, Vec<(usize, usize, usize)>), Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, js_code, SourceType::mjs()).parse();
    if !parsed.errors.is_empty() {
        return Ok((js_code.to_string(), vec![(0, js_code.len(), 0)]));
    }

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut temp_counter = 0usize;
    let mut retained = BTreeMap::<usize, (usize, usize, usize)>::new();

    for stmt in &parsed.program.body {
        match stmt {
            Statement::ImportDeclaration(imp) => {
                let start = imp.span.start as usize;
                let end = imp.span.end as usize;
                if imp.import_kind.is_type() {
                    replacements.push((start, end, String::new()));
                    continue;
                }
                let raw_spec = imp.source.value.as_str();
                let resolved = resolve_in_lookup(lookup, importer_path, raw_spec)
                    .unwrap_or_else(|| normalize_module_path(raw_spec));
                let require_expr = format!(
                    "(typeof __nfRequire === 'function' ? __nfRequire({resolved:?}) : (__nfProgram && __nfProgram.requireModule ? __nfProgram.requireModule({resolved:?}) : globalThis.__nfRequire({resolved:?})))"
                );

                let Some(specs) = &imp.specifiers else {
                    replacements.push((start, end, format!("{require_expr};")));
                    continue;
                };
                if specs.is_empty() {
                    replacements.push((start, end, format!("{require_expr};")));
                    continue;
                }

                temp_counter += 1;
                let mod_var = format!("__nfMod_{temp_counter}");
                let mut lines = vec![format!("const {mod_var} = {require_expr};")];
                for spec in specs {
                    match spec {
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            if s.import_kind.is_type() {
                                continue;
                            }
                            let imported = module_export_name_str(&s.imported);
                            let local = s.local.name.as_str();
                            lines.push(format!("const {local} = {mod_var}[{imported:?}];"));
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            let local = s.local.name.as_str();
                            lines.push(format!(
                                "const {local} = ({mod_var} && {mod_var}.__esModule) ? {mod_var}.default : (({mod_var} && {mod_var}.default !== undefined) ? {mod_var}.default : {mod_var});"
                            ));
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            let local = s.local.name.as_str();
                            lines.push(format!("const {local} = {mod_var};"));
                        }
                    }
                }
                replacements.push((start, end, lines.join(" ")));
            }
            Statement::ExportNamedDeclaration(exp) if allow_exports => {
                let start = exp.span.start as usize;
                let end = exp.span.end as usize;
                if exp.export_kind.is_type() {
                    replacements.push((start, end, String::new()));
                    continue;
                }
                if let Some(src) = &exp.source {
                    let raw_spec = src.value.as_str();
                    let resolved = resolve_in_lookup(lookup, importer_path, raw_spec)
                        .unwrap_or_else(|| normalize_module_path(raw_spec));
                    temp_counter += 1;
                    let mod_var = format!("__nfReexp_{temp_counter}");
                    let mut lines = vec![format!("const {mod_var} = __nfRequire({resolved:?});")];
                    for s in &exp.specifiers {
                        if s.export_kind.is_type() {
                            continue;
                        }
                        let local = module_export_name_str(&s.local);
                        let exported = module_export_name_str(&s.exported);
                        lines.push(format!(
                            "Object.defineProperty(__nfExports, {exported:?}, {{ enumerable: true, configurable: true, get: function() {{ return {mod_var}[{local:?}]; }} }});"
                        ));
                    }
                    replacements.push((start, end, lines.join(" ")));
                } else if let Some(decl) = &exp.declaration {
                    let decl_span = decl.span();
                    let decl_code = &js_code[decl_span.start as usize..decl_span.end as usize];
                    retained.insert(start, (decl_span.start as usize, decl_span.end as usize, 0));
                    let mut bound_names = BTreeSet::new();
                    match decl {
                        Declaration::VariableDeclaration(var_decl) => {
                            for d in &var_decl.declarations {
                                collect_binding_names(&d.id.kind, &mut bound_names);
                            }
                        }
                        Declaration::FunctionDeclaration(fn_decl) => {
                            if let Some(id) = &fn_decl.id {
                                bound_names.insert(id.name.as_str().to_string());
                            }
                        }
                        Declaration::ClassDeclaration(cls_decl) => {
                            if let Some(id) = &cls_decl.id {
                                bound_names.insert(id.name.as_str().to_string());
                            }
                        }
                        _ => {}
                    }
                    let mut out = decl_code.to_string();
                    if !out.trim_end().ends_with(';') && !out.trim_end().ends_with('}') {
                        out.push(';');
                    }
                    for name in bound_names {
                        out.push_str(&format!(
                            "\nObject.defineProperty(__nfExports, {name:?}, {{ enumerable: true, configurable: true, get: function() {{ return {name}; }}, set: function(__v) {{ {name} = __v; }} }});"
                        ));
                    }
                    replacements.push((start, end, out));
                } else {
                    let mut lines = Vec::new();
                    for s in &exp.specifiers {
                        if s.export_kind.is_type() {
                            continue;
                        }
                        let local = module_export_name_str(&s.local);
                        let exported = module_export_name_str(&s.exported);
                        lines.push(format!(
                            "Object.defineProperty(__nfExports, {exported:?}, {{ enumerable: true, configurable: true, get: function() {{ return {local}; }} }});"
                        ));
                    }
                    replacements.push((start, end, lines.join(" ")));
                }
            }
            Statement::ExportDefaultDeclaration(exp) if allow_exports => {
                let start = exp.span.start as usize;
                let end = exp.span.end as usize;
                let replacement = match &exp.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(fn_decl) => {
                        let code = &js_code[fn_decl.span.start as usize..fn_decl.span.end as usize];
                        if let Some(id) = &fn_decl.id {
                            let name = id.name.as_str();
                            format!("{code}\n__nfExports.default = {name};")
                        } else {
                            format!("__nfExports.default = ({code});")
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(cls_decl) => {
                        let code =
                            &js_code[cls_decl.span.start as usize..cls_decl.span.end as usize];
                        if let Some(id) = &cls_decl.id {
                            let name = id.name.as_str();
                            format!("{code}\n__nfExports.default = {name};")
                        } else {
                            format!("__nfExports.default = ({code});")
                        }
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => String::new(),
                    other => {
                        let span = other.span();
                        let code = &js_code[span.start as usize..span.end as usize];
                        format!("__nfExports.default = ({code});")
                    }
                };
                let span = exp.declaration.span();
                if !matches!(
                    exp.declaration,
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(_)
                ) {
                    let original = &js_code[span.start as usize..span.end as usize];
                    let at = if replacement.starts_with(original) {
                        0
                    } else {
                        "__nfExports.default = (".len()
                    };
                    retained.insert(start, (span.start as usize, span.end as usize, at));
                }
                replacements.push((start, end, replacement));
            }
            Statement::ExportAllDeclaration(exp) if allow_exports => {
                let start = exp.span.start as usize;
                let end = exp.span.end as usize;
                if exp.export_kind.is_type() {
                    replacements.push((start, end, String::new()));
                    continue;
                }
                let raw_spec = exp.source.value.as_str();
                let resolved = resolve_in_lookup(lookup, importer_path, raw_spec)
                    .unwrap_or_else(|| normalize_module_path(raw_spec));
                let replacement = if let Some(exported) = &exp.exported {
                    let name = module_export_name_str(exported);
                    format!("__nfExports[{name:?}] = __nfRequire({resolved:?});")
                } else {
                    format!(
                        "{{ const __nfStar = __nfRequire({resolved:?}); Object.keys(__nfStar).forEach(function(__k) {{ if (__k !== 'default' && __k !== '__esModule') {{ Object.defineProperty(__nfExports, __k, {{ enumerable: true, configurable: true, get: function() {{ return __nfStar[__k]; }} }}); }} }}); }}"
                    )
                };
                replacements.push((start, end, replacement));
            }
            _ => {}
        }
    }

    let _ = language;
    if replacements.is_empty() {
        return Ok((js_code.to_string(), vec![(0, js_code.len(), 0)]));
    }

    let mut out = String::with_capacity(js_code.len() + 256);
    let mut cursor = 0usize;
    let mut ranges = Vec::new();
    for (start, end, rep) in replacements {
        if start >= cursor && end <= js_code.len() {
            ranges.push((cursor, start, out.len()));
            out.push_str(&js_code[cursor..start]);
            if let Some((from, to, at)) = retained.get(&start) {
                ranges.push((*from, *to, out.len() + at));
            }
            out.push_str(&rep);
            cursor = end;
        }
    }
    ranges.push((cursor, js_code.len(), out.len()));
    out.push_str(&js_code[cursor..]);
    Ok((out, ranges))
}

/// Compile a JS or TS frame/object handler against a `CompiledModuleGraph`.
pub fn compile_handler_with_modules(
    script: &Script,
    graph: &CompiledModuleGraph,
) -> Result<String, Vec<Diagnostic>> {
    compile_handler_with_modules_mapped(script, graph, "handler").map(|(code, _)| code)
}
pub(crate) fn compile_handler_with_modules_mapped(
    script: &Script,
    graph: &CompiledModuleGraph,
    source_name: &str,
) -> Result<(String, SourceMap), Vec<Diagnostic>> {
    // Check if the handler has top-level `import` or `export` statements or JSX
    let allocator = Allocator::default();
    let wrapped_for_check = format!("function __neoflash_handler__() {{\n{}\n}}", script.source);
    let source_type_fn = source_type_for(script.language, None, false);
    let parsed_fn = Parser::new(&allocator, &wrapped_for_check, source_type_fn).parse();

    // If wrapping inside a function fails to parse because of top-level `import`/`export`,
    // parse as a module so imports can be validated and rewritten!
    let has_module_syntax = !parsed_fn.errors.is_empty()
        || script.source.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("import ") || trimmed.starts_with("export ")
        });

    if has_module_syntax {
        let header =
            extract_module_header(&script.source, script.language, "", &graph.path_lookup)?;
        if header.has_default_export
            || !header.exported_values.is_empty()
            || !header.exported_types.is_empty()
            || header.has_star_reexport
        {
            return Err(vec![Diagnostic::new(script.language, 1, 1, "Frame and object handlers cannot export declarations; move exports to a project source module.".to_string()).with_source(&script.source)]);
        }

        let mut errors = Vec::new();
        for imp in &header.imports {
            match &imp.resolved_path {
                None => {
                    errors.push(
                        Diagnostic::new(
                            script.language,
                            imp.line,
                            imp.column,
                            format!(
                                "Cannot find module '{}' in project source units.",
                                imp.specifier
                            ),
                        )
                        .with_source(&script.source),
                    );
                }
                Some(target_id) => {
                    if let Some(target_mod) = graph.modules.get(target_id) {
                        if let Some((local, dl, dc)) = &imp.default_import
                            && !imp.is_type_only
                            && !target_mod.has_default_export
                            && !target_mod.exported_values.contains("default")
                        {
                            errors.push(Diagnostic::new(script.language, *dl, *dc, format!(
                                    "Module '{target_id}' has no default export (imported as '{local}')."
                                )).with_source(&script.source));
                        }
                        for (imported_name, _, is_type, sl, sc) in &imp.named_imports {
                            let exists_val = target_mod.exported_values.contains(imported_name);
                            let exists_ty = target_mod.exported_types.contains(imported_name);
                            if !exists_val && !exists_ty && !target_mod.has_star_reexport {
                                let kind_label = if *is_type {
                                    "exported type"
                                } else {
                                    "exported member"
                                };
                                errors.push(Diagnostic::new(script.language, *sl, *sc, format!(
                                        "Module '{target_id}' has no {kind_label} '{imported_name}'."
                                    )).with_source(&script.source));
                            }
                        }
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        if script.language == ScriptLanguage::TypeScript {
            let mod_alloc = Allocator::default();
            let mod_st = source_type_for(script.language, None, true);
            let mod_parsed = Parser::new(&mod_alloc, &script.source, mod_st).parse();
            let imported_map = graph.type_exports_for_importer("", &header.imports);
            let check = check_typescript_program(
                &mod_parsed.program,
                &script.source,
                script.language,
                None,
                &imported_map,
            );
            if !check.diagnostics.is_empty() {
                return Err(check.diagnostics);
            }
        }

        let (transformed, map) =
            transform_ts_and_jsx_mapped(&script.source, script.language, Some(source_name), true)?;
        let (rewritten, ranges) =
            rewrite_esm_with_ranges(&transformed, script.language, "", &graph.path_lookup, false)?;
        let map = crate::sourcemap::relocate_map(&map, &transformed, &rewritten, &ranges);
        let trim_start = rewritten.len() - rewritten.trim_start().len();
        let code = rewritten.trim().to_owned();
        let map = crate::sourcemap::relocate_map(
            &map,
            &rewritten,
            &code,
            &[(trim_start, trim_start + code.len(), 0)],
        );
        let needs_jsx = code.contains("__nfJsx") || code.contains("__nfFragment");
        if needs_jsx {
            let preamble = format!("{}\n", jsx_inline_preamble_js());
            let map = crate::sourcemap::shift_map(
                &map,
                preamble.bytes().filter(|c| *c == b'\n').count(),
                0,
            );
            return Ok((format!("{preamble}{code}"), map));
        }
        return Ok((code, map));
    }

    crate::compile_web_mapped(
        script,
        script.language == ScriptLanguage::TypeScript,
        source_name,
    )
}

/// Inline JSX helper preamble for standalone scripts/handlers that use JSX without a project bundle.
pub fn jsx_inline_preamble_js() -> &'static str {
    r#"const __nfFragment = globalThis.__nfFragment || Symbol.for("neoflash.fragment");
const MovieClip = globalThis.MovieClip || { __nfStageType: "MovieClip" };
const Sprite = globalThis.Sprite || { __nfStageType: "Sprite" };
const TextField = globalThis.TextField || { __nfStageType: "TextField" };
const Shape = globalThis.Shape || { __nfStageType: "Shape" };
const SimpleButton = globalThis.SimpleButton || { __nfStageType: "SimpleButton" };
const __nfJsx = globalThis.__nfJsx || function __nfJsx(tag, props) {
  var children = [];
  for (var i = 2; i < arguments.length; i++) {
    var arg = arguments[i];
    if (Array.isArray(arg)) {
      for (var j = 0; j < arg.length; j++) {
        if (arg[j] !== null && arg[j] !== undefined && arg[j] !== false && arg[j] !== true) children.push(arg[j]);
      }
    } else if (arg !== null && arg !== undefined && arg !== false && arg !== true) {
      children.push(arg);
    }
  }
  var normProps = Object.assign({}, props || {});
  if (children.length === 1) normProps.children = children[0];
  else if (children.length > 1) normProps.children = children;
  if (tag === __nfFragment || tag === "Fragment") {
    return { __nfNode: "fragment", type: "Fragment", props: normProps, children: children, textContent: children.map(function(c) { return typeof c === "object" && c && c.textContent !== undefined ? c.textContent : String(c); }).join("") };
  }
  if (typeof tag === "function") {
    if (tag.__nfStageType) {
      tag = { __nfStageType: tag.__nfStageType };
    } else {
      try {
        return tag(normProps);
      } catch (e) {
        var inst = new tag(normProps);
        return inst && typeof inst.render === "function" ? inst.render() : inst;
      }
    }
  }
  var stageType = (tag && tag.__nfStageType) ? tag.__nfStageType : (typeof tag === "string" && (/^(MovieClip|Sprite|TextField|Shape|SimpleButton|Bitmap|Container|Stage)$/).test(tag) ? tag : null);
  if (stageType) {
    var textVal = normProps.text !== undefined ? String(normProps.text) : children.filter(function(c) { return typeof c === "string" || typeof c === "number"; }).join("");
    var node = {
      __nfNode: "display",
      type: stageType,
      tagName: stageType,
      name: normProps.name || normProps.id || "",
      x: Number(normProps.x ?? 0),
      y: Number(normProps.y ?? 0),
      scaleX: Number(normProps.scaleX ?? 1),
      scaleY: Number(normProps.scaleY ?? 1),
      rotation: Number(normProps.rotation ?? 0),
      alpha: Number(normProps.alpha ?? 1),
      visible: normProps.visible !== undefined ? Boolean(normProps.visible) : true,
      text: textVal,
      textContent: textVal,
      props: normProps,
      children: children,
      addChild: function(child) { this.children.push(child); return child; }
    };
    return node;
  }
  var tagName = String(tag || "div");
  if (typeof document !== "undefined" && document && typeof document.createElement === "function") {
    var domEl = document.createElement(tagName);
    Object.keys(normProps).forEach(function(key) {
      if (key === "children") return;
      var val = normProps[key];
      if (key === "className" || key === "class") {
        domEl.className = String(val);
        if (typeof domEl.setAttribute === "function") domEl.setAttribute("class", String(val));
      } else if (key === "style" && val && typeof val === "object") {
        Object.assign(domEl.style, val);
      } else if (key.startsWith("on") && typeof val === "function" && typeof domEl.addEventListener === "function") {
        domEl.addEventListener(key.slice(2).toLowerCase(), val);
      } else if (val !== undefined && val !== null) {
        domEl[key] = val;
        if (typeof domEl.setAttribute === "function" && typeof val !== "object" && typeof val !== "function") {
          domEl.setAttribute(key, String(val));
        }
      }
    });
    children.forEach(function(child) {
      if (child && typeof child === "object" && child.nodeType && typeof domEl.appendChild === "function") {
        domEl.appendChild(child);
      } else if (typeof document.createTextNode === "function" && typeof domEl.appendChild === "function") {
        domEl.appendChild(document.createTextNode(String(child)));
      }
    });
    domEl.props = normProps;
    return domEl;
  }
  var attrs = {};
  Object.keys(normProps).forEach(function(k) {
    if (k === "children" || typeof normProps[k] === "function") return;
    if (k === "className") attrs["class"] = String(normProps[k]);
    else if (k === "style" && normProps[k] && typeof normProps[k] === "object") {
      attrs["style"] = Object.keys(normProps[k]).map(function(sk) { return sk + ":" + normProps[k][sk]; }).join(";");
    } else if (normProps[k] !== undefined && normProps[k] !== null) {
      attrs[k] = String(normProps[k]);
    }
  });
  var attrStr = Object.keys(attrs).map(function(k) { return " " + k + "=\"" + attrs[k] + "\""; }).join("");
  var innerText = children.map(function(c) { return (c && typeof c === "object" && c.textContent !== undefined) ? c.textContent : String(c); }).join("");
  var innerHtml = children.map(function(c) { return (c && typeof c === "object" && c.outerHTML !== undefined) ? c.outerHTML : String(c); }).join("");
  return {
    __nfNode: "dom",
    nodeType: 1,
    type: tagName,
    tagName: tagName.toUpperCase(),
    localName: tagName,
    id: normProps.id || "",
    className: normProps.className || normProps["class"] || "",
    style: normProps.style || {},
    attributes: attrs,
    props: normProps,
    children: children,
    childNodes: children,
    textContent: innerText,
    innerHTML: innerHtml,
    outerHTML: "<" + tagName + attrStr + ">" + innerHtml + "</" + tagName + ">",
    getAttribute: function(name) { return name === "className" ? this. attributes["class"] : (this.attributes[name] ?? null); },
    setAttribute: function(name, value) { this.attributes[name] = String(value); if (name === "class") this.className = String(value); },
    appendChild: function(child) { this.children.push(child); return child; }
  };
};
globalThis.__nfJsx = __nfJsx;
globalThis.__nfFragment = __nfFragment;"#
}

fn emit_bundled_module_runtime(
    order: &[String],
    modules: &BTreeMap<String, CompiledModule>,
) -> (String, SourceMap) {
    let mut out = String::new();
    out.push_str(jsx_inline_preamble_js());
    out.push('\n');
    out.push_str(crate::sourcemap::debug_runtime_js());
    out.push('\n');
    out.push_str(
        r#"const __nfModuleFactories = Object.create(null);
const __nfModuleCache = Object.create(null);
const __nfModuleAliases = Object.create(null);
function __nfNormalizeModuleId(spec) {
  const clean = String(spec || "").trim().replace(/\\/g, "/").replace(/^\.\//, "");
  if (__nfModuleFactories[clean]) return clean;
  if (__nfModuleAliases[clean]) return __nfModuleAliases[clean];
  const exts = [".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx", "/index.js", "/index.jsx"];
  for (let i = 0; i < exts.length; i++) {
    const candidate = clean + exts[i];
    if (__nfModuleFactories[candidate]) return candidate;
  }
  return clean;
}
function __nfRegisterModule(id, aliases, factory) {
  __nfModuleFactories[id] = factory;
  if (Array.isArray(aliases)) {
    for (let i = 0; i < aliases.length; i++) {
      __nfModuleAliases[aliases[i]] = id;
    }
  }
}
function __nfRequire(spec) {
  const id = __nfNormalizeModuleId(spec);
  if (__nfModuleCache[id]) {
    return __nfModuleCache[id].exports;
  }
  const factory = __nfModuleFactories[id];
  if (typeof factory !== "function") {
    throw new Error("Cannot find module '" + spec + "'");
  }
  const moduleRecord = { id: id, exports: Object.create(null), loaded: false };
  Object.defineProperty(moduleRecord.exports, "__esModule", { value: true, enumerable: false });
  __nfModuleCache[id] = moduleRecord;
  factory(moduleRecord.exports, __nfRequire, moduleRecord, __nfJsx, __nfFragment);
  moduleRecord.loaded = true;
  return moduleRecord.exports;
}
globalThis.__nfRequire = __nfRequire;
"#,
    );

    let mut maps = Vec::new();
    for id in order {
        let Some(module) = modules.get(id) else {
            continue;
        };
        let stem = strip_known_extension(id).to_string();
        let mut aliases = vec![
            id.clone(),
            stem.clone(),
            format!("./{id}"),
            format!("./{stem}"),
        ];
        if let Some(dir_prefix) = stem.strip_suffix("/index") {
            aliases.push(dir_prefix.to_string());
            aliases.push(format!("./{dir_prefix}"));
        }
        aliases.sort();
        aliases.dedup();
        let aliases_json = serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".to_string());
        out.push_str(&format!("__nfRegisterModule({id:?}, {aliases_json}, function(__nfExports, __nfRequire, module, __nfJsx, __nfFragment) {{\n"));
        maps.push(crate::sourcemap::shift_map(
            &module.source_map,
            out.bytes().filter(|c| *c == b'\n').count(),
            0,
        ));
        out.push_str(&module.compiled_body);
        out.push_str("\n});\n");
    }

    // Eagerly evaluate all registered modules in topological order so top-level side effects and exports are ready
    for id in order {
        out.push_str(&format!("__nfRequire({id:?});\n"));
    }

    out.push_str(
        r#"if (typeof __nfProgram === "object" && __nfProgram !== null) {
  __nfProgram.modules = __nfModuleCache;
  __nfProgram.requireModule = __nfRequire;
  __nfProgram.jsx = __nfJsx;
  __nfProgram.Fragment = __nfFragment;
  __nfProgram.debug = globalThis.__nfDebug;
}
"#,
    );

    (out, crate::sourcemap::combine_maps(&maps))
}
