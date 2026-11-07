//! Project code editing shares the existing Actions workspace and Undo history.
//!
//! The NF2 script workspace (`NF2/Code Editor` + compiler messages + script bar) is shared by
//! frame/object actions and project source files; see docs/DESIGN-SYSTEM.md › Panel files.
use crate::app::{Editor, ScriptTarget};
use crate::code_tools::CodeCommand;
use crate::script_api::{HintContext, ThisKind};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::CodeEditorPrefs;
use eframe::egui;
use neoflash_core::{Script, ScriptLanguage, SourceUnit, SymbolKind, new_id};
use neoflash_script::{Diagnostic, DiagnosticSeverity};
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::tokens::{radius, space};
use std::hash::{Hash, Hasher};

#[path = "sources_diagnostics.rs"]
mod diagnostics_ui;

pub(crate) struct CodeFeedback {
    target: egui::Id,
    fingerprint: u64,
    text: String,
    success: bool,
    diagnostics: Vec<Diagnostic>,
}

fn diagnostic_severity(severity: DiagnosticSeverity) -> Severity {
    match severity {
        DiagnosticSeverity::Error => Severity::Error,
        DiagnosticSeverity::Warning => Severity::Warning,
        DiagnosticSeverity::Info => Severity::Info,
    }
}

/// Languages offered for frame and object actions (Actions toolbar).
pub(crate) const ACTION_LANGUAGES: [ScriptLanguage; 5] = [
    ScriptLanguage::ActionScript3,
    ScriptLanguage::ActionScript2,
    ScriptLanguage::ActionScript1,
    ScriptLanguage::JavaScript,
    ScriptLanguage::TypeScript,
];

/// Languages offered for project source files.
const SOURCE_LANGUAGES: [ScriptLanguage; 4] = [
    ScriptLanguage::ActionScript3,
    ScriptLanguage::ActionScript2,
    ScriptLanguage::TypeScript,
    ScriptLanguage::JavaScript,
];

