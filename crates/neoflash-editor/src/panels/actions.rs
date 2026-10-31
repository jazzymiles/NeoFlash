//! Library wrapper, bottom-panel dispatch, Output and the Actions (script) editor.
//!
//! NF2 references: `panels/*/png/panel-actions-floating.png`, `interface/*/png/editor-actions-tab.png`
//! (Actions) and `panels/*/png/panels-more-2.png` (Output).
use super::actions_sidebar::{SIDEBAR_MIN_PANEL, SIDEBAR_WIDTH};
use crate::app::*;
use crate::code_tools::CodeCommand;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::{self as nf, code::Severity};
use neoflash_ui::icons::i16;
use neoflash_ui::metrics::CONTROL;
use neoflash_ui::tokens::{TextStyle, radius, space};

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
    fn output_panel(&mut self, ui: &mut egui::Ui) {
        let filter_id = egui::Id::new("neoflash-output-filter");
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        let needle = filter.trim().to_lowercase();
        let visible: Vec<usize> = (0..self.output.len())
            .filter(|i| needle.is_empty() || self.output[*i].to_lowercase().contains(&needle))
            .collect();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let search = (ui.available_width() - 2. * (CONTROL + space::S2)).max(80.);
            ui.add(nf::Search::new(&mut filter, "Filter output").width(search));
            let copy = ui.add_enabled_semantic(
                !visible.is_empty(),
                nf::IconButton::new(i16::COPY, "Copy output").shortcut("⌘C"),
            );
            if copy.clicked() {
                let text = visible
                    .iter()
                    .map(|i| self.output[*i].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
            }
            let clear = ui.add_enabled_semantic(
                !self.output.is_empty(),
                nf::IconButton::new(i16::TRASH, "Clear output"),
            );
            if clear.clicked() {
                self.output.clear();
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
        let p = neoflash_ui::palette(ui.ctx());
        egui::Frame::new()
            .fill(p.bg_field)
            .corner_radius(radius::MD)
            .inner_margin(egui::Margin::same(space::S2 as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("output_contents")
                    .stick_to_bottom(true)
                    .themed()
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        for i in &visible {
                            ui.add(nf::LogLine {
                                severity: Severity::Info,
                                time: "",
                                text: &self.output[*i],
                            });
                        }
                        if visible.is_empty() {
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
            let right = 140. + CONTROL + 2. * 2.;
            // In a wrapped row `available_width` is the full row width, not what is left of it.
            let rest = ui.max_rect().right() - ui.cursor().left() - right - space::S4;
            if rest > 0. {
                ui.add_space(rest);
            }
            let assist = ui.add_enabled(
                false,
                nf::Button::ghost("Script assist").icon(i16::WAND),
            );
            nf::not_available(assist, "Script assist");
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
        let original = match self.script_target {
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
        }
        .unwrap_or_default();
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
            self.actions_sidebar(&mut side_ui, script.language, &tab);
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
            let response = self.script_workspace(
                ui,
                &mut script.source,
                language,
                "this.stop();\n// Use this.gotoAndPlay(1) to control a timeline.",
                &tab,
            );
            text_editing = response.changed()
                || response.gained_focus()
                || (response.has_focus() && ui.input(|input| input.focused));
        });
        if text_editing && self.playing {
            self.set_playing(false);
        }
        if script != original && editable {
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
                    let scripts = &mut s.symbol_mut().timeline.scripts;
                    scripts.retain(|fs| fs.frame != frame);
                    if !script.source.trim().is_empty()
                        || script.language != ScriptLanguage::JavaScript
                    {
                        scripts.push(FrameScript { frame, script });
                    }
                }
                ScriptTarget::Object => {
                    let event = s.script_event;
                    s.ensure_selected_key();
                    if let Some(e) = s.selected_element_mut() {
                        e.scripts.retain(|os| os.event != event);
                        if !script.source.trim().is_empty()
                            || script.language != ScriptLanguage::JavaScript
                        {
                            e.scripts.push(ObjectScript { event, script });
                        }
                    }
                }
            });
        }
    }
}
