//! Library wrapper, bottom-panel dispatch, Output and the Actions (script) editor.
//!
//! NF2 references: `panels/*/png/panel-actions-floating.png`, `interface/*/png/editor-actions-tab.png`
//! (Actions) and `panels/*/png/panels-more-2.png` (Output).
use super::actions_sidebar::{SIDEBAR_MIN_PANEL, SIDEBAR_WIDTH, ScriptLocation};
use super::help_as_reference::{
    JS_TS_API_ENTRIES, JS_TS_CATEGORIES, JsTsApiEntry, JsTsEntryKind, search_js_ts_entries,
};
use crate::app::*;
use crate::code_tools::CodeCommand;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::{self as nf, TreeItemType, code::Severity};
use neoflash_ui::icons::{Icon, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use std::collections::HashSet;

/// Toolbar commands of the NF2 Actions panel in Figma order. Commands without an action are not
/// provided yet and are shown disabled with "Not available yet" (DESIGN-SYSTEM rule 9).
const CODE_TOOLS: [&[(neoflash_ui::Icon, &str, Option<CodeCommand>)]; 3] = [
    &[
        (
            i16::AUTOFORMAT,
            "Auto-format",
            Some(CodeCommand::AutoFormat),
        ),
        (
            i16::CODEHINT,
            "Show code hint",
            Some(CodeCommand::ShowHints),
        ),
        (i16::DEBUG, "Debug options", None),
    ],
    &[
        (i16::COLLAPSE, "Collapse between braces", None),
        (i16::EXPAND, "Expand all", None),
    ],
    &[
        (
            i16::COMMENT_BLOCK,
            "Apply block comment",
            Some(CodeCommand::BlockComment),
        ),
        (
            i16::COMMENT_LINE,
            "Apply line comment",
            Some(CodeCommand::LineComment),
        ),
        (
            i16::UNCOMMENT,
            "Remove comment",
            Some(CodeCommand::Uncomment),
        ),
    ],
];

impl Editor {
    pub(crate) fn library_contents(&mut self, ui: &mut egui::Ui) {
        self.library_body(ui);
    }
    pub(crate) fn bottom_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let ctx = ui.ctx().clone();
        match panel {
            PanelId::Properties => {
                egui::ScrollArea::vertical()
                    .id_salt("property_contents")
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        // Padding lives inside the scroll content so the floating scrollbar sits in
                        // the 16 pt gutter instead of clipping the fields.
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(neoflash_ui::tokens::space::S4 as i8))
                            .show(ui, |ui| self.properties(ui, &ctx));
                    });
            }
            PanelId::Actions => self.actions_contents(ui),
            PanelId::Accessibility => {
                egui::ScrollArea::vertical()
                    .id_salt("accessibility_contents")
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(neoflash_ui::tokens::space::S3 as i8))
                            .show(ui, |ui| self.accessibility_panel(ui));
                    });
            }
            PanelId::Output => self.output_panel(ui),
            _ => {}
        }
    }

    /// Output: filter, copy and clear, then the log in `NF2/Mono`. Editor messages carry no level or
    /// time stamp, so the NF2 "Show: All / Errors / Warnings / Trace" select is not offered.
    pub(crate) fn output_panel(&mut self, ui: &mut egui::Ui) {
        let filter_id = egui::Id::new(("neoflash-output-filter", &self.clipboard.session));
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        let all_lines: Vec<String> = self
            .output
            .iter()
            .flat_map(|entry| {
                if entry.is_empty() {
                    vec![String::new()]
                } else {
                    entry.lines().map(str::to_owned).collect::<Vec<_>>()
                }
            })
            .collect();
        let mut visible_lines: Vec<String> = Vec::new();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let search = (ui.available_width()
                - 2. * (neoflash_ui::metrics::control() + space::S2))
                .max(80.);
            let search_resp = ui.add(nf::Search::new(&mut filter, "Filter output").width(search));
            if search_resp.changed() {
                ui.ctx().request_repaint();
            }
            let needle = filter.trim().to_lowercase();
            visible_lines = all_lines
                .iter()
                .filter(|line| needle.is_empty() || line.to_lowercase().contains(&needle))
                .cloned()
                .collect();
            let copy_sc = self.tooltip_sc(crate::commands::CommandId::EditCopy);
            let mut copy_btn = nf::IconButton::new(i16::COPY, "Copy output");
            if let Some(sc) = copy_sc.as_deref() {
                copy_btn = copy_btn.shortcut(sc);
            }
            let copy = ui.add_enabled_semantic(!visible_lines.is_empty(), copy_btn);
            if copy.clicked() {
                ui.ctx().copy_text(visible_lines.join("\n"));
            }
            let clear = ui.add_enabled_semantic(
                !self.output.is_empty(),
                nf::IconButton::new(i16::TRASH, "Clear output"),
            );
            if clear.clicked() {
                self.output.clear();
                visible_lines.clear();
            }
        });
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        ui.add_space(space::S2);
        if self.output.is_empty() {
            nf::EmptyState {
                icon: i16::TERMINAL,
                title: "No output yet",
                body: "Status messages, script checks and export results appear here.",
                action: None,
            }
            .show(ui);
            return;
        }
        let needle_active = !filter.trim().is_empty();
        let p = neoflash_ui::palette(ui.ctx());
        egui::Frame::new()
            .fill(p.bg_field)
            .corner_radius(radius::MD)
            .inner_margin(egui::Margin::same(space::S2 as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt(("output_contents", &self.clipboard.session))
                    .stick_to_bottom(!needle_active)
                    .themed()
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        for line in &visible_lines {
                            ui.add(nf::LogLine {
                                severity: Severity::Info,
                                time: "",
                                text: line,
                            });
                        }
                        if visible_lines.is_empty() {
                            ui.label(
                                neoflash_ui::typography::rich(
                                    format!("No output matches “{}”.", filter.trim()),
                                    TextStyle::Small,
                                )
                                .color(p.text_tertiary),
                            );
                        }
                    });
            });
    }

    pub(crate) fn actions_editor_id(&self) -> egui::Id {
        if self.script_target == ScriptTarget::Source {
            return egui::Id::new((
                "neoflash-project-source-editor",
                &self.clipboard.session,
                &self.selected_source,
            ));
        }
        let target = match self.script_target {
            ScriptTarget::Frame => "frame".to_owned(),
            ScriptTarget::Source => {
                format!("source:{}", self.selected_source.as_deref().unwrap_or(""))
            }
            ScriptTarget::Object => format!(
                "object:{}:{:?}",
                self.selected_element().map_or("", |e| e.id.as_str()),
                self.script_event
            ),
        };
        egui::Id::new((
            "neoflash-actions-editor",
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
            target,
        ))
    }
    pub(crate) fn actions_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Actions,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| self.actions_body(ui));
    }

    /// NF2 Actions toolbar: target and language selects, object event, Check, code tools (format,
    /// hints, comments), sidebar toggle, Script assist (disabled) and help.
    fn actions_toolbar(&mut self, ui: &mut egui::Ui, script: &mut Script, context: &str) {
        let source = self.script_target == ScriptTarget::Source;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2., space::S1);
            let target_width = (ui.available_width() * 0.3).clamp(140., 280.);
            nf::Select::new("actions_target", context)
                .icon(i16::TARGET)
                .width(target_width)
                .show(ui, |ui| {
                    let frame = format!("Frame {}", self.frame + 1);
                    let items = [
                        (ScriptTarget::Frame, frame.as_str(), true),
                        (ScriptTarget::Object, "Selected object", self.selected_editable()),
                        (ScriptTarget::Source, "Project files", true),
                    ];
                    for (target, label, enabled) in items {
                        let item = nf::MenuItem::new(label).checked(self.script_target == target);
                        if ui.add_enabled_semantic(enabled, item).clicked() {
                            self.script_target = target;
                        }
                    }
                })
                .0
                .on_hover_text(context);
            if !source {
                ui.add_space(space::S1);
                crate::sources::language_select(
                    ui,
                    "language",
                    &mut script.language,
                    &crate::sources::ACTION_LANGUAGES,
                    160.,
                );
            }
            if self.script_target == ScriptTarget::Object {
                ui.add_space(space::S1);
                let label = format!("Event: {:?}", self.script_event);
                nf::Select::new("actions_object_event", &label)
                    .width(150.)
                    .show(ui, |ui| {
                        for event in [ObjectEvent::Click, ObjectEvent::Load, ObjectEvent::EnterFrame] {
                            let name = format!("{event:?}");
                            let item = nf::MenuItem::new(&name).checked(self.script_event == event);
                            if ui.add(item).clicked() {
                                self.script_event = event;
                            }
                        }
                    });
            }
            ui.add_space(space::S1);
            nf::divider(ui, true);
            let check_label = if source { "Check project" } else { "Check script" };
            let check = ui.add_enabled_semantic(
                !source || self.selected_source.is_some(),
                nf::IconButton::new(i16::CHECK, check_label),
            );
            if check.clicked() {
                if source {
                    self.check_project_sources(true);
                } else {
                    self.check_action_source(script, true);
                }
            }
            let editable = if source {
                self.selected_source.is_some()
            } else {
                self.script_target == ScriptTarget::Frame || self.selected_editable()
            };
            let source_language = self
                .project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
                .map(|s| s.language);
            let language = if source { source_language } else { Some(script.language) };
            let has_api = language.and_then(crate::script_api::api).is_some();
            for group in CODE_TOOLS {
                for (icon, label, command) in group {
                    let Some(command) = command else {
                        if *label == "Debug options" {
                            if ui.add(nf::IconButton::new(*icon, label)).clicked() {
                                self.open_debug_options(ui.ctx());
                            }
                            continue;
                        }
                        if *label == "Collapse between braces" {
                            let button = ui.add_enabled_semantic(
                                editable,
                                nf::IconButton::new(*icon, label),
                            );
                            if button.clicked() {
                                self.collapse_between_braces_in_current_editor(
                                    ui.ctx(),
                                    &script.source,
                                );
                            }
                            continue;
                        }
                        if *label == "Expand all" {
                            let has_folds = !self.folded_lines().is_empty();
                            let button = ui.add_enabled_semantic(
                                editable && has_folds,
                                nf::IconButton::new(*icon, label),
                            );
                            if button.clicked() {
                                self.expand_all_folds();
                            }
                            continue;
                        }
                        let button = ui.add_enabled(false, nf::IconButton::new(*icon, label));
                        nf::not_available(button, label);
                        continue;
                    };
                    let enabled = editable && (*command != CodeCommand::ShowHints || has_api);
                    let button = ui.add_enabled_semantic(enabled, nf::IconButton::new(*icon, label));
                    let button = if *command == CodeCommand::ShowHints {
                        button.on_hover_text("Show code hint (⌃Space)")
                    } else {
                        button
                    };
                    if button.clicked() {
                        self.actions_ui.pending = Some(command.clone());
                    }
                }
                nf::divider(ui, true);
            }
            let sidebar = ui.add(
                nf::IconButton::new(i16::SIDEBAR, "Show toolbox sidebar").selected(self.actions_ui.sidebar),
            );
            if sidebar.clicked() {
                self.actions_ui.sidebar = !self.actions_ui.sidebar;
            }
            let right = 140. + neoflash_ui::metrics::control() + 2. * 2.;
            // In a wrapped row `available_width` is the full row width, not what is left of it.
            let rest = ui.max_rect().right() - ui.cursor().left() - right - space::S4;
            if rest > 0. {
                ui.add_space(rest);
            }
            let assist = ui.add_enabled_semantic(
                editable,
                nf::Button::ghost("Script assist").icon(i16::WAND),
            );
            if editable {
                nf::popup_below(&assist, 240., |ui| {
                    for (item_label, template) in
                        crate::sources::ScriptAssistTemplate::default_menu_items()
                    {
                        if ui.add(nf::MenuItem::new(item_label)).clicked() {
                            if source {
                                self.insert_script_assist_snippet(template);
                            } else {
                                let snippet = crate::sources::format_script_assist_snippet(
                                    &template,
                                    script.language,
                                );
                                crate::sources::append_script_assist_to_source(
                                    &mut script.source,
                                    &snippet,
                                );
                            }
                        }
                    }
                });
            }
            let help = ui.add(nf::IconButton::new(i16::HELP, "About Actions"));
            nf::popup_below(&help, 280., |ui| {
                let p = neoflash_ui::palette(ui.ctx());
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S2 as i8))
                    .show(ui, |ui| {
                        ui.set_max_width(264.);
                        ui.label(
                            neoflash_ui::typography::rich("Actions", TextStyle::BodyStrong)
                                .color(p.text_primary),
                        );
                        ui.label(
                            neoflash_ui::typography::rich(
                                "Frame and object scripts are stored in the document. Test Movie runs scripts in the browser; Check validates the code against current language support.",
                                TextStyle::Small,
                            )
                            .color(p.text_secondary),
                        );
                    });
            });
        });
    }

    fn actions_body(&mut self, ui: &mut egui::Ui) {
        let has_frame_script =
            self.symbol().timeline.scripts.iter().any(|script| {
                script.frame == self.frame && !script.script.source.trim().is_empty()
            });
        let object_id = self.selected_element().map(|element| element.id.clone());
        let target_before = self.script_target;
        let event_before = self.script_event;
        let location_before = self.current_script_location();
        let empty_lang_id = egui::Id::new("neoflash-actions-empty-language");
        let stored = match self.script_target {
            ScriptTarget::Source => None,
            ScriptTarget::Frame => self
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|s| s.frame == self.frame)
                .map(|s| s.script.clone()),
            ScriptTarget::Object => self
                .selected_element()
                .and_then(|e| e.scripts.iter().find(|s| s.event == self.script_event))
                .map(|s| s.script.clone()),
        };
        let pending_lang = ui
            .data(|d| d.get_temp::<ScriptLanguage>(empty_lang_id))
            .or_else(|| stored.as_ref().map(|s| s.language))
            .unwrap_or(ScriptLanguage::JavaScript);
        let has_stored_script = stored.as_ref().is_some_and(|s| !s.source.is_empty());
        let original = match &stored {
            Some(s) if !s.source.is_empty() => s.clone(),
            _ => Script {
                language: pending_lang,
                source: String::new(),
            },
        };
        let mut script = original.clone();
        let context = match self.script_target {
            ScriptTarget::Frame => format!(
                "{} · Frame {}{}",
                self.symbol().name,
                self.frame + 1,
                if has_frame_script { " · script" } else { "" }
            ),
            ScriptTarget::Object => format!(
                "{} · {} · {:?}",
                self.symbol().name,
                self.selected_element()
                    .map_or("Object", |e| e.name.as_str()),
                self.script_event
            ),
            ScriptTarget::Source => "Project files".to_owned(),
        };
        let tab = match self.script_target {
            ScriptTarget::Object => format!(
                "{} : {:?}",
                self.selected_element()
                    .map_or("Object", |e| if e.name.is_empty() {
                        "Object"
                    } else {
                        e.name.as_str()
                    }),
                self.script_event
            ),
            _ => format!("{} : Frame {}", self.symbol().name, self.frame + 1),
        };
        // Figma Actions (docked): 44 pt toolbar = 32 pt controls + 6 pt top/bottom, 8 pt sides,
        // then a 1 pt separator above the editor.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(space::S2 as i8, 6))
            .show(ui, |ui| self.actions_toolbar(ui, &mut script, &context));
        if !has_stored_script && script.language != pending_lang {
            ui.data_mut(|d| d.insert_temp(empty_lang_id, script.language));
        }
        let line_y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            line_y,
            egui::Stroke::new(1.0_f32, neoflash_ui::palette(ui.ctx()).border_subtle),
        );
        if target_before != self.script_target || event_before != self.script_event {
            ui.ctx().request_repaint();
            return;
        }
        // Sidebar (toolbox + Script navigator) left of the script area, as in the NF2 Actions panel.
        let rest = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(rest);
        let main = if self.actions_ui.sidebar && rest.width() >= SIDEBAR_MIN_PANEL {
            let side =
                egui::Rect::from_min_size(rest.min, egui::vec2(SIDEBAR_WIDTH, rest.height()));
            let mut side_ui = ui.new_child(
                egui::UiBuilder::new().id_salt("actions-sidebar").max_rect(
                    side.shrink(space::S2)
                        .with_max_x(side.right() - 1. - space::S2),
                ),
            );
            self.actions_sidebar_with_js_ts(&mut side_ui, script.language, &tab);
            ui.painter().vline(
                side.right() - 0.5,
                side.y_range(),
                egui::Stroke::new(1.0_f32, neoflash_ui::palette(ui.ctx()).border_subtle),
            );
            egui::Rect::from_min_max(egui::pos2(side.right(), rest.top()), rest.max)
        } else {
            rest
        };
        let mut main_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("actions-main")
                .max_rect(main),
        );
        let ui = &mut main_ui;
        if target_before != self.script_target
            || event_before != self.script_event
            || self.current_script_location() != location_before
        {
            ui.ctx().request_repaint();
            return;
        }
        if self.script_target == ScriptTarget::Source {
            self.project_sources(ui);
            return;
        }
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            nf::EmptyState {
                icon: i16::SELECT,
                title: "No object selected",
                body: "Select one editable object to edit its actions, or choose Frame.",
                action: None,
            }
            .show(ui);
            return;
        }
        if self.symbol().kind == SymbolKind::Button
            && self.script_target == ScriptTarget::Frame
            && !has_frame_script
        {
            nf::EmptyState {
                icon: i16::BUTTON_SYMBOL,
                title: "Button states contain artwork",
                body: "Attach actions to the named Button instance on its parent timeline, or use its linked class in Project files.",
                action: None,
            }
            .show(ui);
            return;
        }
        let editable = self.script_target == ScriptTarget::Frame || self.selected_editable();
        let mut text_editing = false;
        let language = script.language;
        ui.add_enabled_ui_semantic(editable, |ui| {
            let response = self.script_workspace(ui, &mut script.source, language, "", &tab);
            text_editing = response.changed()
                || response.gained_focus()
                || (response.has_focus() && ui.input(|input| input.focused));
        });
        if text_editing && self.playing {
            self.set_playing(false);
        }
        let has_new_code = !script.source.is_empty();
        let should_commit = editable
            && script != original
            && if has_new_code {
                !has_stored_script || script != original
            } else {
                stored.is_some()
            };
        if should_commit {
            ui.data_mut(|d| d.insert_temp(empty_lang_id, script.language));
            if self.playing {
                self.set_playing(false);
            }
            self.finish_edits();
            if self.script_target == ScriptTarget::Object
                && let Some(id) = object_id
            {
                self.select_single_id(id);
            }
            self.transact(|s| match s.script_target {
                ScriptTarget::Source => unreachable!("Project files use their own editor"),
                ScriptTarget::Frame => {
                    let frame = s.frame;
                    if has_new_code && frame >= s.symbol().timeline.frame_count {
                        s.ensure_key(false);
                    }
                    let scripts = &mut s.symbol_mut().timeline.scripts;
                    scripts.retain(|fs| fs.frame != frame);
                    if has_new_code {
                        scripts.push(FrameScript { frame, script });
                    }
                }
                ScriptTarget::Object => {
                    let event = s.script_event;
                    if has_new_code {
                        s.ensure_selected_key();
                    }
                    if let Some(e) = s.selected_element_mut() {
                        e.scripts.retain(|os| os.event != event);
                        if has_new_code {
                            e.scripts.push(ObjectScript { event, script });
                        }
                    }
                }
            });
        }
    }

    fn effective_sidebar_language(&self, script_language: ScriptLanguage) -> ScriptLanguage {
        if self.script_target == ScriptTarget::Source {
            self.project
                .sources
                .iter()
                .find(|s| Some(&s.id) == self.selected_source.as_ref())
                .map_or(script_language, |s| s.language)
        } else {
            script_language
        }
    }

    pub(crate) fn actions_sidebar_with_js_ts(
        &mut self,
        ui: &mut egui::Ui,
        script_language: ScriptLanguage,
        current_label: &str,
    ) {
        let language = self.effective_sidebar_language(script_language);
        if !matches!(
            language,
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
        ) {
            self.actions_sidebar(ui, script_language, current_label);
            return;
        }
        let full = ui.available_rect_before_wrap();
        let navigator_h = (full.height() * 0.42).clamp(120., 260.);
        ui.add(nf::Search::new(
            &mut self.actions_ui.search,
            "Search toolbox",
        ));
        ui.add_space(space::S1);
        let toolbox_h = (ui.available_height() - navigator_h - space::S2)
            .max(2. * neoflash_ui::metrics::control());
        egui::ScrollArea::vertical()
            .id_salt("actions-toolbox")
            .max_height(toolbox_h)
            .min_scrolled_height(toolbox_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                self.js_ts_toolbox_tree(ui, language);
            });
        let p = neoflash_ui::palette(ui.ctx());
        ui.add_space(space::S1);
        let y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            egui::Stroke::new(1.0_f32, p.border_subtle),
        );
        ui.add_space(space::S3);
        ui.label(
            neoflash_ui::typography::rich("Script navigator", TextStyle::Overline)
                .color(p.text_secondary),
        );
        ui.add_space(space::S1);
        egui::ScrollArea::vertical()
            .id_salt("actions-navigator")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                self.js_ts_script_navigator(ui, current_label);
            });
    }

    fn js_ts_folder_row(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        icon: Icon,
        depth: usize,
    ) -> (bool, egui::Response) {
        let open = self.actions_ui.open.contains(key);
        let kind = if open {
            TreeItemType::FolderOpen
        } else {
            TreeItemType::FolderClosed
        };
        let row = nf::TreeItem::new(label, kind)
            .icon(icon)
            .depth(depth)
            .show(ui);
        if row.toggle || row.row.clicked() {
            if open {
                self.actions_ui.open.remove(key);
            } else {
                self.actions_ui.open.insert(key.to_owned());
            }
        }
        (open, row.row)
    }

    fn js_ts_toolbox_tree(&mut self, ui: &mut egui::Ui, language: ScriptLanguage) {
        let include_ts = language == ScriptLanguage::TypeScript;
        let needle = self.actions_ui.search.trim().to_owned();
        if !needle.is_empty() {
            let results = search_js_ts_entries(&needle, include_ts);
            let needle_lower = needle.to_lowercase();
            let matching_sources: Vec<(String, String)> = self
                .project
                .sources
                .iter()
                .filter(|s| {
                    matches!(
                        s.language,
                        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
                    ) && (include_ts || s.language == ScriptLanguage::JavaScript)
                        && s.path.to_lowercase().contains(&needle_lower)
                })
                .map(|s| (s.id.clone(), s.path.clone()))
                .collect();
            if results.is_empty() && matching_sources.is_empty() {
                let p = neoflash_ui::palette(ui.ctx());
                ui.label(
                    neoflash_ui::typography::rich(
                        format!("No toolbox entries match “{needle}”."),
                        TextStyle::Small,
                    )
                    .color(p.text_tertiary),
                );
                return;
            }
            for entry in results {
                self.js_ts_toolbox_leaf(ui, entry, 0);
            }
            for (id, path) in matching_sources {
                self.js_ts_project_module_leaf(ui, &id, &path, 0);
            }
            return;
        }

        let project_modules: Vec<(String, String)> = self
            .project
            .sources
            .iter()
            .filter(|s| {
                matches!(
                    s.language,
                    ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
                ) && (include_ts || s.language == ScriptLanguage::JavaScript)
            })
            .map(|s| (s.id.clone(), s.path.clone()))
            .collect();
        if !project_modules.is_empty()
            && self
                .js_ts_folder_row(ui, "box:jsts:project", "Project Modules", i16::FILE_CODE, 0)
                .0
        {
            for (id, path) in &project_modules {
                self.js_ts_project_module_leaf(ui, id, path, 1);
            }
        }

        for &(cat, cat_desc, cat_ts_only) in JS_TS_CATEGORIES {
            if cat_ts_only && !include_ts {
                continue;
            }
            let has_entries = JS_TS_API_ENTRIES
                .iter()
                .any(|e| e.category == cat && (include_ts || !e.ts_only));
            if !has_entries {
                continue;
            }
            let key = format!("box:jsts:cat:{cat}");
            let (cat_open, cat_row) = self.js_ts_folder_row(ui, &key, cat, i16::PACKAGE, 0);
            cat_row.on_hover_text(cat_desc);
            if !cat_open {
                continue;
            }
            for class_entry in JS_TS_API_ENTRIES.iter().filter(|e| {
                e.category == cat && (include_ts || !e.ts_only) && e.kind == JsTsEntryKind::Class
            }) {
                let class_key = format!("box:jsts:class:{}", class_entry.name);
                let (open, row) =
                    self.js_ts_folder_row(ui, &class_key, class_entry.name, i16::CLASS, 1);
                let row = row.on_hover_text(js_ts_entry_tooltip(class_entry));
                if row.double_clicked() {
                    self.actions_ui.selected = Some(class_key.clone());
                    self.actions_ui.pending = Some(js_ts_toolbox_insertion(class_entry));
                }
                if open {
                    self.js_ts_toolbox_leaf(ui, class_entry, 2);
                    for member_entry in JS_TS_API_ENTRIES.iter().filter(|e| {
                        (include_ts || !e.ts_only)
                            && e.kind == JsTsEntryKind::Member(class_entry.name)
                    }) {
                        self.js_ts_toolbox_leaf(ui, member_entry, 2);
                    }
                }
            }
            for entry in JS_TS_API_ENTRIES.iter().filter(|e| {
                e.category == cat
                    && (include_ts || !e.ts_only)
                    && !matches!(e.kind, JsTsEntryKind::Class | JsTsEntryKind::Member(_))
            }) {
                self.js_ts_toolbox_leaf(ui, entry, 1);
            }
        }
    }

    fn js_ts_toolbox_leaf(
        &mut self,
        ui: &mut egui::Ui,
        entry: &'static JsTsApiEntry,
        depth: usize,
    ) {
        let key = format!("box:jsts:{}:{}", entry.category, entry.name);
        let row = nf::TreeItem::new(entry.name, TreeItemType::Symbol(js_ts_entry_icon(entry)))
            .depth(depth)
            .meta(entry.provenance)
            .selected(self.actions_ui.selected.as_deref() == Some(&key))
            .show(ui);
        let row = row.row.on_hover_text(js_ts_entry_tooltip(entry));
        if row.clicked() {
            self.actions_ui.selected = Some(key);
        }
        if row.double_clicked() {
            self.actions_ui.pending = Some(js_ts_toolbox_insertion(entry));
        }
    }

    fn js_ts_project_module_leaf(&mut self, ui: &mut egui::Ui, id: &str, path: &str, depth: usize) {
        let key = format!("box:jsts:module:{id}");
        let row = nf::TreeItem::new(path, TreeItemType::Symbol(i16::FILE_CODE))
            .depth(depth)
            .meta("Project module")
            .selected(self.actions_ui.selected.as_deref() == Some(&key))
            .show(ui);
        let row = row.row.on_hover_text(format!(
            "import \"./{path}\";\n[Project module]\nProject module ({path})\nDouble-click to insert ES module import."
        ));
        if row.clicked() {
            self.actions_ui.selected = Some(key);
        }
        if row.double_clicked() {
            let text = format!("import \"./{path}\";");
            let caret = text.chars().count();
            self.actions_ui.pending = Some(CodeCommand::Insert {
                text,
                caret,
                import: None,
            });
        }
    }

    fn js_ts_script_navigator(&mut self, ui: &mut egui::Ui, current_label: &str) {
        if self
            .js_ts_folder_row(ui, "nav:current", "Current selection", i16::TARGET, 0)
            .0
        {
            let icon = if self.script_target == ScriptTarget::Source {
                i16::FILE_CODE
            } else {
                i16::FRAME
            };
            nf::TreeItem::new(current_label, TreeItemType::Symbol(icon))
                .depth(1)
                .selected(true)
                .show(ui);
        }
        let root = self.project.root.clone();
        let root_name = self
            .project
            .symbol(&root)
            .map_or("Scene", |s| s.name.as_str())
            .to_owned();
        let mut chosen = None;
        if self
            .js_ts_folder_row(ui, "nav:root", &root_name, i16::SCENE, 0)
            .0
        {
            for (label, icon, location) in self.js_ts_navigator_entries(&root) {
                chosen = chosen.or(self.js_ts_navigator_leaf(ui, &label, icon, &location, 1));
            }
        }
        if self
            .js_ts_folder_row(ui, "nav:symbols", "Symbol definitions", i16::SYMBOL, 0)
            .0
        {
            let symbols: Vec<String> = self
                .project
                .symbols
                .iter()
                .filter(|s| s.id != root)
                .map(|s| s.id.clone())
                .collect();
            for symbol in symbols {
                for (label, icon, location) in self.js_ts_navigator_entries(&symbol) {
                    chosen = chosen.or(self.js_ts_navigator_leaf(ui, &label, icon, &location, 1));
                }
            }
        }
        if !self.project.sources.is_empty()
            && self
                .js_ts_folder_row(ui, "nav:sources", "Project files", i16::FOLDER, 0)
                .0
        {
            let sources: Vec<(String, Icon, ScriptLocation)> = self
                .project
                .sources
                .iter()
                .map(|s| {
                    (
                        s.path.clone(),
                        i16::FILE_CODE,
                        ScriptLocation::Source(s.id.clone()),
                    )
                })
                .collect();
            for (label, icon, location) in sources {
                chosen = chosen.or(self.js_ts_navigator_leaf(ui, &label, icon, &location, 1));
            }
        }
        if let Some(location) = chosen {
            self.open_script_location(location);
        }
    }

    fn js_ts_navigator_leaf(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        icon: Icon,
        location: &ScriptLocation,
        depth: usize,
    ) -> Option<ScriptLocation> {
        let current = self.current_script_location().as_ref() == Some(location);
        let row = nf::TreeItem::new(label, TreeItemType::Symbol(icon))
            .depth(depth)
            .selected(current)
            .show(ui);
        row.row.clicked().then(|| location.clone())
    }

    fn js_ts_navigator_entries(&self, symbol_id: &str) -> Vec<(String, Icon, ScriptLocation)> {
        let Some(symbol) = self.project.symbol(symbol_id) else {
            return Vec::new();
        };
        let mut frames: Vec<u32> = symbol
            .timeline
            .scripts
            .iter()
            .filter(|s| !s.script.source.trim().is_empty())
            .map(|s| s.frame)
            .collect();
        frames.sort_unstable();
        frames.dedup();
        let mut entries: Vec<(String, Icon, ScriptLocation)> = frames
            .into_iter()
            .map(|frame| {
                (
                    format!("{} : Frame {}", symbol.name, frame + 1),
                    i16::FRAME,
                    ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        frame,
                    },
                )
            })
            .collect();
        let mut seen = HashSet::new();
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for script in element
                        .scripts
                        .iter()
                        .filter(|s| !s.script.source.trim().is_empty())
                    {
                        if !seen.insert((element.id.clone(), format!("{:?}", script.event))) {
                            continue;
                        }
                        let name = if element.name.is_empty() {
                            "Object"
                        } else {
                            &element.name
                        };
                        entries.push((
                            format!("{name} : {:?}", script.event),
                            i16::MOVIECLIP,
                            ScriptLocation::Object {
                                symbol: symbol.id.clone(),
                                frame: key.frame,
                                element: element.id.clone(),
                                event: script.event,
                            },
                        ));
                    }
                }
            }
        }
        entries
    }
}