impl Editor {
    fn code_fingerprint(&self, script: &Script) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        script.source.hash(&mut hash);
        std::mem::discriminant(&script.language).hash(&mut hash);
        self.project.root.hash(&mut hash);
        self.project.document_class.hash(&mut hash);
        for unit in &self.project.sources {
            unit.id.hash(&mut hash);
            unit.path.hash(&mut hash);
            unit.source.hash(&mut hash);
            std::mem::discriminant(&unit.language).hash(&mut hash);
        }
        // Includes are compilation inputs even when the visible source has not changed.
        // Binary media contents do not affect the language service or compiler diagnostics.
        for resource in &self.project.resources {
            resource.id.hash(&mut hash);
            resource.path.hash(&mut hash);
            if resource.path.ends_with(".as") {
                resource.data_base64.hash(&mut hash);
            }
        }
        for symbol in &self.project.symbols {
            symbol.id.hash(&mut hash);
            std::mem::discriminant(&symbol.kind).hash(&mut hash);
            symbol.class_name.hash(&mut hash);
            symbol.linkage_id.hash(&mut hash);
            for entry in &symbol.timeline.scripts {
                entry.frame.hash(&mut hash);
                std::mem::discriminant(&entry.script.language).hash(&mut hash);
                entry.script.source.hash(&mut hash);
            }
            for element in symbol
                .timeline
                .layers
                .iter()
                .flat_map(|layer| &layer.keyframes)
                .flat_map(|key| &key.elements)
            {
                element.id.hash(&mut hash);
                element.name.hash(&mut hash);
                std::mem::discriminant(&element.kind).hash(&mut hash);
                if let neoflash_core::ElementKind::Instance { symbol_id, .. } = &element.kind {
                    symbol_id.hash(&mut hash);
                }
                for entry in &element.scripts {
                    std::mem::discriminant(&entry.event).hash(&mut hash);
                    std::mem::discriminant(&entry.script.language).hash(&mut hash);
                    entry.script.source.hash(&mut hash);
                }
            }
        }
        hash.finish()
    }

    /// `announce` also writes the result to Output (explicit Check); live checks stay silent.
    fn record_code_check(&mut self, script: &Script, diagnostics: Vec<Diagnostic>, announce: bool) {
        let success = !diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error);
        let text = if diagnostics.is_empty() {
            "Code accepted for current language support.".to_owned()
        } else {
            diagnostics
                .iter()
                .map(|d| format!("{}: {}", diagnostic_severity(d.severity).label(), d))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if announce {
            self.message(text.clone());
        }
        self.code_feedback = Some(CodeFeedback {
            target: self.actions_editor_id(),
            fingerprint: self.code_fingerprint(script),
            text,
            success,
            diagnostics,
        });
    }

    pub(crate) fn check_action_source(&mut self, script: &Script, announce: bool) {
        let mut diagnostics = neoflash_script::compile_project(&self.project)
            .and_then(|program| neoflash_script::compile_with_program(script, &program))
            .err()
            .unwrap_or_default();
        diagnostics.extend(
            neoflash_script::check_warnings(script)
                .iter()
                .map(|warning| warning.to_diagnostic_with_source(&script.source)),
        );
        if let Some(context) = self.hint_context(script.language) {
            diagnostics.extend(
                context
                    .semantic_analysis(&script.source)
                    .diagnostics()
                    .iter()
                    .cloned(),
            );
        }
        self.record_code_check(script, diagnostics, announce);
    }

    /// "Check project": compile every project source; feedback attaches to the selected file.
    pub(crate) fn check_project_sources(&mut self, announce: bool) {
        let mut diagnostics = neoflash_script::compile_project(&self.project)
            .err()
            .unwrap_or_default();
        for unit in &self.project.sources {
            let script = Script {
                language: unit.language,
                source: unit.source.clone(),
            };
            diagnostics.extend(
                neoflash_script::check_warnings_with_path(&script, Some(&unit.path))
                    .iter()
                    .map(|warning| warning.to_diagnostic_with_source(&script.source)),
            );
            if let Some(context) = self.source_hint_context(unit) {
                diagnostics.extend(
                    context
                        .semantic_analysis(&script.source)
                        .diagnostics()
                        .iter()
                        .cloned()
                        .map(|mut diagnostic| {
                            diagnostic.path = Some(unit.path.clone());
                            diagnostic
                        }),
                );
            }
        }
        if let Some(unit) = self
            .project
            .sources
            .iter()
            .find(|unit| Some(&unit.id) == self.selected_source.as_ref())
            .cloned()
        {
            self.record_code_check(
                &Script {
                    language: unit.language,
                    source: unit.source,
                },
                diagnostics,
                announce,
            );
        }
    }

    /// Result of the last check if it still describes `script` in the current editor.
    fn current_feedback(&self, script: &Script) -> Option<&CodeFeedback> {
        self.code_feedback.as_ref().filter(|feedback| {
            feedback.target == self.actions_editor_id()
                && feedback.fingerprint == self.code_fingerprint(script)
        })
    }

    fn diagnostic_is_current(&self, diagnostic: &Diagnostic) -> bool {
        match diagnostic.path.as_deref() {
            None => self.script_target != ScriptTarget::Source,
            Some(path) => {
                self.script_target == ScriptTarget::Source
                    && self.project.sources.iter().any(|unit| {
                        Some(&unit.id) == self.selected_source.as_ref() && unit.path == path
                    })
            }
        }
    }

    fn jump_to_diagnostic(&mut self, ctx: &egui::Context, source: &str, diagnostic: &Diagnostic) {
        if let Some(path) = diagnostic.path.as_deref() {
            if let Some(unit) = self
                .project
                .sources
                .iter()
                .find(|unit| unit.path == path)
                .cloned()
            {
                self.selected_source = Some(unit.id);
                self.script_target = ScriptTarget::Source;
                jump_to(
                    ctx,
                    self.actions_editor_id(),
                    &unit.source,
                    diagnostic.line,
                    diagnostic.column,
                );
            }
        } else if self.diagnostic_is_current(diagnostic) {
            jump_to(
                ctx,
                self.actions_editor_id(),
                source,
                diagnostic.line,
                diagnostic.column,
            );
        }
    }

    fn diagnostic_action(
        &mut self,
        action: Option<diagnostics_ui::Action>,
        diagnostics: &[Diagnostic],
    ) {
        match action {
            Some(diagnostics_ui::Action::Fix(fix)) => {
                self.actions_ui.pending = Some(CodeCommand::ApplyQuickFix(fix));
            }
            Some(diagnostics_ui::Action::CompilerErrors) => {
                self.show_actions_compiler_diagnostics(diagnostics);
            }
            Some(diagnostics_ui::Action::NoFix) => self.message("No quick fix at the caret."),
            None => {}
        }
    }

    /// NF2 script workspace: code editor (gutter, syntax colours, current line), compiler messages
    /// (click jumps to the line) and the script bar (script tab, caret position, problems).
    /// Fills the remaining space of `ui`; returns the code `TextEdit` response.
    pub(crate) fn script_workspace(
        &mut self,
        ui: &mut egui::Ui,
        source: &mut String,
        language: ScriptLanguage,
        hint: &str,
        tab: &str,
    ) -> egui::Response {
        let editor_id = self.actions_editor_id();
        self.fonts
            .ensure_loaded(ui.ctx(), &self.workspace.code_editor.font);
        let code_prefs = self.workspace.code_editor.clone();
        let before_command = source.clone();
        let script = Script {
            language,
            source: source.clone(),
        };
        // Navigation and fixes can be requested before the delayed live check has run.
        if ui.ctx().memory(|m| m.has_focus(editor_id))
            && ui.input(|i| {
                i.events.iter().any(|event| matches!(event,
                egui::Event::Key { key, modifiers, pressed: true, .. }
                    if *key == egui::Key::F8 || (*key == egui::Key::Period && modifiers.command)
            ))
            })
            && self.current_feedback(&script).is_none()
        {
            if self.script_target == ScriptTarget::Source {
                self.check_project_sources(false);
            } else {
                self.check_action_source(&script, false);
            }
        }
        let mut diagnostic_ui = diagnostics_ui::DiagnosticUi::load(
            ui.ctx(),
            editor_id,
            self.current_feedback(&script).map(|f| f.fingerprint),
        );
        let current_diagnostics = self
            .current_feedback(&script)
            .map(|f| {
                f.diagnostics
                    .iter()
                    .filter(|d| self.diagnostic_is_current(d))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let action = if !self.has_modal_dialog_in(ui.ctx()) && ui.is_enabled() {
            diagnostic_ui.before_editor(ui.ctx(), editor_id, source, &current_diagnostics)
        } else {
            diagnostic_ui.close();
            None
        };
        self.diagnostic_action(action, &current_diagnostics);
        let show_hints = self.apply_code_command(ui.ctx(), editor_id, source);
        if *source != before_command {
            diagnostic_ui.close();
        }
        let feedback = self
            .current_feedback(&Script {
                language,
                source: source.clone(),
            })
            .map(|f| (f.success, f.text.clone(), f.diagnostics.clone()));
        let tab_key = self.current_source_tab_key(tab);
        let mut markers: Vec<(usize, Severity)> = feedback
            .iter()
            .flat_map(|(_, _, diagnostics)| {
                diagnostics
                    .iter()
                    .filter(|d| self.diagnostic_is_current(d))
                    .map(|d| (d.line, diagnostic_severity(d.severity)))
            })
            .collect();
        for bp in self.breakpoints_for(&tab_key) {
            if !markers.iter().any(|(line, _)| *line == bp.line) {
                markers.push((bp.line, Severity::Info));
            }
        }
        let message_count = feedback.as_ref().map_or(0, |(_, _, d)| d.len().max(1));
        let bar_h = neoflash_ui::metrics::script_tab() + 2.;
        let full = ui.available_rect_before_wrap();
        let diag_h = if message_count == 0 {
            0.
        } else {
            (message_count as f32 * (52. + space::S1))
                .min(120.)
                .min(full.height() / 3.)
        };
        let bar =
            egui::Rect::from_min_max(egui::pos2(full.left(), full.bottom() - bar_h), full.max);
        let diag = egui::Rect::from_min_max(
            egui::pos2(full.left(), bar.top() - diag_h),
            egui::pos2(full.right(), bar.top()),
        );
        let gap = if diag_h > 0. { space::S2 } else { 0. };
        let code = egui::Rect::from_min_max(
            full.min,
            egui::pos2(full.right(), (diag.top() - gap).max(full.top() + 60.)),
        );
        let hint_context = || self.hint_context(language);
        let paint_diagnostics = if *source == before_command {
            current_diagnostics.as_slice()
        } else {
            &[]
        };
        let (mut response, caret, action, gutter_click) = ui
            .scope_builder(egui::UiBuilder::new().max_rect(code), |ui| {
                code_editor(
                    ui,
                    editor_id,
                    source,
                    hint,
                    &markers,
                    &hint_context,
                    show_hints,
                    &code_prefs,
                    paint_diagnostics,
                    &mut diagnostic_ui,
                )
            })
            .inner;
        if let Some((clicked_line, shift_held)) = gutter_click {
            if shift_held {
                self.toggle_fold_line_for(&tab_key, clicked_line);
            } else {
                self.toggle_breakpoint(&tab_key, clicked_line);
            }
        }
        // Popup actions execute now, under the checked fingerprint, never as a stale next-frame edit.
        self.diagnostic_action(action, &current_diagnostics);
        self.apply_code_command(ui.ctx(), editor_id, source);
        if *source != before_command {
            diagnostic_ui.close();
            response.mark_changed();
        }
        diagnostic_ui.store(ui.ctx(), editor_id);
        self.live_check(ui.ctx(), editor_id, &response, source, language);
        if let Some((success, text, diagnostics)) = &feedback {
            ui.scope_builder(egui::UiBuilder::new().max_rect(diag), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("code_feedback", editor_id))
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = space::S1;
                        if diagnostics.is_empty() {
                            let severity = if *success {
                                Severity::Success
                            } else {
                                Severity::Error
                            };
                            ui.add(nf::LogLine {
                                severity,
                                time: "",
                                text,
                            });
                        }
                        for diagnostic in diagnostics {
                            let path = diagnostic
                                .path
                                .as_ref()
                                .map_or(String::new(), |path| format!("{path} · "));
                            let location = format!(
                                "{path}Line {}, column {}",
                                diagnostic.line, diagnostic.column
                            );
                            let message = diagnostic.code.as_ref().map_or_else(
                                || diagnostic.message.clone(),
                                |code| format!("[{code}] {}", diagnostic.message),
                            );
                            let row = ui.add(nf::CompilerMessage {
                                severity: diagnostic_severity(diagnostic.severity),
                                message: &message,
                                location: &location,
                            });
                            if row.clicked() {
                                self.jump_to_diagnostic(ui.ctx(), source, diagnostic);
                            }
                        }
                    });
            });
        }
        let p = neoflash_ui::palette(ui.ctx());
        ui.painter().hline(
            bar.x_range(),
            bar.top() + 0.5,
            egui::Stroke::new(1.0_f32, p.border_subtle),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(nf::ScriptTab {
                    label: tab,
                    pinned: false,
                    active: true,
                })
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(egui::vec2(space::S2, 0.)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                let lines = source.chars().filter(|c| *c == '\n').count() + 1;
                let problems = feedback.as_ref().map(|(_, _, d)| {
                    (
                        d.iter()
                            .filter(|d| d.severity == DiagnosticSeverity::Error)
                            .count(),
                        d.iter()
                            .filter(|d| d.severity == DiagnosticSeverity::Warning)
                            .count(),
                    )
                });
                nf::code::script_status(ui, caret.0, caret.1, lines, problems);
            },
        );
        ui.advance_cursor_after_rect(full);
        response
    }

    /// Apply the queued toolbar/toolbox command to the visible editor. Returns `true` when code
    /// hints should open at the caret.
    fn apply_code_command(
        &mut self,
        ctx: &egui::Context,
        editor_id: egui::Id,
        source: &mut String,
    ) -> bool {
        let Some(command) = self.actions_ui.pending.take() else {
            return false;
        };
        ctx.memory_mut(|memory| memory.request_focus(editor_id));
        if command == CodeCommand::ShowHints {
            return true;
        }
        let mut state = egui::text_edit::TextEditState::load(ctx, editor_id).unwrap_or_default();
        let end = source.chars().count();
        let selection = state
            .cursor
            .char_range()
            .map_or((end, end), |r| (r.secondary.index, r.primary.index));
        if let Some((text, (start, stop))) = crate::code_tools::apply_with_tab_size(
            &command,
            source,
            selection,
            self.workspace.code_editor.tab_size,
        ) {
            *source = text;
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(stop),
                )));
            state.store(ctx, editor_id);
        }
        false
    }

    /// Code-hint resolver for the open script: `this` follows the edited timeline/object (or the
    /// class of the open project file) and named stage instances of the current timeline.
    fn hint_context(&self, language: ScriptLanguage) -> Option<HintContext<'static>> {
        if self.script_target == ScriptTarget::Source {
            return self
                .project
                .sources
                .iter()
                .find(|unit| Some(&unit.id) == self.selected_source.as_ref())
                .and_then(|unit| self.source_hint_context(unit));
        }
        let kind_of =
            |element: &neoflash_core::Element| ThisKind::of_element(&self.project, &element.kind);
        let this_kind = match self.script_target {
            ScriptTarget::Object => self.selected_element().map_or(ThisKind::MovieClip, kind_of),
            _ => ThisKind::MovieClip,
        };
        let mut seen = std::collections::HashSet::new();
        let instances = self
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|layer| &layer.keyframes)
            .flat_map(|key| &key.elements)
            .filter(|element| !element.name.is_empty() && seen.insert(element.name.clone()))
            .map(|element| (element.name.clone(), kind_of(element)))
            .collect();
        Some(HintContext::new(&self.project, language, this_kind)?.with_instances(instances))
    }

    fn source_hint_context(&self, unit: &SourceUnit) -> Option<HintContext<'static>> {
        let context = HintContext::new(&self.project, unit.language, ThisKind::MovieClip)?;
        let class = crate::script_api::scan_class(unit).map(|class| class.qualified);
        // An unrelated project class does not acquire the current timeline's named instances.
        // Linked/document classes see their own timelines; conflicting instance kinds stay unknown.
        let mut instances = std::collections::BTreeMap::<String, Option<ThisKind>>::new();
        if let Some(class) = &class {
            for symbol in self.project.symbols.iter().filter(|symbol| {
                symbol.class_name.as_ref() == Some(class)
                    || (symbol.id == self.project.root
                        && self.project.document_class.as_ref() == Some(class))
            }) {
                for element in symbol
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|layer| &layer.keyframes)
                    .flat_map(|key| &key.elements)
                    .filter(|element| !element.name.is_empty())
                {
                    let kind = ThisKind::of_element(&self.project, &element.kind);
                    instances
                        .entry(element.name.clone())
                        .and_modify(|current| {
                            if *current != Some(kind) {
                                *current = None;
                            }
                        })
                        .or_insert(Some(kind));
                }
            }
        }
        Some(
            context
                .with_this_class(class.unwrap_or_default())
                .with_instances(
                    instances
                        .into_iter()
                        .filter_map(|(name, kind)| kind.map(|kind| (name, kind)))
                        .collect(),
                ),
        )
    }

    /// Check the open script ~0.6 s after the last edit, silently, so markers and the problem count
    /// follow typing. Explicit Check still reports to Output.
    fn live_check(
        &mut self,
        ctx: &egui::Context,
        editor_id: egui::Id,
        response: &egui::Response,
        source: &str,
        language: ScriptLanguage,
    ) {
        const DELAY: f64 = 0.6;
        let now = ctx.input(|input| input.time);
        if response.changed() {
            self.actions_ui.edited = Some((editor_id, now));
        }
        let Some((id, at)) = self.actions_ui.edited else {
            return;
        };
        if id != editor_id {
            self.actions_ui.edited = None;
            return;
        }
        let wait = at + DELAY - now;
        if wait > 0. {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            return;
        }
        self.actions_ui.edited = None;
        if self.script_target == ScriptTarget::Source {
            self.check_project_sources(false);
        } else {
            let script = Script {
                language,
                source: source.to_owned(),
            };
            self.check_action_source(&script, false);
        }
    }

    pub(crate) fn project_sources(&mut self, ui: &mut egui::Ui) {
        if !self
            .project
            .sources
            .iter()
            .any(|source| Some(&source.id) == self.selected_source.as_ref())
        {
            self.selected_source = self.project.sources.first().map(|source| source.id.clone());
        }
        let index = self
            .project
            .sources
            .iter()
            .position(|s| Some(&s.id) == self.selected_source.as_ref());
        let original = index.map(|index| self.project.sources[index].clone());
        let mut unit = original.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            let file_label = unit
                .as_ref()
                .map_or("No source files", |u| u.path.as_str())
                .to_owned();
            let select_w =
                (ui.available_width() - 160. - 3. * neoflash_ui::metrics::control() - 16.)
                    .clamp(120., 320.);
            nf::Select::new("project_source_file", &file_label)
                .icon(neoflash_ui::icons::i16::FILE_CODE)
                .width(select_w)
                .show(ui, |ui| {
                    for source in &self.project.sources {
                        let current = Some(&source.id) == self.selected_source.as_ref();
                        if ui
                            .add(nf::MenuItem::new(&source.path).checked(current))
                            .clicked()
                        {
                            self.selected_source = Some(source.id.clone());
                        }
                    }
                });
            if let Some(unit) = &mut unit {
                language_select(
                    ui,
                    "source_language",
                    &mut unit.language,
                    &SOURCE_LANGUAGES,
                    160.,
                );
            }
            ui.add_space(space::S1);
            nf::divider(ui, true);
            let new = ui.add(nf::IconButton::new(
                neoflash_ui::icons::i16::NEW_FILE,
                "New class",
            ));
            nf::popup_below(&new, 200., |ui| {
                for (label, language) in [
                    ("New AS3 class", ScriptLanguage::ActionScript3),
                    ("New AS2 class", ScriptLanguage::ActionScript2),
                ] {
                    if ui.add(nf::MenuItem::new(label)).clicked() {
                        self.new_source_class(language);
                    }
                }
            });
            if ui
                .add(nf::IconButton::new(
                    neoflash_ui::icons::i16::OPEN,
                    "Import source file…",
                ))
                .clicked()
            {
                self.import_source_file();
            }
            let remove = ui.add_enabled_semantic(
                self.selected_source.is_some(),
                nf::IconButton::new(neoflash_ui::icons::i16::TRASH, "Remove source file"),
            );
            if remove.clicked() {
                let selected = self.selected_source.clone();
                self.transact_named("Delete source file", |editor| {
                    editor
                        .project
                        .sources
                        .retain(|source| Some(&source.id) != selected.as_ref())
                });
                self.selected_source = None;
            }
        });
        ui.add_space(space::S1);
        let mut document_class = self.project.document_class.clone().unwrap_or_default();
        let mut symbol_class = self.symbol().class_name.clone().unwrap_or_default();
        let mut linkage_id = self.symbol().linkage_id.clone().unwrap_or_default();
        let is_root = self.symbol_id() == self.project.root;
        let is_movie_clip = self.symbol().kind == SymbolKind::MovieClip;
        nf::SectionHeader::new("Class linkage")
            .id_salt("actions-class-linkage")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(
                    nf::TextField::new(&mut document_class, "Document class")
                        .prefix("Document")
                        .hint("app.Main")
                        .mono(),
                );
                if !is_root {
                    ui.add(
                        nf::TextField::new(&mut symbol_class, "Symbol class")
                            .prefix("Symbol class")
                            .hint("ui.Component")
                            .mono(),
                    );
                    if is_movie_clip {
                        ui.add(
                            nf::TextField::new(&mut linkage_id, "Export identifier")
                                .prefix("Export ID")
                                .hint("card-library-id")
                                .mono(),
                        );
                    }
                }
            });
        let document_class =
            (!document_class.trim().is_empty()).then(|| document_class.trim().to_owned());
        let symbol_class =
            (!symbol_class.trim().is_empty()).then(|| symbol_class.trim().to_owned());
        let linkage_id = (!linkage_id.trim().is_empty()).then(|| linkage_id.trim().to_owned());
        if document_class != self.project.document_class
            || symbol_class != self.symbol().class_name
            || linkage_id != self.symbol().linkage_id
        {
            self.transact_named("Class linkage", |editor| {
                editor.project.enable_program();
                editor.project.document_class = document_class;
                editor.symbol_mut().class_name = symbol_class;
                editor.symbol_mut().linkage_id = linkage_id;
            });
        }
        let (Some(index), Some(original), Some(mut unit)) = (index, original, unit) else {
            let new = nf::EmptyState {
                icon: neoflash_ui::icons::i16::FILE_CODE,
                title: "No project classes",
                body: "Add a project class to reuse code across frames and symbol instances. Files are stored inside the project.",
                action: Some("New AS3 class"),
            }
            .show(ui);
            if new {
                self.new_source_class(ScriptLanguage::ActionScript3);
            }
            return;
        };
        nf::SectionHeader::new("File properties")
            .id_salt("actions-file-properties")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(
                    nf::TextField::new(&mut unit.path, "Path")
                        .prefix("Path")
                        .mono(),
                );
            });
        let tab = unit.path.clone();
        let response = self.script_workspace(ui, &mut unit.source, unit.language, "", &tab);
        if response.has_focus() && self.playing {
            self.set_playing(false);
        }
        if unit != original {
            self.transact_named("Edit script", |editor| editor.project.sources[index] = unit);
        }
    }

    fn new_source_class(&mut self, language: ScriptLanguage) {
        let mut index = 1;
        while self
            .project
            .sources
            .iter()
            .any(|s| s.path == format!("Class{index}.as"))
        {
            index += 1;
        }
        let name = format!("Class{index}");
        let indent = " ".repeat(usize::from(self.workspace.code_editor.tab_size.clamp(1, 8)));
        let indent2 = indent.repeat(2);
        let source = if language == ScriptLanguage::ActionScript2 {
            format!("class {name} {{\n{indent}public function {name}() {{\n{indent}}}\n}}\n")
        } else {
            format!(
                "package {{\n{indent}public class {name} {{\n{indent2}public function {name}() {{\n{indent2}}}\n{indent}}}\n}}\n"
            )
        };
        let unit = SourceUnit {
            id: new_id("source"),
            path: format!("{name}.as"),
            language,
            source,
        };
        self.selected_source = Some(unit.id.clone());
        self.transact_named("New class", |editor| {
            editor.project.enable_program();
            editor.project.sources.push(unit);
        });
    }

    fn import_source_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Project source", &["as", "js", "ts"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(source) if source.len() <= 1_048_576 => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if self
                    .project
                    .sources
                    .iter()
                    .any(|s| s.path.eq_ignore_ascii_case(&name))
                {
                    self.message(
                        "A source file with this name already exists. Rename it before importing.",
                    );
                } else {
                    let language = match path.extension().and_then(|s| s.to_str()) {
                        Some("js") => ScriptLanguage::JavaScript,
                        Some("ts") => ScriptLanguage::TypeScript,
                        _ => ScriptLanguage::ActionScript3,
                    };
                    let unit = SourceUnit {
                        id: new_id("source"),
                        path: name,
                        language,
                        source,
                    };
                    self.selected_source = Some(unit.id.clone());
                    self.transact(|editor| {
                        editor.project.enable_program();
                        editor.project.sources.push(unit);
                    });
                }
            }
            Ok(_) => self.message("A source file is limited to 1 MiB."),
            Err(error) => self.message(format!("Could not read source: {error}")),
        }
    }
}

