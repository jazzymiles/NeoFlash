//! Deterministic AST-to-Swift 6 code generator (`compile_project_scripts_to_swift`).
//!
//! Compiles ActionScript 1.0 / 2.0 / 3.0 timeline scripts, stage object handlers, and class
//! source units into strict Swift 6 `@MainActor` code targeting NeoFlash's native Xcode runtime,
//! extracting the `SwiftHost` static proxy contract and enforcing ARC-safe `[weak self]` closures.

use super::emit_as2::{classify_as3_event_type, is_stage_expr};
use super::emit_as3::op_prec;
use super::hir::{
    AccessorKind, HirType, ParsedClass, ParsedScript, ProjectStageIndex, TypeEnv, fold_100_to_01,
    is_atomic_expr, map_as2_direct_prop_to_modern, map_as2_key_constant,
    map_as2_percent_prop_to_modern, map_as3_keyboard_constant, parse_class_unit,
    parse_timeline_script, sanitize_swift_type_ident, short_type_name, stmts_use_key_is_down,
};
use super::swift_contract::{
    SwiftProxyMethod, SwiftProxyParam, inspect_swift_host_class, is_swift_host_class,
    map_portable_as_type_to_swift,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{
    Case, CatchClause, Expr, Function, LegacyTrigger, PropertyKey, Stmt,
};
use crate::byte_offset_to_line_col;
use neoflash_core::{
    AuthoredPlayback, ObjectEvent, Project, ResourceUnit, Script, ScriptLanguage, Symbol,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Compiled Swift 6 representation of a single NeoFlash `Symbol` (root scene or library symbol).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftCompiledSymbol {
    pub symbol_id: String,
    pub swift_class_name: String,
    pub properties_swift: String,
    pub frame_methods_swift: BTreeMap<u32, String>,
    pub helper_methods_swift: String,
    pub instance_handlers_swift: Vec<(String, ObjectEvent, String)>,
}

/// Complete Swift 6 script compilation output for a `Project`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftCompiledProject {
    pub host_methods: Vec<SwiftProxyMethod>,
    pub symbols: Vec<SwiftCompiledSymbol>,
    pub source_classes_swift: String,
    pub issues: Vec<ConversionIssue>,
}

/// Escape Swift 6 reserved keywords with backticks when used as identifiers/members.
pub(crate) fn escape_swift_ident(raw: &str) -> String {
    if matches!(
        raw,
        "default"
            | "guard"
            | "in"
            | "repeat"
            | "where"
            | "func"
            | "let"
            | "var"
            | "class"
            | "struct"
            | "enum"
            | "protocol"
            | "extension"
            | "Self"
            | "Type"
            | "init"
            | "deinit"
            | "subscript"
            | "operator"
            | "actor"
            | "import"
            | "typealias"
            | "associatedtype"
            | "inout"
            | "internal"
            | "public"
            | "private"
            | "fileprivate"
            | "open"
            | "static"
            | "is"
            | "as"
            | "Any"
            | "fallthrough"
            | "rethrows"
            | "throws"
            | "defer"
            | "async"
            | "await"
            | "some"
            | "any"
            | "borrowing"
            | "consuming"
            | "isolated"
            | "nonisolated"
    ) {
        format!("`{raw}`")
    } else {
        raw.to_owned()
    }
}

/// Compile all ActionScript source units, timeline frame scripts, and stage object scripts in
/// `project` into Swift 6 code and extract the `SwiftHost` proxy contract.
pub fn compile_project_scripts_to_swift(project: &Project) -> SwiftCompiledProject {
    let stage = ProjectStageIndex::build(project);
    let includes = AuthoredPlayback::include_lookup_or_empty(project);

    let mut host_methods = Vec::new();
    let mut host_method_map: BTreeMap<String, SwiftProxyMethod> = BTreeMap::new();
    let mut issues = Vec::new();
    let mut class_blocks = Vec::new();

    // 1. First pass over SourceUnits: extract `SwiftHost` contract first so calls across all scripts
    //    know the exact Swift parameter types of `SwiftHost` methods!
    let mut parsed_units: Vec<(usize, ParsedClass)> = Vec::new();
    for (idx, unit) in project.sources.iter().enumerate() {
        if !matches!(
            unit.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            issues.push(ConversionIssue {
                kind: ConversionIssueKind::ManualAttention,
                location: unit.path.clone(),
                line: 1,
                column: 1,
                message: format!(
                    "Source unit '{}' uses {:?}; Xcode/Swift export compiles ActionScript AST units.",
                    unit.path, unit.language
                ),
            });
            continue;
        }
        match parse_class_unit(unit, &includes) {
            Ok(parsed) => {
                if is_swift_host_class(Some(&unit.path), &parsed) {
                    let (extracted, contract_warnings) =
                        inspect_swift_host_class(&parsed, unit.language, Some(&unit.path));
                    for w in contract_warnings {
                        issues.push(ConversionIssue {
                            kind: ConversionIssueKind::ManualAttention,
                            location: unit.path.clone(),
                            line: w.line,
                            column: w.column,
                            message: w.message,
                        });
                    }
                    for m in extracted {
                        host_method_map.insert(m.name.clone(), m.clone());
                        host_methods.push(m);
                    }
                } else {
                    parsed_units.push((idx, parsed));
                }
            }
            Err(err) => {
                // Might be an `#include`-only snippet file rather than a `class`; skip if not a class declaration
                if unit.source.contains("class ") || unit.source.contains("interface ") {
                    issues.push(ConversionIssue {
                        kind: ConversionIssueKind::ManualAttention,
                        location: unit.path.clone(),
                        line: 1,
                        column: 1,
                        message: format!("Failed to parse class unit '{}': {err}", unit.path),
                    });
                }
            }
        }
    }

    // 1b. Discover implicit / ad-hoc Flash proxy calls (`SwiftHost.<fn>(...)`, `fscommand("<fn>", ...)`,
    //     `ExternalInterface.call("<fn>", ...)`) across all project classes, timeline frame scripts, and
    //     stage button/clip `ObjectScript` handlers so Xcode export immediately generates empty stub
    //     functions in `App/AppSwiftHost.swift` even without an explicit `SwiftHost.as` declaration!
    let discovered_proxies =
        discover_implicit_host_proxies(project, &parsed_units, &stage, &includes, &host_method_map);
    for m in discovered_proxies {
        host_method_map.insert(m.name.clone(), m.clone());
        host_methods.push(m);
    }

    // 2. Emit non-`SwiftHost` project classes into `source_classes_swift`
    for (idx, parsed) in &parsed_units {
        let unit = &project.sources[*idx];
        let (swift_cls, cls_issues) =
            emit_swift_class(parsed, &unit.path, &stage, &host_method_map);
        issues.extend(cls_issues);
        if !swift_cls.trim().is_empty() {
            class_blocks.push(swift_cls);
        }
    }

    // 3. Compile each Symbol's timeline properties, frame scripts, helper functions, and object handlers
    let mut used_swift_names = BTreeSet::new();
    let mut compiled_symbols = Vec::new();

    for sym in &project.symbols {
        let base_cls_name = if sym.id == project.root {
            "MainTimeline".to_owned()
        } else if let Some(cls) = sym.class_name.as_deref().filter(|s| !s.trim().is_empty()) {
            sanitize_swift_type_ident(short_type_name(cls))
        } else {
            sanitize_swift_type_ident(&sym.name)
        };
        let mut swift_class_name = base_cls_name.clone();
        let mut suffix = 2usize;
        while !used_swift_names.insert(swift_class_name.clone()) {
            swift_class_name = format!("{base_cls_name}_{suffix}");
            suffix += 1;
        }

        let (compiled_sym, sym_issues) = compile_symbol_to_swift(
            project,
            sym,
            swift_class_name,
            &stage,
            &includes,
            &host_method_map,
        );
        issues.extend(sym_issues);
        compiled_symbols.push(compiled_sym);
    }

    SwiftCompiledProject {
        host_methods,
        symbols: compiled_symbols,
        source_classes_swift: class_blocks.join("\n\n"),
        issues,
    }
}

trait IncludeLookupExt {
    fn include_lookup_or_empty(project: &Project) -> BTreeMap<String, ResourceUnit>;
}

impl IncludeLookupExt for AuthoredPlayback {
    fn include_lookup_or_empty(project: &Project) -> BTreeMap<String, ResourceUnit> {
        let mut map = BTreeMap::new();
        for res in &project.resources {
            let norm = crate::modules::normalize_module_path(&res.path);
            if !norm.is_empty() {
                map.insert(norm, res.clone());
            }
        }
        for unit in &project.sources {
            let norm = crate::modules::normalize_module_path(&unit.path);
            if !norm.is_empty()
                && !map.contains_key(&norm)
                && let Ok(res) = ResourceUnit::from_bytes(
                    unit.id.clone(),
                    unit.path.clone(),
                    "text/plain",
                    unit.source.as_bytes(),
                )
            {
                map.insert(norm, res);
            }
        }
        map
    }
}

fn compile_symbol_to_swift(
    _project: &Project,
    sym: &Symbol,
    swift_class_name: String,
    stage: &ProjectStageIndex,
    includes: &BTreeMap<String, ResourceUnit>,
    host_methods: &BTreeMap<String, SwiftProxyMethod>,
) -> (SwiftCompiledSymbol, Vec<ConversionIssue>) {
    let mut issues = Vec::new();
    let mut prop_lines = Vec::new();
    let mut prop_types: BTreeMap<String, HirType> = BTreeMap::new();

    // 1. Stage child properties (`scoreText: NFTextField`, `hero: NFMovieClip`) are preallocated
    //    and bound to the live timeline by `scene_codegen.rs` (`bindPreallocatedNamedChild`), so
    //    we register their types here without redeclaring optional duplicates in `properties_swift`.
    let mut stage_child_props: BTreeSet<String> = BTreeSet::new();
    if let Some(children) = stage.symbol_children.get(&sym.id) {
        for (child_name, info) in children {
            prop_types.insert(child_name.clone(), info.hir_type.clone());
            stage_child_props.insert(child_name.clone());
        }
    }

    // 2. Parse all timeline frame scripts first to lift top-level `var`s & `function`s onto symbol scope
    let mut parsed_frames: Vec<(u32, String, ParsedScript)> = Vec::new();
    let mut lifted_functions: BTreeMap<String, (Function, ScriptLanguage, String, String)> =
        BTreeMap::new();

    for entry in &sym.timeline.scripts {
        if entry.script.source.trim().is_empty() {
            continue;
        }
        let loc = format!("{}[frame {}]", sym.name, entry.frame + 1);
        if !matches!(
            entry.script.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            issues.push(ConversionIssue {
                kind: ConversionIssueKind::ManualAttention,
                location: loc,
                line: 1,
                column: 1,
                message: "Non-ActionScript frame script skipped in Swift AST compiler.".into(),
            });
            continue;
        }
        match parse_timeline_script(&entry.script, "", includes) {
            Ok(parsed) => {
                let mut prelim_env = TypeEnv::new(Some(sym.id.clone()), None);
                for (k, v) in &prop_types {
                    prelim_env.define(k.clone(), v.clone());
                }
                for stmt in &parsed.statements {
                    match stmt.unlocated() {
                        Stmt::Var(vars) => {
                            for (vname, vty, val_opt) in vars {
                                if prop_types.contains_key(vname) {
                                    continue;
                                }
                                let inferred = vty
                                    .as_deref()
                                    .map(HirType::from_annotation)
                                    .unwrap_or_else(|| {
                                        val_opt.as_ref().map_or(HirType::Unknown, |e| {
                                            prelim_env.infer_expr(e, stage)
                                        })
                                    });
                                let escaped = escape_swift_ident(vname);
                                let (swift_ty, def_val) =
                                    swift_property_type_and_default(&inferred);
                                prop_lines
                                    .push(format!("public var {escaped}: {swift_ty} = {def_val}"));
                                prelim_env.define(vname.clone(), inferred.clone());
                                prop_types.insert(vname.clone(), inferred);
                            }
                        }
                        Stmt::Function(f) => {
                            if let Some(fname) = &f.name {
                                lifted_functions.entry(fname.clone()).or_insert_with(|| {
                                    (
                                        (**f).clone(),
                                        parsed.language,
                                        loc.clone(),
                                        parsed.expanded_source.clone(),
                                    )
                                });
                            }
                        }
                        _ => {}
                    }
                }
                parsed_frames.push((entry.frame, loc, parsed));
            }
            Err(err) => {
                issues.push(ConversionIssue {
                    kind: ConversionIssueKind::ManualAttention,
                    location: loc,
                    line: 1,
                    column: 1,
                    message: format!("Failed to parse frame script: {err}"),
                });
            }
        }
    }

    let helper_names: BTreeSet<String> = lifted_functions.keys().cloned().collect();
    let mut helper_return_types: BTreeMap<String, HirType> = BTreeMap::new();
    for (fname, (f, _, _, _)) in &lifted_functions {
        let rty = f
            .returns
            .as_deref()
            .map_or(HirType::Void, HirType::from_annotation);
        helper_return_types.insert(fname.clone(), rty);
    }

    // 3. Emit lifted helper functions (`@MainActor func relayout(...)`)
    let mut helper_blocks = Vec::new();
    for (fname, (f, lang, loc, raw_src)) in &lifted_functions {
        let mut emitter = SwiftEmitter::new(
            loc,
            raw_src,
            *lang == ScriptLanguage::ActionScript3,
            stage,
            host_methods,
            Some(sym.id.clone()),
            None,
        );
        emitter.set_instance_props(prop_types.clone());
        emitter.stage_child_props = stage_child_props.clone();
        emitter.instance_methods = helper_names.clone();
        for (hn, hr) in &helper_return_types {
            emitter.env.functions.insert(hn.clone(), hr.clone());
        }
        let code = emitter.emit_helper_method(fname, f);
        issues.extend(emitter.issues);
        helper_blocks.push(code);
    }

    // 4. Emit frame methods (`frame_0`, `frame_1`, ...)
    let mut frame_methods_swift = BTreeMap::new();
    let mut key_shim_emitted_on_symbol = false;

    for (frame_idx, loc, parsed) in &parsed_frames {
        let mut emitter = SwiftEmitter::new(
            loc,
            &parsed.expanded_source,
            parsed.language == ScriptLanguage::ActionScript3,
            stage,
            host_methods,
            Some(sym.id.clone()),
            None,
        );
        emitter.set_instance_props(prop_types.clone());
        emitter.stage_child_props = stage_child_props.clone();
        emitter.instance_methods = helper_names.clone();
        emitter.top_level_is_symbol_frame = true;
        for (hn, hr) in &helper_return_types {
            emitter.env.functions.insert(hn.clone(), hr.clone());
        }
        emitter.precollect(&parsed.statements);

        let needs_key_shim = parsed.language != ScriptLanguage::ActionScript3
            && stmts_use_key_is_down(&parsed.statements);
        emitter.use_key_down_shim = needs_key_shim;
        if needs_key_shim && !key_shim_emitted_on_symbol {
            key_shim_emitted_on_symbol = true;
            prop_lines.push("private var _keysDown: Set<UInt> = []".into());
            helper_blocks.push(
                "@MainActor private func _isKeyDown(_ code: Int) -> Bool {\n    return self._keysDown.contains(UInt(max(0, code)))\n}".into(),
            );
        }

        emitter.indent = 1;
        let mut body_parts = Vec::new();
        if needs_key_shim {
            let pad = emitter.pad();
            body_parts.push(format!(
                "{pad}self.stage?.addEventListener(\"keyDown\") {{ [weak self] event in\n\
                 {pad}    guard let self else {{ return }}\n\
                 {pad}    self._keysDown.insert(event.keyCode)\n\
                 {pad}}}\n\
                 {pad}self.stage?.addEventListener(\"keyUp\") {{ [weak self] event in\n\
                 {pad}    guard let self else {{ return }}\n\
                 {pad}    self._keysDown.remove(event.keyCode)\n\
                 {pad}}}"
            ));
        }

        let rendered = emitter.emit_stmt_list(&parsed.statements);
        if !rendered.trim().is_empty() {
            body_parts.push(rendered);
        }
        issues.extend(emitter.issues);

        let method_code = if body_parts.is_empty() {
            format!("@MainActor public func frame_{frame_idx}() {{}}")
        } else {
            format!(
                "@MainActor public func frame_{frame_idx}() {{\n{}\n}}",
                body_parts.join("\n")
            )
        };
        frame_methods_swift.insert(*frame_idx, method_code);
    }

    // 5. Emit stage instance `ObjectScript` handlers (including classic AS1/AS2 `on(release)` /
    //    `onClipEvent(...)` blocks and unnamed Stage buttons bound by `elementId`)
    let mut instance_handlers_swift = Vec::new();
    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            for el in &kf.elements {
                if el.scripts.is_empty() {
                    continue;
                }
                let target_key = if el.name.is_empty() {
                    el.id.clone()
                } else {
                    el.name.clone()
                };
                for os in &el.scripts {
                    if os.script.source.trim().is_empty() {
                        continue;
                    }
                    let loc = format!("{}::{}::{:?}", sym.name, target_key, os.event);
                    if let Ok(parsed) = parse_object_script_for_swift(&os.script, includes) {
                        let mut handler_blocks: Vec<(ObjectEvent, Vec<Stmt>)> = Vec::new();
                        let mut direct_stmts: Vec<Stmt> = Vec::new();

                        for stmt in &parsed.statements {
                            if let Stmt::LegacyEvent(triggers, func) = stmt.unlocated() {
                                let mapped_ev = map_legacy_triggers_to_object_event(triggers)
                                    .unwrap_or(os.event);
                                handler_blocks.push((mapped_ev, func.body.clone()));
                            } else {
                                direct_stmts.push(stmt.clone());
                            }
                        }
                        if !direct_stmts.is_empty() {
                            handler_blocks.push((os.event, direct_stmts));
                        }

                        for (ev, body_stmts) in handler_blocks {
                            let mut emitter = SwiftEmitter::new(
                                &loc,
                                &parsed.expanded_source,
                                parsed.language == ScriptLanguage::ActionScript3,
                                stage,
                                host_methods,
                                Some(sym.id.clone()),
                                None,
                            );
                            emitter.set_instance_props(prop_types.clone());
                            emitter.stage_child_props = stage_child_props.clone();
                            emitter.instance_methods = helper_names.clone();
                            for (hn, hr) in &helper_return_types {
                                emitter.env.functions.insert(hn.clone(), hr.clone());
                            }
                            emitter.indent = 0;
                            let body = emitter.emit_stmt_list(&body_stmts);
                            issues.extend(emitter.issues);
                            if !body.trim().is_empty() || el.scripts.len() == 1 {
                                instance_handlers_swift.push((target_key.clone(), ev, body));
                            }
                        }
                    }
                }
            }
        }
    }

    (
        SwiftCompiledSymbol {
            symbol_id: sym.id.clone(),
            swift_class_name,
            properties_swift: prop_lines.join("\n"),
            frame_methods_swift,
            helper_methods_swift: helper_blocks.join("\n\n"),
            instance_handlers_swift,
        },
        issues,
    )
}

