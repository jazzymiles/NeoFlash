//! Compiler errors panel (`Panel / Compiler errors`, `PP-04`; reference `panels-more-2.png`),
//! opened by Window ▸ Panels ▸ Compiler errors (`⌥F2`).
//!
//! Compiles project source units, timeline frame scripts and object event scripts against the
//! current `Project` snapshot (plus HTML5 publish warnings) without mutating document state.
//! Clicking `Go to source` opens the target script in Actions and moves the caret to the exact
//! 1-based `(line, column)`.
use crate::app::Editor;
use crate::panels::actions_sidebar::ScriptLocation;
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{ElementKind, Project};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocatedScriptDiagnostic {
    pub severity: neoflash_script::DiagnosticSeverity,
    pub code: Option<String>,
    pub message: String,
    pub formatted_message: String,
    pub language: neoflash_core::ScriptLanguage,
    pub symbol_id: Option<String>,
    pub symbol: Option<String>,
    pub layer: Option<String>,
    pub frame: Option<u32>,
    pub element_id: Option<String>,
    pub element_name: Option<String>,
    pub event: Option<neoflash_core::ObjectEvent>,
    pub path: Option<String>,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub location_label: String,
    pub script: Option<ScriptLocation>,
    pub included_path: Option<String>,
    pub quick_fixes: Vec<neoflash_script::QuickFix>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompilerDiagnosticItem {
    pub is_error: bool,
    pub message: String,
    pub location_label: String,
    pub script: Option<ScriptLocation>,
    pub included_path: Option<String>,
    pub line: usize,
    pub column: usize,
    pub quick_fixes: Vec<neoflash_script::QuickFix>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompilerErrorsState {
    pub(crate) fingerprint: Option<u64>,
    pub(crate) items: Vec<CompilerDiagnosticItem>,
}

fn project_diagnostics_fingerprint(project: &Project) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    project.name.hash(&mut hash);
    project.root.hash(&mut hash);
    project.document_class.hash(&mut hash);
    project.scenes.hash(&mut hash);
    project.script_language.hash(&mut hash);
    project.also_compile_compatible_scripts.hash(&mut hash);
    for unit in &project.sources {
        unit.id.hash(&mut hash);
        unit.path.hash(&mut hash);
        std::mem::discriminant(&unit.language).hash(&mut hash);
        unit.source.hash(&mut hash);
    }
    project.resources.len().hash(&mut hash);
    for resource in &project.resources {
        resource.id.hash(&mut hash);
        resource.path.hash(&mut hash);
        resource.media_type.hash(&mut hash);
        if crate::app::standalone_script::has_actionscript_include_extension(std::path::Path::new(
            &resource.path,
        )) {
            resource.data_base64.hash(&mut hash);
        }
    }
    for symbol in &project.symbols {
        symbol.id.hash(&mut hash);
        symbol.name.hash(&mut hash);
        std::mem::discriminant(&symbol.kind).hash(&mut hash);
        symbol.class_name.hash(&mut hash);
        symbol.linkage_id.hash(&mut hash);
        for fs in &symbol.timeline.scripts {
            fs.layer.hash(&mut hash);
            fs.frame.hash(&mut hash);
            std::mem::discriminant(&fs.script.language).hash(&mut hash);
            fs.script.source.hash(&mut hash);
        }
        for layer in &symbol.timeline.layers {
            layer.id.hash(&mut hash);
            layer.name.hash(&mut hash);
            for key in &layer.keyframes {
                key.frame.hash(&mut hash);
                for el in &key.elements {
                    el.id.hash(&mut hash);
                    el.name.hash(&mut hash);
                    std::mem::discriminant(&el.kind).hash(&mut hash);
                    match &el.kind {
                        ElementKind::Text { font_family, .. } => font_family.hash(&mut hash),
                        ElementKind::TextBox { format, .. } => format.font.hash(&mut hash),
                        ElementKind::Instance { symbol_id, .. } => symbol_id.hash(&mut hash),
                        _ => {}
                    }
                    for os in &el.scripts {
                        std::mem::discriminant(&os.event).hash(&mut hash);
                        std::mem::discriminant(&os.script.language).hash(&mut hash);
                        os.script.source.hash(&mut hash);
                    }
                }
            }
        }
    }
    hash.finish()
}

fn diagnostic_message(diagnostic: &neoflash_script::Diagnostic) -> String {
    diagnostic.code.as_ref().map_or_else(
        || diagnostic.message.clone(),
        |code| format!("[{code}] {}", diagnostic.message),
    )
}

fn collect_warnings_with_includes(
    script: &neoflash_core::Script,
    base_path: &str,
    default_path: Option<&str>,
    includes_map: &std::collections::BTreeMap<String, neoflash_core::ResourceUnit>,
) -> Vec<neoflash_script::Diagnostic> {
    if script.language.is_actionscript()
        && script.source.contains("include")
        && let Ok((expanded_script, origins, exp_sources)) =
            neoflash_core::AuthoredPlayback::expand_script_includes(script, base_path, includes_map)
    {
        let mut out = Vec::new();
        for mut warning in neoflash_script::check_warnings_with_path(&expanded_script, default_path)
        {
            if warning.severity == neoflash_script::DiagnosticSeverity::Info {
                continue;
            }
            if let Some((orig_path, orig_line)) = origins.get(warning.line.saturating_sub(1)) {
                if !orig_path.is_empty() && orig_path != base_path {
                    if neoflash_core::AuthoredPlayback::is_builtin_actionscript_include(
                        script.language,
                        orig_path,
                    ) {
                        continue;
                    }
                    let Some(inc_src) = exp_sources.get(orig_path) else {
                        continue;
                    };
                    warning.path = Some(orig_path.clone());
                    warning.line = *orig_line;
                    for qf in &mut warning.quick_fixes {
                        let qf_line = origins
                            .get(qf.line.saturating_sub(1))
                            .filter(|(p, _)| p == orig_path)
                            .map_or(*orig_line, |(_, l)| *l);
                        let byte_len = qf.span.1.saturating_sub(qf.span.0);
                        qf.line = qf_line;
                        let start =
                            neoflash_script::line_col_to_byte_offset(inc_src, qf.line, qf.column);
                        qf.span = (start, (start + byte_len).min(inc_src.len()));
                    }
                    out.push(warning.to_diagnostic_with_source(inc_src));
                } else {
                    warning.path = default_path.map(str::to_owned);
                    warning.line = *orig_line;
                    for qf in &mut warning.quick_fixes {
                        let qf_line = origins
                            .get(qf.line.saturating_sub(1))
                            .filter(|(p, _)| p == orig_path)
                            .map_or(*orig_line, |(_, l)| *l);
                        let byte_len = qf.span.1.saturating_sub(qf.span.0);
                        qf.line = qf_line;
                        let start = neoflash_script::line_col_to_byte_offset(
                            &script.source,
                            qf.line,
                            qf.column,
                        );
                        qf.span = (start, (start + byte_len).min(script.source.len()));
                    }
                    out.push(warning.to_diagnostic_with_source(&script.source));
                }
            }
        }
        return out;
    }

    neoflash_script::check_warnings_with_path(script, default_path)
        .iter()
        .filter(|w| w.severity != neoflash_script::DiagnosticSeverity::Info)
        .map(|warning| warning.to_diagnostic_with_source(&script.source))
        .collect()
}

fn script_diagnostics_with_includes(
    script: &neoflash_core::Script,
    includes_map: &std::collections::BTreeMap<String, neoflash_core::ResourceUnit>,
    result: Result<String, Vec<neoflash_script::Diagnostic>>,
) -> Vec<neoflash_script::Diagnostic> {
    let mut diagnostics = result.err().unwrap_or_default();
    diagnostics.extend(collect_warnings_with_includes(
        script,
        "",
        None,
        includes_map,
    ));
    diagnostics
        .retain(|diagnostic| diagnostic.severity != neoflash_script::DiagnosticSeverity::Info);
    diagnostics
}

fn rewrite_top_level_includes_for_logical_path(
    language: neoflash_core::ScriptLanguage,
    source: &str,
    logical_path: &str,
) -> String {
    if !logical_path.contains('/') || !source.contains("include") || !language.is_actionscript() {
        return source.to_owned();
    }
    let mut out = String::with_capacity(source.len());
    for chunk in source.split_inclusive('\n') {
        let (body, ending) = if let Some(b) = chunk.strip_suffix("\r\n") {
            (b, "\r\n")
        } else if let Some(b) = chunk.strip_suffix('\n') {
            (b, "\n")
        } else {
            (chunk, "")
        };
        let trimmed = body.trim_start();
        let rewritten = if let Some(rest) = trimmed.strip_prefix("#include").or_else(|| {
            (language == neoflash_core::ScriptLanguage::ActionScript3)
                .then(|| trimmed.strip_prefix("include"))
                .flatten()
        }) && let Some(first_quote_idx) = rest.find(['"', '\''])
        {
            let quote_ch = rest[first_quote_idx..].chars().next().unwrap();
            let after_open = &rest[first_quote_idx + quote_ch.len_utf8()..];
            if let Some(end_quote_idx) = after_open.find(quote_ch) {
                let raw_req = &after_open[..end_quote_idx];
                if let Ok(resolved) =
                    crate::app::standalone_script::resolve_include_logical_path_as_or_asi(
                        logical_path,
                        raw_req,
                    )
                {
                    let leading_ws_len = body.len() - trimmed.len();
                    let kw_len = trimmed.len() - rest.len();
                    let prefix = &body[..leading_ws_len + kw_len + first_quote_idx + 1];
                    let suffix = &after_open[end_quote_idx..];
                    Some(format!("{prefix}{resolved}{suffix}"))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        out.push_str(rewritten.as_deref().unwrap_or(body));
        out.push_str(ending);
    }
    out
}

fn foreign_policy_warning(
    disposition: neoflash_core::ScriptDisposition,
    script_lang: neoflash_core::ScriptLanguage,
    proj_lang: neoflash_core::ScriptLanguage,
) -> Option<(&'static str, String)> {
    match disposition {
        neoflash_core::ScriptDisposition::CompiledPrimary => None,
        neoflash_core::ScriptDisposition::CompiledCompatibleWithWarning => Some((
            "compatible-script-language",
            format!(
                "Compiled as compatible language: script is {}, project uses {}.",
                script_lang.display_label(),
                proj_lang.display_label()
            ),
        )),
        neoflash_core::ScriptDisposition::SkippedForeign => Some((
            "skipped-foreign-script",
            format!(
                "Skipped when publishing: script is {}, project uses {}.",
                script_lang.display_label(),
                proj_lang.display_label()
            ),
        )),
    }
}

/// Collect structured, located script diagnostics (compile errors and warnings) across all
/// project sources, sidecar `.as` files, frame scripts, and element handlers without mutating
/// document state.
pub(crate) fn collect_located_script_diagnostics(
    project: &Project,
) -> Vec<LocatedScriptDiagnostic> {
    let mut items = Vec::new();
    let policy_lang = project.script_language;
    let also_compat = project.also_compile_compatible_scripts;

    if let Some(proj_lang) = policy_lang {
        for unit in &project.sources {
            let disp = project.script_disposition_for(proj_lang, also_compat, unit.language);
            if let Some((code, msg)) = foreign_policy_warning(disp, unit.language, proj_lang) {
                items.push(LocatedScriptDiagnostic {
                    severity: neoflash_script::DiagnosticSeverity::Warning,
                    code: Some(code.to_owned()),
                    message: msg.clone(),
                    formatted_message: msg,
                    language: unit.language,
                    symbol_id: None,
                    symbol: None,
                    layer: None,
                    frame: None,
                    element_id: None,
                    element_name: None,
                    event: None,
                    path: Some(unit.path.clone()),
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                    location_label: format!("{} · Line 1", unit.path),
                    script: Some(ScriptLocation::Source(unit.id.clone())),
                    included_path: None,
                    quick_fixes: Vec::new(),
                });
            }
        }
    }

    let includes_map: std::collections::BTreeMap<String, neoflash_core::ResourceUnit> = project
        .resources
        .iter()
        .filter(|r| {
            crate::app::standalone_script::has_actionscript_include_extension(std::path::Path::new(
                &r.path,
            ))
        })
        .map(|r| (r.path.clone(), r.clone()))
        .collect();

    let program = neoflash_script::compile_project(project);
    let mut source_diagnostics = program.as_ref().err().cloned().unwrap_or_default();
    for unit in &project.sources {
        if let Some(proj_lang) = policy_lang
            && !project
                .script_disposition_for(proj_lang, also_compat, unit.language)
                .is_compiled()
        {
            continue;
        }
        let script = neoflash_core::Script {
            language: unit.language,
            source: unit.source.clone(),
        };
        source_diagnostics.extend(collect_warnings_with_includes(
            &script,
            &unit.path,
            Some(&unit.path),
            &includes_map,
        ));
    }
    // Informational code hints remain in Actions; compiler diagnostics cover Error/Warning.
    for diagnostic in source_diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity != neoflash_script::DiagnosticSeverity::Info)
    {
        let unit = diagnostic
            .path
            .as_deref()
            .and_then(|path| project.sources.iter().find(|unit| unit.path == path));
        let included_path = if unit.is_none() {
            diagnostic
                .path
                .as_deref()
                .filter(|p| {
                    crate::app::standalone_script::has_actionscript_include_extension(
                        std::path::Path::new(p),
                    )
                })
                .map(str::to_owned)
        } else {
            None
        };
        let included_res_text = included_path.as_deref().and_then(|inc| {
            project
                .resources
                .iter()
                .find(|r| r.path == inc || r.path.eq_ignore_ascii_case(inc))
                .and_then(|r| r.decoded_bytes().ok())
                .and_then(|b| String::from_utf8(b).ok())
        });
        let quick_fixes = unit
            .map(|unit| {
                crate::code_tools::suggest_quick_fixes_for_diagnostic(&diagnostic, &unit.source)
            })
            .or_else(|| {
                included_res_text.as_deref().map(|src| {
                    crate::code_tools::suggest_quick_fixes_for_diagnostic(&diagnostic, src)
                })
            })
            .unwrap_or_default();
        let path = unit
            .map(|unit| unit.path.clone())
            .or_else(|| diagnostic.path.clone());
        let location_label = format!(
            "{} · Line {}",
            unit.map_or(path.as_deref().unwrap_or("Project"), |unit| unit
                .path
                .as_str()),
            diagnostic.line
        );
        items.push(LocatedScriptDiagnostic {
            severity: diagnostic.severity,
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            formatted_message: diagnostic_message(&diagnostic),
            language: diagnostic.language,
            symbol_id: None,
            symbol: None,
            layer: None,
            frame: None,
            element_id: None,
            element_name: None,
            event: None,
            path,
            line: diagnostic.line,
            column: diagnostic.column,
            end_line: diagnostic.end_line,
            end_column: diagnostic.end_column,
            location_label,
            script: unit.map(|unit| ScriptLocation::Source(unit.id.clone())),
            included_path,
            quick_fixes,
        });
    }

    let fallback_program = program
        .is_err()
        .then(|| neoflash_script::compile_project(&Project::blank()).ok())
        .flatten();
    let active_program = program.as_ref().ok().or(fallback_program.as_ref());

    for symbol in &project.symbols {
        for fs in &symbol.timeline.scripts {
            if fs.script.source.trim().is_empty() {
                continue;
            }
            let actions_layer_index = symbol
                .timeline
                .action_layer_for_script(fs)
                .or_else(|| {
                    symbol
                        .timeline
                        .layers
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, layer)| layer.name.eq_ignore_ascii_case("actions"))
                        .map(|(index, _)| index)
                })
                .or_else(|| symbol.timeline.layers.len().checked_sub(1));
            let actions_layer = actions_layer_index
                .and_then(|index| symbol.timeline.layers.get(index))
                .map(|layer| layer.name.as_str());
            let frame_path = format!("{} / frame {}", symbol.name, fs.frame + 1);
            if let Some(proj_lang) = policy_lang {
                let disp =
                    project.script_disposition_for(proj_lang, also_compat, fs.script.language);
                if let Some((code, msg)) =
                    foreign_policy_warning(disp, fs.script.language, proj_lang)
                {
                    let location_label = match actions_layer {
                        Some(layer_name) => format!(
                            "{} · layer {:?} · frame {} · Line 1",
                            symbol.name,
                            layer_name,
                            fs.frame + 1,
                        ),
                        None => format!("{} · frame {} · Line 1", symbol.name, fs.frame + 1),
                    };
                    items.push(LocatedScriptDiagnostic {
                        severity: neoflash_script::DiagnosticSeverity::Warning,
                        code: Some(code.to_owned()),
                        message: msg.clone(),
                        formatted_message: msg,
                        language: fs.script.language,
                        symbol_id: Some(symbol.id.clone()),
                        symbol: Some(symbol.name.clone()),
                        layer: actions_layer.map(str::to_owned),
                        frame: Some(fs.frame),
                        element_id: None,
                        element_name: None,
                        event: None,
                        path: Some(frame_path.clone()),
                        line: 1,
                        column: 1,
                        end_line: 1,
                        end_column: 1,
                        location_label,
                        script: Some(ScriptLocation::Frame {
                            symbol: symbol.id.clone(),
                            layer: fs.layer.clone(),
                            frame: fs.frame,
                        }),
                        included_path: None,
                        quick_fixes: Vec::new(),
                    });
                }
                if !disp.is_compiled() {
                    continue;
                }
            }
            let result = match active_program {
                Some(prog) => neoflash_script::compile_with_program(&fs.script, prog),
                None => neoflash_script::compile(&fs.script),
            };
            for err in script_diagnostics_with_includes(&fs.script, &includes_map, result) {
                let inc_path = err.path.clone();
                let inc_source = inc_path.as_deref().and_then(|inc| {
                    project
                        .resources
                        .iter()
                        .find(|r| r.path == inc || r.path.eq_ignore_ascii_case(inc))
                        .and_then(|r| r.decoded_bytes().ok())
                        .and_then(|b| String::from_utf8(b).ok())
                });
                let quick_fixes = if let Some(src) = inc_source.as_deref() {
                    crate::code_tools::suggest_quick_fixes_for_diagnostic(&err, src)
                } else if inc_path.is_none() {
                    crate::code_tools::suggest_quick_fixes_for_diagnostic(&err, &fs.script.source)
                } else {
                    Vec::new()
                };
                let location_label = match (actions_layer, inc_path.as_deref()) {
                    (Some(layer_name), Some(inc)) => format!(
                        "{} · layer {:?} · frame {} ({inc}) · Line {}",
                        symbol.name,
                        layer_name,
                        fs.frame + 1,
                        err.line
                    ),
                    (None, Some(inc)) => format!(
                        "{} · frame {} ({inc}) · Line {}",
                        symbol.name,
                        fs.frame + 1,
                        err.line
                    ),
                    (Some(layer_name), None) => format!(
                        "{} · layer {:?} · frame {} · Line {}",
                        symbol.name,
                        layer_name,
                        fs.frame + 1,
                        err.line
                    ),
                    (None, None) => format!(
                        "{} · frame {} · Line {}",
                        symbol.name,
                        fs.frame + 1,
                        err.line
                    ),
                };
                items.push(LocatedScriptDiagnostic {
                    severity: err.severity,
                    code: err.code.clone(),
                    message: err.message.clone(),
                    formatted_message: diagnostic_message(&err),
                    language: err.language,
                    symbol_id: Some(symbol.id.clone()),
                    symbol: Some(symbol.name.clone()),
                    layer: actions_layer.map(str::to_owned),
                    frame: Some(fs.frame),
                    element_id: None,
                    element_name: None,
                    event: None,
                    path: Some(inc_path.clone().unwrap_or_else(|| frame_path.clone())),
                    line: err.line,
                    column: err.column,
                    end_line: err.end_line,
                    end_column: err.end_column,
                    location_label,
                    script: Some(ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        layer: fs.layer.clone(),
                        frame: fs.frame,
                    }),
                    included_path: inc_path,
                    quick_fixes,
                });
            }
        }

        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    for os in &el.scripts {
                        if os.script.source.trim().is_empty() {
                            continue;
                        }
                        let target = if el.name.is_empty() {
                            "object"
                        } else {
                            el.name.as_str()
                        };
                        let obj_path = format!(
                            "{} / {} / frame {} / {} / {:?}",
                            symbol.name,
                            layer.name,
                            key.frame + 1,
                            target,
                            os.event
                        );
                        if let Some(proj_lang) = policy_lang {
                            let disp = project.script_disposition_for(
                                proj_lang,
                                also_compat,
                                os.script.language,
                            );
                            if let Some((code, msg)) =
                                foreign_policy_warning(disp, os.script.language, proj_lang)
                            {
                                items.push(LocatedScriptDiagnostic {
                                    severity: neoflash_script::DiagnosticSeverity::Warning,
                                    code: Some(code.to_owned()),
                                    message: msg.clone(),
                                    formatted_message: msg,
                                    language: os.script.language,
                                    symbol_id: Some(symbol.id.clone()),
                                    symbol: Some(symbol.name.clone()),
                                    layer: Some(layer.name.clone()),
                                    frame: Some(key.frame),
                                    element_id: Some(el.id.clone()),
                                    element_name: (!el.name.is_empty()).then(|| el.name.clone()),
                                    event: Some(os.event),
                                    path: Some(obj_path.clone()),
                                    line: 1,
                                    column: 1,
                                    end_line: 1,
                                    end_column: 1,
                                    location_label: format!(
                                        "{} · layer {:?} · frame {} · {:?} ({target}) · Line 1",
                                        symbol.name,
                                        layer.name,
                                        key.frame + 1,
                                        os.event,
                                    ),
                                    script: Some(ScriptLocation::Object {
                                        symbol: symbol.id.clone(),
                                        frame: key.frame,
                                        element: el.id.clone(),
                                        event: os.event,
                                    }),
                                    included_path: None,
                                    quick_fixes: Vec::new(),
                                });
                            }
                            if !disp.is_compiled() {
                                continue;
                            }
                        }
                        let result = match active_program {
                            Some(prog) => neoflash_script::compile_with_program(&os.script, prog),
                            None => neoflash_script::compile(&os.script),
                        };
                        for err in
                            script_diagnostics_with_includes(&os.script, &includes_map, result)
                        {
                            let inc_path = err.path.clone();
                            let inc_source = inc_path.as_deref().and_then(|inc| {
                                project
                                    .resources
                                    .iter()
                                    .find(|r| r.path == inc || r.path.eq_ignore_ascii_case(inc))
                                    .and_then(|r| r.decoded_bytes().ok())
                                    .and_then(|b| String::from_utf8(b).ok())
                            });
                            let quick_fixes = if let Some(src) = inc_source.as_deref() {
                                crate::code_tools::suggest_quick_fixes_for_diagnostic(&err, src)
                            } else if inc_path.is_none() {
                                crate::code_tools::suggest_quick_fixes_for_diagnostic(
                                    &err,
                                    &os.script.source,
                                )
                            } else {
                                Vec::new()
                            };
                            let location_label = match inc_path.as_deref() {
                                Some(inc) => format!(
                                    "{} · layer {:?} · frame {} · {:?} ({target}, {inc}) · Line {}",
                                    symbol.name,
                                    layer.name,
                                    key.frame + 1,
                                    os.event,
                                    err.line
                                ),
                                None => format!(
                                    "{} · layer {:?} · frame {} · {:?} ({target}) · Line {}",
                                    symbol.name,
                                    layer.name,
                                    key.frame + 1,
                                    os.event,
                                    err.line
                                ),
                            };
                            items.push(LocatedScriptDiagnostic {
                                severity: err.severity,
                                code: err.code.clone(),
                                message: err.message.clone(),
                                formatted_message: diagnostic_message(&err),
                                language: err.language,
                                symbol_id: Some(symbol.id.clone()),
                                symbol: Some(symbol.name.clone()),
                                layer: Some(layer.name.clone()),
                                frame: Some(key.frame),
                                element_id: Some(el.id.clone()),
                                element_name: (!el.name.is_empty()).then(|| el.name.clone()),
                                event: Some(os.event),
                                path: Some(inc_path.clone().unwrap_or_else(|| obj_path.clone())),
                                line: err.line,
                                column: err.column,
                                end_line: err.end_line,
                                end_column: err.end_column,
                                location_label,
                                script: Some(ScriptLocation::Object {
                                    symbol: symbol.id.clone(),
                                    frame: key.frame,
                                    element: el.id.clone(),
                                    event: os.event,
                                }),
                                included_path: inc_path,
                                quick_fixes,
                            });
                        }
                    }
                }
            }
        }
    }

    items
}