pub(crate) fn source_language_name(language: ScriptLanguage) -> &'static str {
    match language {
        ScriptLanguage::ActionScript1 => "ActionScript 1.0",
        ScriptLanguage::ActionScript2 => "ActionScript 2.0",
        ScriptLanguage::ActionScript3 => "ActionScript 3.0",
        ScriptLanguage::JavaScript => "JavaScript",
        ScriptLanguage::TypeScript => "TypeScript",
    }
}

/// `NF2/Select` of script languages. Returns `true` when the language changed.
pub(crate) fn language_select(
    ui: &mut egui::Ui,
    id: &str,
    language: &mut ScriptLanguage,
    options: &[ScriptLanguage],
    width: f32,
) -> bool {
    let mut changed = false;
    nf::Select::new(id, source_language_name(*language))
        .width(width)
        .show(ui, |ui| {
            for option in options {
                let item =
                    nf::MenuItem::new(source_language_name(*option)).checked(option == language);
                if ui.add(item).clicked() && option != language {
                    *language = *option;
                    changed = true;
                }
            }
        });
    changed
}

const MARKER_GUTTER: f32 = space::S1 + 16.0 + space::S1;

/// Resolved `egui::FontId` for `CodeEditorPrefs`: empty or `"Roboto Mono"` uses the bundled NF2
/// monospace font; installed custom families fall back to monospace if unavailable in `ctx`.
pub(crate) fn code_font_id(ctx: &egui::Context, prefs: &CodeEditorPrefs) -> egui::FontId {
    let size = prefs.font_size.clamp(8.0, 32.0);
    let trimmed = prefs.font.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("Roboto Mono") {
        return egui::FontId::new(size, egui::FontFamily::Monospace);
    }
    let named = crate::fonts::styled_family(trimmed, false, false);
    let family = if ctx.fonts_mut(|fonts| fonts.families().contains(&named)) {
        named
    } else {
        egui::FontFamily::Monospace
    };
    egui::FontId::new(size, family)
}

/// Syntax-coloured code layout (NF2 Night tokens) applying `CodeEditorPrefs` font, size, line
/// height, and word wrap without replacing TextEdit IDs or input state.
pub(crate) fn code_layout_with_prefs(
    ui: &egui::Ui,
    text: &dyn egui::TextBuffer,
    wrap_width: f32,
    prefs: &CodeEditorPrefs,
) -> std::sync::Arc<egui::Galley> {
    let ctx = ui.ctx();
    let font_id = code_font_id(ctx, prefs);
    let line_px = font_id.size * prefs.line_height.clamp(1.0, 3.0);
    let mut job = nf::code::highlight(ctx, text.as_str());
    for section in &mut job.sections {
        section.format.font_id = font_id.clone();
        section.format.line_height = Some(line_px);
    }
    job.wrap.max_width = if prefs.word_wrap {
        wrap_width
    } else {
        f32::INFINITY
    };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn paint_marker_gutter(
    ui: &egui::Ui,
    rect: egui::Rect,
    markers: &[(usize, Severity)],
    code: &egui::Galley,
    galley_pos: egui::Pos2,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, p.bg_panel);
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, p.border_subtle),
    );
    let mut line = 1;
    let mut first_row = true;
    for row in &code.rows {
        let r = row.rect().translate(galley_pos.to_vec2());
        if first_row && r.bottom() >= rect.top() && r.top() <= rect.bottom() {
            let marker = markers
                .iter()
                .filter(|(l, _)| *l == line)
                .map(|(_, s)| *s)
                .max_by_key(|s| *s as u8);
            if let Some(severity) = marker {
                let icon = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + space::S1 + 8.0, r.center().y),
                    egui::vec2(16.0, 16.0),
                );
                neoflash_ui::icons::paint(
                    &painter,
                    ui.ctx(),
                    icon,
                    severity.icon(),
                    severity.color(p),
                );
            }
        }
        first_row = row.ends_with_newline;
        if row.ends_with_newline {
            line += 1;
        }
    }
}

/// `NF2/Code Editor`: `bg/field` frame, gutter with line numbers and markers, current-line band,
/// syntax-coloured `TextEdit` with the given stable `id`. Returns the response and the 1-based caret.
/// Code-hint popover state of one code editor (egui temp memory, keyed by the editor id).
#[derive(Clone, Default)]
struct HintState {
    open: bool,
    /// Opened by ⌃Space / toolbar: also lists scope names before anything is typed.
    explicit: bool,
    /// Timestamp (`input.time`) when an automatic `.` hint was triggered and is waiting for `hint_delay_ms`.
    pending_at: Option<f64>,
    selected: usize,
    count: usize,
    /// Char index where the hinted word starts; moving to another word closes the popover.
    anchor: Option<usize>,
    hovered: bool,
    scroll: bool,
}

/// Replace the typed prefix with hint `index` and move the caret into the inserted text.
fn accept_hint(
    ctx: &egui::Context,
    id: egui::Id,
    source: &mut String,
    context: &HintContext,
    explicit: bool,
    index: usize,
) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let Some(caret) = state.cursor.char_range().map(|r| r.primary.index) else {
        return;
    };
    let Some(hints) = context.hints(source, caret, explicit) else {
        return;
    };
    let Some(item) = hints.items.get(index) else {
        return;
    };
    let byte = |i: usize| {
        source
            .char_indices()
            .nth(i)
            .map_or(source.len(), |(b, _)| b)
    };
    let (from, to) = (byte(hints.start), byte(caret));
    source.replace_range(from..to, &item.insert);
    let position = hints.start + item.caret;
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(position),
        )));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// Code hints popover under the caret (`NF2/Menu Item` rows with the type on the right, Figma