fn swift_property_type_and_default(ty: &HirType) -> (String, &'static str) {
    match ty {
        HirType::Int => ("Int".into(), "0"),
        HirType::UInt => ("UInt".into(), "0"),
        HirType::Float => ("Double".into(), "0.0"),
        HirType::Bool => ("Bool".into(), "false"),
        HirType::String => ("String".into(), "\"\""),
        HirType::Array | HirType::Vector(_) => ("NFValue".into(), "[]"),
        HirType::Dictionary => ("NFValue".into(), "[:]"),
        HirType::Object | HirType::Unknown | HirType::Function => ("NFValue".into(), ".undefined"),
        other => (format!("{}?", other.to_swift_type()), "nil"),
    }
}

struct SwiftEmitter<'a> {
    location: &'a str,
    raw_source: &'a str,
    source_is_as3: bool,
    stage: &'a ProjectStageIndex,
    host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
    env: TypeEnv,
    instance_props: BTreeMap<String, HirType>,
    stage_child_props: BTreeSet<String>,
    static_props: BTreeSet<String>,
    instance_methods: BTreeSet<String>,
    static_methods: BTreeSet<String>,
    current_class_short: Option<String>,
    class_has_stage: bool,
    top_level_is_symbol_frame: bool,
    use_key_down_shim: bool,
    in_key_handler_event: Option<String>,
    local_scopes: Vec<BTreeSet<String>>,
    key_down_handlers: BTreeMap<String, Expr>,
    key_up_handlers: BTreeMap<String, Expr>,
    resize_handlers: BTreeMap<String, Expr>,
    explicit_key_listeners: BTreeSet<String>,
    explicit_stage_listeners: BTreeSet<String>,
    issues: Vec<ConversionIssue>,
    indent: usize,
}

impl<'a> SwiftEmitter<'a> {
    fn new(
        location: &'a str,
        raw_source: &'a str,
        source_is_as3: bool,
        stage: &'a ProjectStageIndex,
        host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
        symbol_id: Option<String>,
        class_name: Option<String>,
    ) -> Self {
        Self {
            location,
            raw_source,
            source_is_as3,
            stage,
            host_methods,
            env: TypeEnv::new(symbol_id, class_name),
            instance_props: BTreeMap::new(),
            stage_child_props: BTreeSet::new(),
            static_props: BTreeSet::new(),
            instance_methods: BTreeSet::new(),
            static_methods: BTreeSet::new(),
            current_class_short: None,
            class_has_stage: true,
            top_level_is_symbol_frame: false,
            use_key_down_shim: false,
            in_key_handler_event: None,
            local_scopes: vec![BTreeSet::new()],
            key_down_handlers: BTreeMap::new(),
            key_up_handlers: BTreeMap::new(),
            resize_handlers: BTreeMap::new(),
            explicit_key_listeners: BTreeSet::new(),
            explicit_stage_listeners: BTreeSet::new(),
            issues: Vec::new(),
            indent: 0,
        }
    }

