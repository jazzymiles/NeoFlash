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
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::tokens::{radius, space};
use std::hash::{Hash, Hasher};

pub(crate) struct CodeFeedback {
    target: egui::Id,
    fingerprint: u64,
    text: String,
    success: bool,
    /// (line, column, message) of each diagnostic, 1-based as reported by the compiler.
    diagnostics: Vec<(usize, usize, String)>,
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
        self.project.document_class.hash(&mut hash);
        for unit in &self.project.sources {
            unit.id.hash(&mut hash);
            unit.path.hash(&mut hash);
            unit.source.hash(&mut hash);
            std::mem::discriminant(&unit.language).hash(&mut hash);
        }
        for symbol in &self.project.symbols {
            symbol.id.hash(&mut hash);
            symbol.class_name.hash(&mut hash);
            symbol.linkage_id.hash(&mut hash);
        }
        hash.finish()
    }

    /// `announce` also writes the result to Output (explicit Check); live checks stay silent.
    fn record_code_check(
        &mut self,
        script: &Script,
        result: Result<(), Vec<neoflash_script::Diagnostic>>,
        announce: bool,
    ) {
        let success = result.is_ok();
        let diagnostics: Vec<_> = match &result {
            Ok(()) => Vec::new(),
            Err(errors) => errors
                .iter()
                .map(|error| (error.line, error.column, error.message.clone()))
                .collect(),
        };
        let text = match result {
            Ok(()) => "Code accepted for current language support.".to_owned(),
            Err(errors) => errors
                .into_iter()
                .map(|error| {
                    format!(
                        "Line {}, column {}: {}",
                        error.line, error.column, error.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
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
        let result = neoflash_script::compile_project(&self.project)
            .and_then(|program| neoflash_script::compile_with_program(script, &program))
            .map(|_| ());
        self.record_code_check(script, result, announce);
    }

    /// "Check project": compile every project source; feedback attaches to the selected file.
    pub(crate) fn check_project_sources(&mut self, announce: bool) {
        let result = neoflash_script::compile_project(&self.project).map(|_| ());
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
                result,
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
        let show_hints = self.apply_code_command(ui.ctx(), editor_id, source);
        let feedback = self
            .current_feedback(&Script {
                language,
                source: source.clone(),
            })
            .map(|f| (f.success, f.text.clone(), f.diagnostics.clone()));
        let markers: Vec<(usize, Severity)> = feedback
            .iter()
            .flat_map(|(_, _, diagnostics)| diagnostics.iter().map(|d| (d.0, Severity::Error)))
            .collect();
        let message_count = feedback.as_ref().map_or(0, |(_, _, d)| d.len().max(1));
        let bar_h = neoflash_ui::metrics::SCRIPT_TAB + 2.;
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
        let (response, caret) = ui
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
                )
            })
            .inner;
        self.live_check(ui.ctx(), editor_id, &response, source, language);
        if let Some((success, text, diagnostics)) = &feedback {
            ui.scope_builder(egui::UiBuilder::new().max_rect(diag), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("code_feedback", editor_id))
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = space::S1;
                        if *success || diagnostics.is_empty() {
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
                        for (line, column, message) in diagnostics {
                            let location = format!("Line {line}, column {column}");
                            let row = ui.add(nf::CompilerMessage {
                                severity: Severity::Error,
                                message,
                                location: &location,
                            });
                            if row.clicked() {
                                jump_to(ui.ctx(), editor_id, source, *line, *column);
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
                let problems = feedback.as_ref().map(|(_, _, d)| (d.len(), 0));
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
        let context =
            HintContext::new(&self.project, language, this_kind)?.with_instances(instances);
        if self.script_target != ScriptTarget::Source {
            return Some(context);
        }
        let class = self
            .project
            .sources
            .iter()
            .find(|unit| Some(&unit.id) == self.selected_source.as_ref())
            .and_then(crate::script_api::scan_class)
            .map(|class| class.qualified);
        Some(match class {
            Some(class) => context.with_this_class(class),
            None => context,
        })
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
            let select_w = (ui.available_width() - 160. - 3. * neoflash_ui::metrics::CONTROL - 16.)
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
                    .max_height(8. * neoflash_ui::metrics::ROW)
                    .themed()
                    .show(ui, |ui| {
                        for (index, item) in items.iter().enumerate() {
                            if index == state.selected {
                                let row = egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    egui::vec2(ui.available_width(), neoflash_ui::metrics::ROW),
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
) -> (egui::Response, (usize, usize)) {
    let ctx = ui.ctx().clone();
    let state_id = id.with("code-hint-state");
    let mut hint_state: HintState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
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
                    output.response
                })
                .inner
        })
        .inner;
    ctx.data_mut(|d| d.insert_temp(state_id, hint_state));
    (response, caret)
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