/// "Code hint" 230 pt). Returns the clicked row.
fn hint_popover(
    ui: &egui::Ui,
    id: egui::Id,
    anchor: egui::Pos2,
    items: &[crate::script_api::Hint],
    state: &mut HintState,
) -> Option<usize> {
    let ctx = ui.ctx();
    let p = neoflash_ui::palette(ctx);
    let mut clicked = None;
    let area = egui::Area::new(id.with("code-hints"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .constrain(true)
        .show(ctx, |ui| {
            nf::popover_frame(ctx).show(ui, |ui| {
                nf::menu::menu_style(ui.style_mut());
                ui.set_width(220.);
                egui::ScrollArea::vertical()
                    .id_salt(id.with("code-hints-scroll"))
                    .max_height(8. * neoflash_ui::metrics::row())
                    .themed()
                    .show(ui, |ui| {
                        for (index, item) in items.iter().enumerate() {
                            if index == state.selected {
                                let row = egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    egui::vec2(ui.available_width(), neoflash_ui::metrics::row()),
                                );
                                ui.painter().rect_filled(row, radius::MD, p.bg_hover);
                            }
                            let response =
                                ui.add(nf::MenuItem::new(&item.label).shortcut(&item.detail));
                            if index == state.selected && state.scroll {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
            });
        });
    state.scroll = false;
    state.hovered = area.response.contains_pointer();
    clicked
}

type CodeEditorResult = (
    egui::Response,
    (usize, usize),
    Option<diagnostics_ui::Action>,
    Option<(usize, bool)>,
);

#[allow(clippy::too_many_arguments)]
fn code_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    source: &mut String,
    hint: &str,
    markers: &[(usize, Severity)],
    hints: &dyn Fn() -> Option<HintContext<'static>>,
    show_hints: bool,
    prefs: &CodeEditorPrefs,
    diagnostics: &[Diagnostic],
    diagnostic_ui: &mut diagnostics_ui::DiagnosticUi,
) -> CodeEditorResult {
    let ctx = ui.ctx().clone();
    let checked_source = source.clone();
    let mut diagnostic_action = None;
    let mut gutter_click: Option<(usize, bool)> = None;
    let state_id = id.with("code-hint-state");
    let mut hint_state: HintState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    if diagnostic_ui.is_open() {
        hint_state = HintState::default();
    }
    let focused = ctx.memory(|memory| memory.has_focus(id));
    let frame_typed_dot = ctx.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, egui::Event::Text(t) if t.ends_with('.')))
    });
    let explicit_key =
        focused && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));
    if show_hints || explicit_key {
        hint_state = HintState {
            open: true,
            explicit: true,
            ..Default::default()
        };
    } else if !prefs.code_hints && !hint_state.explicit {
        hint_state = HintState::default();
    }
    let mut pre_edited = false;
    if hint_state.open && hint_state.count > 0 && focused {
        let count = hint_state.count;
        let mut accept = None;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                hint_state.selected = (hint_state.selected + 1) % count;
                hint_state.scroll = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                hint_state.selected = (hint_state.selected + count - 1) % count;
                hint_state.scroll = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
            {
                accept = Some(hint_state.selected);
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == "\t"));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                hint_state.open = false;
            }
        });
        if let Some(index) = accept {
            if let Some(context) = hints() {
                accept_hint(&ctx, id, source, &context, hint_state.explicit, index);
                pre_edited = true;
            }
            hint_state = HintState::default();
        }
    }
    if focused {
        let shift_tab = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
        let plain_tab =
            !shift_tab && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
        if shift_tab || plain_tab {
            ctx.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == "\t"));
            });
            ctx.memory_mut(|m| {
                m.request_focus(id);
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                );
            });
            let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
            let end = source.chars().count();
            let selection = state.cursor.char_range().map_or((end, end), |r| {
                (r.secondary.index.min(end), r.primary.index.min(end))
            });
            let edit = if shift_tab {
                crate::code_tools::outdent_tab(source, selection, prefs.tab_size)
            } else {
                Some(crate::code_tools::indent_tab(
                    source,
                    selection,
                    prefs.tab_size,
                ))
            };
            if let Some((new_text, (start, stop))) = edit {
                if new_text != *source {
                    *source = new_text;
                    pre_edited = true;
                }
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(start),
                        egui::text::CCursor::new(stop),
                    )));
                state.store(&ctx, id);
            }
        }
        if prefs.auto_close_brackets {
            let has_plain_backspace = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::Backspace,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.is_none()
                    )
                })
            });
            if has_plain_backspace {
                let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
                let end = source.chars().count();
                if let Some(range) = state.cursor.char_range()
                    && range.primary.index == range.secondary.index
                    && let Some((new_text, (start, stop))) =
                        crate::code_tools::auto_delete_pair(source, range.primary.index.min(end))
                {
                    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace));
                    *source = new_text;
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(start),
                            egui::text::CCursor::new(stop),
                        )));
                    state.store(&ctx, id);
                    pre_edited = true;
                }
            }
            let has_trigger = ctx.input(|i| {
                !i.modifiers.command
                    && !i.modifiers.ctrl
                    && i.events.iter().any(|e| {
                        matches!(
                            e,
                            egui::Event::Text(t)
                                if t.chars().count() == 1
                                    && crate::code_tools::is_auto_close_trigger(
                                        t.chars().next().unwrap()
                                    )
                        )
                    })
            });
            if has_trigger {
                let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
                let end = source.chars().count();
                let (mut sel_start, mut sel_end) =
                    state.cursor.char_range().map_or((end, end), |r| {
                        (r.secondary.index.min(end), r.primary.index.min(end))
                    });
                let before = source.clone();
                let char_byte =
                    |s: &str, idx: usize| s.char_indices().nth(idx).map_or(s.len(), |(b, _)| b);
                ctx.input_mut(|i| {
                    i.events.retain(|event| {
                        let egui::Event::Text(t) = event else {
                            return true;
                        };
                        if t.chars().count() != 1 {
                            return true;
                        }
                        let ch = t.chars().next().unwrap();
                        if matches!(ch, '\t' | '\n' | '\r') {
                            return true;
                        }
                        let (lo, hi) = (sel_start.min(sel_end), sel_start.max(sel_end));
                        if lo == hi
                            && let Some((new_text, (s, e))) =
                                crate::code_tools::auto_close_char(source, lo, ch)
                        {
                            *source = new_text;
                            sel_start = s;
                            sel_end = e;
                        } else {
                            let (b0, b1) = (char_byte(source, lo), char_byte(source, hi));
                            let mut buf = [0u8; 4];
                            source.replace_range(b0..b1, ch.encode_utf8(&mut buf));
                            sel_start = lo + 1;
                            sel_end = sel_start;
                        }
                        false
                    });
                });
                if *source != before {
                    pre_edited = true;
                }
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(sel_start),
                        egui::text::CCursor::new(sel_end),
                    )));
                state.store(&ctx, id);
            }
        }
    }
    let p = neoflash_ui::palette(&ctx);
    let lines = source.chars().filter(|c| *c == '\n').count() + 1;
    let gutter = if prefs.line_numbers {
        nf::code::gutter_width(ui, lines)
    } else {
        MARKER_GUTTER
    };
    let font_id = code_font_id(&ctx, prefs);
    let mut caret = (1, 1);
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        code_layout_with_prefs(ui, text, wrap_width, prefs)
    };
    let response = egui::Frame::new()
        .fill(p.bg_field)
        .stroke(egui::Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            let viewport_size = ui.available_size();
            let viewport_w = if viewport_size.x.is_finite() {
                viewport_size.x
            } else {
                400.0
            };
            let viewport_h = if viewport_size.y.is_finite() {
                viewport_size.y.max(60.0)
            } else {
                60.0
            };
            ui.set_min_size(egui::vec2(viewport_w, viewport_h));
            let scroll = if prefs.word_wrap {
                egui::ScrollArea::vertical()
            } else {
                egui::ScrollArea::both()
            };
            scroll
                .id_salt(("actions_source_scroll", id))
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    let band = ui.painter().add(egui::Shape::Noop);
                    let mut output = egui::TextEdit::multiline(source)
                        .id(id)
                        .code_editor()
                        .lock_focus(true)
                        .font(font_id.clone())
                        .layouter(&mut layouter)
                        .frame(false)
                        .margin(egui::Margin {
                            left: (gutter + space::S3).min(120.0) as i8,
                            right: space::S2 as i8,
                            top: space::S1 as i8,
                            bottom: space::S1 as i8,
                        })
                        .desired_width(f32::INFINITY)
                        .min_size(egui::vec2(viewport_w, viewport_h))
                        .hint_text(
                            egui::RichText::new(hint)
                                .font(font_id.clone())
                                .color(p.text_tertiary),
                        )
                        .show(ui);
                    if pre_edited {
                        output.response.mark_changed();
                    }
                    let rect = output.response.rect;
                    let gutter_left = rect.left().max(ui.clip_rect().left());
                    let gutter_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter_left, rect.top()),
                        egui::pos2(gutter_left + gutter, rect.bottom()),
                    );
                    let cursor = output.state.cursor.char_range().map(|range| range.primary);
                    if let Some(cursor) = cursor {
                        caret = nf::code::line_col(source, cursor.index);
                        if prefs.highlight_current_line {
                            ui.painter().set(
                                band,
                                nf::code::current_line(
                                    &ctx,
                                    &output.galley,
                                    output.galley_pos,
                                    cursor,
                                    egui::Rangef::new(gutter_rect.right(), rect.right()),
                                ),
                            );
                        }
                    }
                    if prefs.line_numbers {
                        nf::Gutter {
                            markers,
                            current_line: if prefs.highlight_current_line {
                                cursor.map(|_| caret.0)
                            } else {
                                None
                            },
                        }
                        .paint(
                            ui,
                            gutter_rect,
                            &output.galley,
                            output.galley_pos,
                        );
                    } else {
                        paint_marker_gutter(
                            ui,
                            gutter_rect,
                            markers,
                            &output.galley,
                            output.galley_pos,
                        );
                    }
                    if ctx.input(|i| i.pointer.primary_clicked())
                        && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
                        && gutter_rect.contains(pos)
                    {
                        let shift = ctx.input(|i| i.modifiers.shift);
                        let mut line = 1usize;
                        let mut first_row = true;
                        for row in &output.galley.rows {
                            let r = row.rect().translate(output.galley_pos.to_vec2());
                            if first_row && pos.y >= r.top() && pos.y <= r.bottom() {
                                gutter_click = Some((line, shift));
                                break;
                            }
                            first_row = row.ends_with_newline;
                            if row.ends_with_newline {
                                line += 1;
                            }
                        }
                    }
                    if prefs.code_hints && output.response.changed() && frame_typed_dot {
                        if prefs.hint_delay_ms == 0 {
                            hint_state = HintState {
                                open: true,
                                ..Default::default()
                            };
                        } else {
                            let now = ctx.input(|i| i.time);
                            hint_state = HintState {
                                pending_at: Some(now),
                                ..Default::default()
                            };
                        }
                    }
                    if let Some(at) = hint_state.pending_at {
                        let valid_pending = output.response.has_focus()
                            && cursor
                                .and_then(|c| hints().and_then(|h| h.hints(source, c.index, false)))
                                .is_some();
                        if !valid_pending {
                            hint_state = HintState::default();
                        } else {
                            let now = ctx.input(|i| i.time);
                            let remaining = at + f64::from(prefs.hint_delay_ms) / 1000.0 - now;
                            if remaining <= 1e-6 {
                                hint_state.pending_at = None;
                                hint_state.open = true;
                            } else {
                                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                                    remaining,
                                ));
                            }
                        }
                    }
                    if hint_state.open && !output.response.has_focus() && !hint_state.hovered {
                        hint_state = HintState::default();
                    }
                    if hint_state.open {
                        let found = cursor.and_then(|cursor| {
                            hints().and_then(|context| {
                                let found =
                                    context.hints(source, cursor.index, hint_state.explicit)?;
                                Some((context, found, cursor))
                            })
                        });
                        match found {
                            Some((_, found, _))
                                if hint_state.anchor.is_some_and(|a| a != found.start) =>
                            {
                                hint_state = HintState::default();
                            }
                            Some((context, found, cursor)) => {
                                hint_state.anchor = Some(found.start);
                                hint_state.count = found.items.len();
                                hint_state.selected =
                                    hint_state.selected.min(found.items.len() - 1);
                                let at = output.galley.pos_from_cursor(cursor);
                                let anchor = output.galley_pos
                                    + at.left_bottom().to_vec2()
                                    + egui::vec2(0., 2.);
                                if let Some(index) =
                                    hint_popover(ui, id, anchor, &found.items, &mut hint_state)
                                {
                                    accept_hint(
                                        &ctx,
                                        id,
                                        source,
                                        &context,
                                        hint_state.explicit,
                                        index,
                                    );
                                    hint_state = HintState::default();
                                }
                            }
                            None => hint_state = HintState::default(),
                        }
                    }
                    if *source == checked_source {
                        let text_clip = ui.clip_rect().intersect(egui::Rect::from_min_max(
                            egui::pos2(gutter_rect.right(), rect.top()),
                            rect.max,
                        ));
                        diagnostic_action =
                            diagnostic_ui.paint(ui, id, source, diagnostics, &output, text_clip);
                    } else {
                        diagnostic_ui.close();
                    }
                    output.response
                })
                .inner
        })
        .inner;
    ctx.data_mut(|d| d.insert_temp(state_id, hint_state));
    if diagnostic_ui.is_open() {
        // egui handles Escape at begin_pass, before our popup can consume it. Retain text focus
        // for this popup so Escape closes the explanation/menu without leaving the editor.
        ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }
    (response, caret, diagnostic_action, gutter_click)
}

#[cfg(test)]
pub(crate) fn tests_jump(
    ctx: &egui::Context,
    id: egui::Id,
    source: &str,
    line: usize,
    column: usize,
) {
    jump_to(ctx, id, source, line, column);
}