    fn pad(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn set_instance_props(&mut self, props: BTreeMap<String, HirType>) {
        for (k, v) in &props {
            self.env.define(k.clone(), v.clone());
        }
        self.instance_props = props;
    }

    fn push_scope(&mut self) {
        self.env.push_scope();
        self.local_scopes.push(BTreeSet::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop_scope();
        if self.local_scopes.len() > 1 {
            self.local_scopes.pop();
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.local_scopes.iter().any(|s| s.contains(name))
    }

    fn mark_local(&mut self, name: &str) {
        if let Some(top) = self.local_scopes.last_mut() {
            top.insert(name.to_owned());
        }
    }

    fn record_issue(
        &mut self,
        kind: ConversionIssueKind,
        offset: usize,
        message: impl Into<String>,
    ) {
        let (line, column) = byte_offset_to_line_col(self.raw_source, offset);
        self.issues.push(ConversionIssue {
            kind,
            location: self.location.to_owned(),
            line,
            column,
            message: message.into(),
        });
    }

    fn host_receiver(&self) -> &'static str {
        if self.class_has_stage {
            "self.stage?.host?"
        } else {
            "NFRuntime.activeHost?"
        }
    }

    fn emit_host_proxy_call_inner(
        &mut self,
        method: &str,
        payload_args: &[Expr],
        offset: usize,
    ) -> (String, Option<String>) {
        let m_esc = escape_swift_ident(method);
        let contract = self.host_methods.get(method).cloned();
        let mut call_args = Vec::new();
        let param_len = contract.as_ref().map_or(payload_args.len(), |c| {
            c.params.len().max(payload_args.len())
        });
        for idx in 0..param_len {
            if let Some(spec) = contract.as_ref().and_then(|c| c.params.get(idx)) {
                let label = escape_swift_ident(&spec.name);
                if let Some(arg) = payload_args.get(idx) {
                    let expected = match spec.swift_type.as_str() {
                        "Int" => HirType::Int,
                        "UInt" => HirType::UInt,
                        "Double" => HirType::Float,
                        "Bool" => HirType::Bool,
                        "String" => HirType::String,
                        "[NFValue]" => HirType::Array,
                        _ => HirType::Unknown,
                    };
                    let coerced = if spec.swift_type == "[NFValue]" {
                        if let Expr::Array(items) = arg {
                            let parts: Vec<String> = items
                                .iter()
                                .map(|it| {
                                    it.as_ref()
                                        .map(|e| self.wrap_nfvalue(e, offset))
                                        .unwrap_or_else(|| ".undefined".into())
                                })
                                .collect();
                            format!("[{}]", parts.join(", "))
                        } else {
                            self.emit_expr_with_offset(arg, offset)
                        }
                    } else if spec.swift_type == "[String: NFValue]" {
                        self.emit_expr_with_offset(arg, offset)
                    } else {
                        self.emit_coerced_expr(arg, &expected, offset)
                    };
                    call_args.push(format!("{label}: {coerced}"));
                } else {
                    let def_expr = swift_default_for_proxy_type(&spec.swift_type);
                    call_args.push(format!("{label}: {def_expr}"));
                }
            } else if let Some(arg) = payload_args.get(idx) {
                call_args.push(self.emit_expr_with_offset(arg, offset));
            }
        }
        let recv = self.host_receiver();
        let ret_ty = contract.and_then(|c| c.return_swift_type);
        (format!("{recv}.{m_esc}({})", call_args.join(", ")), ret_ty)
    }

    fn infer(&self, expr: &Expr) -> HirType {
        match expr {
            Expr::Call(callee, args, _) => {
                if let Some((method, _)) = match_host_proxy_call(callee, args)
                    && let Some(spec) = self.host_methods.get(&method)
                {
                    return match spec.return_swift_type.as_deref() {
                        None => HirType::Void,
                        Some("Bool") => HirType::Bool,
                        Some("Int") => HirType::Int,
                        Some("UInt") => HirType::UInt,
                        Some("Double") => HirType::Float,
                        Some("String") => HirType::String,
                        Some("[NFValue]") => HirType::Array,
                        _ => HirType::Unknown,
                    };
                }
            }
            Expr::Name(n) => {
                if let Some(t) = self.env.lookup(n) {
                    return t.clone();
                }
                if let Some(t) = self.instance_props.get(n) {
                    return t.clone();
                }
            }
            Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
                if let Some(t) = self.instance_props.get(p) {
                    return t.clone();
                }
            }
            Expr::Member(b, p) if is_external_interface_base(b) && p == "available" => {
                return HirType::Bool;
            }
            _ => {}
        }
        self.env.infer_expr(expr, self.stage)
    }

    fn precollect(&mut self, stmts: &[Stmt]) {
        if self.source_is_as3 {
            return;
        }
        for stmt in stmts {
            if let Stmt::Expr(Expr::Call(callee, args, _)) = stmt.unlocated()
                && args.len() == 1
                && let Expr::Member(base, method) = callee.as_ref()
                && method == "addListener"
                && let Expr::Name(mgr) = base.as_ref()
                && let Some(key) = listener_target_key(&args[0])
            {
                if mgr == "Key" {
                    self.explicit_key_listeners.insert(key);
                } else if mgr == "Stage" {
                    self.explicit_stage_listeners.insert(key);
                }
            }
        }
    }

    fn emit_helper_method(&mut self, raw_name: &str, f: &Function) -> String {
        let fname = escape_swift_ident(raw_name);
        let saved_top = self.top_level_is_symbol_frame;
        self.top_level_is_symbol_frame = false;

        let is_key_fn = f.params.first().is_some_and(|(_, ty)| {
            ty.as_deref()
                .is_some_and(|t| short_type_name(t) == "KeyboardEvent")
        });
        let saved_ke = self.in_key_handler_event.clone();
        if is_key_fn {
            self.in_key_handler_event = f.params.first().map(|(n, _)| n.clone());
        }

        self.push_scope();
        let params_sig = self.emit_swift_params(f, f.offset);
        let ret_hir = f
            .returns
            .as_deref()
            .map_or(HirType::Void, HirType::from_annotation);
        let (discardable, ret_clause) = if ret_hir == HirType::Void {
            ("", String::new())
        } else {
            (
                "@discardableResult ",
                format!(" -> {}", ret_hir.to_swift_type()),
            )
        };

        self.indent = 1;
        let body = self.emit_stmt_list(&f.body);
        self.indent = 0;
        self.pop_scope();
        self.in_key_handler_event = saved_ke;
        self.top_level_is_symbol_frame = saved_top;

        if body.trim().is_empty() {
            format!("{discardable}@MainActor public func {fname}({params_sig}){ret_clause} {{}}")
        } else {
            format!(
                "{discardable}@MainActor public func {fname}({params_sig}){ret_clause} {{\n{body}\n}}"
            )
        }
    }

    fn emit_swift_params(&mut self, f: &Function, offset: usize) -> String {
        let mut out = Vec::new();
        for (i, (raw_p, pty)) in f.params.iter().enumerate() {
            let pname = escape_swift_ident(raw_p);
            let hir_ty = pty
                .as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
            let swift_ty = hir_ty.to_swift_type();
            self.mark_local(raw_p);
            self.env.define(raw_p.clone(), hir_ty.clone());
            if let Some(Some(def)) = f.defaults.get(i) {
                let dc = self.emit_coerced_expr(def, &hir_ty, offset);
                out.push(format!("_ {pname}: {swift_ty} = {dc}"));
            } else {
                out.push(format!("_ {pname}: {swift_ty}"));
            }
        }
        if let Some(rest) = &f.rest {
            let rname = escape_swift_ident(rest);
            self.mark_local(rest);
            self.env.define(rest.clone(), HirType::Array);
            out.push(format!("_ {rname}: NFValue..."));
        }
        out.join(", ")
    }

    fn emit_stmt_list(&mut self, stmts: &[Stmt]) -> String {
        let mut lines = Vec::new();
        for stmt in stmts {
            let s = self.emit_stmt(stmt);
            if !s.trim().is_empty() {
                lines.push(s);
            }
        }
        lines.join("\n")
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> String {
        let offset = match stmt {
            Stmt::Located(off, _) => *off,
            _ => 0,
        };
        let pad = self.pad();
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.emit_stmt(inner),
            Stmt::Empty => String::new(),
            Stmt::Block(body) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                self.indent += 1;
                let inner = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                format!("{pad}do {{\n{inner}\n{pad}}}")
            }
            Stmt::Var(vars) => {
                let mut out = Vec::new();
                for (raw_name, raw_ty, val_opt) in vars {
                    // Top-level timeline frame `var` declarations were lifted to `self.<prop>`!
                    if self.top_level_is_symbol_frame && self.instance_props.contains_key(raw_name)
                    {
                        if let Some(val) = val_opt {
                            let target_ty = self
                                .instance_props
                                .get(raw_name)
                                .cloned()
                                .unwrap_or(HirType::Unknown);
                            let escaped = escape_swift_ident(raw_name);
                            if !self.source_is_as3
                                && let Some(clip_init) = self.try_emit_dynamic_clip_swift(
                                    &format!("self.{escaped}"),
                                    val,
                                    offset,
                                )
                            {
                                out.push(clip_init);
                                continue;
                            }
                            let rhs = self.emit_coerced_expr(val, &target_ty, offset);
                            out.push(format!("{pad}self.{escaped} = {rhs}"));
                        }
                        continue;
                    }

                    let vname = escape_swift_ident(raw_name);
                    if !self.source_is_as3
                        && let Some(val) = val_opt
                        && let Some(clip_init) =
                            self.try_emit_dynamic_clip_swift(&format!("let {vname}"), val, offset)
                    {
                        self.mark_local(raw_name);
                        self.env.define(raw_name.clone(), HirType::Sprite);
                        out.push(clip_init);
                        continue;
                    }

                    let decl_ty = raw_ty
                        .as_deref()
                        .map(HirType::from_annotation)
                        .unwrap_or_else(|| {
                            val_opt.as_ref().map_or(HirType::Unknown, |e| self.infer(e))
                        });
                    let swift_ty = decl_ty.to_swift_type();
                    let already = self.is_local(raw_name);
                    self.mark_local(raw_name);
                    self.env.define(raw_name.clone(), decl_ty.clone());

                    if let Some(val) = val_opt {
                        let rhs = self.emit_coerced_expr(val, &decl_ty, offset);
                        if already {
                            out.push(format!("{pad}{vname} = {rhs}"));
                        } else {
                            out.push(format!("{pad}var {vname}: {swift_ty} = {rhs}"));
                        }
                    } else if !already {
                        let (_, def_val) = swift_property_type_and_default(&decl_ty);
                        out.push(format!("{pad}var {vname}: {swift_ty} = {def_val}"));
                    }
                }
                out.join("\n")
            }
            Stmt::Function(f) => {
                // Top-level timeline functions are already emitted in `helper_methods_swift`
                if self.top_level_is_symbol_frame && f.name.is_some() {
                    return String::new();
                }
                let raw_name = f.name.as_deref().unwrap_or("_localFn");
                let fname = escape_swift_ident(raw_name);
                self.mark_local(raw_name);
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                let params = self.emit_swift_params(f, offset);
                let ret_hir = f
                    .returns
                    .as_deref()
                    .map_or(HirType::Void, HirType::from_annotation);
                let ret_str = if ret_hir == HirType::Void {
                    String::new()
                } else {
                    format!(" -> {}", ret_hir.to_swift_type())
                };
                self.indent += 1;
                let body = self.emit_stmt_list(&f.body);
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                format!("{pad}func {fname}({params}){ret_str} {{\n{body}\n{pad}}}")
            }
            Stmt::Expr(expr) => {
                if let Some(special) = self.try_emit_special_swift_stmt(expr, offset) {
                    return special;
                }
                let s = self.emit_expr_with_offset(expr, offset);
                format!("{pad}{s}")
            }
            Stmt::If(cond, yes, no) => {
                let c = self.emit_bool_cond(cond, offset);
                let yb = self.emit_swift_branch(yes);
                if let Some(no_stmt) = no {
                    if matches!(no_stmt.unlocated(), Stmt::If(_, _, _)) {
                        let nested = self.emit_stmt(no_stmt);
                        format!("{pad}if {c} {yb} else {}", nested.trim_start())
                    } else {
                        let nb = self.emit_swift_branch(no_stmt);
                        format!("{pad}if {c} {yb} else {nb}")
                    }
                } else {
                    format!("{pad}if {c} {yb}")
                }
            }
            Stmt::While(cond, body) => {
                let c = self.emit_bool_cond(cond, offset);
                let b = self.emit_swift_branch(body);
                format!("{pad}while {c} {b}")
            }
            Stmt::DoWhile(body, cond) => {
                let b = self.emit_swift_branch(body);
                let c = self.emit_bool_cond(cond, offset);
                format!("{pad}repeat {b} while {c}")
            }
            Stmt::For(init, cond, update, body) => {
                // Swift 6 removed C-style `for (init; cond; update)` -> lower deterministically to `do { init; while cond { body; update } }`!
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                let mut lines = Vec::new();
                if !matches!(init.unlocated(), Stmt::Empty) {
                    let init_s = self.emit_stmt(init);
                    if !init_s.trim().is_empty() {
                        lines.push(init_s);
                    }
                }
                let cond_s = cond
                    .as_ref()
                    .map(|c| self.emit_bool_cond(c, offset))
                    .unwrap_or_else(|| "true".into());
                self.indent += 1;
                let inner_pad = self.pad();
                let body_s = match body.unlocated() {
                    Stmt::Block(b) => self.emit_stmt_list(b),
                    other => self.emit_stmt(other),
                };
                let mut loop_parts = Vec::new();
                if !body_s.trim().is_empty() {
                    loop_parts.push(body_s);
                }
                if let Some(upd) = update {
                    let u = self.emit_stmt(&Stmt::Expr(upd.clone()));
                    if !u.trim().is_empty() {
                        loop_parts.push(format!("{inner_pad}{}", u.trim_start()));
                    }
                }
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;

                lines.push(format!(
                    "{pad}while {cond_s} {{\n{}\n{pad}}}",
                    loop_parts.join("\n")
                ));
                lines.join("\n")
            }
            Stmt::ForIn(init, coll, is_each, body) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                let var_name = match init.unlocated() {
                    Stmt::Var(vars) if !vars.is_empty() => {
                        let n = &vars[0].0;
                        self.mark_local(n);
                        self.env.define(n.clone(), HirType::Unknown);
                        escape_swift_ident(n)
                    }
                    Stmt::Expr(Expr::Name(n)) => escape_swift_ident(n),
                    _ => "_item".into(),
                };
                let coll_s = self.emit_expr_with_offset(coll, offset);
                let b = self.emit_swift_branch(body);
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                if *is_each {
                    format!("{pad}for {var_name} in {coll_s} {b}")
                } else {
                    format!("{pad}for {var_name} in NFRuntime.keys({coll_s}) {b}")
                }
            }
            Stmt::Switch(disc, cases) => {
                let d = self.emit_expr_with_offset(disc, offset);
                let mut lines = vec![format!("{pad}switch {d} {{")];
                let mut has_default = false;
                for Case { test, body } in cases {
                    if let Some(t) = test {
                        lines.push(format!(
                            "{pad}case {}:",
                            self.emit_expr_with_offset(t, offset)
                        ));
                    } else {
                        has_default = true;
                        lines.push(format!("{pad}default:"));
                    }
                    self.indent += 1;
                    let bpad = self.pad();
                    let filtered: Vec<&Stmt> = body
                        .iter()
                        .filter(|s| !matches!(s.unlocated(), Stmt::Break(None)))
                        .collect();
                    if filtered.is_empty() {
                        lines.push(format!("{bpad}break"));
                    } else {
                        for s in filtered {
                            let rs = self.emit_stmt(s);
                            if !rs.trim().is_empty() {
                                lines.push(rs);
                            }
                        }
                    }
                    self.indent -= 1;
                }
                if !has_default {
                    lines.push(format!("{pad}default:\n{pad}    break"));
                }
                lines.push(format!("{pad}}}"));
                lines.join("\n")
            }
            Stmt::Break(lbl) => match lbl {
                Some(l) => format!("{pad}break {}", escape_swift_ident(l)),
                None => format!("{pad}break"),
            },
            Stmt::Continue(lbl) => match lbl {
                Some(l) => format!("{pad}continue {}", escape_swift_ident(l)),
                None => format!("{pad}continue"),
            },
            Stmt::Return(val, _) => match val {
                Some(v) => format!("{pad}return {}", self.emit_expr_with_offset(v, offset)),
                None => format!("{pad}return"),
            },
            Stmt::Throw(val) => {
                format!(
                    "{pad}NFRuntime.throwError({})",
                    self.emit_expr_with_offset(val, offset)
                )
            }
            Stmt::Try(body, catches, fin) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                let mut out = Vec::new();
                if let Some(fin_body) = fin {
                    self.push_scope();
                    self.indent += 1;
                    let fb = self.emit_stmt_list(fin_body);
                    self.indent -= 1;
                    self.pop_scope();
                    out.push(format!("{pad}defer {{\n{fb}\n{pad}}}"));
                }
                self.push_scope();
                self.indent += 1;
                let b = self.emit_stmt_list(body);
                self.indent -= 1;
                self.pop_scope();
                if let Some(CatchClause {
                    name, body: cbody, ..
                }) = catches.first()
                {
                    let cname = escape_swift_ident(name);
                    self.push_scope();
                    self.mark_local(name);
                    self.indent += 1;
                    let cb = self.emit_stmt_list(cbody);
                    self.indent -= 1;
                    self.pop_scope();
                    out.push(format!(
                        "{pad}do {{\n{b}\n{pad}}} catch let {cname} {{\n{cb}\n{pad}}}"
                    ));
                } else {
                    out.push(format!("{pad}do {{\n{b}\n{pad}}}"));
                }
                self.top_level_is_symbol_frame = saved_top;
                out.join("\n")
            }
            Stmt::Label(lbl, inner) => {
                let s = self.emit_stmt(inner);
                format!("{pad}{}: {}", escape_swift_ident(lbl), s.trim_start())
            }
            Stmt::With(target, body, _) | Stmt::TellTarget(target, body, _) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Dynamic 'with' / 'tellTarget' scope requires explicit receiver references in Swift 6.",
                );
                let tgt = self.emit_expr_with_offset(target, offset);
                let b = self.emit_swift_branch(body);
                format!("{pad}/* FIXME [NeoFlash Swift]: with ({tgt}) */ do {b}")
            }
            Stmt::DefaultXmlNamespace(_) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X 'default xml namespace' is not supported on the native Swift target.",
                );
                format!("{pad}/* FIXME [NeoFlash Swift]: default xml namespace */")
            }
            Stmt::IfFrameLoaded(_, body) => self.emit_stmt(body),
            Stmt::InitClip(_, body, _) => self.emit_stmt_list(body),
            Stmt::LegacyEvent(_, _) => String::new(),
        }
    }

    fn emit_swift_branch(&mut self, stmt: &Stmt) -> String {
        let saved_top = self.top_level_is_symbol_frame;
        self.top_level_is_symbol_frame = false;
        self.push_scope();
        self.indent += 1;
        let inner = match stmt.unlocated() {
            Stmt::Block(b) => self.emit_stmt_list(b),
            other => self.emit_stmt(other),
        };
        self.indent -= 1;
        self.pop_scope();
        self.top_level_is_symbol_frame = saved_top;
        let pad = self.pad();
        if inner.trim().is_empty() {
            "{}".into()
        } else {
            format!("{{\n{inner}\n{pad}}}")
        }
    }

    fn try_emit_dynamic_clip_swift(
        &mut self,
        lhs_binding: &str,
        val: &Expr,
        offset: usize,
    ) -> Option<String> {
        let Expr::Call(callee, args, _) = val else {
            return None;
        };
        let pad = self.pad();
        if let Some(parent_expr) = match_clip_factory(callee, "createEmptyMovieClip")
            && !args.is_empty()
        {
            let parent_sw = self.emit_expr_with_offset(&parent_expr, offset);
            let name_sw = self.emit_expr_with_offset(&args[0], offset);
            let var_ref = lhs_binding.strip_prefix("let ").unwrap_or(lhs_binding);
            return Some(format!(
                "{pad}{lhs_binding} = NFMovieClip(name: {name_sw})\n\
                 {pad}{parent_sw}.addChild({var_ref})"
            ));
        }
        None
    }

    fn try_emit_special_swift_stmt(&mut self, expr: &Expr, offset: usize) -> Option<String> {
        let pad = self.pad();

        // 1. Update statements (`i++` / `++i` -> `i += 1`, `mc._alpha++` -> `mc.alpha += 0.01`)
        if let Expr::Update(op, target, _) = expr {
            let assign_op = if op == "++" { "+=" } else { "-=" };
            if !self.source_is_as3
                && let Some((base_sw, modern)) = self.match_as2_percent_lvalue_swift(target, offset)
            {
                return Some(format!("{pad}{base_sw}.{modern} {assign_op} 0.01"));
            }
            let t = self.emit_lvalue_swift(target, offset);
            return Some(format!("{pad}{t} {assign_op} 1"));
        }

        // 2. AS3 `if (stage != null)` or `stage.addEventListener(...)` / `addEventListener(...)`
        if let Expr::Call(callee, args, call_off) = expr
            && let Some(listener_stmt) = self.try_emit_add_event_listener(callee, args, *call_off)
        {
            return Some(listener_stmt);
        }

        // 3. AS1/AS2 `this.onEnterFrame = ...`, `this.onKeyDown = ...`, `onResize = ...`
        if !self.source_is_as3
            && let Expr::Assign(op, lhs, rhs) = expr
            && op == "="
            && let Some((target_expr, cb_prop)) = extract_as2_callback_target(lhs)
        {
            match cb_prop {
                "onEnterFrame" => {
                    let tgt = self.emit_listener_receiver(&target_expr, offset);
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!(
                        "{pad}{tgt}.addEventListener(\"enterFrame\") {closure}"
                    ));
                }
                "onRelease" | "onPress" | "onRollOver" | "onRollOut" => {
                    let ev = match cb_prop {
                        "onRelease" => "click",
                        "onPress" => "mouseDown",
                        "onRollOver" => "rollOver",
                        _ => "rollOut",
                    };
                    let tgt = self.emit_listener_receiver(&target_expr, offset);
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!("{pad}{tgt}.addEventListener(\"{ev}\") {closure}"));
                }
                "onKeyDown" | "onKeyUp" => {
                    let is_down = cb_prop == "onKeyDown";
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_key_listeners.contains(&key);
                    if is_down {
                        self.key_down_handlers.insert(key, (**rhs).clone());
                    } else {
                        self.key_up_handlers.insert(key, (**rhs).clone());
                    }
                    if has_explicit {
                        return Some(String::new());
                    }
                    let ev = if is_down { "keyDown" } else { "keyUp" };
                    let closure = self.emit_weak_self_closure(rhs, true, offset);
                    return Some(format!(
                        "{pad}self.stage?.addEventListener(\"{ev}\") {closure}"
                    ));
                }
                "onResize" => {
                    let key = listener_target_key(&target_expr).unwrap_or_else(|| "this".into());
                    let has_explicit = self.explicit_stage_listeners.contains(&key);
                    self.resize_handlers.insert(key, (**rhs).clone());
                    if has_explicit {
                        return Some(String::new());
                    }
                    let closure = self.emit_weak_self_closure(rhs, false, offset);
                    return Some(format!(
                        "{pad}self.stage?.addEventListener(\"resize\") {closure}"
                    ));
                }
                _ => {}
            }
        }

        // 4. AS2 `Key.addListener(obj)` & `Stage.addListener(obj)`
        if !self.source_is_as3
            && let Expr::Call(callee, args, _) = expr
            && args.len() == 1
            && let Expr::Member(base, method) = callee.as_ref()
            && method == "addListener"
            && let Expr::Name(mgr) = base.as_ref()
        {
            let key = listener_target_key(&args[0]).unwrap_or_else(|| "this".into());
            if mgr == "Key" {
                let mut lines = Vec::new();
                if let Some(kd_rhs) = self.key_down_handlers.get(&key).cloned() {
                    let closure = self.emit_weak_self_closure(&kd_rhs, true, offset);
                    lines.push(format!(
                        "{pad}self.stage?.addEventListener(\"keyDown\") {closure}"
                    ));
                }
                if let Some(ku_rhs) = self.key_up_handlers.get(&key).cloned() {
                    let closure = self.emit_weak_self_closure(&ku_rhs, true, offset);
                    lines.push(format!(
                        "{pad}self.stage?.addEventListener(\"keyUp\") {closure}"
                    ));
                }
                return Some(lines.join("\n"));
            }
            if mgr == "Stage"
                && let Some(rsz_rhs) = self.resize_handlers.get(&key).cloned()
            {
                let closure = self.emit_weak_self_closure(&rsz_rhs, false, offset);
                return Some(format!(
                    "{pad}self.stage?.addEventListener(\"resize\") {closure}"
                ));
            }
        }

        // 5. Statement-level Flash-to-Swift proxy calls (`SwiftHost.fn(...)`, `fscommand("fn", ...)`,
        //    `ExternalInterface.call("fn", ...)`) emit clean one-line optional-chained calls without `??`
        if let Expr::Call(callee, args, call_off) = expr {
            if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
                let (call_str, ret_ty) =
                    self.emit_host_proxy_call_inner(&method, payload_args, *call_off);
                if ret_ty.is_some() {
                    return Some(format!("{pad}_ = {call_str}"));
                }
                return Some(format!("{pad}{call_str}"));
            }
            if matches!(callee.as_ref(), Expr::Name(n) if n == "fscommand" && !self.is_local(n)) {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    *call_off,
                    "Dynamic 'fscommand(...)' requires a string literal command name to generate a SwiftHostProtocol stub.",
                );
                let raw_args = self.emit_args(args, *call_off);
                return Some(format!(
                    "{pad}/* FIXME [NeoFlash Swift]: dynamic fscommand({raw_args}) */"
                ));
            }
            if let Expr::Member(base, m) = callee.as_ref()
                && is_external_interface_base(base)
            {
                if m == "call" {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *call_off,
                        "Dynamic 'ExternalInterface.call(...)' requires a string literal function name to generate a SwiftHostProtocol stub.",
                    );
                    let raw_args = self.emit_args(args, *call_off);
                    return Some(format!(
                        "{pad}/* FIXME [NeoFlash Swift]: dynamic ExternalInterface.call({raw_args}) */"
                    ));
                }
                if m == "addCallback" {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        *call_off,
                        "'ExternalInterface.addCallback' is inbound from host; call public methods on MainScene directly from Swift.",
                    );
                    let raw_args = self.emit_args(args, *call_off);
                    return Some(format!(
                        "{pad}/* FIXME [NeoFlash Swift]: ExternalInterface.addCallback({raw_args}) */"
                    ));
                }
            }
        }

        None
    }

    fn try_emit_add_event_listener(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        offset: usize,
    ) -> Option<String> {
        if args.len() < 2 {
            return None;
        }
        let receiver_sw = match callee {
            Expr::Name(n) if n == "addEventListener" => "self".to_owned(),
            Expr::Member(base, m) if m == "addEventListener" => {
                self.emit_listener_receiver(base, offset)
            }
            _ => return None,
        };
        let pad = self.pad();
        let ev_str = if let Some(ev) = classify_as3_event_type(&args[0]) {
            format!("\"{ev}\"")
        } else {
            self.emit_expr_with_offset(&args[0], offset)
        };
        let is_key = ev_str == "\"keyDown\"" || ev_str == "\"keyUp\"";
        let closure = self.emit_weak_self_closure(&args[1], is_key, offset);
        Some(format!(
            "{pad}{receiver_sw}.addEventListener({ev_str}) {closure}"
        ))
    }

    fn emit_listener_receiver(&mut self, expr: &Expr, offset: usize) -> String {
        if is_stage_expr(expr) {
            return "self.stage?".into();
        }
        if matches!(expr, Expr::Name(n) if n == "this") {
            return "self".into();
        }
        if let Expr::Name(n) = expr
            && !self.is_local(n)
            && let Some(ty) = self.instance_props.get(n)
        {
            let esc = escape_swift_ident(n);
            if ty.is_nullable_object() && !self.stage_child_props.contains(n) {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        if let Expr::Member(b, p) = expr
            && matches!(b.as_ref(), Expr::Name(n) if n == "this")
            && let Some(ty) = self.instance_props.get(p)
        {
            let esc = escape_swift_ident(p);
            if ty.is_nullable_object() && !self.stage_child_props.contains(p) {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        self.emit_expr_with_offset(expr, offset)
    }

    /// Emit an ARC-safe `{ [weak self] event in guard let self else { return } ... }` listener closure!
    fn emit_weak_self_closure(
        &mut self,
        rhs: &Expr,
        is_key_handler: bool,
        offset: usize,
    ) -> String {
        let pad = self.pad();
        if let Expr::Function(f) = rhs {
            let ev_param = f
                .params
                .first()
                .map(|(n, _)| escape_swift_ident(n))
                .unwrap_or_else(|| "event".into());
            let saved_ke = self.in_key_handler_event.clone();
            if is_key_handler {
                self.in_key_handler_event = Some(ev_param.clone());
            }
            let saved_top = self.top_level_is_symbol_frame;
            self.top_level_is_symbol_frame = false;
            self.push_scope();
            self.mark_local(&ev_param);
            self.env
                .define(ev_param.clone(), HirType::Class("Event".into()));
            self.indent += 1;
            let in_pad = self.pad();
            let body = self.emit_stmt_list(&f.body);
            self.indent -= 1;
            self.pop_scope();
            self.top_level_is_symbol_frame = saved_top;
            self.in_key_handler_event = saved_ke;
            if body.trim().is_empty() {
                format!(
                    "{{ [weak self] {ev_param} in\n{in_pad}guard let self else {{ return }}\n{pad}}}"
                )
            } else {
                format!(
                    "{{ [weak self] {ev_param} in\n{in_pad}guard let self else {{ return }}\n{body}\n{pad}}}"
                )
            }
        } else {
            let target_call = match rhs {
                Expr::Name(fn_name) if !self.is_local(fn_name) => {
                    format!("self.{}(event)", escape_swift_ident(fn_name))
                }
                Expr::Member(b, m) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
                    format!("self.{}(event)", escape_swift_ident(m))
                }
                other => format!("{}(event)", self.emit_expr_with_offset(other, offset)),
            };
            format!(
                "{{ [weak self] event in\n{pad}    guard let self else {{ return }}\n{pad}    {target_call}\n{pad}}}"
            )
        }
    }

    fn emit_bool_cond(&mut self, cond: &Expr, offset: usize) -> String {
        if is_stage_expr(cond) {
            return "self.stage != nil".into();
        }
        let ty = self.infer(cond);
        let s = self.emit_expr_with_offset(cond, offset);
        if ty == HirType::Bool {
            s
        } else if matches!(ty, HirType::Object | HirType::Unknown) {
            // Preserve boolean comparisons directly when AST is already a relational/logical expression
            if matches!(
                cond,
                Expr::Binary(op, _, _)
                    if matches!(
                        op.as_str(),
                        "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "and" | "or"
                    )
            ) || matches!(cond, Expr::Unary(op, _) if op == "!" || op == "not")
            {
                s
            } else {
                format!("NFRuntime.isTruthy({s})")
            }
        } else {
            s
        }
    }

    fn is_nfvalue_type(ty: &HirType) -> bool {
        matches!(
            ty,
            HirType::Object
                | HirType::Unknown
                | HirType::Array
                | HirType::Vector(_)
                | HirType::Dictionary
                | HirType::Function
        )
    }

    fn expr_produces_nfvalue(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Array(_) | Expr::Object(_) | Expr::NewVector(_, _, _) => true,
            Expr::Index(base, _) => Self::is_nfvalue_type(&self.infer(base)),
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return false;
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    if let Some(ty) = self.instance_props.get(prop) {
                        return Self::is_nfvalue_type(ty);
                    }
                    return false;
                }
                if let Expr::Name(cls) = base.as_ref()
                    && matches!(
                        cls.as_str(),
                        "Math"
                            | "Key"
                            | "Keyboard"
                            | "Event"
                            | "MouseEvent"
                            | "KeyboardEvent"
                            | "Stage"
                    )
                {
                    return false;
                }
                Self::is_nfvalue_type(&self.infer(base))
            }
            Expr::Name(n) if !self.is_local(n) => {
                if let Some(ty) = self.instance_props.get(n) {
                    return Self::is_nfvalue_type(ty);
                }
                Self::is_nfvalue_type(&self.infer(expr))
            }
            _ => Self::is_nfvalue_type(&self.infer(expr)),
        }
    }

    fn lvalue_expects_nfvalue(&self, lhs: &Expr) -> bool {
        match lhs {
            Expr::Index(base, _) => Self::is_nfvalue_type(&self.infer(base)),
            Expr::Member(base, prop) => {
                if is_stage_expr(base) {
                    return false;
                }
                if matches!(base.as_ref(), Expr::Name(n) if n == "this") {
                    if let Some(ty) = self.instance_props.get(prop) {
                        return Self::is_nfvalue_type(ty);
                    }
                    return false;
                }
                if let Expr::Name(cls) = base.as_ref()
                    && cls == "Stage"
                {
                    return false;
                }
                Self::is_nfvalue_type(&self.infer(base))
            }
            Expr::Name(n) if !self.is_local(n) => {
                if let Some(ty) = self.instance_props.get(n) {
                    return Self::is_nfvalue_type(ty);
                }
                Self::is_nfvalue_type(&self.infer(lhs))
            }
            _ => Self::is_nfvalue_type(&self.infer(lhs)),
        }
    }

    fn wrap_nfvalue(&mut self, expr: &Expr, offset: usize) -> String {
        if self.expr_produces_nfvalue(expr) {
            return self.emit_expr_with_offset(expr, offset);
        }
        if let Expr::Literal(lit) = expr {
            if lit == "null" || lit == "undefined" {
                return "nil".into();
            }
            return lit.clone();
        }
        let inner = self.emit_expr_with_offset(expr, offset);
        if inner == "nil" || inner == ".undefined" || inner.starts_with("NFValue(") {
            inner
        } else {
            format!("NFValue({inner})")
        }
    }

    fn emit_coerced_expr(&mut self, expr: &Expr, expected: &HirType, offset: usize) -> String {
        if Self::is_nfvalue_type(expected) {
            return self.wrap_nfvalue(expr, offset);
        }
        let actual = self.infer(expr);
        let from_nfvalue = self.expr_produces_nfvalue(expr);
        if *expected == HirType::Float {
            if let Expr::Literal(lit) = expr
                && lit.parse::<i64>().is_ok()
                && !lit.starts_with("0x")
                && !lit.starts_with("0X")
            {
                return format!("{lit}.0");
            }
            if actual.is_integer() || from_nfvalue {
                return self.wrap_double(expr, offset);
            }
        }
        if *expected == HirType::Int && (actual == HirType::Float || from_nfvalue) {
            let inner = self.emit_expr_with_offset(expr, offset);
            return format!("Int({inner})");
        }
        if *expected == HirType::UInt
            && (actual == HirType::Float || actual == HirType::Int || from_nfvalue)
        {
            if let Expr::Literal(lit) = expr
                && (lit.parse::<u64>().is_ok() || lit.starts_with("0x") || lit.starts_with("0X"))
            {
                return lit.clone();
            }
            let inner = self.emit_expr_with_offset(expr, offset);
            return format!("UInt({inner})");
        }
        if *expected == HirType::String && from_nfvalue {
            let inner = self.emit_expr_with_offset(expr, offset);
            return format!("String({inner})");
        }
        if *expected == HirType::Bool && from_nfvalue {
            let inner = self.emit_subexpr(expr, 18, offset);
            return format!("{inner}.isTruthy");
        }
        self.emit_expr_with_offset(expr, offset)
    }

    fn wrap_double(&mut self, expr: &Expr, offset: usize) -> String {
        if let Expr::Call(callee, args, _) = expr
            && matches!(callee.as_ref(), Expr::Name(n) if n == "Number")
            && args.len() == 1
        {
            return format!("Double({})", self.emit_expr_with_offset(&args[0], offset));
        }
        let s = self.emit_expr_with_offset(expr, offset);
        if s.starts_with("Double(") && s.ends_with(')') {
            s
        } else {
            format!("Double({s})")
        }
    }

    fn wrap_int(&mut self, expr: &Expr, offset: usize) -> String {
        if let Expr::Literal(lit) = expr
            && (lit.parse::<i64>().is_ok() || lit.starts_with("0x") || lit.starts_with("0X"))
        {
            return lit.clone();
        }
        let s = self.emit_expr_with_offset(expr, offset);
        if s.starts_with("Int(") && s.ends_with(')') {
            s
        } else {
            format!("Int({s})")
        }
    }

    fn emit_expr_with_offset(&mut self, expr: &Expr, offset: usize) -> String {
        match expr {
            Expr::Literal(raw) => {
                if raw == "null" || raw == "undefined" {
                    "nil".into()
                } else {
                    raw.clone()
                }
            }
            Expr::RegExpLiteral(pat, flags) => {
                format!("NFRegExp(pattern: {:?}, flags: {:?})", pat, flags)
            }
            Expr::TypeValue(name, _) => HirType::from_annotation(name).to_swift_type(),
            Expr::Name(name) => self.emit_swift_ident(name),
            Expr::Member(base, prop) => self.emit_swift_member(base, prop, offset),
            Expr::Index(base, idx) => {
                let b = self.emit_subexpr(base, 18, offset);
                let i = self.emit_expr_with_offset(idx, offset);
                format!("{b}[{i}]")
            }
            Expr::Array(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|it| {
                        it.as_ref()
                            .map(|e| self.wrap_nfvalue(e, offset))
                            .unwrap_or_else(|| ".undefined".into())
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Expr::Object(props) => {
                if props.is_empty() {
                    return "[:]".into();
                }
                let parts: Vec<String> = props
                    .iter()
                    .map(|(PropertyKey { name, .. }, val)| {
                        format!("\"{name}\": {}", self.wrap_nfvalue(val, offset))
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Expr::Unary(op, inner) => {
                let sw_op = if op == "not" { "!" } else { op.as_str() };
                match sw_op {
                    "~" => format!("~{}", self.wrap_int(inner, offset)),
                    "typeof" => format!(
                        "NFRuntime.typeofValue({})",
                        self.emit_expr_with_offset(inner, offset)
                    ),
                    "void" => "()".into(),
                    _ => format!("{sw_op}{}", self.emit_subexpr(inner, 16, offset)),
                }
            }
            Expr::Delete(target, _) => {
                format!(
                    "NFRuntime.deleteProperty({})",
                    self.emit_expr_with_offset(target, offset)
                )
            }
            Expr::Update(op, target, _) => {
                let t = self.emit_lvalue_swift(target, offset);
                let delta = if op == "++" { "+ 1" } else { "- 1" };
                format!("({t} {delta})")
            }
            Expr::Binary(op, lhs, rhs) => self.emit_swift_binary(op, lhs, rhs, offset),
            Expr::Conditional(cond, yes, no) => {
                let c = self.emit_bool_cond(cond, offset);
                let y = self.emit_expr_with_offset(yes, offset);
                let n = self.emit_expr_with_offset(no, offset);
                format!("({c} ? {y} : {n})")
            }
            Expr::Assign(op, lhs, rhs) => {
                if !self.source_is_as3
                    && let Some((base_sw, modern)) =
                        self.match_as2_percent_lvalue_swift(lhs, offset)
                {
                    let is_nf = matches!(lhs.as_ref(), Expr::Member(b, _) if Self::is_nfvalue_type(&self.infer(b)));
                    match op.as_str() {
                        "=" => {
                            let r = self.emit_scaled_100_to_01_swift(rhs, offset);
                            if is_nf {
                                return format!("{base_sw}.{modern} = NFValue({r})");
                            }
                            return format!("{base_sw}.{modern} = {r}");
                        }
                        "+=" | "-=" => {
                            let r = self.emit_scaled_100_to_01_swift(rhs, offset);
                            if is_nf {
                                return format!("{base_sw}.{modern} {op} NFValue({r})");
                            }
                            return format!("{base_sw}.{modern} {op} {r}");
                        }
                        _ => {}
                    }
                }
                let l_ty = if self.lvalue_expects_nfvalue(lhs) {
                    HirType::Unknown
                } else {
                    self.infer(lhs)
                };
                let l = self.emit_lvalue_swift(lhs, offset);
                let r = self.emit_coerced_expr(rhs, &l_ty, offset);
                format!("{l} {op} {r}")
            }
            Expr::Call(callee, args, call_off) => self.emit_swift_call(callee, args, *call_off),
            Expr::NewError(ty, args, call_off)
            | Expr::NewBuiltin(ty, args, call_off)
            | Expr::NewClass(ty, args, call_off)
            | Expr::NewValue(ty, args, call_off) => self.emit_swift_new(ty, args, *call_off),
            Expr::NewVector(_, args, call_off) => {
                if args.len() == 1
                    && let Expr::Array(items) = &args[0]
                {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|it| {
                            it.as_ref()
                                .map(|e| self.emit_expr_with_offset(e, *call_off))
                                .unwrap_or_else(|| ".undefined".into())
                        })
                        .collect();
                    format!("[{}]", parts.join(", "))
                } else {
                    "[]".into()
                }
            }
            Expr::InstanceOf(lhs, rhs) => {
                let l = self.emit_expr_with_offset(lhs, offset);
                let r = self.emit_expr_with_offset(rhs, offset);
                format!("({l} is {r})")
            }
            Expr::TypeTest(op, inner, ty_name, tt_off) => {
                let inner_sw = self.emit_expr_with_offset(inner, *tt_off);
                let sw_ty = HirType::from_annotation(ty_name).to_swift_type();
                if op == "as" {
                    format!("({inner_sw} as? {sw_ty})")
                } else {
                    format!("({inner_sw} is {sw_ty})")
                }
            }
            Expr::Function(f) => {
                let saved_top = self.top_level_is_symbol_frame;
                self.top_level_is_symbol_frame = false;
                self.push_scope();
                let params: Vec<String> = f
                    .params
                    .iter()
                    .map(|(n, _)| {
                        self.mark_local(n);
                        escape_swift_ident(n)
                    })
                    .collect();
                self.indent += 1;
                let body = self.emit_stmt_list(&f.body);
                self.indent -= 1;
                self.pop_scope();
                self.top_level_is_symbol_frame = saved_top;
                let pad = self.pad();
                if params.is_empty() {
                    format!(
                        "{{ [weak self] in\n{pad}    guard let self else {{ return }}\n{body}\n{pad}}}"
                    )
                } else {
                    format!(
                        "{{ [weak self] {} in\n{pad}    guard let self else {{ return }}\n{body}\n{pad}}}",
                        params.join(", ")
                    )
                }
            }
            Expr::XmlLiteral(raw, _) => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "E4X XML literals are not supported on the native Swift target.",
                );
                format!("/* FIXME [NeoFlash Swift]: E4X XML literal {raw} */ NFValue.null")
            }
            Expr::XmlSelect {
                base,
                local,
                attribute,
                descendant,
                ..
            } => {
                let b = self.emit_expr_with_offset(base, offset);
                let l = self.emit_expr_with_offset(local, offset);
                let op_sym = if *descendant { ".." } else { "." };
                let at_sym = if *attribute { "@" } else { "" };
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("E4X XML operator '{op_sym}{at_sym}{l}' is not supported on the native Swift target."),
                );
                format!(
                    "/* FIXME [NeoFlash Swift]: E4X {b}{op_sym}{at_sym}{l} */ NFValue.undefined"
                )
            }
            Expr::XmlWithAttr {
                local,
                attribute,
                descendant,
                ..
            } => {
                let l = self.emit_expr_with_offset(local, offset);
                let op_sym = if *descendant { ".." } else { "" };
                let at_sym = if *attribute { "@" } else { "" };
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("E4X XML selector '{op_sym}{at_sym}{l}' is not supported on the native Swift target."),
                );
                format!("/* FIXME [NeoFlash Swift]: E4X {op_sym}{at_sym}{l} */ NFValue.undefined")
            }
            Expr::XmlFilter(base, predicate, x_off) => {
                let b = self.emit_expr_with_offset(base, *x_off);
                let p = self.emit_expr_with_offset(predicate, *x_off);
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    *x_off,
                    "E4X XML filter predicate '.(...)' is not supported on the native Swift target.",
                );
                format!("/* FIXME [NeoFlash Swift]: E4X {b}.({p}) */ NFValue.undefined")
            }
        }
    }

    fn emit_swift_binary(&mut self, op: &str, lhs: &Expr, rhs: &Expr, offset: usize) -> String {
        // Special pattern: `(expr) | 0` integer truncation -> `Int(Double(a) / Double(b))` or `Int(expr)`!
        if op == "|" && matches!(rhs, Expr::Literal(s) if s == "0") {
            let inner = self.emit_expr_with_offset(lhs, offset);
            return format!("Int({inner})");
        }
        if op == ">>>" && matches!(rhs, Expr::Literal(s) if s == "0") {
            let inner = self.emit_expr_with_offset(lhs, offset);
            return format!("UInt({inner})");
        }

        // Division `a / b` ALWAYS promotes to floating-point `Double(a) / Double(b)` in ActionScript!
        if op == "/" {
            let ld = self.wrap_double(lhs, offset);
            let rd = self.wrap_double(rhs, offset);
            return format!("{ld} / {rd}");
        }

        // Bitwise operators `&`, `|`, `^`, `<<`, `>>`, `>>>` coerce operands via `Int(...)`!
        if matches!(op, "&" | "|" | "^" | "<<" | ">>" | ">>>") {
            let sw_op = if op == ">>>" { ">>" } else { op };
            let li = self.wrap_int(lhs, offset);
            let ri = self.wrap_int(rhs, offset);
            return format!("{li} {sw_op} {ri}");
        }

        let lt = self.infer(lhs);
        let rt = self.infer(rhs);

        // String concatenation `+` when either operand is String
        if (op == "+" || op == "add") && (lt == HirType::String || rt == HirType::String) {
            let ls = if lt == HirType::String {
                self.emit_subexpr(lhs, 12, offset)
            } else {
                format!(
                    "String(describing: {})",
                    self.emit_expr_with_offset(lhs, offset)
                )
            };
            let rs = if rt == HirType::String {
                self.emit_subexpr(rhs, 12, offset)
            } else {
                format!(
                    "String(describing: {})",
                    self.emit_expr_with_offset(rhs, offset)
                )
            };
            return format!("{ls} + {rs}");
        }

        // Mixed `Int`/`UInt` + `Double` arithmetic or comparison promotes integer via `Double(...)`!
        if matches!(
            op,
            "+" | "-" | "*" | "%" | "<" | "<=" | ">" | ">=" | "==" | "!=" | "===" | "!=="
        ) && !is_null_or_undef(lhs)
            && !is_null_or_undef(rhs)
        {
            let mixed_float = (lt == HirType::Float && rt.is_integer())
                || (rt == HirType::Float && lt.is_integer());
            if op == "%" && (lt == HirType::Float || rt == HirType::Float) {
                let ld = if lt == HirType::Float {
                    self.emit_subexpr(lhs, 18, offset)
                } else {
                    self.wrap_double(lhs, offset)
                };
                let rd = if rt == HirType::Float {
                    self.emit_expr_with_offset(rhs, offset)
                } else {
                    self.wrap_double(rhs, offset)
                };
                return format!("{ld}.truncatingRemainder(dividingBy: {rd})");
            }
            if mixed_float {
                let cmp_op = match op {
                    "===" => "==",
                    "!==" => "!=",
                    other => other,
                };
                let prec = op_prec(cmp_op);
                let l = if lt.is_integer() {
                    self.wrap_double(lhs, offset)
                } else {
                    self.emit_subexpr(lhs, prec, offset)
                };
                let r = if rt.is_integer() {
                    self.wrap_double(rhs, offset)
                } else {
                    self.emit_subexpr(rhs, prec, offset)
                };
                return format!("{l} {cmp_op} {r}");
            }
            let mixed_int_uint =
                (lt == HirType::UInt && rt == HirType::Int && !is_nonneg_int_literal(rhs))
                    || (lt == HirType::Int && rt == HirType::UInt && !is_nonneg_int_literal(lhs));
            if mixed_int_uint {
                let cmp_op = match op {
                    "===" => "==",
                    "!==" => "!=",
                    other => other,
                };
                let prec = op_prec(cmp_op);
                let l = if lt == HirType::UInt {
                    self.wrap_int(lhs, offset)
                } else {
                    self.emit_subexpr(lhs, prec, offset)
                };
                let r = if rt == HirType::UInt {
                    self.wrap_int(rhs, offset)
                } else {
                    self.emit_subexpr(rhs, prec, offset)
                };
                return format!("{l} {cmp_op} {r}");
            }
        }

        // Null checks (`x != null` / `x == undefined`) -> `x != nil` / `x == nil`
        if matches!(op, "==" | "!=" | "===" | "!==") {
            let is_eq = matches!(op, "==" | "===");
            let cmp = if is_eq { "==" } else { "!=" };
            if is_null_or_undef(rhs) {
                let l = self.emit_subexpr(lhs, 9, offset);
                return format!("{l} {cmp} nil");
            }
            if is_null_or_undef(lhs) {
                let r = self.emit_subexpr(rhs, 9, offset);
                return format!("{r} {cmp} nil");
            }
        }

        let sw_op = match op {
            "and" => "&&",
            "or" => "||",
            "eq" | "===" => "==",
            "ne" | "<>" | "!==" => "!=",
            other => other,
        };
        let prec = op_prec(sw_op);
        let l = self.emit_subexpr(lhs, prec, offset);
        let r = self.emit_subexpr(rhs, prec, offset);
        format!("{l} {sw_op} {r}")
    }

    fn emit_swift_ident(&mut self, name: &str) -> String {
        match name {
            "this" => return "self".into(),
            "null" | "undefined" => return "nil".into(),
            "true" | "false" => return name.to_owned(),
            "NaN" => return "Double.nan".into(),
            "Infinity" => return "Double.infinity".into(),
            _ => {}
        }

        if self.is_local(name) {
            return escape_swift_ident(name);
        }

        if self.instance_props.contains_key(name) || self.instance_methods.contains(name) {
            return format!("self.{}", escape_swift_ident(name));
        }

        if let Some(cls) = &self.current_class_short
            && (self.static_props.contains(name) || self.static_methods.contains(name))
        {
            return format!("{cls}.{}", escape_swift_ident(name));
        }

        if !self.source_is_as3 {
            if let Some(modern) = map_as2_percent_prop_to_modern(name) {
                return format!("(self.{modern} * 100.0)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(name) {
                return format!("self.{modern}");
            }
        }

        if matches!(
            name,
            "stage"
                | "parent"
                | "graphics"
                | "x"
                | "y"
                | "scaleX"
                | "scaleY"
                | "alpha"
                | "visible"
                | "rotation"
                | "width"
                | "height"
                | "currentFrame"
                | "totalFrames"
        ) {
            return format!("self.{name}");
        }

        escape_swift_ident(name)
    }

    fn emit_lvalue_swift(&mut self, lhs: &Expr, offset: usize) -> String {
        if let Expr::Name(name) = lhs {
            if self.is_local(name) {
                return escape_swift_ident(name);
            }
            if self.instance_props.contains_key(name) {
                let esc = escape_swift_ident(name);
                return format!("self.{esc}");
            } else if let Some(cls) = &self.current_class_short
                && self.static_props.contains(name)
            {
                return format!("{cls}.{}", escape_swift_ident(name));
            }
            if !self.source_is_as3
                && let Some(modern) = map_as2_direct_prop_to_modern(name)
            {
                return format!("self.{modern}");
            }
            if matches!(
                name.as_str(),
                "x" | "y"
                    | "scaleX"
                    | "scaleY"
                    | "alpha"
                    | "visible"
                    | "rotation"
                    | "width"
                    | "height"
                    | "name"
            ) {
                return format!("self.{name}");
            }
            return escape_swift_ident(name);
        }

        if let Expr::Member(base, prop) = lhs {
            if is_stage_expr(base) {
                return format!("self.stage?.{}", escape_swift_ident(prop));
            }
            if !self.source_is_as3
                && let Expr::Name(cls) = base.as_ref()
                && cls == "Stage"
            {
                return format!("self.stage?.{}", escape_swift_ident(prop));
            }
            let b = self.emit_optional_chain_base(base, offset);
            let mapped = if !self.source_is_as3 {
                map_as2_direct_prop_to_modern(prop)
                    .map(str::to_owned)
                    .unwrap_or_else(|| escape_swift_ident(prop))
            } else {
                escape_swift_ident(prop)
            };
            return format!("{b}.{mapped}");
        }

        self.emit_expr_with_offset(lhs, offset)
    }

    fn emit_optional_chain_base(&mut self, base: &Expr, offset: usize) -> String {
        if is_stage_expr(base) {
            return "self.stage?".into();
        }
        if let Expr::Name(n) = base
            && !self.is_local(n)
            && let Some(ty) = self.instance_props.get(n)
        {
            let esc = escape_swift_ident(n);
            if matches!(
                ty,
                HirType::TextField
                    | HirType::MovieClip
                    | HirType::Sprite
                    | HirType::Button
                    | HirType::Class(_)
            ) && !self.stage_child_props.contains(n)
            {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        if let Expr::Member(b, p) = base
            && matches!(b.as_ref(), Expr::Name(n) if n == "this")
            && let Some(ty) = self.instance_props.get(p)
        {
            let esc = escape_swift_ident(p);
            if matches!(
                ty,
                HirType::TextField
                    | HirType::MovieClip
                    | HirType::Sprite
                    | HirType::Button
                    | HirType::Class(_)
            ) && !self.stage_child_props.contains(p)
            {
                return format!("self.{esc}?");
            }
            return format!("self.{esc}");
        }
        self.emit_subexpr(base, 18, offset)
    }

    fn match_as2_percent_lvalue_swift(
        &mut self,
        expr: &Expr,
        offset: usize,
    ) -> Option<(String, &'static str)> {
        match expr {
            Expr::Member(base, prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                let b = self.emit_optional_chain_base(base, offset);
                Some((b, modern))
            }
            Expr::Name(prop) if !self.is_local(prop) => {
                let modern = map_as2_percent_prop_to_modern(prop)?;
                Some(("self".into(), modern))
            }
            _ => None,
        }
    }

    fn emit_scaled_100_to_01_swift(&mut self, expr: &Expr, offset: usize) -> String {
        if let Some(folded) = fold_100_to_01(expr, true) {
            return folded;
        }
        let d = self.wrap_double(expr, offset);
        if is_atomic_expr(expr) {
            format!("{d} / 100.0")
        } else {
            format!("({d}) / 100.0")
        }
    }

    fn emit_swift_member(&mut self, base: &Expr, prop: &str, offset: usize) -> String {
        if is_external_interface_base(base) && prop == "available" {
            return "true".into();
        }

        if let Expr::Name(cls) = base {
            if cls == "Key"
                && let Some((_, code)) = map_as2_key_constant(prop)
            {
                return format!("UInt({code})");
            }
            if cls == "Keyboard"
                && let Some((_, code)) = map_as3_keyboard_constant(prop)
            {
                return format!("UInt({code})");
            }
            if let Some(ev) = classify_as3_event_type(&Expr::Member(
                Box::new(Expr::Name(cls.clone())),
                prop.to_owned(),
            )) {
                return format!("\"{ev}\"");
            }
            if cls == "StageScaleMode" {
                return match prop {
                    "NO_SCALE" => "\"noScale\"".into(),
                    "SHOW_ALL" => "\"showAll\"".into(),
                    "EXACT_FIT" => "\"exactFit\"".into(),
                    "NO_BORDER" => "\"noBorder\"".into(),
                    _ => format!("\"{prop}\""),
                };
            }
            if cls == "StageAlign" {
                return match prop {
                    "TOP_LEFT" => "\"TL\"".into(),
                    "TOP" => "\"T\"".into(),
                    "TOP_RIGHT" => "\"TR\"".into(),
                    "LEFT" => "\"L\"".into(),
                    "RIGHT" => "\"R\"".into(),
                    "BOTTOM_LEFT" => "\"BL\"".into(),
                    "BOTTOM" => "\"B\"".into(),
                    "BOTTOM_RIGHT" => "\"BR\"".into(),
                    _ => format!("\"{prop}\""),
                };
            }
            if !self.source_is_as3 && cls == "Stage" {
                return match prop {
                    "width" => "(self.stage?.stageWidth ?? 0.0)".into(),
                    "height" => "(self.stage?.stageHeight ?? 0.0)".into(),
                    "scaleMode" => "(self.stage?.scaleMode ?? \"showAll\")".into(),
                    "align" => "(self.stage?.align ?? \"\")".into(),
                    _ => format!("self.stage?.{}", escape_swift_ident(prop)),
                };
            }
            if cls == "Math" && prop == "PI" {
                return "Double.pi".into();
            }
        }

        if is_stage_expr(base) {
            if prop == "stage3Ds" {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "Stage3D is not supported on the native Swift 2D scene target.",
                );
                return "/* FIXME [NeoFlash Swift]: stage.stage3Ds */ []".into();
            }
            return match prop {
                "stageWidth" | "stageHeight" => format!("(self.stage?.{prop} ?? 0.0)"),
                _ => format!("self.stage?.{}", escape_swift_ident(prop)),
            };
        }

        let b = self.emit_optional_chain_base(base, offset);
        if !self.source_is_as3 {
            if let Some(modern) = map_as2_percent_prop_to_modern(prop) {
                return format!("(({b}.{modern}) * 100.0)");
            }
            if let Some(modern) = map_as2_direct_prop_to_modern(prop) {
                return format!("{b}.{modern}");
            }
        }
        format!("{b}.{}", escape_swift_ident(prop))
    }

    fn emit_swift_call(&mut self, callee: &Expr, args: &[Expr], offset: usize) -> String {
        // 0. `super(...)` constructor chaining -> `super.init(...)`
        if matches!(callee, Expr::Name(n) if n == "super") {
            return format!("super.init({})", self.emit_args(args, offset));
        }

        // 1. `SwiftHost.foo(a, b)`, `fscommand("foo", a)`, `ExternalInterface.call("foo", a)` -> `self.stage?.host?.foo(...)`!
        if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
            let (call_str, ret_ty) = self.emit_host_proxy_call_inner(&method, payload_args, offset);
            if let Some(rt) = ret_ty {
                let def_expr = swift_default_for_proxy_type(&rt);
                return format!("({call_str} ?? {def_expr})");
            }
            return call_str;
        }
        if matches!(callee, Expr::Name(n) if n == "fscommand" && !self.is_local(n)) {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                "Dynamic 'fscommand(...)' requires a string literal command name to generate a SwiftHostProtocol stub.",
            );
            return format!(
                "/* FIXME [NeoFlash Swift]: fscommand({}) */ NFValue.undefined",
                self.emit_args(args, offset)
            );
        }
        if let Expr::Member(base, m) = callee
            && is_external_interface_base(base)
            && matches!(m.as_str(), "call" | "addCallback")
        {
            self.record_issue(
                ConversionIssueKind::ManualAttention,
                offset,
                format!("'ExternalInterface.{m}(...)' requires a static method contract or direct MainScene call in Swift."),
            );
            return format!(
                "/* FIXME [NeoFlash Swift]: ExternalInterface.{m}({}) */ NFValue.undefined",
                self.emit_args(args, offset)
            );
        }

        // 2. Type casts (`int(expr)`, `uint(expr)`, `Number(expr)`, `String(expr)`, `Boolean(expr)`) and `eval`
        if let Expr::Name(fn_name) = callee {
            match fn_name.as_str() {
                "int" if args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("Int({inner})");
                }
                "uint" if args.len() == 1 => {
                    let inner = self.emit_expr_with_offset(&args[0], offset);
                    return format!("UInt({inner})");
                }
                "Number" if args.len() == 1 => {
                    return self.wrap_double(&args[0], offset);
                }
                "String" if args.len() == 1 => {
                    return format!(
                        "String(describing: {})",
                        self.emit_expr_with_offset(&args[0], offset)
                    );
                }
                "Boolean" if args.len() == 1 => {
                    return self.emit_bool_cond(&args[0], offset);
                }
                "trace" => {
                    return format!("NFRuntime.trace({})", self.emit_args(args, offset));
                }
                "eval" => {
                    self.record_issue(
                        ConversionIssueKind::ManualAttention,
                        offset,
                        "'eval()' is not supported on the native Swift target.",
                    );
                    return format!(
                        "/* FIXME [NeoFlash Swift]: eval({}) */ NFValue.undefined",
                        self.emit_args(args, offset)
                    );
                }
                "stop" | "play" | "nextFrame" | "prevFrame" | "gotoAndPlay" | "gotoAndStop"
                | "addChild" | "removeChild"
                    if !self.is_local(fn_name) =>
                {
                    return format!("self.{fn_name}({})", self.emit_args(args, offset));
                }
                other if !self.is_local(other) && self.instance_methods.contains(other) => {
                    return format!(
                        "self.{}({})",
                        escape_swift_ident(other),
                        self.emit_args(args, offset)
                    );
                }
                _ => {}
            }
        }

        // 3. `Math.*`, `Key.*`, AS2 drawing API
        if let Expr::Member(base, method) = callee {
            if let Expr::Name(cls) = base.as_ref() {
                if cls == "Math" {
                    match method.as_str() {
                        "floor" | "ceil" | "round" | "sin" | "cos" | "tan" | "sqrt" | "exp"
                        | "log"
                            if args.len() == 1 =>
                        {
                            let a = self.wrap_double(&args[0], offset);
                            return format!("Foundation.{method}({a})");
                        }
                        "abs" if args.len() == 1 => {
                            return format!(
                                "abs({})",
                                self.emit_expr_with_offset(&args[0], offset)
                            );
                        }
                        "min" | "max" if args.len() == 2 => {
                            let a = self.wrap_double(&args[0], offset);
                            let b = self.wrap_double(&args[1], offset);
                            return format!("{method}({a}, {b})");
                        }
                        "pow" if args.len() == 2 => {
                            let a = self.wrap_double(&args[0], offset);
                            let b = self.wrap_double(&args[1], offset);
                            return format!("Foundation.pow({a}, {b})");
                        }
                        "atan2" if args.len() == 2 => {
                            let a = self.wrap_double(&args[0], offset);
                            let b = self.wrap_double(&args[1], offset);
                            return format!("Foundation.atan2({a}, {b})");
                        }
                        "random" if args.is_empty() => {
                            return "Double.random(in: 0.0..<1.0)".into();
                        }
                        _ => {}
                    }
                }
                if !self.source_is_as3 && cls == "Key" {
                    match method.as_str() {
                        "getCode" => {
                            if let Some(ev) = &self.in_key_handler_event {
                                return format!("{ev}.keyCode");
                            }
                            return "0".into();
                        }
                        "isDown" if args.len() == 1 => {
                            let code = self.wrap_int(&args[0], offset);
                            if self.use_key_down_shim {
                                return format!("self._isKeyDown({code})");
                            }
                            return format!("(self.stage?.isKeyDown({code}) ?? false)");
                        }
                        _ => {}
                    }
                }
            }

            if matches!(base.as_ref(), Expr::Member(_, g) if g == "graphics")
                && matches!(
                    method.as_str(),
                    "moveTo"
                        | "lineTo"
                        | "curveTo"
                        | "cubicCurveTo"
                        | "drawRect"
                        | "drawCircle"
                        | "drawEllipse"
                )
            {
                let c = self.emit_subexpr(callee, 18, offset);
                let a = args
                    .iter()
                    .map(|arg| self.emit_coerced_expr(arg, &HirType::Float, offset))
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("{c}({a})");
            }

            if !self.source_is_as3
                && matches!(
                    method.as_str(),
                    "clear"
                        | "beginFill"
                        | "lineStyle"
                        | "moveTo"
                        | "lineTo"
                        | "curveTo"
                        | "endFill"
                )
                && !matches!(base.as_ref(), Expr::Member(_, g) if g == "graphics")
            {
                let b = self.emit_optional_chain_base(base, offset);
                let mapped_args = match method.as_str() {
                    "beginFill" if args.len() >= 2 => format!(
                        "{}, {}",
                        self.emit_expr_with_offset(&args[0], offset),
                        self.emit_scaled_100_to_01_swift(&args[1], offset)
                    ),
                    "lineStyle" if args.len() >= 3 => format!(
                        "{}, {}, {}",
                        self.emit_coerced_expr(&args[0], &HirType::Float, offset),
                        self.emit_expr_with_offset(&args[1], offset),
                        self.emit_scaled_100_to_01_swift(&args[2], offset)
                    ),
                    "lineStyle" if !args.is_empty() => {
                        let mut parts =
                            vec![self.emit_coerced_expr(&args[0], &HirType::Float, offset)];
                        for rest in &args[1..] {
                            parts.push(self.emit_expr_with_offset(rest, offset));
                        }
                        parts.join(", ")
                    }
                    "moveTo" | "lineTo" | "curveTo" => args
                        .iter()
                        .map(|arg| self.emit_coerced_expr(arg, &HirType::Float, offset))
                        .collect::<Vec<_>>()
                        .join(", "),
                    _ => self.emit_args(args, offset),
                };
                return format!("{b}.graphics.{method}({mapped_args})");
            }
        }

        let c = self.emit_subexpr(callee, 18, offset);
        let a = self.emit_args(args, offset);
        format!("{c}({a})")
    }

    fn emit_swift_new(&mut self, ty: &str, args: &[Expr], offset: usize) -> String {
        let short = short_type_name(ty);
        match short {
            "ByteArray" => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    "flash.utils.ByteArray is not supported on the native Swift target; pass portable Array/String data via SwiftHost.",
                );
                "/* FIXME [NeoFlash Swift]: new ByteArray() */ NFValue.null".into()
            }
            "Stage3D" | "Context3D" => {
                self.record_issue(
                    ConversionIssueKind::ManualAttention,
                    offset,
                    format!("{short} is not supported on the native Swift target."),
                );
                format!("/* FIXME [NeoFlash Swift]: new {short}() */ NFValue.null")
            }
            "Array" => "[]".into(),
            "Object" | "Dictionary" => "[:]".into(),
            _ => {
                let sw_ty = HirType::from_annotation(ty).to_swift_type();
                format!("{sw_ty}({})", self.emit_args(args, offset))
            }
        }
    }

    fn emit_args(&mut self, args: &[Expr], offset: usize) -> String {
        args.iter()
            .map(|a| self.emit_expr_with_offset(a, offset))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn emit_subexpr(&mut self, expr: &Expr, parent_prec: u8, offset: usize) -> String {
        let my_prec = match expr {
            Expr::Assign(_, _, _) => 2,
            Expr::Conditional(_, _, _) => 3,
            Expr::Binary(op, _, _) => op_prec(op),
            Expr::TypeTest(_, _, _, _) | Expr::InstanceOf(_, _) => 10,
            Expr::Unary(_, _) | Expr::Delete(_, _) => 16,
            _ => 19,
        };
        let s = self.emit_expr_with_offset(expr, offset);
        if my_prec < parent_prec {
            format!("({s})")
        } else {
            s
        }
    }
}

