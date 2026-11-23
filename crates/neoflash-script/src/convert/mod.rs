//! Deterministic AST-based script conversion (`AS1/AS2 <-> AS3`, `AS1/AS2/AS3 -> JS`) and
//! Swift 6 scene/proxy code generation (`compile_project_scripts_to_swift`).
//!
//! Zero AI / LLM heuristics: every rewrite operates over NeoFlash's ActionScript lexer, AST,
//! stage symbol/instance graph (`ProjectStageIndex`), and lexical type environment (`TypeEnv`).

pub(crate) mod emit_as2;
pub(crate) mod emit_as3;
pub(crate) mod emit_js;
pub(crate) mod emit_swift;
pub(crate) mod hir;
pub(crate) mod swift_contract;

#[cfg(test)]
mod tests;

pub use emit_swift::{SwiftCompiledProject, SwiftCompiledSymbol, compile_project_scripts_to_swift};
pub use swift_contract::{SwiftProxyMethod, SwiftProxyParam};

use crate::actionscript::ast::Stmt;
use emit_as2::{emit_as2_class, emit_as2_timeline_script};
use emit_as3::{SynthesizedLinkage, emit_as3_class, emit_as3_timeline_script};
use emit_js::{
    display_base_shim_block, emit_js_class, emit_js_timeline_script, relative_module_specifier,
    to_js_module_path,
};
use hir::{
    LiftedObjectHandler, LiftedTriggerKind, ParsedClass, ProjectStageIndex, parse_class_unit,
    parse_timeline_script, short_type_name,
};
use neoflash_core::{
    FrameScript, ObjectEvent, Project, ResourceUnit, Script, ScriptLanguage, SourceUnit,
    SymbolKind, new_id,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Target language for deterministic AST project script conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversionTarget {
    ActionScript3,
    ActionScript2,
    JavaScript,
}

impl ConversionTarget {
    pub fn script_language(self) -> ScriptLanguage {
        match self {
            Self::ActionScript3 => ScriptLanguage::ActionScript3,
            Self::ActionScript2 => ScriptLanguage::ActionScript2,
            Self::JavaScript => ScriptLanguage::JavaScript,
        }
    }
}

/// Classification of non-1:1 transformations recorded during script conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversionIssueKind {
    /// A deterministic runtime helper shim (e.g. `Key.isDown` listener state) was injected.
    ShimInjected,
    /// A construct was lowered to a compatible approximation (e.g. `Vector.<T>` -> `Array`).
    Approximated,
    /// A construct has no automatic equivalent in the target VM (e.g. `E4X`, `Stage3D`) and was marked with `FIXME`.
    ManualAttention,
}