/// Move the caret of the code editor `id` to a 1-based line / column and focus it.
pub(crate) fn jump_to(ctx: &egui::Context, id: egui::Id, source: &str, line: usize, column: usize) {
    let index = nf::code::char_index(source, line, column);
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(index),
        )));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use neoflash_core::{Element, FrameScript, ObjectEvent, ObjectScript, Project};

    fn render(editor: &mut Editor, ctx: &egui::Context, quick_fix: bool) -> egui::FullOutput {
        let modifiers = egui::Modifiers::COMMAND;
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400., 540.),
                )),
                modifiers: if quick_fix {
                    modifiers
                } else {
                    egui::Modifiers::NONE
                },
                events: if quick_fix {
                    vec![egui::Event::Key {
                        key: egui::Key::Period,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.actions_contents(ui));
            },
        )
    }

    fn source(editor: &Editor, target: ScriptTarget) -> String {
        match target {
            ScriptTarget::Frame => editor.symbol().timeline.scripts[0].script.source.clone(),
            ScriptTarget::Object => editor.selected_element().unwrap().scripts[0]
                .script
                .source
                .clone(),
            ScriptTarget::Source => editor.project.sources[0].source.clone(),
        }
    }

    #[test]
    fn actions_diagnostic_quick_fix_family_uses_current_source_and_one_undo() {
        for target in [
            ScriptTarget::Frame,
            ScriptTarget::Object,
            ScriptTarget::Source,
        ] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut project = Project::blank();
            let text = if target == ScriptTarget::Source {
                "class Helper {\n public function run():Boolean { return not false; }\n}"
            } else {
                "// é: character positions\ntrace(not false);"
            };
            let script = Script {
                language: ScriptLanguage::ActionScript2,
                source: text.into(),
            };
            let mut element = Element {
                id: "tooling-object".into(),
                ..Element::default()
            };
            element.scripts.push(ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: script.clone(),
            });
            let element_id = element.id.clone();
            project.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .push(element);
            project.symbols[0].timeline.scripts.push(FrameScript {
                frame: 0,
                script: script.clone(),
            });
            project.sources.push(SourceUnit {
                id: "helper-source".into(),
                path: "Helper.as".into(),
                language: ScriptLanguage::ActionScript2,
                source: if target == ScriptTarget::Source {
                    text.into()
                } else {
                    "class Helper {}".into()
                },
            });
            project.enable_program();
            let mut editor = Editor::from_project(&ctx, project);
            editor.script_target = target;
            editor.script_event = ObjectEvent::EnterFrame;
            editor.selected_source = Some("helper-source".into());
            editor.select_single_id(element_id);
            let original = editor.project.clone();
            let _ = render(&mut editor, &ctx, false);
            if target == ScriptTarget::Source {
                editor.check_project_sources(false);
            } else {
                editor.check_action_source(&script, false);
            }
            let feedback = editor.current_feedback(&script).unwrap();
            assert!(feedback.success, "{:?}", feedback.diagnostics);
            assert!(
                feedback
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == DiagnosticSeverity::Warning
                        && d.code.as_deref() == Some("deprecated-construct")
                        && d.line == 2)
            );
            let output = render(&mut editor, &ctx, false);
            let labels: Vec<_> = output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .into_iter()
                .filter_map(|(_, node)| node.label().map(str::to_owned))
                .collect();
            assert!(
                labels.iter().any(|label| label.starts_with("Warning:")),
                "{labels:?}"
            );
            jump_to(&ctx, editor.actions_editor_id(), text, 2, 1);
            let _ = render(&mut editor, &ctx, true);
            assert_eq!(
                source(&editor, target),
                text.replace("not false", "! false")
            );
            assert!(
                editor.actions_ui.edited.is_some(),
                "command edits schedule live diagnostics"
            );
            editor.project.validate().unwrap();
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());
            // An in-bounds edit from the old snapshot must not be applied to new text.
            let changed = text.replace("not false", "true");
            editor.transact(|editor| match target {
                ScriptTarget::Frame => {
                    editor.symbol_mut().timeline.scripts[0].script.source = changed.clone()
                }
                ScriptTarget::Object => {
                    editor.selected_element_mut().unwrap().scripts[0]
                        .script
                        .source = changed.clone()
                }
                ScriptTarget::Source => editor.project.sources[0].source = changed.clone(),
            });
            jump_to(&ctx, editor.actions_editor_id(), &changed, 2, 1);
            let _ = render(&mut editor, &ctx, true);
            assert_eq!(source(&editor, target), changed);
            assert!(editor.output.last().unwrap().contains("No quick fix"));
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());
            if target == ScriptTarget::Frame {
                let ambiguous = "// two suggestions on the same line\ntrace(not false and true);";
                editor.transact(|editor| {
                    editor.symbol_mut().timeline.scripts[0].script.source = ambiguous.into()
                });
                jump_to(&ctx, editor.actions_editor_id(), ambiguous, 2, 1);
                let _ = render(&mut editor, &ctx, true);
                assert_eq!(source(&editor, target), ambiguous);
                assert!(
                    ctx.data(|d| d.get_temp::<diagnostics_ui::DiagnosticUi>(
                        diagnostics_ui::state_id(editor.actions_editor_id())
                    ))
                    .unwrap()
                    .is_open()
                );
                editor.undo();
                assert_eq!(editor.project, original);
                assert!(!editor.history.can_undo());
            }
        }
    }

    #[test]
    fn project_diagnostics_navigate_to_their_source_without_foreign_markers() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.sources = vec![
            SourceUnit {
                id: "good".into(),
                path: "good.ts".into(),
                language: ScriptLanguage::TypeScript,
                source: "export const count: number = 1;".into(),
            },
            SourceUnit {
                id: "bad".into(),
                path: "bad.ts".into(),
                language: ScriptLanguage::TypeScript,
                source: "// é\nexport const count: number = \"bad\";".into(),
            },
        ];
        project.enable_program();
        let mut editor = Editor::from_project(&ctx, project);
        editor.script_target = ScriptTarget::Source;
        editor.selected_source = Some("good".into());
        editor.check_project_sources(false);
        let diagnostic = editor
            .code_feedback
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .find(|d| {
                d.severity == DiagnosticSeverity::Error && d.path.as_deref() == Some("bad.ts")
            })
            .expect("project errors carry their source path")
            .clone();
        assert!(!editor.diagnostic_is_current(&diagnostic));
        editor.jump_to_diagnostic(&ctx, "not the error's source", &diagnostic);
        assert_eq!(editor.selected_source.as_deref(), Some("bad"));
        assert!(editor.diagnostic_is_current(&diagnostic));
        let state = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id()).unwrap();
        let caret = state.cursor.char_range().unwrap().primary.index;
        assert_eq!(
            nf::code::line_col(&editor.project.sources[1].source, caret),
            (diagnostic.line, diagnostic.column)
        );
        assert!(!editor.history.can_undo());
        editor.project.validate().unwrap();
    }

    #[test]
    fn actions_flow_feedback_is_advisory_scoped_and_tracks_dependencies() {
        let ctx = egui::Context::default();
        let text = "function consume(point:Point):void { trace(point.x); }\nvar wrong:Rectangle = new Rectangle(0,0,10,10);\nconsume(wrong);";
        let script = Script {
            language: ScriptLanguage::ActionScript3,
            source: text.into(),
        };
        let mut project = Project::blank();
        project.symbols[0].timeline.scripts.push(FrameScript {
            frame: 0,
            script: script.clone(),
        });
        let mut editor = Editor::from_project(&ctx, project);
        let original = editor.project.clone();
        // Exercise the same debounce used by the visible Actions editor, including its delayed run.
        for (time, changed) in [(1., true), (1.7, false)] {
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut response = ui.label("analysis");
                        if changed {
                            response.mark_changed();
                        }
                        editor.live_check(
                            ctx,
                            editor.actions_editor_id(),
                            &response,
                            text,
                            script.language,
                        );
                    });
                },
            );
        }
        let feedback = editor.current_feedback(&script).unwrap();
        assert!(
            feedback.success,
            "advisory checking must not redefine compiler support: {:?}",
            feedback.diagnostics
        );
        let diagnostic = feedback
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some("flow-argument-type"))
            .expect("Actions live checks consume semantic analysis");
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.line, 3);
        assert!(diagnostic.path.is_none());
        assert!(
            diagnostic.fixes.is_empty(),
            "type-flow advice must not rewrite intentional coercions"
        );
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());

        let context = editor.hint_context(script.language).unwrap();
        let reassigned = "var value = new Point(); value = new Rectangle(); value.";
        let hints = context
            .hints(reassigned, reassigned.chars().count(), true)
            .expect("incomplete member access keeps preceding flow facts");
        assert!(hints.items.iter().any(|hint| hint.label == "bottomRight"));
        assert!(!hints.items.iter().any(|hint| hint.label == "length"));
        let joined = "var value = new Point(); if (condition) { value = new Rectangle(); } value.";
        let hints = context.hints(joined, joined.chars().count(), true);
        assert!(
            hints.is_none_or(|hints| hints
                .items
                .iter()
                .all(|hint| hint.label != "bottomRight" && hint.label != "length")),
            "a branch join cannot retain one branch's concrete type"
        );

        for (language, source) in [
            (
                ScriptLanguage::ActionScript2,
                "import flash.geom.Point; import flash.geom.Rectangle; function consume(point:Point):Void {} var wrong:Rectangle = new Rectangle(); consume(wrong);",
            ),
            (
                ScriptLanguage::ActionScript3,
                "function consume(value:Number):Number { return '4'; } consume('3');",
            ),
            (
                ScriptLanguage::ActionScript3,
                "function consume(point:Point):void {} var dynamicValue:* = {}; consume(dynamicValue);",
            ),
        ] {
            let current = Script {
                language,
                source: source.into(),
            };
            editor.check_action_source(&current, false);
            let feedback = editor.current_feedback(&current).unwrap();
            assert!(feedback.success, "{}: {:?}", source, feedback.diagnostics);
            assert!(
                !feedback.diagnostics.iter().any(|d| d
                    .code
                    .as_deref()
                    .is_some_and(|code| code.starts_with("flow-"))),
                "intentional coercion/dynamic flow: {:?}",
                feedback.diagnostics
            );
        }

        let included = Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"logic.as\"\ntrace(value);".into(),
        };
        editor.project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "logic",
                "logic.as",
                "text/plain",
                b"var value = 1;",
            )
            .unwrap(),
        );
        editor.check_action_source(&included, false);
        assert!(editor.current_feedback(&included).unwrap().success);
        editor.project.resources[0] = neoflash_core::ResourceUnit::from_bytes(
            "logic",
            "logic.as",
            "text/plain",
            b"var value = ;",
        )
        .unwrap();
        assert!(
            editor.current_feedback(&included).is_none(),
            "include edits invalidate cached acceptance"
        );
        editor.check_action_source(&included, false);
        assert!(!editor.current_feedback(&included).unwrap().success);

        editor.project.resources.clear();
        let class_text = "package { import flash.geom.Point; import flash.geom.Rectangle; public class Helper { public function consume(point:Point):void {} public function run():void { consume(new Rectangle()); } } }";
        editor.project.sources.push(SourceUnit {
            id: "helper-flow".into(),
            path: "Helper.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: class_text.into(),
        });
        editor.project.enable_program();
        editor.script_target = ScriptTarget::Source;
        editor.selected_source = Some("helper-flow".into());
        editor.check_project_sources(false);
        let diagnostic = editor
            .code_feedback
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .find(|d| d.code.as_deref() == Some("flow-argument-type"))
            .expect("project source semantic diagnostics are collected");
        assert_eq!(diagnostic.path.as_deref(), Some("Helper.as"));
        assert!(editor.diagnostic_is_current(diagnostic));

        // Unrelated timeline instances are not implicit fields of a helper class.
        let stage_element = Element {
            id: "named-flow-instance".into(),
            name: "stageOnly".into(),
            ..Element::default()
        };
        editor.project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(stage_element);
        let context = editor.hint_context(ScriptLanguage::ActionScript3).unwrap();
        let hints = context
            .hints(class_text, class_text.chars().count(), true)
            .unwrap();
        assert!(!hints.items.iter().any(|hint| hint.label == "stageOnly"));
        let before = editor.code_fingerprint(&script);
        editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].name =
            "renamed".into();
        assert_ne!(before, editor.code_fingerprint(&script));
        let before = editor.code_fingerprint(&script);
        editor.project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
            .scripts
            .push(neoflash_core::ObjectScript {
                event: neoflash_core::ObjectEvent::Load,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "#initclip\nvar initialized = true;\n#endinitclip".into(),
                },
            });
        assert_ne!(before, editor.code_fingerprint(&script));
    }

    #[test]
    fn conditional_and_data_breakpoints_watch_evaluation_and_live_mutation() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert!(!editor.has_breakpoint("Main.as", 12));
        assert!(editor.toggle_breakpoint("Main.as", 12));
        assert!(editor.has_breakpoint("Main.as", 12));
        assert!(!editor.toggle_breakpoint("Main.as", 12));
        assert!(!editor.has_breakpoint("Main.as", 12));

        editor.set_paused_debugger_scopes(vec![DebuggerScope {
            name: "Local".into(),
            variables: vec![
                DebuggerVariable::scalar("score", "40", "Number"),
                DebuggerVariable::object(
                    "mc",
                    "MovieClip",
                    vec![
                        DebuggerVariable::scalar("x", "120.5", "Number"),
                        DebuggerVariable::scalar("visible", "true", "Boolean"),
                    ],
                ),
                DebuggerVariable::object(
                    "player",
                    "Object",
                    vec![DebuggerVariable::object(
                        "stats",
                        "Object",
                        vec![
                            DebuggerVariable::scalar("hp", "25", "Number"),
                            DebuggerVariable::scalar("maxHp", "100", "Number"),
                        ],
                    )],
                ),
                DebuggerVariable {
                    name: "items".into(),
                    value: "[Array(2)]".into(),
                    kind: "Array".into(),
                    children: vec![
                        DebuggerVariable::scalar("0", "potion", "String"),
                        DebuggerVariable::scalar("1", "shield", "String"),
                    ],
                },
            ],
        }]);
        assert_eq!(editor.paused_debugger_scopes().len(), 1);

        // Expression evaluation: identifiers, dotted paths, array indexing, arithmetic, comparisons, booleans
        assert_eq!(editor.evaluate_debugger_expression("score").unwrap(), "40");
        assert_eq!(
            editor.evaluate_debugger_expression("mc.x").unwrap(),
            "120.5"
        );
        assert_eq!(
            editor
                .evaluate_debugger_expression("player.stats.hp")
                .unwrap(),
            "25"
        );
        assert_eq!(
            editor.evaluate_debugger_expression("items[0]").unwrap(),
            "potion"
        );
        assert_eq!(
            editor.evaluate_debugger_expression("items[1]").unwrap(),
            "shield"
        );
        assert_eq!(
            editor.evaluate_debugger_expression("score + 10").unwrap(),
            "50"
        );
        assert_eq!(
            editor
                .evaluate_debugger_expression("player.stats.maxHp - player.stats.hp * 2")
                .unwrap(),
            "50"
        );
        assert_eq!(
            editor
                .evaluate_debugger_expression("player.stats.hp <= 0")
                .unwrap(),
            "false"
        );
        assert_eq!(
            editor
                .evaluate_debugger_expression("player.stats.hp <= 25 && mc.visible")
                .unwrap(),
            "true"
        );
        assert!(
            editor
                .evaluate_debugger_expression("unknownVar.prop")
                .is_err()
        );

        // Watches
        let w0 = editor.add_debugger_watch("player.stats.hp");
        let w1 = editor.add_debugger_watch("score + 10");
        let w2 = editor.add_debugger_watch("missingSymbol");
        let watches = editor.debugger_watches();
        assert_eq!(watches[w0].last_value, "25");
        assert!(!watches[w0].is_error);
        assert_eq!(watches[w1].last_value, "50");
        assert!(!watches[w1].is_error);
        assert!(watches[w2].is_error);

        // Conditional breakpoint
        editor.set_breakpoint_spec(
            "Main.as",
            BreakpointSpec::new(20).with_condition("player.stats.hp <= 0"),
        );
        assert!(!editor.should_pause_at_breakpoint("Main.as", 20));
        assert_eq!(editor.breakpoint_spec("Main.as", 20).unwrap().hit_count, 0);

        // Data / property-change breakpoint
        editor.set_breakpoint_spec(
            "Main.as",
            BreakpointSpec::new(30).with_watch_property("player.stats.hp"),
        );
        // Unchanged from baseline -> does not pause
        assert!(!editor.should_pause_at_breakpoint("Main.as", 30));

        // Deep object inspection & live in-pause variable editing
        let hp_var = editor
            .inspect_debugger_variable_path(0, "player.stats.hp")
            .expect("nested variable found");
        assert_eq!(hp_var.value, "25");

        editor
            .set_debugger_variable_value(0, "player.stats.hp", "0")
            .expect("live mutation succeeds");
        assert_eq!(
            editor
                .evaluate_debugger_expression("player.stats.hp")
                .unwrap(),
            "0"
        );
        // Watch automatically refreshes on variable mutation
        assert_eq!(editor.debugger_watches()[w0].last_value, "0");

        // Now both the conditional breakpoint (hp <= 0) and the data breakpoint (hp changed 25 -> 0) pause!
        assert!(editor.should_pause_at_breakpoint("Main.as", 20));
        assert_eq!(editor.breakpoint_spec("Main.as", 20).unwrap().hit_count, 1);
        assert!(editor.should_pause_at_breakpoint("Main.as", 30));
        assert_eq!(editor.breakpoint_spec("Main.as", 30).unwrap().hit_count, 1);
        // Second check without further hp change does not pause data breakpoint
        assert!(!editor.should_pause_at_breakpoint("Main.as", 30));

        assert!(editor.remove_debugger_watch(w2));
        assert_eq!(editor.debugger_watches().len(), 2);
    }

    #[test]
    fn code_folding_and_bounded_script_assist_across_languages() {
        let src = "\
function onEnterFrame() {
    if (ready) {
        trace(\"{not a brace}\");
        step();
    }
}
";
        let regions = compute_fold_regions(src);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].start_line, 1);
        assert_eq!(regions[0].end_line, 6);
        assert_eq!(regions[1].start_line, 2);
        assert_eq!(regions[1].end_line, 5);

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert_eq!(editor.visible_source_lines(src).len(), 6);
        assert!(editor.toggle_fold_line(2));
        let visible_inner_folded = editor.visible_source_lines(src);
        // Lines 3, 4, 5 are hidden inside the fold starting on line 2
        assert_eq!(visible_inner_folded.len(), 3);
        assert_eq!(visible_inner_folded[0].0, 1);
        assert_eq!(visible_inner_folded[1].0, 2);
        assert!(visible_inner_folded[1].1.contains('…'));
        assert_eq!(visible_inner_folded[2].0, 6);

        // Unfold line 2
        assert!(!editor.toggle_fold_line(2));
        assert_eq!(editor.visible_source_lines(src).len(), 6);

        // Bounded Script Assist across AS2, AS3, JS, TS
        let stop_as3 = format_script_assist_snippet(
            &ScriptAssistTemplate::StopTimeline,
            ScriptLanguage::ActionScript3,
        );
        assert_eq!(stop_as3, "stop();\n");
        let stop_js = format_script_assist_snippet(
            &ScriptAssistTemplate::StopTimeline,
            ScriptLanguage::JavaScript,
        );
        assert_eq!(stop_js, "this.stop();\n");

        let rel_as2 = format_script_assist_snippet(
            &ScriptAssistTemplate::OnReleaseHandler {
                body: "play();".into(),
            },
            ScriptLanguage::ActionScript2,
        );
        assert!(rel_as2.contains("on (release)"));

        let rel_as3 = format_script_assist_snippet(
            &ScriptAssistTemplate::OnReleaseHandler {
                body: "play();".into(),
            },
            ScriptLanguage::ActionScript3,
        );
        assert!(rel_as3.contains("MouseEvent.CLICK"));

        let ef_ts = format_script_assist_snippet(
            &ScriptAssistTemplate::EnterFrameLoop {
                body: "this.x += 1;".into(),
            },
            ScriptLanguage::TypeScript,
        );
        assert!(ef_ts.contains("\"enterFrame\""));
        assert!(ef_ts.contains(": void =>"));

        let var_as2 = format_script_assist_snippet(
            &ScriptAssistTemplate::VariableDeclaration {
                name: "score".into(),
                var_type: "Number".into(),
                initial_value: "0".into(),
            },
            ScriptLanguage::ActionScript2,
        );
        assert_eq!(var_as2, "var score:Number = 0;\n");

        let var_ts = format_script_assist_snippet(
            &ScriptAssistTemplate::VariableDeclaration {
                name: "score".into(),
                var_type: "number".into(),
                initial_value: "0".into(),
            },
            ScriptLanguage::TypeScript,
        );
        assert_eq!(var_ts, "let score: number = 0;\n");

        // Inserting via Editor method updates the frame script and supports Undo
        editor.script_target = ScriptTarget::Frame;
        assert!(
            editor
                .insert_script_assist_snippet(ScriptAssistTemplate::GotoAndPlayFrame { frame: 5 })
        );
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "this.gotoAndPlay(5);\n"
        );
        assert!(editor.history.can_undo());
    }
}