fn emit_swift_class(
    parsed: &ParsedClass,
    location: &str,
    stage: &ProjectStageIndex,
    host_methods: &BTreeMap<String, SwiftProxyMethod>,
) -> (String, Vec<ConversionIssue>) {
    let cls_name = sanitize_swift_type_ident(&parsed.short_name);
    let mut emitter = SwiftEmitter::new(
        location,
        &parsed.source,
        parsed.language == ScriptLanguage::ActionScript3,
        stage,
        host_methods,
        None,
        Some(parsed.qualified_name.clone()),
    );
    emitter.current_class_short = Some(cls_name.clone());
    emitter.class_has_stage = parsed.base_class.as_deref().is_some_and(|base| {
        matches!(
            HirType::from_annotation(base),
            HirType::MovieClip | HirType::Sprite | HirType::Button | HirType::TextField
        ) || matches!(
            short_type_name(base),
            "DisplayObject" | "InteractiveObject" | "DisplayObjectContainer" | "Shape"
        )
    });

    let mut cls_props = BTreeMap::new();
    for f in &parsed.fields {
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        if f.is_static {
            emitter.static_props.insert(f.name.clone());
        } else {
            cls_props.insert(f.name.clone(), ty);
        }
    }
    emitter.set_instance_props(cls_props);
    for m in &parsed.methods {
        if let Some(mn) = &m.function.name
            && mn != &parsed.short_name
        {
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            if m.is_static {
                emitter.static_methods.insert(mn.clone());
            } else {
                emitter.instance_methods.insert(mn.clone());
                emitter.env.functions.insert(mn.clone(), rty);
            }
        }
    }

    if parsed.is_interface {
        let mut reqs = Vec::new();
        for m in &parsed.methods {
            let fname = escape_swift_ident(m.function.name.as_deref().unwrap_or("method"));
            emitter.push_scope();
            let params = emitter.emit_swift_params(&m.function, m.function.offset);
            emitter.pop_scope();
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let ret = if rty == HirType::Void {
                String::new()
            } else {
                format!(" -> {}", rty.to_swift_type())
            };
            reqs.push(format!("    func {fname}({params}){ret}"));
        }
        let code = format!(
            "@MainActor\npublic protocol {cls_name}: AnyObject {{\n{}\n}}",
            reqs.join("\n")
        );
        return (code, emitter.issues);
    }

    let mut members = Vec::new();
    for f in &parsed.fields {
        let fname = escape_swift_ident(&f.name);
        let ty =
            f.ty.as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
        let stat = if f.is_static { "static " } else { "" };
        let kw = if f.is_const { "let" } else { "var" };
        if let Some(val) = &f.value {
            let sw_ty = ty.to_swift_type();
            let vc = emitter.emit_coerced_expr(val, &ty, f.offset);
            members.push(format!("    public {stat}{kw} {fname}: {sw_ty} = {vc}"));
        } else {
            let (sw_ty, def_val) = swift_property_type_and_default(&ty);
            members.push(format!("    public {stat}var {fname}: {sw_ty} = {def_val}"));
        }
    }

    for m in &parsed.methods {
        let raw_name = m.function.name.as_deref().unwrap_or(&parsed.short_name);
        let is_ctor = raw_name == parsed.short_name && m.accessor.is_none() && !m.is_static;
        let fname = escape_swift_ident(raw_name);
        let stat = if m.is_static && !is_ctor {
            "static "
        } else {
            ""
        };
        let ovr = if m.is_override && !is_ctor && !m.is_static {
            "override "
        } else {
            ""
        };

        emitter.push_scope();
        let params = emitter.emit_swift_params(&m.function, m.function.offset);
        emitter.indent = 2;
        let body = emitter.emit_stmt_list(&m.function.body);
        emitter.indent = 0;
        emitter.pop_scope();

        if is_ctor {
            let mut ctor_lines = Vec::new();
            if !body.trim().is_empty() {
                ctor_lines.push(body);
            }
            if parsed.base_class.is_some()
                && !m.function.body.iter().any(|s| {
                    matches!(
                        s.unlocated(),
                        Stmt::Expr(Expr::Call(c, _, _)) if matches!(c.as_ref(), Expr::Name(n) if n == "super")
                    )
                })
            {
                ctor_lines.push("        super.init()".into());
            }
            members.push(format!(
                "    public init({params}) {{\n{}\n    }}",
                ctor_lines.join("\n")
            ));
        } else if let Some(acc) = m.accessor {
            let prop_ty = match acc {
                AccessorKind::Get => m
                    .function
                    .returns
                    .as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation)
                    .to_swift_type(),
                AccessorKind::Set => m
                    .function
                    .params
                    .first()
                    .and_then(|(_, t)| t.as_deref())
                    .map_or(HirType::Unknown, HirType::from_annotation)
                    .to_swift_type(),
            };
            match acc {
                AccessorKind::Get => {
                    members.push(format!(
                        "    public {stat}{ovr}var {fname}: {prop_ty} {{\n        get {{\n{body}\n        }}\n    }}"
                    ));
                }
                AccessorKind::Set => {
                    members.push(format!(
                        "    public {stat}func set_{fname}({params}) {{\n{body}\n    }}"
                    ));
                }
            }
        } else {
            let rty = m
                .function
                .returns
                .as_deref()
                .map_or(HirType::Void, HirType::from_annotation);
            let (discardable, ret) = if rty == HirType::Void {
                ("", String::new())
            } else {
                (
                    "    @discardableResult\n",
                    format!(" -> {}", rty.to_swift_type()),
                )
            };
            members.push(format!(
                "{discardable}    public {stat}{ovr}func {fname}({params}){ret} {{\n{body}\n    }}"
            ));
        }
    }

    let mut supers = Vec::new();
    if let Some(base) = &parsed.base_class
        && short_type_name(base) != "Object"
    {
        supers.push(HirType::from_annotation(base).to_swift_type());
    }
    for iface in &parsed.interfaces {
        supers.push(sanitize_swift_type_ident(short_type_name(iface)));
    }
    let super_clause = if supers.is_empty() {
        String::new()
    } else {
        format!(": {}", supers.join(", "))
    };

    let code = format!(
        "@MainActor\npublic class {cls_name}{super_clause} {{\n{}\n}}",
        members.join("\n\n")
    );
    (code, emitter.issues)
}