pub(crate) fn collect_compiler_diagnostics(project: &Project) -> Vec<CompilerDiagnosticItem> {
    let resolved = project.with_resolved_script_language();
    let mut items: Vec<CompilerDiagnosticItem> = collect_located_script_diagnostics(&resolved)
        .into_iter()
        .map(|diag| CompilerDiagnosticItem {
            is_error: diag.severity == neoflash_script::DiagnosticSeverity::Error,
            message: diag.formatted_message,
            location_label: diag.location_label,
            script: diag.script,
            included_path: diag.included_path,
            line: diag.line,
            column: diag.column,
            quick_fixes: diag.quick_fixes,
        })
        .collect();

    for warning in neoflash_export::export_warnings_without_foreign_scripts(&resolved) {
        items.push(CompilerDiagnosticItem {
            is_error: false,
            message: warning,
            location_label: format!("{} · HTML5 publish", resolved.name),
            script: None,
            included_path: None,
            line: 1,
            column: 1,
            quick_fixes: Vec::new(),
        });
    }

    items
}

impl Editor {
    /// Bring current Actions diagnostics into the existing panel, including advisory semantic
    /// warnings that the project compiler does not emit. The caller supplies only diagnostics
    /// from the open script's current checked fingerprint; ordinary refresh retires added rows.
    pub(crate) fn show_actions_compiler_diagnostics(
        &mut self,
        diagnostics: &[neoflash_script::Diagnostic],
    ) {
        use neoflash_script::DiagnosticSeverity;
        self.refresh_compiler_diagnostics();
        if let Some(location) = self.current_script_location() {
            let source_text = self.script_source_for(&location).unwrap_or_default();
            let label = match &location {
                ScriptLocation::Source(id) => self
                    .project
                    .sources
                    .iter()
                    .find(|unit| &unit.id == id)
                    .map_or_else(|| "Project source".into(), |unit| unit.path.clone()),
                ScriptLocation::Frame { symbol, frame, .. } => format!(
                    "{} · frame {}",
                    self.project
                        .symbol(symbol)
                        .map_or("Timeline", |symbol| symbol.name.as_str()),
                    frame + 1,
                ),
                ScriptLocation::Object {
                    symbol,
                    frame,
                    element,
                    event,
                } => {
                    let symbol = self.project.symbol(symbol);
                    let name = symbol
                        .and_then(|symbol| {
                            symbol
                                .timeline
                                .layers
                                .iter()
                                .filter_map(|layer| layer.key_at(*frame))
                                .flat_map(|key| &key.elements)
                                .find(|candidate| &candidate.id == element)
                        })
                        .map_or("object", |element| {
                            if element.name.is_empty() {
                                "object"
                            } else {
                                &element.name
                            }
                        });
                    format!(
                        "{} · frame {} · {event:?} ({name})",
                        symbol.map_or("Timeline", |symbol| symbol.name.as_str()),
                        frame + 1
                    )
                }
            };
            for diagnostic in diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity != DiagnosticSeverity::Info)
            {
                let message = diagnostic_message(diagnostic);
                let is_error = diagnostic.severity == DiagnosticSeverity::Error;
                if self.compiler_errors.items.iter().any(|item| {
                    item.script.as_ref() == Some(&location)
                        && item.line == diagnostic.line
                        && item.column == diagnostic.column
                        && item.message == message
                        && item.is_error == is_error
                }) {
                    continue;
                }
                let inc_path = diagnostic.path.clone().filter(|p| {
                    crate::app::standalone_script::has_actionscript_include_extension(
                        std::path::Path::new(p),
                    ) && !self.project.sources.iter().any(|u| u.path == *p)
                });
                let quick_fixes = if let Some(ref inc) = inc_path {
                    let inc_text = self
                        .resolve_external_as_path_for_navigation(None, std::path::Path::new(inc))
                        .and_then(|disk_path| {
                            self.open_standalone_as_tab_buffers()
                                .into_iter()
                                .find(|(p, _)| crate::document_sessions::same_path(p, &disk_path))
                                .map(|(_, s)| s)
                                .or_else(|| std::fs::read_to_string(disk_path).ok())
                        })
                        .or_else(|| {
                            self.project
                                .resources
                                .iter()
                                .find(|r| r.path == *inc || r.path.eq_ignore_ascii_case(inc))
                                .and_then(|r| r.decoded_bytes().ok())
                                .and_then(|b| String::from_utf8(b).ok())
                        });
                    inc_text
                        .as_deref()
                        .map(|txt| {
                            crate::code_tools::suggest_quick_fixes_for_diagnostic(diagnostic, txt)
                        })
                        .unwrap_or_default()
                } else {
                    crate::code_tools::suggest_quick_fixes_for_diagnostic(diagnostic, &source_text)
                };
                let loc_label = match inc_path.as_deref() {
                    Some(inc) => format!("{label} ({inc}) · Line {}", diagnostic.line),
                    None => format!("{label} · Line {}", diagnostic.line),
                };
                self.compiler_errors.items.push(CompilerDiagnosticItem {
                    is_error,
                    message,
                    location_label: loc_label,
                    script: Some(location.clone()),
                    included_path: inc_path,
                    line: diagnostic.line,
                    column: diagnostic.column,
                    quick_fixes,
                });
            }
        }
        if !diagnostics.is_empty()
            && diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == DiagnosticSeverity::Info)
        {
            self.message(
                "Informational hints remain in Actions; Compiler errors shows errors and warnings.",
            );
        }
        self.show_panel(crate::workspace::PanelId::CompilerErrors);
    }

    pub(crate) fn refresh_compiler_diagnostics(&mut self) {
        let (hydrated, include_fp) = self.hydrated_project_and_fingerprint();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        project_diagnostics_fingerprint(&self.project).hash(&mut hasher);
        self.standalone_script.hash(&mut hasher);
        include_fp.hash(&mut hasher);
        let fp = hasher.finish();
        if self.compiler_errors.fingerprint != Some(fp) {
            self.compiler_errors.items = if self.is_standalone_script_session()
                && let Some(unit) = self.project.sources.first()
                && !Self::is_standalone_class_or_package_unit(unit)
            {
                let mut standalone_proj = hydrated.into_owned();
                let logical_unit_path = standalone_proj
                    .sources
                    .first()
                    .map_or_else(|| unit.path.clone(), |u| u.path.clone());
                let rewritten_source = rewrite_top_level_includes_for_logical_path(
                    unit.language,
                    &unit.source,
                    &logical_unit_path,
                );
                standalone_proj.sources.clear();
                standalone_proj.document_class = None;
                standalone_proj.script_language = Some(unit.language);
                if let Some(root_sym) = standalone_proj.symbols.first_mut() {
                    root_sym.name = unit.path.clone();
                    root_sym.timeline.scripts = vec![neoflash_core::FrameScript {
                        layer: None,
                        frame: 0,
                        script: neoflash_core::Script {
                            language: unit.language,
                            source: rewritten_source,
                        },
                    }];
                }
                collect_located_script_diagnostics(&standalone_proj)
                    .into_iter()
                    .map(|diag| CompilerDiagnosticItem {
                        is_error: diag.severity == neoflash_script::DiagnosticSeverity::Error,
                        message: diag.formatted_message,
                        location_label: match diag.included_path.as_deref() {
                            Some(inc) => format!("{inc} · Line {}", diag.line),
                            None => format!("{} · Line {}", unit.path, diag.line),
                        },
                        script: diag
                            .included_path
                            .is_none()
                            .then(|| ScriptLocation::Source(unit.id.clone())),
                        included_path: diag.included_path,
                        line: diag.line,
                        column: diag.column,
                        quick_fixes: diag.quick_fixes,
                    })
                    .collect()
            } else if self.is_standalone_script_session() {
                let resolved = hydrated.with_resolved_script_language();
                collect_located_script_diagnostics(&resolved)
                    .into_iter()
                    .map(|diag| CompilerDiagnosticItem {
                        is_error: diag.severity == neoflash_script::DiagnosticSeverity::Error,
                        message: diag.formatted_message,
                        location_label: diag.location_label,
                        script: diag.script,
                        included_path: diag.included_path,
                        line: diag.line,
                        column: diag.column,
                        quick_fixes: diag.quick_fixes,
                    })
                    .collect()
            } else {
                collect_compiler_diagnostics(&hydrated)
            };
            self.compiler_errors.fingerprint = Some(fp);
        }
    }

    pub(crate) fn script_source_for(&self, location: &ScriptLocation) -> Option<String> {
        match location {
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                let timeline = &self.project.symbol(symbol)?.timeline;
                let script = layer
                    .as_deref()
                    .and_then(|id| {
                        timeline
                            .scripts
                            .iter()
                            .find(|fs| fs.frame == *frame && fs.layer.as_deref() == Some(id))
                    })
                    .or_else(|| {
                        timeline
                            .scripts
                            .iter()
                            .find(|fs| fs.frame == *frame && fs.layer.is_none())
                    })?;
                Some(script.script.source.clone())
            }
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => self
                .project
                .symbol(symbol)?
                .timeline
                .layers
                .iter()
                .filter_map(|layer| layer.key_at(*frame))
                .flat_map(|key| &key.elements)
                .find(|el| &el.id == element)?
                .scripts
                .iter()
                .find(|os| os.event == *event)
                .map(|os| os.script.source.clone()),
            ScriptLocation::Source(id) => self
                .project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map(|u| u.source.clone()),
        }
    }

    pub(crate) fn jump_to_compiler_diagnostic(&mut self, ctx: &egui::Context, idx: usize) -> bool {
        self.refresh_compiler_diagnostics();
        let Some(item) = self.compiler_errors.items.get(idx).cloned() else {
            return false;
        };
        if let Some(inc_path) = item.included_path.as_deref()
            && let Some(disk_path) =
                self.resolve_external_as_path_for_navigation(None, std::path::Path::new(inc_path))
            && self.open_standalone_script_session(ctx, disk_path)
        {
            let target_source = self.standalone_script_source().unwrap_or("").to_owned();
            let editor_id = self.actions_editor_id();
            crate::sources::jump_to(ctx, editor_id, &target_source, item.line, item.column);
            return true;
        }
        let Some(loc) = item.script else {
            return false;
        };
        let source = self.script_source_for(&loc).unwrap_or_default();
        self.open_script_location(loc);
        self.open_actions();
        self.sync_actions_diagnostics_after_navigation();
        let editor_id = self.actions_editor_id();
        crate::sources::jump_to(ctx, editor_id, &source, item.line, item.column);
        true
    }

    /// Apply a Quick-Fix from a diagnostic row in `Compiler errors`, recording a single 1-step
    /// Undo entry (`"Apply quick fix"`), opening the script in `Actions`, and refreshing diagnostics.
    pub(crate) fn apply_compiler_diagnostic_quick_fix(
        &mut self,
        ctx: &egui::Context,
        diag_idx: usize,
        fix_idx: usize,
    ) -> bool {
        self.refresh_compiler_diagnostics();
        let Some(item) = self.compiler_errors.items.get(diag_idx).cloned() else {
            return false;
        };
        let Some(fix) = item.quick_fixes.get(fix_idx).cloned() else {
            return false;
        };

        if let Some(inc_path) = item.included_path.as_deref() {
            let norm_inc = inc_path.trim().replace('\\', "/");

            // 1. Matching `project.sources` unit.
            if let Some((unit_id, current_source)) = self
                .project
                .sources
                .iter()
                .find(|u| {
                    let norm = u.path.trim().replace('\\', "/");
                    norm == norm_inc || norm.eq_ignore_ascii_case(&norm_inc)
                })
                .map(|u| (u.id.clone(), u.source.clone()))
            {
                let Some((updated_source, (caret, _))) =
                    crate::code_tools::apply_quick_fix_edit(&current_source, &fix)
                else {
                    return false;
                };
                let target_id = unit_id.clone();
                let updated_for_edit = updated_source.clone();
                self.transact_named("Apply quick fix", move |ed| {
                    if let Some(unit) = ed.project.sources.iter_mut().find(|u| u.id == target_id) {
                        unit.source = updated_for_edit;
                    }
                });
                self.invalidate_include_dependents_across_sessions();
                self.open_script_location(ScriptLocation::Source(unit_id));
                if !self.is_standalone_script_session() {
                    self.open_actions();
                }
                let editor_id = self.actions_editor_id();
                let (line, col) = c::code::line_col(&updated_source, caret);
                crate::sources::jump_to(ctx, editor_id, &updated_source, line, col);
                self.refresh_compiler_diagnostics();
                self.message(format!("Applied quick fix: {}", fix.title));
                return true;
            }

            // 2. External `.as` / `.asi` file resolved via `resolve_external_as_path_for_navigation`.
            if let Some(disk_path) =
                self.resolve_external_as_path_for_navigation(None, std::path::Path::new(&norm_inc))
                && self.open_standalone_script_session(ctx, disk_path)
            {
                let Some(current_source) = self.standalone_script_source().map(str::to_owned)
                else {
                    return false;
                };
                let Some((updated_source, (caret, _))) =
                    crate::code_tools::apply_quick_fix_edit(&current_source, &fix)
                else {
                    return false;
                };
                let updated_for_edit = updated_source.clone();
                self.transact_named("Apply quick fix", move |ed| {
                    if let Some(unit) = ed.project.sources.first_mut() {
                        unit.source = updated_for_edit;
                    }
                });
                self.invalidate_include_dependents_across_sessions();
                let editor_id = self.actions_editor_id();
                let (line, col) = c::code::line_col(&updated_source, caret);
                crate::sources::jump_to(ctx, editor_id, &updated_source, line, col);
                self.refresh_compiler_diagnostics();
                self.message(format!("Applied quick fix: {}", fix.title));
                return true;
            }

            // 3. Embedded `ResourceUnit` in `project.resources`.
            if let Some((res_id, res_path, res_media, current_source)) = self
                .project
                .resources
                .iter()
                .find(|r| r.path == norm_inc || r.path.eq_ignore_ascii_case(&norm_inc))
                .and_then(|r| {
                    let bytes = r.decoded_bytes().ok()?;
                    let text = String::from_utf8(bytes).ok()?;
                    Some((r.id.clone(), r.path.clone(), r.media_type.clone(), text))
                })
            {
                let Some((updated_source, _)) =
                    crate::code_tools::apply_quick_fix_edit(&current_source, &fix)
                else {
                    return false;
                };
                let Ok(updated_res) = neoflash_core::ResourceUnit::from_bytes(
                    res_id.clone(),
                    res_path,
                    res_media,
                    updated_source.as_bytes(),
                ) else {
                    return false;
                };
                self.transact_named("Apply quick fix", move |ed| {
                    if let Some(slot) = ed.project.resources.iter_mut().find(|r| r.id == res_id) {
                        *slot = updated_res;
                    }
                });
                self.invalidate_include_dependents_across_sessions();
                self.refresh_compiler_diagnostics();
                self.message(format!("Applied quick fix: {}", fix.title));
                return true;
            }

            // Never fall through to mutating `outer_loc` when `included_path` was set!
            return false;
        }

        let Some(loc) = item.script.clone() else {
            return false;
        };
        let Some(current_source) = self.script_source_for(&loc) else {
            return false;
        };
        let Some((updated_source, (caret, _))) =
            crate::code_tools::apply_quick_fix_edit(&current_source, &fix)
        else {
            return false;
        };

        let loc_for_edit = loc.clone();
        let updated_for_edit = updated_source.clone();
        self.transact_named("Apply quick fix", move |ed| match &loc_for_edit {
            ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            } => {
                if let Some(sym) = ed.project.symbols.iter_mut().find(|s| s.id == *symbol)
                    && let Some(fs) = sym.timeline.scripts.iter_mut().find(|fs| {
                        fs.frame == *frame
                            && match layer.as_deref() {
                                Some(id) => fs.layer.as_deref() == Some(id),
                                None => fs.layer.is_none(),
                            }
                    })
                {
                    fs.script.source = updated_for_edit;
                }
            }
            ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            } => {
                if let Some(sym) = ed.project.symbols.iter_mut().find(|s| s.id == *symbol) {
                    for layer in &mut sym.timeline.layers {
                        let Some(kf_frame) = layer.key_at(*frame).map(|k| k.frame) else {
                            continue;
                        };
                        if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == kf_frame)
                            && let Some(el) = key.elements.iter_mut().find(|e| e.id == *element)
                            && let Some(os) = el.scripts.iter_mut().find(|os| os.event == *event)
                        {
                            os.script.source = updated_for_edit;
                            break;
                        }
                    }
                }
            }
            ScriptLocation::Source(id) => {
                if let Some(unit) = ed.project.sources.iter_mut().find(|u| u.id == *id) {
                    unit.source = updated_for_edit;
                }
            }
        });

        if matches!(loc, ScriptLocation::Source(_)) || self.is_standalone_script_session() {
            self.invalidate_include_dependents_across_sessions();
        }
        self.open_script_location(loc);
        if !self.is_standalone_script_session() {
            self.open_actions();
        }
        let editor_id = self.actions_editor_id();
        let (line, col) = c::code::line_col(&updated_source, caret);
        crate::sources::jump_to(ctx, editor_id, &updated_source, line, col);
        self.refresh_compiler_diagnostics();
        self.message(format!("Applied quick fix: {}", fix.title));
        true
    }

    pub(crate) fn compiler_errors_panel_ui(&mut self, ui: &mut Ui) {
        self.refresh_compiler_diagnostics();
        let nf = neoflash_ui::palette(ui.ctx());
        let error_count = self
            .compiler_errors
            .items
            .iter()
            .filter(|i| i.is_error)
            .count();
        let warning_count = self.compiler_errors.items.len() - error_count;
        let warnings_badge = if warning_count == 1 {
            "1 warning".to_owned()
        } else {
            format!("{warning_count} warnings")
        };
        let errors_badge = if error_count == 1 {
            "1 error".to_owned()
        } else {
            format!("{error_count} errors")
        };

        ui.horizontal(|ui| {
            ui.set_height(neoflash_ui::metrics::control());
            ui.spacing_mut().item_spacing.x = space::s2();
            ui.add(c::Badge::new(&warnings_badge, c::BadgeType::Warning));
            ui.add(c::Badge::new(&errors_badge, c::BadgeType::Neutral));
        });
        ui.add_space(space::s1());

        if self.compiler_errors.items.is_empty() {
            c::EmptyState {
                icon: i16::CHECK,
                title: "No compiler errors",
                body: "Project sources, timeline scripts and publish checks passed.",
                action: None,
            }
            .show(ui);
            return;
        }

        let mut jump_target = None;
        let mut quick_fix_target = None;
        egui::ScrollArea::vertical()
            .id_salt("compiler-errors-scroll")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let gap = space::s2();
                let button = neoflash_ui::metrics::control();
                for (idx, item) in self.compiler_errors.items.iter().enumerate() {
                    let (icon, color) = if item.is_error {
                        (i16::ERROR, nf.danger_default)
                    } else {
                        (i16::WARNING, nf.warning_default)
                    };
                    let can_jump = item.script.is_some() || item.included_path.is_some();
                    // One record per row: icon, message, location, then its actions at the end.
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), button),
                        egui::Sense::hover(),
                    );
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        egui::Stroke::new(1.0_f32, nf.border_subtle),
                    );
                    let action_count = if item.quick_fixes.is_empty() {
                        1.0
                    } else {
                        2.0
                    };
                    let actions_width = action_count * button + (action_count - 1.0) * gap;
                    let actions = rect.with_min_x(rect.right() - actions_width - space::s1());
                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(
                            rect.left() + space::s1() + icon.side() / 2.0,
                            rect.center().y,
                        ),
                        egui::Vec2::splat(icon.side()),
                    );
                    icons::paint(ui.painter(), ui.ctx(), icon_rect, icon, color);
                    let text = egui::Rect::from_min_max(
                        egui::pos2(icon_rect.right() + gap, rect.top()),
                        egui::pos2(
                            (actions.left() - gap).max(icon_rect.right() + gap),
                            rect.bottom(),
                        ),
                    );
                    let line = |ui: &Ui, text: &str, style: TextStyle, color, width: f32| {
                        let mut job = egui::text::LayoutJob::single_section(
                            text.to_owned(),
                            neoflash_ui::typography::format_line(style, color),
                        );
                        job.wrap.max_width = width.max(0.0);
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                    let message = line(
                        ui,
                        &item.message,
                        TextStyle::Body,
                        nf.text_primary,
                        text.width(),
                    );
                    let message_width = message.size().x;
                    let location_room = text.width() - message_width - gap;
                    let location = (location_room > neoflash_ui::metrics::field_num()).then(|| {
                        line(
                            ui,
                            &item.location_label,
                            TextStyle::Small,
                            nf.text_secondary,
                            location_room,
                        )
                    });
                    let clipped =
                        message.elided || location.as_ref().is_none_or(|location| location.elided);
                    let painter = ui.painter().with_clip_rect(text.intersect(ui.clip_rect()));
                    painter.galley(
                        egui::pos2(text.left(), text.center().y - message.size().y / 2.0),
                        message,
                        nf.text_primary,
                    );
                    if let Some(location) = location {
                        painter.galley(
                            egui::pos2(
                                text.left() + message_width + gap,
                                text.center().y - location.size().y / 2.0,
                            ),
                            location,
                            nf.text_secondary,
                        );
                    }
                    let text_resp = ui.interact(
                        text,
                        ui.id().with(("compiler-error-row", idx)),
                        if can_jump {
                            egui::Sense::click()
                        } else {
                            egui::Sense::hover()
                        },
                    );
                    text_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            format!("{} {}", item.message, item.location_label),
                        )
                    });
                    let text_resp = if clipped {
                        // The row keeps one line; the full record is in its tooltip.
                        c::tooltip(
                            text_resp,
                            &format!("{}\n{}", item.message, item.location_label),
                            None,
                        )
                    } else {
                        text_resp
                    };
                    if can_jump && text_resp.clicked() {
                        jump_target = Some(idx);
                    }
                    let mut actions_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(actions.with_max_x(rect.right() - space::s1()))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    let ui = &mut actions_ui;
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.push_id(("compiler-error-actions", idx), |ui| {
                        if ui
                            .add_enabled(
                                can_jump,
                                c::IconButton::new(i16::EXTERNAL, "Go to source"),
                            )
                            .clicked()
                        {
                            jump_target = Some(idx);
                        }
                        if let Some(first_fix) = item.quick_fixes.first() {
                            let fix_resp = ui.add_enabled(
                                can_jump,
                                c::IconButton::new(i16::WAND, &first_fix.title),
                            );
                            if crate::ui_icons::hover_tooltip(
                                fix_resp,
                                &format!("Quick fix: {}", first_fix.title),
                                None,
                                Some("Fix target is unavailable in the current document"),
                            )
                            .clicked()
                            {
                                quick_fix_target = Some((idx, 0));
                            }
                        }
                    });
                }
            });

        if let Some((diag_idx, fix_idx)) = quick_fix_target {
            let ctx = ui.ctx().clone();
            self.apply_compiler_diagnostic_quick_fix(&ctx, diag_idx, fix_idx);
        } else if let Some(idx) = jump_target {
            let ctx = ui.ctx().clone();
            self.jump_to_compiler_diagnostic(&ctx, idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_api::{HintContext, ThisKind};
    use neoflash_core::{FrameScript, ResourceUnit, Script, ScriptLanguage};

    #[test]
    fn actions_semantic_handoff_navigates_once_and_retires_with_context_changes() {
        let ctx = egui::Context::default();
        let source = "function consume(point:Point):void { trace(point.x); }\nvar wrong:Rectangle = new Rectangle(0,0,10,10);\nconsume(wrong);";
        let mut project = Project::blank();
        project.format_version = 8;
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            },
        });
        project.resources.push(
            ResourceUnit::from_bytes("include", "support.as", "text/plain", b"var a = 1;").unwrap(),
        );
        project.validate().unwrap();
        let context =
            HintContext::new(&project, ScriptLanguage::ActionScript3, ThisKind::MovieClip).unwrap();
        let analysis = context.semantic_analysis(source);
        let diagnostic = analysis
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some("flow-argument-type"))
            .unwrap()
            .clone();
        let mut editor = Editor::from_project(&ctx, project);
        let original = editor.project.clone();
        editor.refresh_compiler_diagnostics();
        assert!(
            !editor
                .compiler_errors
                .items
                .iter()
                .any(|item| item.message.contains("flow-argument-type"))
        );
        editor.show_actions_compiler_diagnostics(std::slice::from_ref(&diagnostic));
        editor.show_actions_compiler_diagnostics(std::slice::from_ref(&diagnostic));
        let matching: Vec<_> = editor
            .compiler_errors
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.message.contains("flow-argument-type"))
            .collect();
        assert_eq!(matching.len(), 1);
        let (index, item) = matching[0];
        assert!(!item.is_error);
        assert_eq!(
            (item.line, item.column),
            (diagnostic.line, diagnostic.column)
        );
        assert!(editor.jump_to_compiler_diagnostic(&ctx, index));
        let caret = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id())
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
            .primary
            .index;
        assert_eq!(
            c::code::line_col(source, caret),
            (diagnostic.line, diagnostic.column)
        );
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());

        // An include can change without altering its path, kind or encoded length.
        editor.project.resources[0] =
            ResourceUnit::from_bytes("include", "support.as", "text/plain", b"var a = 2;").unwrap();
        editor.refresh_compiler_diagnostics();
        assert!(
            !editor
                .compiler_errors
                .items
                .iter()
                .any(|item| item.message.contains("flow-argument-type"))
        );

        let count = editor.compiler_errors.items.len();
        let mut info = diagnostic.clone();
        info.language = ScriptLanguage::TypeScript;
        info.message = "Add an explicit type annotation.".into();
        info.severity = neoflash_script::DiagnosticSeverity::Info;
        editor.show_actions_compiler_diagnostics(&[info]);
        assert_eq!(editor.compiler_errors.items.len(), count);
        assert!(
            editor
                .output
                .last()
                .unwrap()
                .contains("Informational hints remain in Actions")
        );
    }

    #[test]
    fn compiler_errors_panel_applies_quick_fixes_with_single_step_undo() {
        let ctx = egui::Context::default();
        let bad_frame_script = "let score: number = 10;\nscr = 20;";
        let bad_source_unit = "export function check(a: boolean, b: boolean): boolean {\n    let unusedFlag: number = 1;\n    return a && b;\n}\n";
        let mut project = Project::blank();
        project.format_version = 8;
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::TypeScript,
                source: bad_frame_script.into(),
            },
        });
        project.sources.push(neoflash_core::SourceUnit {
            id: "src-check".into(),
            path: "check.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: bad_source_unit.into(),
        });
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.refresh_compiler_diagnostics();

        // 1. Frame script typo diagnostic has a QuickFix ("Replace 'scr' with 'score'")
        let frame_diag_idx = editor
            .compiler_errors
            .items
            .iter()
            .position(|item| {
                item.message.contains("Did you mean 'score'") && !item.quick_fixes.is_empty()
            })
            .expect("expected frame script typo diagnostic with quick fix");
        assert!(editor.apply_compiler_diagnostic_quick_fix(&ctx, frame_diag_idx, 0));
        assert_eq!(
            editor.project.symbols[0].timeline.scripts[0].script.source,
            "let score: number = 10;\nscore = 20;"
        );
        assert!(editor.history.can_undo());
        assert_eq!(editor.history.undo_label(), Some("Apply quick fix"));
        assert!(
            !editor
                .compiler_errors
                .items
                .iter()
                .any(|item| item.message.contains("Did you mean 'score'"))
        );

        // 1-step Undo restores the exact original frame script and diagnostic
        editor.undo();
        assert_eq!(
            editor.project.symbols[0].timeline.scripts[0].script.source,
            bad_frame_script
        );
        editor.refresh_compiler_diagnostics();
        assert!(
            editor
                .compiler_errors
                .items
                .iter()
                .any(|item| item.message.contains("Did you mean 'score'"))
        );

        // 2. Project SourceUnit unused variable warning has a QuickFix ("Prefix with '_'")
        let src_diag_idx = editor
            .compiler_errors
            .items
            .iter()
            .position(|item| item.message.contains("unusedFlag") && !item.quick_fixes.is_empty())
            .expect("expected source unit unused variable warning with quick fix");
        assert!(editor.apply_compiler_diagnostic_quick_fix(&ctx, src_diag_idx, 0));
        assert!(
            editor.project.sources[0]
                .source
                .contains("let _unusedFlag: number = 1;")
        );
        editor.undo();
        assert_eq!(editor.project.sources[0].source, bad_source_unit);
        editor.project.validate().unwrap();
    }

    #[test]
    fn t414_quick_fix_on_included_warning_edits_helper_as_and_leaves_outer_frame_script_untouched()
    {
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-t414-inc-qf-{}-{}",
            std::process::id(),
            neoflash_core::new_id("dir")
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let helper_path = tmp.join("helper.as");
        std::fs::write(&helper_path, "// header\ntrace(not false);\n").unwrap();

        // Outer frame script intentionally has matching column text on line 2 to prove `outer_loc` is NEVER corrupted!
        let outer_source = "#include \"helper.as\"\ntrace(not true);\n";
        let mut project = Project::blank();
        project.name = "IncludeQuickFix".into();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let root_id = project.root.clone();
        project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: outer_source.into(),
                },
            });
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        let nflash_path = tmp.join("IncludeQuickFix.nflash");
        std::fs::write(&nflash_path, project.to_json().unwrap()).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        assert!(editor.try_open_path(nflash_path, &ctx));
        editor.refresh_compiler_diagnostics();

        let inc_diag_idx = editor
            .compiler_errors
            .items
            .iter()
            .position(|item| {
                item.included_path.as_deref() == Some("helper.as") && !item.quick_fixes.is_empty()
            })
            .expect("expected quick-fixable warning in included helper.as");

        assert!(editor.apply_compiler_diagnostic_quick_fix(&ctx, inc_diag_idx, 0));
        assert!(
            editor.is_standalone_script_session(),
            "applying quick-fix on disk helper.as must open/focus standalone .as session"
        );
        assert_eq!(editor.session_count(), 2);
        assert_eq!(editor.history.undo_label(), Some("Apply quick fix"));
        assert!(
            editor.project.sources[0].source.contains("trace(!"),
            "helper.as standalone session must receive the quick fix, got: {}",
            editor.project.sources[0].source
        );

        // Switch back to Tab 0 (`.nflash`): outer frame script must be 100% untouched and clean!
        assert!(editor.switch_session_tab(&ctx, 0));
        assert_eq!(
            editor.project.symbols[0].timeline.scripts[0].script.source,
            outer_source
        );
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());
        editor.refresh_compiler_diagnostics();
        assert!(
            !editor
                .compiler_errors
                .items
                .iter()
                .any(|item| item.included_path.as_deref() == Some("helper.as")),
            "helper.as warning must be resolved in dependent .nflash session via Tier-1 tab override"
        );

        // Also verify embedded `ResourceUnit` quick-fix leaves outer script untouched!
        let mut res_proj = Project::blank();
        res_proj.format_version = 3;
        res_proj.script_language = Some(ScriptLanguage::ActionScript2);
        res_proj.resources.push(
            ResourceUnit::from_bytes(
                "res-inc",
                "embedded.as",
                "text/plain",
                b"trace(not false);\n",
            )
            .unwrap(),
        );
        res_proj.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "#include \"embedded.as\"\nstop();\n".into(),
            },
        });
        let mut res_editor = Editor::from_project(&ctx, res_proj);
        res_editor.refresh_compiler_diagnostics();
        let res_diag_idx = res_editor
            .compiler_errors
            .items
            .iter()
            .position(|item| {
                item.included_path.as_deref() == Some("embedded.as") && !item.quick_fixes.is_empty()
            })
            .expect("expected quick-fixable warning in embedded.as resource");
        assert!(res_editor.apply_compiler_diagnostic_quick_fix(&ctx, res_diag_idx, 0));
        assert_eq!(
            res_editor.project.symbols[0].timeline.scripts[0]
                .script
                .source,
            "#include \"embedded.as\"\nstop();\n"
        );
        let updated_embedded =
            String::from_utf8(res_editor.project.resources[0].decoded_bytes().unwrap()).unwrap();
        assert!(updated_embedded.contains("trace(!"));
        assert!(res_editor.history.can_undo());
        res_editor.undo();

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