/// Conditional and data/property-change breakpoint metadata for a source line (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakpointSpec {
    pub line: usize,
    pub condition: Option<String>,
    pub hit_count: u32,
    pub watch_property: Option<String>,
}

impl BreakpointSpec {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(line: usize) -> Self {
        Self {
            line: line.max(1),
            condition: None,
            hit_count: 0,
            watch_property: None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        let condition = condition.into();
        self.condition = (!condition.trim().is_empty()).then(|| condition.trim().to_owned());
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_watch_property(mut self, watch_property: impl Into<String>) -> Self {
        let watch_property = watch_property.into();
        self.watch_property =
            (!watch_property.trim().is_empty()).then(|| watch_property.trim().to_owned());
        self
    }
}

/// Watched debugger expression evaluated against the paused stack frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchExpression {
    pub expression: String,
    pub last_value: String,
    pub is_error: bool,
}

/// Hierarchical variable node in a paused debugger scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebuggerVariable {
    pub name: String,
    pub value: String,
    pub kind: String,
    pub children: Vec<DebuggerVariable>,
}

impl DebuggerVariable {
    pub fn scalar(
        name: impl Into<String>,
        value: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            kind: kind.into(),
            children: Vec::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn object(
        name: impl Into<String>,
        class_name: impl Into<String>,
        children: Vec<DebuggerVariable>,
    ) -> Self {
        let class_str = class_name.into();
        Self {
            name: name.into(),
            value: format!("[object {class_str}]"),
            kind: class_str,
            children,
        }
    }
}

/// Named variable scope (`Local`, `Closure`, `Global`) in a paused stack frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebuggerScope {
    pub name: String,
    pub variables: Vec<DebuggerVariable>,
}

/// Deterministic brace/block fold range (1-based inclusive line numbers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRegion {
    pub start_line: usize,
    pub end_line: usize,
    pub label: String,
}

/// Bounded language-aware Script Assist templates for structured statement insertion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptAssistTemplate {
    StopTimeline,
    GotoAndPlayFrame {
        frame: u32,
    },
    GotoAndStopFrame {
        frame: u32,
    },
    OnReleaseHandler {
        body: String,
    },
    EnterFrameLoop {
        body: String,
    },
    TraceExpression {
        expression: String,
    },
    VariableDeclaration {
        name: String,
        var_type: String,
        initial_value: String,
    },
}

impl ScriptAssistTemplate {
    pub(crate) fn default_menu_items() -> Vec<(&'static str, Self)> {
        vec![
            ("Stop timeline", Self::StopTimeline),
            ("Goto and play frame…", Self::GotoAndPlayFrame { frame: 1 }),
            ("Goto and stop frame…", Self::GotoAndStopFrame { frame: 1 }),
            (
                "On click / release handler",
                Self::OnReleaseHandler {
                    body: "stop();".into(),
                },
            ),
            (
                "Enter frame loop",
                Self::EnterFrameLoop {
                    body: "// frame update".into(),
                },
            ),
            (
                "Trace / log expression",
                Self::TraceExpression {
                    expression: "\"trace\"".into(),
                },
            ),
            (
                "Declare variable",
                Self::VariableDeclaration {
                    name: "value".into(),
                    var_type: "Number".into(),
                    initial_value: "0".into(),
                },
            ),
        ]
    }
}

/// Format a `ScriptAssistTemplate` for the given `ScriptLanguage`.
pub fn format_script_assist_snippet(
    template: &ScriptAssistTemplate,
    language: ScriptLanguage,
) -> String {
    match template {
        ScriptAssistTemplate::StopTimeline => match language {
            ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3 => "stop();\n".to_owned(),
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => "this.stop();\n".to_owned(),
        },
        ScriptAssistTemplate::GotoAndPlayFrame { frame } => {
            let f = (*frame).max(1);
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("gotoAndPlay({f});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("this.gotoAndPlay({f});\n")
                }
            }
        }
        ScriptAssistTemplate::GotoAndStopFrame { frame } => {
            let f = (*frame).max(1);
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("gotoAndStop({f});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("this.gotoAndStop({f});\n")
                }
            }
        }
        ScriptAssistTemplate::OnReleaseHandler { body } => {
            let body_line = body.trim();
            match language {
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    format!("on (release) {{\n    {body_line}\n}}\n")
                }
                ScriptLanguage::ActionScript3 => format!(
                    "this.addEventListener(MouseEvent.CLICK, function(event:MouseEvent):void {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::JavaScript => format!(
                    "this.addEventListener(\"click\", (event) => {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::TypeScript => format!(
                    "this.addEventListener(\"click\", (event: MouseEvent): void => {{\n    {body_line}\n}});\n"
                ),
            }
        }
        ScriptAssistTemplate::EnterFrameLoop { body } => {
            let body_line = body.trim();
            match language {
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    format!("this.onEnterFrame = function():Void {{\n    {body_line}\n}};\n")
                }
                ScriptLanguage::ActionScript3 => format!(
                    "this.addEventListener(Event.ENTER_FRAME, function(event:Event):void {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::JavaScript => format!(
                    "this.addEventListener(\"enterFrame\", () => {{\n    {body_line}\n}});\n"
                ),
                ScriptLanguage::TypeScript => format!(
                    "this.addEventListener(\"enterFrame\", (): void => {{\n    {body_line}\n}});\n"
                ),
            }
        }
        ScriptAssistTemplate::TraceExpression { expression } => {
            let expr = expression.trim();
            match language {
                ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3 => format!("trace({expr});\n"),
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    format!("console.log({expr});\n")
                }
            }
        }
        ScriptAssistTemplate::VariableDeclaration {
            name,
            var_type,
            initial_value,
        } => {
            let name = name.trim();
            let var_type = var_type.trim();
            let init = initial_value.trim();
            match language {
                ScriptLanguage::ActionScript1 => format!("var {name} = {init};\n"),
                ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript3 => {
                    if var_type.is_empty() {
                        format!("var {name} = {init};\n")
                    } else {
                        format!("var {name}:{var_type} = {init};\n")
                    }
                }
                ScriptLanguage::JavaScript => format!("let {name} = {init};\n"),
                ScriptLanguage::TypeScript => {
                    if var_type.is_empty() {
                        format!("let {name} = {init};\n")
                    } else {
                        format!("let {name}: {var_type} = {init};\n")
                    }
                }
            }
        }
    }
}

pub(crate) fn append_script_assist_to_source(source: &mut String, snippet: &str) {
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(snippet);
}

/// Compute deterministic brace/block folding regions (`{ ... }`) for `source`, ignoring braces
/// inside comments and string literals.
pub fn compute_fold_regions(source: &str) -> Vec<FoldRegion> {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut regions: Vec<FoldRegion> = Vec::new();
    let mut in_block_comment = false;

    for (idx, raw_line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0;
        let mut in_string: Option<char> = None;

        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();

            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if let Some(quote) = in_string {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    in_string = None;
                }
                i += 1;
                continue;
            }

            if c == '/' && next == Some('/') {
                break;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            if c == '"' || c == '\'' {
                in_string = Some(c);
                i += 1;
                continue;
            }

            if c == '{' {
                let header = raw_line.trim();
                let prefix = header.strip_suffix('{').unwrap_or(header).trim();
                let label = if prefix.is_empty() {
                    "{ … }".to_owned()
                } else {
                    format!("{prefix} {{ … }}")
                };
                stack.push((line_no, label));
            } else if c == '}'
                && let Some((start_line, label)) = stack.pop()
                && line_no > start_line
            {
                regions.push(FoldRegion {
                    start_line,
                    end_line: line_no,
                    label,
                });
            }
            i += 1;
        }
    }

    regions.sort_by_key(|r| (r.start_line, r.end_line));
    regions
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
struct DebuggerAuthoringState {
    breakpoints: std::collections::BTreeMap<String, Vec<BreakpointSpec>>,
    last_watched_values: std::collections::BTreeMap<(String, usize, String), String>,
    debugger_watches: Vec<WatchExpression>,
    paused_scopes: Vec<DebuggerScope>,
    folded_lines_by_tab: std::collections::BTreeMap<String, std::collections::BTreeSet<usize>>,
}

static DEBUGGER_AUTHORING_STATES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, DebuggerAuthoringState>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

#[derive(Clone, Debug, PartialEq)]
enum EvalValue {
    Number(f64),
    Bool(bool),
    Str(String),
    Null,
    Undefined,
}