fn match_clip_factory(callee: &Expr, method_name: &str) -> Option<Expr> {
    match callee {
        Expr::Member(base, m) if m == method_name => Some((**base).clone()),
        Expr::Name(m) if m == method_name => Some(Expr::Name("this".into())),
        _ => None,
    }
}

fn listener_target_key(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(n) => Some(n.clone()),
        Expr::Member(b, p) if matches!(b.as_ref(), Expr::Name(n) if n == "this") => {
            Some(format!("this.{p}"))
        }
        _ => None,
    }
}

fn extract_as2_callback_target(lhs: &Expr) -> Option<(Expr, &str)> {
    match lhs {
        Expr::Member(base, prop) => Some(((**base).clone(), prop.as_str())),
        Expr::Name(prop)
            if matches!(
                prop.as_str(),
                "onEnterFrame" | "onKeyDown" | "onKeyUp" | "onResize" | "onRelease" | "onPress"
            ) =>
        {
            Some((Expr::Name("this".into()), prop.as_str()))
        }
        _ => None,
    }
}

fn is_null_or_undef(expr: &Expr) -> bool {
    matches!(expr, Expr::Literal(s) if s == "null")
        || matches!(expr, Expr::Name(s) if s == "undefined" || s == "null")
}

fn is_nonneg_int_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal(s) if s.parse::<u64>().is_ok() || s.starts_with("0x") || s.starts_with("0X")
    )
}

