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
use neoflash_ui::typography::rich;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompilerDiagnosticItem {
    pub is_error: bool,
    pub message: String,
    pub location_label: String,
    pub script: Option<ScriptLocation>,
    pub line: usize,
    pub column: usize,
    pub quick_fixes: Vec<neoflash_script::QuickFix>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompilerErrorsState {
    fingerprint: Option<u64>,
    pub(crate) items: Vec<CompilerDiagnosticItem>,
}

fn project_diagnostics_fingerprint(project: &Project) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    project.name.hash(&mut hash);
    project.root.hash(&mut hash);
    project.document_class.hash(&mut hash);
    project.scenes.hash(&mut hash);
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
        if resource.path.ends_with(".as") {
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

fn script_diagnostics(
    script: &neoflash_core::Script,
    result: Result<String, Vec<neoflash_script::Diagnostic>>,
) -> Vec<neoflash_script::Diagnostic> {
    let mut diagnostics = result.err().unwrap_or_default();
    diagnostics.extend(
        neoflash_script::check_warnings(script)
            .iter()
            .map(|warning| warning.to_diagnostic_with_source(&script.source)),
    );
    diagnostics
        .retain(|diagnostic| diagnostic.severity != neoflash_script::DiagnosticSeverity::Info);
    diagnostics
}

pub(crate) fn collect_compiler_diagnostics(project: &Project) -> Vec<CompilerDiagnosticItem> {
    let mut items = Vec::new();
    let program = neoflash_script::compile_project(project);
    let mut source_diagnostics = program.as_ref().err().cloned().unwrap_or_default();
    for unit in &project.sources {
        let script = neoflash_core::Script {
            language: unit.language,
            source: unit.source.clone(),
        };
        source_diagnostics.extend(
            neoflash_script::check_warnings_with_path(&script, Some(&unit.path))
                .iter()
                .map(|warning| warning.to_diagnostic_with_source(&unit.source)),
        );
    }
    // Informational code hints remain in Actions; this panel has Error/Warning categories.
    for diagnostic in source_diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity != neoflash_script::DiagnosticSeverity::Info)
    {
        let unit = diagnostic
            .path
            .as_deref()
            .and_then(|path| project.sources.iter().find(|unit| unit.path == path));
        let quick_fixes = unit
            .map(|unit| {
                crate::code_tools::suggest_quick_fixes_for_diagnostic(&diagnostic, &unit.source)
            })
            .unwrap_or_default();
        items.push(CompilerDiagnosticItem {
            is_error: diagnostic.severity == neoflash_script::DiagnosticSeverity::Error,
            message: diagnostic_message(&diagnostic),
            location_label: format!(
                "{} · Line {}",
                unit.map_or("Project", |unit| unit.path.as_str()),
                diagnostic.line
            ),
            script: unit.map(|unit| ScriptLocation::Source(unit.id.clone())),
            line: diagnostic.line,
            column: diagnostic.column,
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
            let actions_layer = symbol
                .timeline
                .action_layer_for_frame(fs.frame)
                .and_then(|index| symbol.timeline.layers.get(index))
                .or_else(|| {
                    symbol
                        .timeline
                        .layers
                        .iter()
                        .rev()
                        .find(|layer| layer.name.eq_ignore_ascii_case("actions"))
                })
                .or_else(|| symbol.timeline.layers.last())
                .map(|layer| layer.name.as_str());
            let result = match active_program {
                Some(prog) => neoflash_script::compile_with_program(&fs.script, prog),
                None => neoflash_script::compile(&fs.script),
            };
            {
                for err in script_diagnostics(&fs.script, result) {
                    let quick_fixes = crate::code_tools::suggest_quick_fixes_for_diagnostic(
                        &err,
                        &fs.script.source,
                    );
                    let location_label = match actions_layer {
                        Some(layer_name) => format!(
                            "{} · layer {:?} · frame {} · Line {}",
                            symbol.name,
                            layer_name,
                            fs.frame + 1,
                            err.line
                        ),
                        None => format!(
                            "{} · frame {} · Line {}",
                            symbol.name,
                            fs.frame + 1,
                            err.line
                        ),
                    };
                    items.push(CompilerDiagnosticItem {
                        is_error: err.severity == neoflash_script::DiagnosticSeverity::Error,
                        message: diagnostic_message(&err),
                        location_label,
                        script: Some(ScriptLocation::Frame {
                            symbol: symbol.id.clone(),
                            frame: fs.frame,
                        }),
                        line: err.line,
                        column: err.column,
                        quick_fixes,
                    });
                }
            }
        }

        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    for os in &el.scripts {
                        if os.script.source.trim().is_empty() {
                            continue;
                        }
                        let result = match active_program {
                            Some(prog) => neoflash_script::compile_with_program(&os.script, prog),
                            None => neoflash_script::compile(&os.script),
                        };
                        {
                            let target = if el.name.is_empty() {
                                "object"
                            } else {
                                el.name.as_str()
                            };
                            for err in script_diagnostics(&os.script, result) {
                                let quick_fixes =
                                    crate::code_tools::suggest_quick_fixes_for_diagnostic(
                                        &err,
                                        &os.script.source,
                                    );
                                items.push(CompilerDiagnosticItem {
                                    is_error: err.severity
                                        == neoflash_script::DiagnosticSeverity::Error,
                                    message: diagnostic_message(&err),
                                    location_label: format!(
                                        "{} · layer {:?} · frame {} · {:?} ({target}) · Line {}",
                                        symbol.name,
                                        layer.name,
                                        key.frame + 1,
                                        os.event,
                                        err.line
                                    ),
                                    script: Some(ScriptLocation::Object {
                                        symbol: symbol.id.clone(),
                                        frame: key.frame,
                                        element: el.id.clone(),
                                        event: os.event,
                                    }),
                                    line: err.line,
                                    column: err.column,
                                    quick_fixes,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    for warning in neoflash_export::export_warnings(project) {
        items.push(CompilerDiagnosticItem {
            is_error: false,
            message: warning,
            location_label: format!("{} · HTML5 publish", project.name),
            script: None,
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
                ScriptLocation::Frame { symbol, frame } => format!(
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
                let quick_fixes =
                    crate::code_tools::suggest_quick_fixes_for_diagnostic(diagnostic, &source_text);
                self.compiler_errors.items.push(CompilerDiagnosticItem {
                    is_error,
                    message,
                    location_label: format!("{label} · Line {}", diagnostic.line),
                    script: Some(location.clone()),
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
        let fp = project_diagnostics_fingerprint(&self.project);
        if self.compiler_errors.fingerprint != Some(fp) {
            self.compiler_errors.items = collect_compiler_diagnostics(&self.project);
            self.compiler_errors.fingerprint = Some(fp);
        }
    }

    pub(crate) fn script_source_for(&self, location: &ScriptLocation) -> Option<String> {
        match location {
            ScriptLocation::Frame { symbol, frame } => self
                .project
                .symbol(symbol)?
                .timeline
                .scripts
                .iter()
                .find(|fs| fs.frame == *frame)
                .map(|fs| fs.script.source.clone()),
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
        let Some(loc) = item.script else {
            return false;
        };
        let source = self.script_source_for(&loc).unwrap_or_default();
        self.open_script_location(loc);
        self.open_actions();
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
        let Some(loc) = item.script.clone() else {
            return false;
        };
        let Some(fix) = item.quick_fixes.get(fix_idx).cloned() else {
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
            ScriptLocation::Frame { symbol, frame } => {
                if let Some(sym) = ed.project.symbols.iter_mut().find(|s| s.id == *symbol)
                    && let Some(fs) = sym
                        .timeline
                        .scripts
                        .iter_mut()
                        .find(|fs| fs.frame == *frame)
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

        self.open_script_location(loc);
        self.open_actions();
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
            ui.add(c::Badge::new(&warnings_badge, c::BadgeType::Warning));
            ui.add(c::Badge::new(&errors_badge, c::BadgeType::Neutral));
        });

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
                ui.spacing_mut().item_spacing.y = space::S2;
                for (idx, item) in self.compiler_errors.items.iter().enumerate() {
                    let (icon, color) = if item.is_error {
                        (i16::ERROR, nf.danger_default)
                    } else {
                        (i16::WARNING, nf.warning_default)
                    };
                    let can_jump = item.script.is_some();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        icons::show(ui, icon, color);
                        let text_resp = ui
                            .vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    rich(&item.message, TextStyle::Body).color(nf.text_primary),
                                );
                                ui.label(
                                    rich(&item.location_label, TextStyle::Small)
                                        .color(nf.text_secondary),
                                );
                            })
                            .response;
                        if can_jump && text_resp.interact(egui::Sense::click()).clicked() {
                            jump_target = Some(idx);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    can_jump,
                                    c::IconButton::new(i16::EXTERNAL, "Go to source"),
                                )
                                .clicked()
                            {
                                jump_target = Some(idx);
                            }
                            if let Some(first_fix) = item.quick_fixes.first()
                                && ui
                                    .add_enabled(
                                        can_jump,
                                        c::IconButton::new(i16::WAND, &first_fix.title),
                                    )
                                    .on_hover_text(format!("Quick Fix: {}", first_fix.title))
                                    .clicked()
                            {
                                quick_fix_target = Some((idx, 0));
                            }
                        });
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
}