impl EvalValue {
    fn from_variable(var: &DebuggerVariable) -> Self {
        let trimmed = var.value.trim();
        match var.kind.as_str() {
            "Number" | "int" | "uint" => {
                if let Ok(n) = trimmed.parse::<f64>() {
                    return Self::Number(n);
                }
            }
            "Boolean" => {
                if trimmed == "true" {
                    return Self::Bool(true);
                }
                if trimmed == "false" {
                    return Self::Bool(false);
                }
            }
            "Null" => return Self::Null,
            "Undefined" => return Self::Undefined,
            _ => {}
        }
        if trimmed == "true" {
            Self::Bool(true)
        } else if trimmed == "false" {
            Self::Bool(false)
        } else if trimmed == "null" {
            Self::Null
        } else if trimmed == "undefined" {
            Self::Undefined
        } else if let Ok(n) = trimmed.parse::<f64>() {
            Self::Number(n)
        } else {
            let unquoted = trimmed
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| {
                    trimmed
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                })
                .unwrap_or(trimmed);
            Self::Str(unquoted.to_owned())
        }
    }

    fn format_display(&self) -> String {
        match self {
            Self::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Self::Bool(b) => b.to_string(),
            Self::Str(s) => s.clone(),
            Self::Null => "null".to_owned(),
            Self::Undefined => "undefined".to_owned(),
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Bool(b) => *b,
            Self::Str(s) => !s.is_empty(),
            Self::Null | Self::Undefined => false,
        }
    }

    fn as_f64(&self) -> Result<f64, String> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Self::Str(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("TypeError: cannot convert '{s}' to Number")),
            Self::Null => Ok(0.0),
            Self::Undefined => Err("TypeError: undefined is not a number".to_owned()),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn split_var_path(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in path.trim().chars() {
        match ch {
            '.' | '[' | ']' => {
                let seg = current.trim().trim_matches('"').trim_matches('\'');
                if !seg.is_empty() {
                    segments.push(seg.to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let tail = current.trim().trim_matches('"').trim_matches('\'');
    if !tail.is_empty() {
        segments.push(tail.to_owned());
    }
    segments
}

fn find_variable_in_slice<'a>(
    vars: &'a [DebuggerVariable],
    segments: &[String],
) -> Option<&'a DebuggerVariable> {
    let (first, rest) = segments.split_first()?;
    let bracketed = format!("[{first}]");
    let found = vars
        .iter()
        .find(|v| v.name == *first || v.name == bracketed)
        .or_else(|| first.parse::<usize>().ok().and_then(|idx| vars.get(idx)))?;
    if rest.is_empty() {
        Some(found)
    } else {
        find_variable_in_slice(&found.children, rest)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn find_variable_in_slice_mut<'a>(
    vars: &'a mut [DebuggerVariable],
    segments: &[String],
) -> Option<&'a mut DebuggerVariable> {
    let (first, rest) = segments.split_first()?;
    let bracketed = format!("[{first}]");
    let pos = vars
        .iter()
        .position(|v| v.name == *first || v.name == bracketed)
        .or_else(|| first.parse::<usize>().ok().filter(|&idx| idx < vars.len()))?;
    let found = &mut vars[pos];
    if rest.is_empty() {
        Some(found)
    } else {
        find_variable_in_slice_mut(&mut found.children, rest)
    }
}

struct ExprEvaluator<'a> {
    scopes: &'a [DebuggerScope],
}

impl<'a> ExprEvaluator<'a> {
    fn eval(&self, input: &str) -> Result<EvalValue, String> {
        let s = input.trim();
        if s.is_empty() {
            return Err("Empty expression".to_owned());
        }
        self.eval_or(s)
    }

    fn split_top_level<'b>(s: &'b str, ops: &[&'b str]) -> Option<(&'b str, &'b str, &'b str)> {
        let bytes = s.as_bytes();
        let mut depth_paren = 0i32;
        let mut depth_bracket = 0i32;
        let mut in_quote: Option<u8> = None;
        let mut i = bytes.len();
        while i > 0 {
            i -= 1;
            let b = bytes[i];
            if let Some(q) = in_quote {
                if b == q && (i == 0 || bytes[i - 1] != b'\\') {
                    in_quote = None;
                }
                continue;
            }
            match b {
                b'"' | b'\'' => in_quote = Some(b),
                b')' => depth_paren += 1,
                b'(' => depth_paren -= 1,
                b']' => depth_bracket += 1,
                b'[' => depth_bracket -= 1,
                _ if depth_paren == 0 && depth_bracket == 0 => {
                    for &op in ops {
                        let op_len = op.len();
                        if i + op_len <= bytes.len() && &s[i..i + op_len] == op {
                            if (op == "+" || op == "-") && i == 0 {
                                continue;
                            }
                            if (op == "<" || op == ">")
                                && i + 1 < bytes.len()
                                && bytes[i + 1] == b'='
                            {
                                continue;
                            }
                            if op == "="
                                && (i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'<' | b'>')
                                    || (i + 1 < bytes.len() && bytes[i + 1] == b'='))
                            {
                                continue;
                            }
                            let left = s[..i].trim();
                            let right = s[i + op_len..].trim();
                            if !left.is_empty() && !right.is_empty() {
                                return Some((left, op, right));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn eval_or(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, _, r)) = Self::split_top_level(s, &["||"]) {
            let lv = self.eval_or(l)?;
            if lv.is_truthy() {
                return Ok(EvalValue::Bool(true));
            }
            let rv = self.eval_and(r)?;
            return Ok(EvalValue::Bool(rv.is_truthy()));
        }
        self.eval_and(s)
    }

    fn eval_and(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, _, r)) = Self::split_top_level(s, &["&&"]) {
            let lv = self.eval_and(l)?;
            if !lv.is_truthy() {
                return Ok(EvalValue::Bool(false));
            }
            let rv = self.eval_equality(r)?;
            return Ok(EvalValue::Bool(rv.is_truthy()));
        }
        self.eval_equality(s)
    }

    fn eval_equality(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["===", "!==", "==", "!="]) {
            let lv = self.eval_equality(l)?;
            let rv = self.eval_relational(r)?;
            let eq = match (&lv, &rv) {
                (EvalValue::Number(a), EvalValue::Number(b)) => (a - b).abs() < 1e-9,
                _ => lv.format_display() == rv.format_display(),
            };
            let res = if op.starts_with('!') { !eq } else { eq };
            return Ok(EvalValue::Bool(res));
        }
        self.eval_relational(s)
    }

    fn eval_relational(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["<=", ">=", "<", ">"]) {
            let lv = self.eval_relational(l)?.as_f64()?;
            let rv = self.eval_additive(r)?.as_f64()?;
            let res = match op {
                "<=" => lv <= rv,
                ">=" => lv >= rv,
                "<" => lv < rv,
                ">" => lv > rv,
                _ => unreachable!(),
            };
            return Ok(EvalValue::Bool(res));
        }
        self.eval_additive(s)
    }

    fn eval_additive(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["+", "-"]) {
            let lv = self.eval_additive(l)?;
            let rv = self.eval_multiplicative(r)?;
            if op == "+" && (matches!(lv, EvalValue::Str(_)) || matches!(rv, EvalValue::Str(_))) {
                return Ok(EvalValue::Str(format!(
                    "{}{}",
                    lv.format_display(),
                    rv.format_display()
                )));
            }
            let a = lv.as_f64()?;
            let b = rv.as_f64()?;
            return Ok(EvalValue::Number(if op == "+" { a + b } else { a - b }));
        }
        self.eval_multiplicative(s)
    }

    fn eval_multiplicative(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["*", "/", "%"]) {
            let a = self.eval_multiplicative(l)?.as_f64()?;
            let b = self.eval_unary(r)?.as_f64()?;
            let val = match op {
                "*" => a * b,
                "/" => {
                    if b == 0.0 {
                        return Err("RangeError: division by zero".to_owned());
                    }
                    a / b
                }
                "%" => {
                    if b == 0.0 {
                        return Err("RangeError: modulo by zero".to_owned());
                    }
                    a % b
                }
                _ => unreachable!(),
            };
            return Ok(EvalValue::Number(val));
        }
        self.eval_unary(s)
    }

    fn eval_unary(&self, s: &str) -> Result<EvalValue, String> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix('!') {
            let v = self.eval_unary(rest)?;
            return Ok(EvalValue::Bool(!v.is_truthy()));
        }
        if let Some(rest) = s.strip_prefix('-') {
            let v = self.eval_unary(rest)?.as_f64()?;
            return Ok(EvalValue::Number(-v));
        }
        if let Some(rest) = s.strip_prefix('+') {
            let v = self.eval_unary(rest)?.as_f64()?;
            return Ok(EvalValue::Number(v));
        }
        self.eval_primary(s)
    }

    fn eval_primary(&self, s: &str) -> Result<EvalValue, String> {
        let s = s.trim();
        if s.starts_with('(') && s.ends_with(')') {
            return self.eval(&s[1..s.len() - 1]);
        }
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            return Ok(EvalValue::Str(s[1..s.len() - 1].to_owned()));
        }
        if s == "true" {
            return Ok(EvalValue::Bool(true));
        }
        if s == "false" {
            return Ok(EvalValue::Bool(false));
        }
        if s == "null" {
            return Ok(EvalValue::Null);
        }
        if s == "undefined" {
            return Ok(EvalValue::Undefined);
        }
        if let Ok(n) = s.parse::<f64>() {
            return Ok(EvalValue::Number(n));
        }
        if s.contains('(') || s.contains(')') {
            return Err(format!(
                "Unsupported expression '{s}': debugger evaluation is bounded to pure member paths, indexing, and arithmetic/comparison operators"
            ));
        }

        // Resolve member/index path (supporting expressions inside `[...]`)
        let mut resolved_segments = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '.' => {
                    if !current.trim().is_empty() {
                        resolved_segments.push(current.trim().to_owned());
                        current.clear();
                    }
                    i += 1;
                }
                '[' => {
                    if !current.trim().is_empty() {
                        resolved_segments.push(current.trim().to_owned());
                        current.clear();
                    }
                    let mut depth = 1;
                    i += 1;
                    let start = i;
                    while i < chars.len() && depth > 0 {
                        if chars[i] == '[' {
                            depth += 1;
                        } else if chars[i] == ']' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            i += 1;
                        }
                    }
                    let inner: String = chars[start..i].iter().collect();
                    let idx_val = self.eval(&inner)?;
                    resolved_segments.push(idx_val.format_display());
                    if i < chars.len() && chars[i] == ']' {
                        i += 1;
                    }
                }
                ch => {
                    current.push(ch);
                    i += 1;
                }
            }
        }
        if !current.trim().is_empty() {
            resolved_segments.push(current.trim().to_owned());
        }

        for scope in self.scopes {
            if let Some(var) = find_variable_in_slice(&scope.variables, &resolved_segments) {
                return Ok(EvalValue::from_variable(var));
            }
        }

        Err(format!(
            "ReferenceError: '{s}' is not defined in the paused stack frame"
        ))
    }
}

impl Editor {
    fn with_authoring_state<R>(&self, f: impl FnOnce(&DebuggerAuthoringState) -> R) -> R {
        let guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let empty = DebuggerAuthoringState::default();
        let state = guard.get(&self.clipboard.session).unwrap_or(&empty);
        f(state)
    }

    fn with_authoring_state_mut<R>(
        &mut self,
        f: impl FnOnce(&mut DebuggerAuthoringState) -> R,
    ) -> R {
        let mut guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(self.clipboard.session.clone()).or_default();
        f(state)
    }

    pub(crate) fn current_source_tab_key(&self, fallback_tab: &str) -> String {
        if self.script_target == ScriptTarget::Source
            && let Some(unit) = self
                .project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
        {
            return unit.path.clone();
        }
        let trimmed = fallback_tab.trim();
        if trimmed.is_empty() {
            format!("Frame {}", self.frame + 1)
        } else {
            trimmed.to_owned()
        }
    }

    fn normalize_breakpoint_key(path: &str) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            "Frame 1".to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// Toggle an authored breakpoint on `line` (1-based) in `path`. Returns `true` if added, `false` if removed.
    pub fn toggle_breakpoint(&mut self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let added = self.with_authoring_state_mut(|state| {
            let list = state.breakpoints.entry(key.clone()).or_default();
            if let Some(pos) = list.iter().position(|bp| bp.line == line) {
                list.remove(pos);
                false
            } else {
                list.push(BreakpointSpec::new(line));
                list.sort_by_key(|bp| bp.line);
                true
            }
        });
        if added {
            if !self
                .debugger
                .breakpoints
                .iter()
                .any(|bp| bp.source == key && bp.line == line)
            {
                self.debugger
                    .breakpoints
                    .push(crate::debugger_protocol::AuthoredBreakpoint {
                        source: key,
                        line,
                        column: None,
                    });
            }
        } else {
            self.debugger
                .breakpoints
                .retain(|bp| !(bp.source == key && bp.line == line));
        }
        added
    }