fn parse_object_script_for_swift(
    script: &Script,
    includes: &BTreeMap<String, ResourceUnit>,
) -> Result<ParsedScript, String> {
    match parse_timeline_script(script, "", includes) {
        Ok(parsed) => Ok(parsed),
        Err(err) if script.language == ScriptLanguage::ActionScript3 => {
            // Allow classic `on(release)` / `onClipEvent(...)` snippets typed on a Stage button/clip
            // inside an AS3 document to parse cleanly through the AS2 grammar fallback!
            let as2_probe = Script {
                language: ScriptLanguage::ActionScript2,
                source: script.source.clone(),
            };
            parse_timeline_script(&as2_probe, "", includes).map_err(|_| err)
        }
        Err(err) => Err(err),
    }
}

fn map_legacy_triggers_to_object_event(triggers: &[LegacyTrigger]) -> Option<ObjectEvent> {
    for trig in triggers {
        match trig.event.to_ascii_lowercase().as_str() {
            "load" | "initialize" => return Some(ObjectEvent::Load),
            "enterframe" => return Some(ObjectEvent::EnterFrame),
            "release" | "press" | "releaseoutside" | "mousedown" | "mouseup" => {
                return Some(ObjectEvent::Click);
            }
            _ => {}
        }
    }
    None
}