fn js_ts_entry_icon(entry: &JsTsApiEntry) -> Icon {
    match entry.kind {
        JsTsEntryKind::Class | JsTsEntryKind::TypeConstruct => i16::CLASS,
        JsTsEntryKind::Binding => i16::VARIABLE,
        JsTsEntryKind::Function | JsTsEntryKind::Keyword | JsTsEntryKind::Member(_) => i16::CODE,
    }
}

pub(crate) fn js_ts_entry_tooltip(entry: &JsTsApiEntry) -> String {
    format!(
        "{}\n[{}]\nAvailability: {}\n{}",
        entry.signature, entry.provenance, entry.availability, entry.summary
    )
}

pub(crate) fn js_ts_toolbox_insertion(entry: &JsTsApiEntry) -> CodeCommand {
    let text = entry.insertion.to_owned();
    let total = text.chars().count();
    let caret = if text.ends_with("(\"\");") {
        total.saturating_sub(3)
    } else if text.ends_with("();") {
        total.saturating_sub(2)
    } else {
        total
    };
    CodeCommand::Insert {
        text,
        caret,
        import: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_and_ts_toolbox_queries_and_insertions_cover_required_entries() {
        let required_queries = [
            "this",
            "event",
            "trace",
            "gotoAndPlay",
            "import",
            "interface",
            "Promise",
            "fetch",
            "CanvasRenderingContext2D",
        ];
        for query in required_queries {
            let ts_results = search_js_ts_entries(query, true);
            assert!(
                !ts_results.is_empty(),
                "expected TypeScript toolbox search results for query `{query}`"
            );
            let first = ts_results[0];
            assert!(!first.signature.is_empty());
            assert!(!first.provenance.is_empty());
            assert!(!first.availability.is_empty());
            assert!(!first.summary.is_empty());
            let tooltip = js_ts_entry_tooltip(first);
            assert!(tooltip.contains(first.provenance));
            assert!(tooltip.contains(first.availability));

            let cmd = js_ts_toolbox_insertion(first);
            let Some((edited, (caret, _))) = crate::code_tools::apply(&cmd, "", (0, 0)) else {
                panic!("expected CodeCommand::Insert for `{query}` to produce an edit");
            };
            assert!(
                !edited.is_empty(),
                "expected non-empty insertion snippet for `{query}`"
            );
            assert!(caret <= edited.chars().count());
        }

        // JavaScript toolbox omits TypeScript-only syntax (`interface`, `type`, `<T>`) while
        // keeping `// @ts-check` and all ECMAScript/NeoFlash/Web API entries.
        let js_interface = search_js_ts_entries("interface", false);
        assert!(
            js_interface.is_empty(),
            "JavaScript toolbox should filter out TypeScript-only `interface` entry"
        );
        let ts_interface = search_js_ts_entries("interface", true);
        assert!(
            ts_interface.iter().any(|e| e.name == "interface"),
            "TypeScript toolbox should include `interface` entry"
        );
    }
}
