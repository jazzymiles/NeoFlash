//! Project code editing shares the existing Actions workspace and Undo history.
//!
//! The NF2 script workspace (`NF2/Code Editor` + compiler messages + script bar) is shared by
//! frame/object actions and project source files; see docs/DESIGN-SYSTEM.md › Panel files.
use crate::app::{Editor, ScriptTarget};
use crate::code_tools::CodeCommand;
use crate::script_api::{HintContext, ThisKind};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui;
use neoflash_core::{Script, ScriptLanguage, SourceUnit, SymbolKind, new_id};
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::tokens::{TextStyle, radius, space};
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
        if let Some((text, (start, stop))) = crate::code_tools::apply(&command, source, selection) {
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
        let source = if language == ScriptLanguage::ActionScript2 {
            format!("class {name} {{\n    public function {name}() {{\n    }}\n}}\n")
        } else {
            format!(
                "package {{\n    public class {name} {{\n        public function {name}() {{\n        }}\n    }}\n}}\n"
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

/// Syntax-coloured code layout (NF2 Night tokens, `NF2/Mono`) without replacing TextEdit IDs or input state.
pub(crate) fn code_layout(
    ui: &egui::Ui,
    text: &dyn egui::TextBuffer,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = nf::code::highlight(ui.ctx(), text.as_str());
    job.wrap.max_width = wrap_width;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// `NF2/Code Editor`: `bg/field` frame, gutter with line numbers and markers, current-line band,
/// syntax-coloured `TextEdit` with the given stable `id`. Returns the response and the 1-based caret.
/// Code-hint popover state of one code editor (egui temp memory, keyed by the editor id).
#[derive(Clone, Default)]
struct HintState {
    open: bool,
    /// Opened by ⌃Space / toolbar: also lists scope names before anything is typed.
    explicit: bool,
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

fn code_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    source: &mut String,
    hint: &str,
    markers: &[(usize, Severity)],
    hints: &dyn Fn() -> Option<HintContext<'static>>,
    show_hints: bool,
) -> (egui::Response, (usize, usize)) {
    let ctx = ui.ctx().clone();
    let state_id = id.with("code-hint-state");
    let mut hint_state: HintState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let focused = ctx.memory(|memory| memory.has_focus(id));
    let explicit_key =
        focused && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));
    if show_hints || explicit_key {
        hint_state = HintState {
            open: true,
            explicit: true,
            ..Default::default()
        };
    }
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
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                hint_state.open = false;
            }
        });
        if let Some(index) = accept {
            if let Some(context) = hints() {
                accept_hint(&ctx, id, source, &context, hint_state.explicit, index);
            }
            hint_state = HintState::default();
        }
    }
    let p = neoflash_ui::palette(&ctx);
    let lines = source.chars().filter(|c| *c == '\n').count() + 1;
    let gutter = nf::code::gutter_width(ui, lines);
    let mut caret = (1, 1);
    let mut layouter = code_layout;
    let response = egui::Frame::new()
        .fill(p.bg_field)
        .stroke(egui::Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .id_salt(("actions_source_scroll", id))
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    let band = ui.painter().add(egui::Shape::Noop);
                    let output = egui::TextEdit::multiline(source)
                        .id(id)
                        .code_editor()
                        .font(neoflash_ui::typography::font(TextStyle::Mono))
                        .layouter(&mut layouter)
                        .frame(false)
                        .margin(egui::Margin {
                            left: (gutter + space::S3) as i8,
                            right: space::S2 as i8,
                            top: space::S1 as i8,
                            bottom: space::S1 as i8,
                        })
                        .desired_width(f32::INFINITY)
                        .min_size(egui::vec2(
                            ui.available_width(),
                            ui.available_height().max(60.),
                        ))
                        .hint_text(
                            neoflash_ui::typography::rich(hint, TextStyle::Mono)
                                .color(p.text_tertiary),
                        )
                        .show(ui);
                    let rect = output.response.rect;
                    let cursor = output.state.cursor.char_range().map(|range| range.primary);
                    if let Some(cursor) = cursor {
                        caret = nf::code::line_col(source, cursor.index);
                        ui.painter().set(
                            band,
                            nf::code::current_line(
                                &ctx,
                                &output.galley,
                                output.galley_pos,
                                cursor,
                                egui::Rangef::new(rect.left() + gutter, rect.right()),
                            ),
                        );
                    }
                    nf::Gutter {
                        markers,
                        current_line: cursor.map(|_| caret.0),
                    }
                    .paint(
                        ui,
                        egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.left() + gutter, rect.bottom()),
                        ),
                        &output.galley,
                        output.galley_pos,
                    );
                    let typed_dot = output.response.changed()
                        && ui.input(|i| {
                            i.events
                                .iter()
                                .any(|e| matches!(e, egui::Event::Text(t) if t.ends_with('.')))
                        });
                    if typed_dot {
                        hint_state = HintState {
                            open: true,
                            ..Default::default()
                        };
                    }
                    if hint_state.open && !output.response.has_focus() && !hint_state.hovered {
                        hint_state = HintState::default();
                    }
                    let found = match (hint_state.open, cursor) {
                        (true, Some(cursor)) => hints().and_then(|context| {
                            let found = context.hints(source, cursor.index, hint_state.explicit)?;
                            Some((context, found, cursor))
                        }),
                        _ => None,
                    };
                    match found {
                        Some((_, found, _))
                            if hint_state.anchor.is_some_and(|a| a != found.start) =>
                        {
                            hint_state = HintState::default();
                        }
                        Some((context, found, cursor)) => {
                            hint_state.anchor = Some(found.start);
                            hint_state.count = found.items.len();
                            hint_state.selected = hint_state.selected.min(found.items.len() - 1);
                            let at = output.galley.pos_from_cursor(cursor);
                            let anchor =
                                output.galley_pos + at.left_bottom().to_vec2() + egui::vec2(0., 2.);
                            if let Some(index) =
                                hint_popover(ui, id, anchor, &found.items, &mut hint_state)
                            {
                                accept_hint(&ctx, id, source, &context, hint_state.explicit, index);
                                hint_state = HintState::default();
                            }
                        }
                        None => hint_state = HintState::default(),
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
fn jump_to(ctx: &egui::Context, id: egui::Id, source: &str, line: usize, column: usize) {
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