fn unquote_string_literal(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    if (quote != b'"' && quote != b'\'') || bytes[bytes.len() - 1] != quote {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(esc) = chars.next() {
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => out.push(other),
                }
            }
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn proxy_method_ident_from_literal(raw_lit: &str) -> Option<String> {
    let decoded = unquote_string_literal(raw_lit)?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut ident = String::with_capacity(trimmed.len());
    let mut prev_underscore = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            ident.push(ch);
            prev_underscore = ch == '_';
        } else if !prev_underscore && !ident.is_empty() {
            ident.push('_');
            prev_underscore = true;
        }
    }
    let cleaned = ident.trim_matches('_');
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.as_bytes()[0].is_ascii_digit() {
        Some(format!("_{cleaned}"))
    } else {
        Some(cleaned.to_owned())
    }
}

fn is_external_interface_base(expr: &Expr) -> bool {
    match expr {
        Expr::Name(n) => n == "ExternalInterface",
        Expr::Member(_, prop) => prop == "ExternalInterface",
        _ => false,
    }
}

fn match_host_proxy_call<'b>(callee: &'b Expr, args: &'b [Expr]) -> Option<(String, &'b [Expr])> {
    if let Expr::Member(base, method) = callee
        && matches!(base.as_ref(), Expr::Name(n) if n == "SwiftHost")
    {
        let clean = method.trim();
        if !clean.is_empty() {
            return Some((clean.to_owned(), args));
        }
    }
    if matches!(callee, Expr::Name(n) if n == "fscommand")
        && let Some(Expr::Literal(cmd_lit)) = args.first()
        && let Some(name) = proxy_method_ident_from_literal(cmd_lit)
    {
        return Some((name, &args[1..]));
    }
    if let Expr::Member(base, m) = callee
        && m == "call"
        && is_external_interface_base(base)
        && let Some(Expr::Literal(fn_lit)) = args.first()
        && let Some(name) = proxy_method_ident_from_literal(fn_lit)
    {
        return Some((name, &args[1..]));
    }
    None
}

fn swift_default_for_proxy_type(swift_ty: &str) -> &'static str {
    match swift_ty.trim() {
        "Bool" => "false",
        "Int" | "UInt" => "0",
        "Double" => "0.0",
        "String" => "\"\"",
        "[NFValue]" => "[]",
        "[String: NFValue]" => "[:]",
        _ => ".undefined",
    }
}

#[derive(Clone, Debug)]
struct InferredProxySig {
    params: Vec<(String, String, String)>, // (name, swift_type, as_type)
    return_swift_type: Option<String>,
}

fn merge_proxy_swift_types(
    existing_swift: &str,
    existing_as: &str,
    next_swift: &str,
    _next_as: &str,
) -> (String, String) {
    if existing_swift == next_swift {
        return (existing_swift.to_owned(), existing_as.to_owned());
    }
    let is_num_a = matches!(existing_swift, "Int" | "UInt" | "Double");
    let is_num_b = matches!(next_swift, "Int" | "UInt" | "Double");
    if is_num_a && is_num_b {
        if existing_swift == "Double" || next_swift == "Double" {
            return ("Double".into(), "Number".into());
        }
        return ("Int".into(), "int".into());
    }
    ("NFValue".into(), "*".into())
}

fn infer_arg_param_name(arg: &Expr, idx: usize) -> String {
    let raw = match arg {
        Expr::Name(n)
            if !matches!(
                n.as_str(),
                "true" | "false" | "null" | "undefined" | "this" | "NaN" | "Infinity"
            ) =>
        {
            Some(n.as_str())
        }
        Expr::Member(_, prop) => Some(prop.as_str()),
        _ => None,
    };
    if let Some(candidate) = raw {
        let cleaned: String = candidate
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let trimmed = cleaned.trim_start_matches('_');
        if !trimmed.is_empty() && !trimmed.as_bytes()[0].is_ascii_digit() {
            return trimmed.to_owned();
        }
    }
    format!("arg{idx}")
}

fn infer_portable_proxy_arg_type(
    arg: &Expr,
    env: &TypeEnv,
    stage: &ProjectStageIndex,
) -> (&'static str, &'static str) {
    if matches!(arg, Expr::Object(_)) {
        return ("[String: NFValue]", "Object");
    }
    if matches!(arg, Expr::Array(_) | Expr::NewVector(_, _, _)) {
        return ("[NFValue]", "Array");
    }
    match env.infer_expr(arg, stage) {
        HirType::Int => ("Int", "int"),
        HirType::UInt => ("UInt", "uint"),
        HirType::Float => ("Double", "Number"),
        HirType::Bool => ("Bool", "Boolean"),
        HirType::String => ("String", "String"),
        HirType::Array | HirType::Vector(_) => ("[NFValue]", "Array"),
        _ => ("NFValue", "*"),
    }
}