/// A single diagnostic note recorded in a [`ConversionReport`] or [`SwiftCompiledProject`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionIssue {
    pub kind: ConversionIssueKind,
    pub location: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Summary and diagnostic report produced by [`convert_project_scripts`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionReport {
    pub source_language: ScriptLanguage,
    pub target_language: ScriptLanguage,
    pub converted_frame_scripts: usize,
    pub converted_object_scripts: usize,
    pub converted_source_units: usize,
    pub auto_named_instances: usize,
    pub issues: Vec<ConversionIssue>,
}

/// Convert all timeline frame scripts, stage instance object scripts, and source class units in
/// `project` to `target` deterministically, returning the validated converted [`Project`] and
/// a detailed [`ConversionReport`].
pub fn convert_project_scripts(
    project: &Project,
    target: ConversionTarget,
) -> Result<(Project, ConversionReport), String> {
    let source_language = project
        .script_language
        .unwrap_or_else(|| project.infer_script_language());
    let target_language = target.script_language();

    let mut has_actionscript = false;
    let mut web_language = None;
    for (language, source) in script_entries(project) {
        if !source.trim().is_empty() {
            if language.is_actionscript() {
                has_actionscript = true;
            } else {
                web_language = Some(language);
            }
        }
    }
    if target_language.is_actionscript()
        && let Some(language) =
            web_language.or_else(|| (!source_language.is_actionscript()).then_some(source_language))
    {
        return Err(format!(
            "Converting {language:?} scripts to {target_language:?} is not supported: NeoFlash's deterministic script converter translates ActionScript 1.0/2.0 <-> ActionScript 3.0 and ActionScript 1.0/2.0/3.0 -> JavaScript."
        ));
    }

    let mut out = project.clone();
    let mut report = ConversionReport {
        source_language,
        target_language,
        converted_frame_scripts: 0,
        converted_object_scripts: 0,
        converted_source_units: 0,
        auto_named_instances: 0,
        issues: Vec::new(),
    };

    if target == ConversionTarget::JavaScript
        && (web_language.is_some() || !source_language.is_actionscript())
    {
        normalize_web_to_js(&mut out, &mut report)?;
        if !has_actionscript {
            out.script_language = Some(target_language);
            out.upgrade_format_for_features();
            validate_converted_project(&out)?;
            return Ok((out, report));
        }
    }

    let has_any_different_lang =
        script_entries(&out).any(|(language, _)| language != target_language);
    if source_language == target_language && !has_any_different_lang {
        out.script_language = Some(target_language);
        out.upgrade_format_for_features();
        validate_converted_project(&out)?;
        return Ok((out, report));
    }

    // 4. Build `#include` lookup map across `resources` and `sources`.
    let includes = build_include_lookup(&out);

    match target {
        ConversionTarget::ActionScript3 => convert_to_as3(&mut out, &includes, &mut report)?,
        ConversionTarget::ActionScript2 => convert_to_as2(&mut out, &includes, &mut report)?,
        ConversionTarget::JavaScript => convert_to_js(&mut out, &includes, &mut report)?,
    }

    out.script_language = Some(target_language);
    out.upgrade_format_for_features();
    validate_converted_project(&out)?;
    Ok((out, report))
}

fn script_entries(project: &Project) -> impl Iterator<Item = (ScriptLanguage, &str)> {
    project
        .sources
        .iter()
        .map(|unit| (unit.language, unit.source.as_str()))
        .chain(project.symbols.iter().flat_map(|symbol| {
            let timeline = &symbol.timeline;
            timeline
                .scripts
                .iter()
                .map(|frame| &frame.script)
                .chain(
                    timeline
                        .layers
                        .iter()
                        .flat_map(|layer| &layer.keyframes)
                        .flat_map(|frame| &frame.elements)
                        .flat_map(|element| element.scripts.iter().map(|object| &object.script)),
                )
                .map(|script| (script.language, script.source.as_str()))
        }))
}

fn validate_converted_project(project: &Project) -> Result<(), String> {
    project
        .validate()
        .map_err(|err| format!("Converted project failed document validation: {err}"))
}

fn build_include_lookup(project: &Project) -> BTreeMap<String, ResourceUnit> {
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

fn normalize_web_to_js(project: &mut Project, report: &mut ConversionReport) -> Result<(), String> {
    for unit in &mut project.sources {
        if unit.language == ScriptLanguage::TypeScript {
            let (code, _) = crate::compile_web_mapped(
                &Script {
                    language: ScriptLanguage::TypeScript,
                    source: unit.source.clone(),
                },
                true,
                &unit.path,
            )
            .map_err(|errs| {
                errs.into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            })?;
            unit.source = code;
            unit.language = ScriptLanguage::JavaScript;
            if let Some(stem) = unit.path.strip_suffix(".ts") {
                unit.path = format!("{stem}.js");
            } else if let Some(stem) = unit.path.strip_suffix(".tsx") {
                unit.path = format!("{stem}.jsx");
            }
            report.converted_source_units += 1;
        }
    }
    for sym in &mut project.symbols {
        for fs in &mut sym.timeline.scripts {
            if fs.script.language == ScriptLanguage::TypeScript {
                let (code, _) =
                    crate::compile_web_mapped(&fs.script, true, &sym.name).map_err(|errs| {
                        errs.into_iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("; ")
                    })?;
                fs.script.source = code;
                fs.script.language = ScriptLanguage::JavaScript;
                report.converted_frame_scripts += 1;
            }
        }
        for layer in &mut sym.timeline.layers {
            for kf in &mut layer.keyframes {
                for el in &mut kf.elements {
                    for os in &mut el.scripts {
                        if os.script.language == ScriptLanguage::TypeScript {
                            let (code, _) = crate::compile_web_mapped(&os.script, true, &el.name)
                                .map_err(|errs| {
                                errs.into_iter()
                                    .map(|e| e.to_string())
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            })?;
                            os.script.source = code;
                            os.script.language = ScriptLanguage::JavaScript;
                            report.converted_object_scripts += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// =========================================================================
// Stage ObjectScript & Legacy `on(...)` / `onClipEvent(...)` Lifter
// =========================================================================

/// Auto-name unnamed stage elements that carry `ObjectScript`s (`btn_1`, `clip_1` across all
/// keyframes of that element ID in the layer) and lift their handlers onto per-frame buckets
/// on the parent symbol timeline!
fn lift_symbol_object_scripts(
    project_snapshot: &Project,
    sym_idx: usize,
    out_project: &mut Project,
    includes: &BTreeMap<String, ResourceUnit>,
    report: &mut ConversionReport,
) -> Result<BTreeMap<u32, Vec<LiftedObjectHandler>>, String> {
    let mut used_names = BTreeSet::new();
    let mut names_by_element = BTreeMap::<_, BTreeSet<_>>::new();
    let mut owners_by_name = BTreeMap::<_, BTreeSet<_>>::new();
    for layer in &out_project.symbols[sym_idx].timeline.layers {
        for kf in &layer.keyframes {
            for el in &kf.elements {
                if !el.name.is_empty() {
                    let key = (layer.id.as_str(), el.id.as_str());
                    names_by_element
                        .entry(key)
                        .or_default()
                        .insert(el.name.as_str());
                    owners_by_name
                        .entry(el.name.as_str())
                        .or_default()
                        .insert(key);
                    used_names.insert(el.name.clone());
                }
            }
        }
    }

    let mut btn_counter = 0usize;
    let mut clip_counter = 0usize;
    let mut auto_names_by_layer_el: BTreeMap<(String, String), String> = BTreeMap::new();

    // Pass 1: discover unnamed elements with scripts and assign deterministic `btn_N` / `clip_N`
    for layer in &out_project.symbols[sym_idx].timeline.layers {
        for kf in &layer.keyframes {
            for el in &kf.elements {
                if !el.scripts.iter().any(|os| {
                    os.script.language.is_actionscript() && !os.script.source.trim().is_empty()
                }) || !el.name.is_empty()
                {
                    continue;
                }
                let key = (layer.id.clone(), el.id.clone());
                if auto_names_by_layer_el.contains_key(&key) {
                    continue;
                }
                // An authored name on another keyframe belongs to the same instance
                // when that identity and name are unambiguous throughout the symbol.
                if let Some(names) = names_by_element.get(&(layer.id.as_str(), el.id.as_str()))
                    && names.len() == 1
                    && let Some(name) = names.first()
                    && owners_by_name[name].len() == 1
                {
                    auto_names_by_layer_el.insert(key, (*name).to_owned());
                    report.auto_named_instances += 1;
                    continue;
                }
                let is_btn = el.effective_symbol_kind(project_snapshot) == Some(SymbolKind::Button)
                    || el.scripts.iter().any(|os| {
                        os.script.source.contains("on(") || os.script.source.contains("on (")
                    });
                let generated = loop {
                    let candidate = if is_btn {
                        btn_counter += 1;
                        format!("btn_{btn_counter}")
                    } else {
                        clip_counter += 1;
                        format!("clip_{clip_counter}")
                    };
                    if used_names.insert(candidate.clone()) {
                        break candidate;
                    }
                };
                auto_names_by_layer_el.insert(key, generated);
                report.auto_named_instances += 1;
            }
        }
    }

    // Propagate generated instance names across ALL keyframes of that `(layer_id, element_id)`!
    if !auto_names_by_layer_el.is_empty() {
        for layer in &mut out_project.symbols[sym_idx].timeline.layers {
            for kf in &mut layer.keyframes {
                for el in &mut kf.elements {
                    if el.name.is_empty()
                        && let Some(assigned) =
                            auto_names_by_layer_el.get(&(layer.id.clone(), el.id.clone()))
                    {
                        el.name = assigned.clone();
                    }
                }
            }
        }
    }

    // Pass 2: parse and drain `el.scripts` into `lifted_by_frame`
    let sym_name = out_project.symbols[sym_idx].name.clone();
    let mut lifted_by_frame: BTreeMap<u32, Vec<LiftedObjectHandler>> = BTreeMap::new();

    for layer in &mut out_project.symbols[sym_idx].timeline.layers {
        for kf in &mut layer.keyframes {
            let frame_no = kf.frame;
            for el in &mut kf.elements {
                if el.scripts.is_empty() {
                    continue;
                }
                let inst_name = el.name.clone();
                let is_button =
                    el.effective_symbol_kind(project_snapshot) == Some(SymbolKind::Button);
                let drained: Vec<_> = std::mem::take(&mut el.scripts);

                for os in drained {
                    if !os.script.language.is_actionscript()
                        && report.target_language == ScriptLanguage::JavaScript
                    {
                        el.scripts.push(os);
                        continue;
                    }
                    if os.script.source.trim().is_empty() {
                        continue;
                    }
                    let loc = format!("{sym_name}::{inst_name}::{:?}", os.event);
                    let parsed = parse_timeline_script(&os.script, "", includes)
                        .map_err(|e| format!("{loc}: {e}"))?;

                    let mut plain_stmts = Vec::new();
                    for stmt in parsed.statements {
                        if let Stmt::LegacyEvent(triggers, fn_box) = stmt.unlocated() {
                            for trig in triggers {
                                if let Some(kind) = LiftedTriggerKind::from_legacy(trig) {
                                    // Button `on(...)` executes with parent timeline as `this`;
                                    // MovieClip `onClipEvent(...)` executes with instance as `this`!
                                    let parent_is_this = !trig.clip_event;
                                    lifted_by_frame.entry(frame_no).or_default().push(
                                        LiftedObjectHandler {
                                            instance_name: Some(inst_name.clone()),
                                            parent_is_this,
                                            trigger: kind,
                                            body: fn_box.body.clone(),
                                        },
                                    );
                                }
                            }
                        } else {
                            plain_stmts.push(stmt);
                        }
                    }

                    if !plain_stmts.is_empty() {
                        let (trigger, parent_is_this) = match os.event {
                            ObjectEvent::Load => (LiftedTriggerKind::Load, is_button),
                            ObjectEvent::Click => (LiftedTriggerKind::MouseClick, is_button),
                            ObjectEvent::EnterFrame => (LiftedTriggerKind::EnterFrame, false),
                        };
                        lifted_by_frame
                            .entry(frame_no)
                            .or_default()
                            .push(LiftedObjectHandler {
                                instance_name: Some(inst_name.clone()),
                                parent_is_this,
                                trigger,
                                body: plain_stmts,
                            });
                    }

                    report.converted_object_scripts += 1;
                }
            }
        }
    }

    Ok(lifted_by_frame)
}

/// A layered frame can contain several independently authored scripts. Attach each
/// set of lifted handlers to the first script exactly once, retaining script order.
fn ensure_handler_frames(
    timeline: &mut neoflash_core::Timeline,
    handlers: &BTreeMap<u32, Vec<LiftedObjectHandler>>,
) {
    let existing: BTreeSet<_> = timeline.scripts.iter().map(|fs| fs.frame).collect();
    for &frame in handlers.keys().filter(|f| !existing.contains(f)) {
        timeline.scripts.push(FrameScript {
            layer: timeline.layers.first().map(|l| l.id.clone()),
            frame,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: String::new(),
            },
        });
    }
    timeline.scripts.sort_by_key(|fs| fs.frame);
}

fn retain_action_source(unit: &SourceUnit, report: &mut ConversionReport) {
    report.issues.push(ConversionIssue {
        kind: ConversionIssueKind::ManualAttention,
        location: unit.path.clone(),
        line: 1,
        column: 1,
        message: "Retained the original ActionScript source as an editable project file. Referenced includes are expanded in their converted callers; this file is not a target-language class.".into(),
    });
}

/// Split any top-level `Stmt::LegacyEvent` blocks that were placed inside a `FrameScript` itself.
fn extract_frame_legacy_events(stmts: &[Stmt]) -> (Vec<Stmt>, Vec<LiftedObjectHandler>) {
    let mut kept = Vec::new();
    let mut lifted = Vec::new();
    for stmt in stmts {
        if let Stmt::LegacyEvent(triggers, fn_box) = stmt.unlocated() {
            for trig in triggers {
                if let Some(kind) = LiftedTriggerKind::from_legacy(trig) {
                    lifted.push(LiftedObjectHandler {
                        instance_name: None,
                        parent_is_this: true,
                        trigger: kind,
                        body: fn_box.body.clone(),
                    });
                }
            }
        } else {
            kept.push(stmt.clone());
        }
    }
    (kept, lifted)
}

// =========================================================================
// Target 1: ActionScript 3.0 (`AS1/AS2 -> AS3`)
// =========================================================================

fn convert_to_as3(
    out: &mut Project,
    includes: &BTreeMap<String, ResourceUnit>,
    report: &mut ConversionReport,
) -> Result<(), String> {
    let snapshot = out.clone();
    let mut lifted_per_symbol: Vec<BTreeMap<u32, Vec<LiftedObjectHandler>>> = Vec::new();
    for sym_idx in 0..out.symbols.len() {
        let lifted = lift_symbol_object_scripts(&snapshot, sym_idx, out, includes, report)?;
        lifted_per_symbol.push(lifted);
    }

    // Build stage index AFTER auto-naming stage instances so `this.btn_1` / `this.clip_1` types are known!
    let stage = ProjectStageIndex::build(out);
    let mut all_synthesized: BTreeMap<String, SynthesizedLinkage> = BTreeMap::new();

    // 1. Convert `SourceUnit` classes
    let mut converted_sources = Vec::new();
    for unit in &out.sources {
        if unit.language == ScriptLanguage::ActionScript3 && !unit.source.contains("#include") {
            converted_sources.push(unit.clone());
            continue;
        }
        match parse_class_unit(unit, includes) {
            Ok(parsed_cls) => {
                let emitted = emit_as3_class(&parsed_cls, &unit.path, &stage);
                report.issues.extend(emitted.issues);
                for syn in emitted.synthesized_linkages {
                    all_synthesized.insert(syn.linkage_id.clone(), syn);
                }
                let expected_path = format!("{}.as", parsed_cls.qualified_name.replace('.', "/"));
                let new_path = if unit.path.trim().is_empty() {
                    expected_path
                } else {
                    unit.path.clone()
                };
                converted_sources.push(SourceUnit {
                    id: unit.id.clone(),
                    path: new_path,
                    language: ScriptLanguage::ActionScript3,
                    source: emitted.source,
                });
                report.converted_source_units += 1;
            }
            Err(err) => {
                if unit.source.contains("class ") || unit.source.contains("interface ") {
                    return Err(format!("{}: {err}", unit.path));
                }
                retain_action_source(unit, report);
                converted_sources.push(unit.clone());
            }
        }
    }
    out.sources = converted_sources;

    // 2. Convert each Symbol's timeline `FrameScript`s (and merge lifted `ObjectScript`s)
    for (sym_idx, mut lifted_map) in lifted_per_symbol.into_iter().enumerate() {
        let sym_id = out.symbols[sym_idx].id.clone();
        let sym_name = out.symbols[sym_idx].name.clone();

        // Ensure every frame with lifted object handlers has a `FrameScript` slot
        ensure_handler_frames(&mut out.symbols[sym_idx].timeline, &lifted_map);

        for fs in &mut out.symbols[sym_idx].timeline.scripts {
            let mut frame_lifted = lifted_map.remove(&fs.frame).unwrap_or_default();
            if fs.script.source.trim().is_empty() && frame_lifted.is_empty() {
                fs.script.language = ScriptLanguage::ActionScript3;
                continue;
            }
            let loc = format!("{sym_name}[frame {}]", fs.frame + 1);
            let mut parsed = if fs.script.source.trim().is_empty() {
                hir::ParsedScript {
                    expanded_source: String::new(),
                    language: ScriptLanguage::ActionScript2,
                    imports: Vec::new(),
                    statements: Vec::new(),
                }
            } else {
                parse_timeline_script(&fs.script, "", includes)
                    .map_err(|e| format!("{loc}: {e}"))?
            };
            let (kept_stmts, inline_legacy) = extract_frame_legacy_events(&parsed.statements);
            parsed.statements = kept_stmts;
            frame_lifted.extend(inline_legacy);

            let emitted = emit_as3_timeline_script(&parsed, &frame_lifted, &sym_id, &loc, &stage);
            report.issues.extend(emitted.issues);
            for syn in emitted.synthesized_linkages {
                all_synthesized.insert(syn.linkage_id.clone(), syn);
            }
            fs.script.source = emitted.source;
            fs.script.language = ScriptLanguage::ActionScript3;
            report.converted_frame_scripts += 1;
        }
    }

    // 3. Ensure every `attachMovie` linkage symbol has `class_name` set and a concrete AS3 class in `out.sources`!
    let existing_classes: BTreeSet<String> = out
        .sources
        .iter()
        .filter_map(|u| parse_class_unit(u, &BTreeMap::new()).ok())
        .flat_map(|c| [c.short_name, c.qualified_name])
        .collect();

    for syn in all_synthesized.values() {
        for sym in &mut out.symbols {
            if sym.linkage_id.as_deref() == Some(syn.linkage_id.as_str())
                && sym
                    .class_name
                    .as_deref()
                    .is_none_or(|c| c.trim().is_empty())
            {
                sym.class_name = Some(syn.class_name.clone());
            }
        }
        let short = short_type_name(&syn.class_name);
        if !existing_classes.contains(&syn.class_name) && !existing_classes.contains(short) {
            let (base_import, base_short) = if syn.symbol_kind == SymbolKind::Button {
                ("flash.display.SimpleButton", "SimpleButton")
            } else {
                ("flash.display.MovieClip", "MovieClip")
            };
            let pkg_part = syn
                .class_name
                .rsplit_once('.')
                .map(|(p, _)| format!(" {p}"))
                .unwrap_or_default();
            let stub_src = format!(
                "package{pkg_part} {{\n    import {base_import};\n\n    public dynamic class {short} extends {base_short} {{\n        public function {short}() {{\n            super();\n        }}\n    }}\n}}\n"
            );
            let stub_path = format!("{}.as", syn.class_name.replace('.', "/"));
            out.sources.push(SourceUnit {
                id: new_id("source"),
                path: stub_path,
                language: ScriptLanguage::ActionScript3,
                source: stub_src,
            });
        }
    }

    Ok(())
}

// =========================================================================
// Target 2: ActionScript 2.0 (`AS3 -> AS2` and `AS1 -> AS2`)
// =========================================================================

fn convert_to_as2(
    out: &mut Project,
    includes: &BTreeMap<String, ResourceUnit>,
    report: &mut ConversionReport,
) -> Result<(), String> {
    let stage = ProjectStageIndex::build(out);
    let old_doc_class = out.document_class.take();
    if let Some(doc_cls) = &old_doc_class {
        for sym in &mut out.symbols {
            if sym.id == out.root && sym.class_name.as_deref() == Some(doc_cls.as_str()) {
                sym.class_name = None;
            }
        }
    }

    // 1. Convert `SourceUnit` classes (`package a.b { public class Foo }` -> `class a.b.Foo`)
    let mut converted_sources = Vec::new();
    for unit in &out.sources {
        if unit.language == ScriptLanguage::ActionScript2 && !unit.source.contains("#include") {
            converted_sources.push(unit.clone());
            continue;
        }
        match parse_class_unit(unit, includes) {
            Ok(parsed_cls) => {
                let emitted = emit_as2_class(&parsed_cls, &unit.path, &stage);
                report.issues.extend(emitted.issues);
                let new_path = emitted.normalized_path.unwrap_or_else(|| unit.path.clone());
                converted_sources.push(SourceUnit {
                    id: unit.id.clone(),
                    path: new_path,
                    language: ScriptLanguage::ActionScript2,
                    source: emitted.source,
                });
                report.converted_source_units += 1;
            }
            Err(err) => {
                if unit.source.contains("class ") || unit.source.contains("interface ") {
                    return Err(format!("{}: {err}", unit.path));
                }
                // `#include` snippet or AS1 procedural source unit
                let script = Script {
                    language: unit.language,
                    source: unit.source.clone(),
                };
                let parsed = parse_timeline_script(&script, &unit.path, includes)
                    .map_err(|e| format!("{}: {e}", unit.path))?;
                let emitted = emit_as2_timeline_script(&parsed, None, &unit.path, &stage);
                report.issues.extend(emitted.issues);
                converted_sources.push(SourceUnit {
                    id: unit.id.clone(),
                    path: unit.path.clone(),
                    language: ScriptLanguage::ActionScript2,
                    source: emitted.source,
                });
                report.converted_source_units += 1;
            }
        }
    }
    out.sources = converted_sources;

    // Ensure linked AS3 symbols without `linkage_id` get a `linkage_id` when converted to AS2
    for sym in &mut out.symbols {
        if sym.id != out.root
            && let Some(cls) = &sym.class_name
            && !cls.trim().is_empty()
            && sym
                .linkage_id
                .as_deref()
                .is_none_or(|l| l.trim().is_empty())
        {
            sym.linkage_id = Some(short_type_name(cls).to_owned());
        }
    }

    // 2. Convert timeline `FrameScript`s and `ObjectScript`s
    let root_id = out.root.clone();
    for sym in &mut out.symbols {
        let sym_id = sym.id.clone();
        let sym_name = sym.name.clone();

        for fs in &mut sym.timeline.scripts {
            if fs.script.source.trim().is_empty() {
                fs.script.language = ScriptLanguage::ActionScript2;
                continue;
            }
            let loc = format!("{sym_name}[frame {}]", fs.frame + 1);
            let parsed = parse_timeline_script(&fs.script, "", includes)
                .map_err(|e| format!("{loc}: {e}"))?;
            let emitted = emit_as2_timeline_script(&parsed, Some(&sym_id), &loc, &stage);
            report.issues.extend(emitted.issues);
            fs.script.source = emitted.source;
            fs.script.language = ScriptLanguage::ActionScript2;
            report.converted_frame_scripts += 1;
        }

        for layer in &mut sym.timeline.layers {
            for kf in &mut layer.keyframes {
                for el in &mut kf.elements {
                    for os in &mut el.scripts {
                        if os.script.source.trim().is_empty() {
                            os.script.language = ScriptLanguage::ActionScript2;
                            continue;
                        }
                        let loc = format!("{sym_name}::{}::{:?}", el.name, os.event);
                        let parsed = parse_timeline_script(&os.script, "", includes)
                            .map_err(|e| format!("{loc}: {e}"))?;
                        let emitted =
                            emit_as2_timeline_script(&parsed, Some(&sym_id), &loc, &stage);
                        report.issues.extend(emitted.issues);
                        os.script.source = emitted.source;
                        os.script.language = ScriptLanguage::ActionScript2;
                        report.converted_object_scripts += 1;
                    }
                }
            }
        }
    }

    // 3. If `project.document_class` was set in AS3, attach it directly onto root in AS2 and bind `_documentApp`!
    if let Some(doc_cls) = old_doc_class
        && !doc_cls.trim().is_empty()
        && let Some(root_sym) = out.symbols.iter_mut().find(|s| s.id == root_id)
    {
        root_sym.class_name = Some(doc_cls.clone());
        let bootstrap = "var _documentApp:Object = this;\n".to_owned();
        if let Some(f0) = root_sym
            .timeline
            .scripts
            .iter_mut()
            .find(|fs| fs.frame == 0)
        {
            f0.script.source = format!("{bootstrap}\n{}", f0.script.source);
        } else {
            let layer = root_sym.timeline.layers.first().map(|l| l.id.clone());
            root_sym.timeline.scripts.insert(
                0,
                FrameScript {
                    layer,
                    frame: 0,
                    script: Script {
                        language: ScriptLanguage::ActionScript2,
                        source: bootstrap,
                    },
                },
            );
            report.converted_frame_scripts += 1;
        }
    }

    Ok(())
}

// =========================================================================
// Target 3: JavaScript ES6+ (`AS1/AS2 -> JS` and `AS3 -> JS`)
// =========================================================================

fn convert_to_js(
    out: &mut Project,
    includes: &BTreeMap<String, ResourceUnit>,
    report: &mut ConversionReport,
) -> Result<(), String> {
    let snapshot = out.clone();
    let mut lifted_per_symbol: Vec<BTreeMap<u32, Vec<LiftedObjectHandler>>> = Vec::new();
    for sym_idx in 0..out.symbols.len() {
        let lifted = lift_symbol_object_scripts(&snapshot, sym_idx, out, includes, report)?;
        lifted_per_symbol.push(lifted);
    }

    // Ensure linked AS3 symbols without `linkage_id` get a `linkage_id` before building stage index
    for sym in &mut out.symbols {
        if sym.id != out.root
            && let Some(cls) = &sym.class_name
            && !cls.trim().is_empty()
            && sym
                .linkage_id
                .as_deref()
                .is_none_or(|l| l.trim().is_empty())
        {
            sym.linkage_id = Some(short_type_name(cls).to_owned());
        }
    }

    let stage = ProjectStageIndex::build(out);

    let mut class_to_linkage: BTreeMap<String, String> = BTreeMap::new();
    for sym in &out.symbols {
        if sym.id != out.root
            && let Some(cls) = &sym.class_name
            && !cls.trim().is_empty()
            && let Some(link) = &sym.linkage_id
            && !link.trim().is_empty()
        {
            class_to_linkage.insert(short_type_name(cls).to_owned(), link.clone());
        }
    }

    let old_doc_class = out.document_class.take();
    for sym in &mut out.symbols {
        sym.class_name = None;
    }

    // Pre-parse class units to build `module_paths_by_short` for ES module `import { X } from "./..."`
    let mut parsed_classes: Vec<(usize, ParsedClass)> = Vec::new();
    let mut module_paths_by_short: BTreeMap<String, String> = BTreeMap::new();
    for (idx, unit) in out.sources.iter().enumerate() {
        if !unit.language.is_actionscript() {
            continue;
        }
        match parse_class_unit(unit, includes) {
            Ok(cls) => {
                let mod_path = to_js_module_path(&unit.path, &cls.qualified_name);
                module_paths_by_short.insert(cls.short_name.clone(), mod_path);
                parsed_classes.push((idx, cls));
            }
            Err(err) => {
                if unit.source.contains("class ") || unit.source.contains("interface ") {
                    return Err(format!("{}: {err}", unit.path));
                }
            }
        }
    }
    // Also pre-register linkage class paths so `attachMovie` can import them cleanly
    for info in stage.linkages.values() {
        let short = short_type_name(&info.class_name).to_owned();
        module_paths_by_short
            .entry(short.clone())
            .or_insert_with(|| format!("{}.js", info.class_name.replace('.', "/")));
    }

    let mut all_synthesized: BTreeMap<String, SynthesizedLinkage> = BTreeMap::new();
    let mut new_sources = Vec::new();

    for (idx, parsed_cls) in &parsed_classes {
        let unit = &out.sources[*idx];
        let emitted = emit_js_class(
            parsed_cls,
            &unit.path,
            &unit.path,
            &stage,
            &module_paths_by_short,
            &class_to_linkage,
        );
        report.issues.extend(emitted.issues);
        for syn in emitted.synthesized_linkages {
            all_synthesized.insert(syn.linkage_id.clone(), syn);
        }
        new_sources.push(SourceUnit {
            id: unit.id.clone(),
            path: emitted
                .module_path
                .unwrap_or_else(|| to_js_module_path(&unit.path, &parsed_cls.qualified_name)),
            language: ScriptLanguage::JavaScript,
            source: emitted.source,
        });
        report.converted_source_units += 1;
    }

    let class_indices: BTreeSet<usize> = parsed_classes.iter().map(|(i, _)| *i).collect();
    for (idx, unit) in out.sources.iter().enumerate() {
        if !class_indices.contains(&idx) {
            if unit.language.is_actionscript() {
                retain_action_source(unit, report);
            }
            new_sources.push(unit.clone());
        }
    }
    out.sources = new_sources;

    // Convert each Symbol's timeline `FrameScript`s (and merge lifted `ObjectScript`s)
    for (sym_idx, mut lifted_map) in lifted_per_symbol.into_iter().enumerate() {
        let sym_id = out.symbols[sym_idx].id.clone();
        let sym_name = out.symbols[sym_idx].name.clone();

        ensure_handler_frames(&mut out.symbols[sym_idx].timeline, &lifted_map);

        for fs in &mut out.symbols[sym_idx].timeline.scripts {
            let mut frame_lifted = lifted_map.remove(&fs.frame).unwrap_or_default();
            // Native web frame code is already normalized. Only translate the object
            // handlers being added to this frame; retain its original JavaScript.
            let web_frame = fs.script.language == ScriptLanguage::JavaScript;
            if web_frame && frame_lifted.is_empty() {
                continue;
            }
            if fs.script.source.trim().is_empty() && frame_lifted.is_empty() {
                fs.script.language = ScriptLanguage::JavaScript;
                continue;
            }
            let loc = format!("{sym_name}[frame {}]", fs.frame + 1);
            let mut parsed = if web_frame || fs.script.source.trim().is_empty() {
                hir::ParsedScript {
                    expanded_source: String::new(),
                    language: ScriptLanguage::ActionScript2,
                    imports: Vec::new(),
                    statements: Vec::new(),
                }
            } else {
                parse_timeline_script(&fs.script, "", includes)
                    .map_err(|e| format!("{loc}: {e}"))?
            };
            let (kept_stmts, inline_legacy) = extract_frame_legacy_events(&parsed.statements);
            parsed.statements = kept_stmts;
            frame_lifted.extend(inline_legacy);

            let emitted = emit_js_timeline_script(
                &parsed,
                &frame_lifted,
                &sym_id,
                &loc,
                &stage,
                &module_paths_by_short,
            );
            report.issues.extend(emitted.issues);
            for syn in emitted.synthesized_linkages {
                all_synthesized.insert(syn.linkage_id.clone(), syn);
            }
            if web_frame {
                fs.script.source.push('\n');
                fs.script.source.push_str(&emitted.source);
            } else {
                fs.script.source = emitted.source;
            }
            fs.script.language = ScriptLanguage::JavaScript;
            report.converted_frame_scripts += 1;
        }
    }

    // Synthesize exported JS linkage stub modules for any `attachMovie` symbols without a class module!
    let existing_js_shorts: BTreeSet<String> = parsed_classes
        .into_iter()
        .map(|(_, c)| c.short_name)
        .collect();

    for syn in all_synthesized.values() {
        let short = short_type_name(&syn.class_name).to_owned();
        if !existing_js_shorts.contains(&short) {
            let mod_path = module_paths_by_short
                .get(&short)
                .cloned()
                .unwrap_or_else(|| format!("{short}.js"));
            let base_shim = display_base_shim_block("MovieClip", Some(&syn.linkage_id));
            let stub_src = format!(
                "{base_shim}\n\nexport class {short} extends MovieClip {{\n    constructor() {{\n        super();\n    }}\n}}\n"
            );
            out.sources.push(SourceUnit {
                id: new_id("source"),
                path: mod_path,
                language: ScriptLanguage::JavaScript,
                source: stub_src,
            });
        }
    }

    // Bootstrap `document_class` on root Frame 0 when converting AS3 -> JS so `new Main()` binds directly to root `this`!
    let root_id = out.root.clone();
    if let Some(doc_cls) = old_doc_class
        && !doc_cls.trim().is_empty()
        && let Some(root_sym) = out.symbols.iter_mut().find(|s| s.id == root_id)
    {
        let short = short_type_name(&doc_cls);
        let target_mod_path = module_paths_by_short
            .get(short)
            .cloned()
            .unwrap_or_else(|| format!("{}.js", doc_cls.replace('.', "/")));
        let spec = relative_module_specifier("", &target_mod_path);
        let import_line = format!("import {{ {short} }} from \"{spec}\";");
        let bootstrap_body = format!(
            "globalThis.__nfTrace = typeof trace === \"function\" ? trace : null;\nglobalThis.__nfPendingDisplayTarget = this;\nconst _documentApp = new {short}();"
        );
        if let Some(f0) = root_sym
            .timeline
            .scripts
            .iter_mut()
            .find(|fs| fs.frame == 0)
        {
            let existing = f0.script.source.trim();
            if existing.is_empty() {
                f0.script.source = format!("{import_line}\n\n{bootstrap_body}\n");
            } else {
                let mut existing_imports = Vec::new();
                let mut rest_lines = Vec::new();
                for line in existing.lines() {
                    if rest_lines.is_empty() && line.trim_start().starts_with("import ") {
                        existing_imports.push(line.to_owned());
                    } else {
                        rest_lines.push(line.to_owned());
                    }
                }
                if !existing_imports.iter().any(|l| l == &import_line) {
                    existing_imports.insert(0, import_line);
                }
                let rest = rest_lines.join("\n");
                let rest_trimmed = rest.trim_start();
                if rest_trimmed.is_empty() {
                    f0.script.source =
                        format!("{}\n\n{bootstrap_body}\n", existing_imports.join("\n"));
                } else {
                    f0.script.source = format!(
                        "{}\n\n{bootstrap_body}\n{rest_trimmed}\n",
                        existing_imports.join("\n")
                    );
                }
            }
            f0.script.language = ScriptLanguage::JavaScript;
        } else {
            let layer = root_sym.timeline.layers.first().map(|l| l.id.clone());
            root_sym.timeline.scripts.insert(
                0,
                FrameScript {
                    layer,
                    frame: 0,
                    script: Script {
                        language: ScriptLanguage::JavaScript,
                        source: format!("{import_line}\n\n{bootstrap_body}\n"),
                    },
                },
            );
            report.converted_frame_scripts += 1;
        }
    }

    Ok(())
}