    /// Returns `true` if `path` has a breakpoint at `line` (1-based).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_breakpoint(&self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        self.with_authoring_state(|state| {
            state
                .breakpoints
                .get(&key)
                .is_some_and(|list| list.iter().any(|bp| bp.line == line))
        }) || self
            .debugger
            .breakpoints
            .iter()
            .any(|bp| bp.source == key && bp.line == line)
    }

    /// Configure a conditional or data/property-change breakpoint on `spec.line` in `path`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_breakpoint_spec(&mut self, path: &str, mut spec: BreakpointSpec) {
        let key = Self::normalize_breakpoint_key(path);
        spec.line = spec.line.max(1);
        let line = spec.line;
        let baseline = spec.watch_property.as_deref().and_then(|prop| {
            self.evaluate_debugger_expression(prop)
                .ok()
                .map(|v| (prop.to_owned(), v))
        });
        self.with_authoring_state_mut(|state| {
            if let Some((prop, val)) = baseline {
                state
                    .last_watched_values
                    .insert((key.clone(), line, prop), val);
            }
            let list = state.breakpoints.entry(key.clone()).or_default();
            if let Some(existing) = list.iter_mut().find(|bp| bp.line == line) {
                *existing = spec;
            } else {
                list.push(spec);
                list.sort_by_key(|bp| bp.line);
            }
        });
        if !self
            .debugger
            .breakpoints
            .iter()
            .any(|bp| bp.source == key && bp.line == line)
        {
            self.debugger
                .breakpoints
                .push(crate::debugger_protocol::AuthoredBreakpoint {
                    source: key,
                    line,
                    column: None,
                });
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn breakpoint_spec(&self, path: &str, line: usize) -> Option<BreakpointSpec> {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        self.with_authoring_state(|state| {
            state
                .breakpoints
                .get(&key)
                .and_then(|list| list.iter().find(|bp| bp.line == line).cloned())
        })
    }

    pub fn breakpoints_for(&self, path: &str) -> Vec<BreakpointSpec> {
        let key = Self::normalize_breakpoint_key(path);
        self.with_authoring_state(|state| state.breakpoints.get(&key).cloned().unwrap_or_default())
    }

    /// Evaluate whether execution should pause at `(path, line)`, checking any breakpoint condition
    /// and data/property-change watch, and incrementing `hit_count` when triggered.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn should_pause_at_breakpoint(&mut self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let Some(spec) = self.breakpoint_spec(&key, line) else {
            return false;
        };

        if let Some(cond) = spec.condition.as_deref() {
            let scopes = self.effective_debugger_scopes();
            let evaluator = ExprEvaluator { scopes: &scopes };
            match evaluator.eval(cond) {
                Ok(v) if v.is_truthy() => {}
                _ => return false,
            }
        }

        if let Some(prop) = spec.watch_property.as_deref() {
            let Ok(current_val) = self.evaluate_debugger_expression(prop) else {
                return false;
            };
            let changed = self.with_authoring_state_mut(|state| {
                let watch_key = (key.clone(), line, prop.to_owned());
                match state
                    .last_watched_values
                    .insert(watch_key, current_val.clone())
                {
                    Some(prev) => prev != current_val,
                    None => false,
                }
            });
            if !changed {
                return false;
            }
        }

        self.with_authoring_state_mut(|state| {
            if let Some(list) = state.breakpoints.get_mut(&key)
                && let Some(bp) = list.iter_mut().find(|bp| bp.line == line)
            {
                bp.hit_count = bp.hit_count.saturating_add(1);
            }
        });
        true
    }

    fn effective_debugger_scopes(&self) -> Vec<DebuggerScope> {
        let authored = self.with_authoring_state(|state| state.paused_scopes.clone());
        if !authored.is_empty() {
            return authored;
        }
        if let Some(snapshot) = self.debugger.snapshot() {
            vec![DebuggerScope {
                name: "Local".to_owned(),
                variables: snapshot
                    .locals
                    .iter()
                    .map(|v| DebuggerVariable::scalar(&v.name, &v.value, &v.kind))
                    .collect(),
            }]
        } else {
            Vec::new()
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_paused_debugger_scopes(&mut self, scopes: Vec<DebuggerScope>) {
        self.with_authoring_state_mut(|state| {
            state.paused_scopes = scopes;
        });
        self.refresh_debugger_watches();
    }

    pub fn paused_debugger_scopes(&self) -> Vec<DebuggerScope> {
        self.effective_debugger_scopes()
    }

    /// Inspect a potentially nested `DebuggerVariable` by path (e.g. `"player.stats.hp"` or `"items[0]"`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn inspect_debugger_variable_path(
        &self,
        scope_idx: usize,
        var_path: &str,
    ) -> Option<DebuggerVariable> {
        let segments = split_var_path(var_path);
        if segments.is_empty() {
            return None;
        }
        let scopes = self.effective_debugger_scopes();
        let scope = scopes.get(scope_idx)?;
        find_variable_in_slice(&scope.variables, &segments).cloned()
    }

    /// Edit a variable or nested object property in the paused debugger scope and refresh watches.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_debugger_variable_value(
        &mut self,
        scope_idx: usize,
        var_path: &str,
        new_value: &str,
    ) -> Result<String, String> {
        let segments = split_var_path(var_path);
        if segments.is_empty() {
            return Err("Variable path must not be empty".to_owned());
        }
        let result = self.with_authoring_state_mut(|state| {
            let scope = state
                .paused_scopes
                .get_mut(scope_idx)
                .ok_or_else(|| format!("Scope index {scope_idx} is out of bounds"))?;
            let var =
                find_variable_in_slice_mut(&mut scope.variables, &segments).ok_or_else(|| {
                    format!("Variable '{var_path}' not found in scope '{}'", scope.name)
                })?;
            let trimmed = new_value.trim();
            var.value = trimmed.to_owned();
            if var.children.is_empty() {
                if trimmed == "true" || trimmed == "false" {
                    var.kind = "Boolean".to_owned();
                } else if trimmed.parse::<f64>().is_ok() {
                    var.kind = "Number".to_owned();
                } else if trimmed == "null" {
                    var.kind = "Null".to_owned();
                } else if trimmed == "undefined" {
                    var.kind = "Undefined".to_owned();
                } else {
                    var.kind = "String".to_owned();
                }
            }
            Ok(var.value.clone())
        });
        if result.is_ok() {
            self.refresh_debugger_watches();
        }
        result
    }

    /// Evaluate an expression (identifiers, dotted member paths, array indexing, arithmetic, comparisons,
    /// and boolean conditions) against the current paused stack frame's `DebuggerVariable` tree.
    pub fn evaluate_debugger_expression(&self, expr: &str) -> Result<String, String> {
        let scopes = self.effective_debugger_scopes();
        let evaluator = ExprEvaluator { scopes: &scopes };
        evaluator.eval(expr).map(|v| v.format_display())
    }

    pub fn debugger_watches(&self) -> Vec<WatchExpression> {
        self.with_authoring_state(|state| state.debugger_watches.clone())
    }

    pub fn add_debugger_watch(&mut self, expr: impl Into<String>) -> usize {
        let expression = expr.into().trim().to_owned();
        let eval_res = self.evaluate_debugger_expression(&expression);
        let (last_value, is_error) = match eval_res {
            Ok(val) => (val, false),
            Err(err) => (err, true),
        };
        self.with_authoring_state_mut(|state| {
            let idx = state.debugger_watches.len();
            state.debugger_watches.push(WatchExpression {
                expression,
                last_value,
                is_error,
            });
            idx
        })
    }

    pub fn remove_debugger_watch(&mut self, index: usize) -> bool {
        self.with_authoring_state_mut(|state| {
            if index < state.debugger_watches.len() {
                state.debugger_watches.remove(index);
                true
            } else {
                false
            }
        })
    }

    pub fn refresh_debugger_watches(&mut self) {
        let expressions: Vec<String> = self.with_authoring_state(|state| {
            state
                .debugger_watches
                .iter()
                .map(|w| w.expression.clone())
                .collect()
        });
        let evaluated: Vec<(String, bool)> = expressions
            .iter()
            .map(|expr| match self.evaluate_debugger_expression(expr) {
                Ok(v) => (v, false),
                Err(e) => (e, true),
            })
            .collect();
        self.with_authoring_state_mut(|state| {
            for (watch, (val, is_err)) in state.debugger_watches.iter_mut().zip(evaluated) {
                watch.last_value = val;
                watch.is_error = is_err;
            }
        });
    }

    fn active_fold_tab_key(&self) -> String {
        if self.script_target == ScriptTarget::Source
            && let Some(unit) = self
                .project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
        {
            return unit.path.clone();
        }
        let target_label = match self.script_target {
            ScriptTarget::Frame => "frame",
            ScriptTarget::Object => "object",
            ScriptTarget::Source => "source",
        };
        format!("{}:{}:{target_label}", self.symbol_id(), self.frame)
    }

    pub fn folded_lines(&self) -> std::collections::BTreeSet<usize> {
        let key = self.active_fold_tab_key();
        self.with_authoring_state(|state| {
            state
                .folded_lines_by_tab
                .get(&key)
                .cloned()
                .unwrap_or_default()
        })
    }

    pub(crate) fn toggle_fold_line_for(&mut self, tab_key: &str, line: usize) -> bool {
        let line = line.max(1);
        self.with_authoring_state_mut(|state| {
            let set = state
                .folded_lines_by_tab
                .entry(tab_key.to_owned())
                .or_default();
            if set.contains(&line) {
                set.remove(&line);
                false
            } else {
                set.insert(line);
                true
            }
        })
    }

    /// Toggle code folding at 1-based `line` for the active script tab. Returns `true` if now folded.
    pub fn toggle_fold_line(&mut self, line: usize) -> bool {
        let key = self.active_fold_tab_key();
        self.toggle_fold_line_for(&key, line)
    }

    pub fn expand_all_folds(&mut self) {
        let key = self.active_fold_tab_key();
        self.with_authoring_state_mut(|state| {
            state.folded_lines_by_tab.remove(&key);
        });
    }

    pub(crate) fn collapse_between_braces_in_current_editor(
        &mut self,
        ctx: &egui::Context,
        fallback_source: &str,
    ) -> bool {
        let source = if self.script_target == ScriptTarget::Source {
            self.project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
                .map_or(fallback_source, |s| s.source.as_str())
                .to_owned()
        } else {
            fallback_source.to_owned()
        };
        let regions = compute_fold_regions(&source);
        if regions.is_empty() {
            return false;
        }
        let editor_id = self.actions_editor_id();
        let caret_line = egui::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range().map(|r| r.primary.index))
            .map_or(1, |idx| nf::code::line_col(&source, idx).0);
        let target_line = regions
            .iter()
            .filter(|r| r.start_line <= caret_line && caret_line <= r.end_line)
            .max_by_key(|r| r.start_line)
            .or_else(|| regions.first())
            .map(|r| r.start_line);
        if let Some(line) = target_line {
            self.toggle_fold_line(line)
        } else {
            false
        }
    }

    /// Return the visible 1-based `(line_number, line_text)` pairs after applying active fold regions.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn visible_source_lines(&self, source: &str) -> Vec<(usize, String)> {
        let regions = compute_fold_regions(source);
        let folded = self.folded_lines();
        let active_regions: Vec<&FoldRegion> = regions
            .iter()
            .filter(|r| folded.contains(&r.start_line))
            .collect();

        let mut visible = Vec::new();
        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = idx + 1;
            if active_regions
                .iter()
                .any(|r| r.start_line < line_no && line_no <= r.end_line)
            {
                continue;
            }
            if let Some(region) = active_regions.iter().find(|r| r.start_line == line_no) {
                visible.push((line_no, region.label.clone()));
            } else {
                visible.push((line_no, raw_line.to_owned()));
            }
        }
        visible
    }

    /// Insert a bounded Script Assist statement into the active frame, object, or project source script.
    pub fn insert_script_assist_snippet(&mut self, template: ScriptAssistTemplate) -> bool {
        match self.script_target {
            ScriptTarget::Source => {
                let Some(selected_id) = self.selected_source.clone() else {
                    return false;
                };
                let Some(unit) = self.project.sources.iter().find(|s| s.id == selected_id) else {
                    return false;
                };
                let snippet = format_script_assist_snippet(&template, unit.language);
                self.transact_named("Script assist", |editor| {
                    if let Some(u) = editor
                        .project
                        .sources
                        .iter_mut()
                        .find(|s| s.id == selected_id)
                    {
                        append_script_assist_to_source(&mut u.source, &snippet);
                    }
                });
                true
            }
            ScriptTarget::Frame => {
                let frame = self.frame;
                let language = self
                    .symbol()
                    .timeline
                    .scripts
                    .iter()
                    .find(|s| s.frame == frame)
                    .map_or(ScriptLanguage::JavaScript, |s| s.script.language);
                let snippet = format_script_assist_snippet(&template, language);
                self.transact_named("Script assist", |editor| {
                    let scripts = &mut editor.symbol_mut().timeline.scripts;
                    if let Some(existing) = scripts.iter_mut().find(|s| s.frame == frame) {
                        append_script_assist_to_source(&mut existing.script.source, &snippet);
                    } else {
                        scripts.push(neoflash_core::FrameScript {
                            frame,
                            script: Script {
                                language,
                                source: snippet,
                            },
                        });
                    }
                });
                true
            }
            ScriptTarget::Object => {
                if !self.selected_editable() {
                    return false;
                }
                let event = self.script_event;
                let language = self
                    .selected_element()
                    .and_then(|e| e.scripts.iter().find(|s| s.event == event))
                    .map_or(ScriptLanguage::JavaScript, |s| s.script.language);
                let snippet = format_script_assist_snippet(&template, language);
                self.transact_named("Script assist", |editor| {
                    editor.ensure_selected_key();
                    if let Some(el) = editor.selected_element_mut() {
                        if let Some(existing) = el.scripts.iter_mut().find(|s| s.event == event) {
                            append_script_assist_to_source(&mut existing.script.source, &snippet);
                        } else {
                            el.scripts.push(neoflash_core::ObjectScript {
                                event,
                                script: Script {
                                    language,
                                    source: snippet,
                                },
                            });
                        }
                    }
                });
                true
            }
        }
    }
}