struct ProxyCallScanner<'a> {
    stage: &'a ProjectStageIndex,
    explicit_host_methods: &'a BTreeMap<String, SwiftProxyMethod>,
    discovered_order: Vec<String>,
    discovered: BTreeMap<String, InferredProxySig>,
    env: TypeEnv,
    current_fn_return: Option<&'static str>,
}

impl<'a> ProxyCallScanner<'a> {
    fn record_call(&mut self, name: String, payload_args: &[Expr], expected_ret: Option<&str>) {
        if self.explicit_host_methods.contains_key(&name) {
            return;
        }
        let mut inferred_params = Vec::with_capacity(payload_args.len());
        let mut used_names = BTreeSet::new();
        for (idx, arg) in payload_args.iter().enumerate() {
            let base_pname = infer_arg_param_name(arg, idx);
            let mut pname = base_pname.clone();
            let mut suffix = 2usize;
            while !used_names.insert(pname.clone()) {
                pname = format!("{base_pname}{suffix}");
                suffix += 1;
            }
            let (sw_ty, as_ty) = infer_portable_proxy_arg_type(arg, &self.env, self.stage);
            inferred_params.push((pname, sw_ty.to_owned(), as_ty.to_owned()));
        }

        if let Some(existing) = self.discovered.get_mut(&name) {
            for (idx, (pname, sw_ty, as_ty)) in inferred_params.into_iter().enumerate() {
                if let Some(slot) = existing.params.get_mut(idx) {
                    let (merged_sw, merged_as) =
                        merge_proxy_swift_types(&slot.1, &slot.2, &sw_ty, &as_ty);
                    slot.1 = merged_sw;
                    slot.2 = merged_as;
                    if slot.0.starts_with("arg") && !pname.starts_with("arg") {
                        slot.0 = pname;
                    }
                } else {
                    existing.params.push((pname, sw_ty, as_ty));
                }
            }
            // Deduplicate param names if widening merged a duplicate
            let mut seen = BTreeSet::new();
            for (idx, slot) in existing.params.iter_mut().enumerate() {
                if !seen.insert(slot.0.clone()) {
                    slot.0 = format!("arg{idx}");
                    seen.insert(slot.0.clone());
                }
            }
            match (&existing.return_swift_type, expected_ret) {
                (_, None) => {}
                (None, Some(r)) => existing.return_swift_type = Some(r.to_owned()),
                (Some(prev), Some(r)) if prev != r => {
                    let (merged, _) = merge_proxy_swift_types(prev, "*", r, "*");
                    existing.return_swift_type = Some(merged);
                }
                _ => {}
            }
        } else {
            self.discovered_order.push(name.clone());
            self.discovered.insert(
                name,
                InferredProxySig {
                    params: inferred_params,
                    return_swift_type: expected_ret.map(str::to_owned),
                },
            );
        }
    }

    fn scan_stmt_list(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.scan_stmt(stmt);
        }
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt.unlocated() {
            Stmt::Located(_, inner) => self.scan_stmt(inner),
            Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::DefaultXmlNamespace(_) => {}
            Stmt::Block(b) | Stmt::InitClip(_, b, _) => {
                self.env.push_scope();
                self.scan_stmt_list(b);
                self.env.pop_scope();
            }
            Stmt::Var(vars) => {
                for (vname, vty, val_opt) in vars {
                    let expected = vty
                        .as_deref()
                        .and_then(map_portable_as_type_to_swift)
                        .map(|(sw, _)| sw)
                        .or(Some("NFValue"));
                    if let Some(val) = val_opt {
                        self.scan_expr(val, expected);
                    }
                    let hir_ty =
                        vty.as_deref()
                            .map(HirType::from_annotation)
                            .unwrap_or_else(|| {
                                val_opt.as_ref().map_or(HirType::Unknown, |e| {
                                    self.env.infer_expr(e, self.stage)
                                })
                            });
                    self.env.define(vname.clone(), hir_ty);
                }
            }
            Stmt::Function(f) => self.scan_function(f),
            Stmt::Expr(expr) => self.scan_expr(expr, None),
            Stmt::If(cond, yes, no) => {
                self.scan_expr(cond, Some("Bool"));
                self.scan_stmt(yes);
                if let Some(n) = no {
                    self.scan_stmt(n);
                }
            }
            Stmt::While(cond, body) => {
                self.scan_expr(cond, Some("Bool"));
                self.scan_stmt(body);
            }
            Stmt::DoWhile(body, cond) => {
                self.scan_stmt(body);
                self.scan_expr(cond, Some("Bool"));
            }
            Stmt::For(init, cond, update, body) => {
                self.env.push_scope();
                self.scan_stmt(init);
                if let Some(c) = cond {
                    self.scan_expr(c, Some("Bool"));
                }
                self.scan_stmt(body);
                if let Some(u) = update {
                    self.scan_expr(u, None);
                }
                self.env.pop_scope();
            }
            Stmt::ForIn(init, coll, _, body) => {
                self.env.push_scope();
                self.scan_stmt(init);
                self.scan_expr(coll, Some("NFValue"));
                self.scan_stmt(body);
                self.env.pop_scope();
            }
            Stmt::Switch(disc, cases) => {
                self.scan_expr(disc, Some("NFValue"));
                for Case { test, body } in cases {
                    if let Some(t) = test {
                        self.scan_expr(t, Some("NFValue"));
                    }
                    self.scan_stmt_list(body);
                }
            }
            Stmt::Return(val, _) => {
                if let Some(v) = val {
                    let ret_hint = self.current_fn_return.or(Some("NFValue"));
                    self.scan_expr(v, ret_hint);
                }
            }
            Stmt::Throw(val) => self.scan_expr(val, Some("NFValue")),
            Stmt::Try(body, catches, fin) => {
                self.scan_stmt_list(body);
                for c in catches {
                    self.env.push_scope();
                    self.scan_stmt_list(&c.body);
                    self.env.pop_scope();
                }
                if let Some(f) = fin {
                    self.scan_stmt_list(f);
                }
            }
            Stmt::Label(_, inner) | Stmt::IfFrameLoaded(_, inner) => self.scan_stmt(inner),
            Stmt::With(tgt, body, _) | Stmt::TellTarget(tgt, body, _) => {
                self.scan_expr(tgt, Some("NFValue"));
                self.scan_stmt(body);
            }
            Stmt::LegacyEvent(_, func) => self.scan_function(func),
        }
    }

    fn scan_function(&mut self, f: &Function) {
        let saved_ret = self.current_fn_return;
        self.current_fn_return = f
            .returns
            .as_deref()
            .and_then(map_portable_as_type_to_swift)
            .map(|(sw, _)| sw);
        self.env.push_scope();
        for (i, (pname, pty)) in f.params.iter().enumerate() {
            let hty = pty
                .as_deref()
                .map_or(HirType::Unknown, HirType::from_annotation);
            self.env.define(pname.clone(), hty);
            if let Some(Some(def)) = f.defaults.get(i) {
                self.scan_expr(def, Some("NFValue"));
            }
        }
        self.scan_stmt_list(&f.body);
        self.env.pop_scope();
        self.current_fn_return = saved_ret;
    }

    fn scan_expr(&mut self, expr: &Expr, expected_ret: Option<&str>) {
        match expr {
            Expr::Call(callee, args, _) => {
                if let Some((method, payload_args)) = match_host_proxy_call(callee, args) {
                    for arg in payload_args {
                        self.scan_expr(arg, Some("NFValue"));
                    }
                    self.record_call(method, payload_args, expected_ret);
                    return;
                }
                self.scan_expr(callee, Some("NFValue"));
                for arg in args {
                    self.scan_expr(arg, Some("NFValue"));
                }
            }
            Expr::Member(base, _)
            | Expr::Delete(base, _)
            | Expr::Update(_, base, _)
            | Expr::TypeTest(_, base, _, _) => self.scan_expr(base, Some("NFValue")),
            Expr::Unary(op, inner) => {
                let hint = if op == "!" || op == "not" {
                    Some("Bool")
                } else if op == "void" {
                    None
                } else {
                    Some("Double")
                };
                self.scan_expr(inner, hint);
            }
            Expr::Index(a, b) | Expr::InstanceOf(a, b) | Expr::XmlFilter(a, b, _) => {
                self.scan_expr(a, Some("NFValue"));
                self.scan_expr(b, Some("NFValue"));
            }
            Expr::Binary(op, lhs, rhs) => {
                let hint = match op.as_str() {
                    "&&" | "||" | "and" | "or" => Some("Bool"),
                    "-" | "*" | "/" | "%" => Some("Double"),
                    "&" | "|" | "^" | "<<" | ">>" | ">>>" => Some("Int"),
                    "+" | "add" => {
                        let lt = self.env.infer_expr(lhs, self.stage);
                        let rt = self.env.infer_expr(rhs, self.stage);
                        if lt == HirType::String || rt == HirType::String {
                            Some("String")
                        } else {
                            Some("Double")
                        }
                    }
                    _ => Some("NFValue"),
                };
                self.scan_expr(lhs, hint);
                self.scan_expr(rhs, hint);
            }
            Expr::Assign(op, lhs, rhs) => {
                self.scan_expr(lhs, Some("NFValue"));
                let hint = if op == "=" {
                    match self.env.infer_expr(lhs, self.stage) {
                        HirType::Bool => Some("Bool"),
                        HirType::Int => Some("Int"),
                        HirType::UInt => Some("UInt"),
                        HirType::Float => Some("Double"),
                        HirType::String => Some("String"),
                        HirType::Array | HirType::Vector(_) => Some("[NFValue]"),
                        _ => Some("NFValue"),
                    }
                } else {
                    Some("Double")
                };
                self.scan_expr(rhs, hint);
            }
            Expr::Conditional(c, y, n) => {
                self.scan_expr(c, Some("Bool"));
                self.scan_expr(y, expected_ret.or(Some("NFValue")));
                self.scan_expr(n, expected_ret.or(Some("NFValue")));
            }
            Expr::Array(items) => {
                for it in items.iter().flatten() {
                    self.scan_expr(it, Some("NFValue"));
                }
            }
            Expr::Object(props) => {
                for (_, val) in props {
                    self.scan_expr(val, Some("NFValue"));
                }
            }
            Expr::Function(f) => self.scan_function(f),
            Expr::NewError(_, args, _)
            | Expr::NewBuiltin(_, args, _)
            | Expr::NewClass(_, args, _)
            | Expr::NewValue(_, args, _)
            | Expr::NewVector(_, args, _)
            | Expr::XmlLiteral(_, args) => {
                for arg in args {
                    self.scan_expr(arg, Some("NFValue"));
                }
            }
            Expr::XmlSelect {
                base,
                local,
                namespace,
                ..
            } => {
                self.scan_expr(base, Some("NFValue"));
                self.scan_expr(local, Some("NFValue"));
                if let Some(ns) = namespace {
                    self.scan_expr(ns, Some("NFValue"));
                }
            }
            Expr::XmlWithAttr {
                local, namespace, ..
            } => {
                self.scan_expr(local, Some("NFValue"));
                if let Some(ns) = namespace {
                    self.scan_expr(ns, Some("NFValue"));
                }
            }
            Expr::Literal(_)
            | Expr::Name(_)
            | Expr::RegExpLiteral(_, _)
            | Expr::TypeValue(_, _) => {}
        }
    }
}

fn discover_implicit_host_proxies(
    project: &Project,
    parsed_units: &[(usize, ParsedClass)],
    stage: &ProjectStageIndex,
    includes: &BTreeMap<String, ResourceUnit>,
    explicit_host_methods: &BTreeMap<String, SwiftProxyMethod>,
) -> Vec<SwiftProxyMethod> {
    let mut scanner = ProxyCallScanner {
        stage,
        explicit_host_methods,
        discovered_order: Vec::new(),
        discovered: BTreeMap::new(),
        env: TypeEnv::new(None, None),
        current_fn_return: None,
    };

    // 1. Scan non-SwiftHost `.as` classes
    for (_, parsed) in parsed_units {
        scanner.env = TypeEnv::new(None, Some(parsed.qualified_name.clone()));
        for f in &parsed.fields {
            let fty =
                f.ty.as_deref()
                    .map_or(HirType::Unknown, HirType::from_annotation);
            scanner.env.define(f.name.clone(), fty);
            if let Some(val) = &f.value {
                let hint =
                    f.ty.as_deref()
                        .and_then(map_portable_as_type_to_swift)
                        .map(|(sw, _)| sw)
                        .or(Some("NFValue"));
                scanner.scan_expr(val, hint);
            }
        }
        for m in &parsed.methods {
            scanner.scan_function(&m.function);
        }
    }

    // 2. Scan all symbol timelines and stage element `ObjectScript`s
    for sym in &project.symbols {
        let mut sym_env = TypeEnv::new(Some(sym.id.clone()), None);
        if let Some(children) = stage.symbol_children.get(&sym.id) {
            for (cname, info) in children {
                sym_env.define(cname.clone(), info.hir_type.clone());
            }
        }
        scanner.env = sym_env;

        for entry in &sym.timeline.scripts {
            if entry.script.source.trim().is_empty() {
                continue;
            }
            if matches!(
                entry.script.language,
                ScriptLanguage::ActionScript1
                    | ScriptLanguage::ActionScript2
                    | ScriptLanguage::ActionScript3
            ) && let Ok(parsed) = parse_timeline_script(&entry.script, "", includes)
            {
                scanner.scan_stmt_list(&parsed.statements);
            }
        }

        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        if os.script.source.trim().is_empty() {
                            continue;
                        }
                        if matches!(
                            os.script.language,
                            ScriptLanguage::ActionScript1
                                | ScriptLanguage::ActionScript2
                                | ScriptLanguage::ActionScript3
                        ) && let Ok(parsed) = parse_object_script_for_swift(&os.script, includes)
                        {
                            scanner.scan_stmt_list(&parsed.statements);
                        }
                    }
                }
            }
        }
    }

    scanner
        .discovered_order
        .into_iter()
        .filter_map(|name| {
            let sig = scanner.discovered.remove(&name)?;
            Some(SwiftProxyMethod {
                name,
                params: sig
                    .params
                    .into_iter()
                    .map(|(pname, swift_type, as_type)| SwiftProxyParam {
                        name: pname,
                        swift_type,
                        as_type,
                    })
                    .collect(),
                return_swift_type: sig.return_swift_type,
            })
        })
        .collect()
}
