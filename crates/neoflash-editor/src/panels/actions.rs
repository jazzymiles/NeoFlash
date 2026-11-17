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
use neoflash_ui::components::{self as nf, TreeItemType};
use neoflash_ui::icons::{Icon, i16};
use neoflash_ui::tokens::{TextStyle, space};
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

    pub(crate) fn actions_editor_id(&self) -> egui::Id {
        match self.effective_actions_location() {
            Some(ScriptLocation::Source(id)) => egui::Id::new((
                "neoflash-project-source-editor",
                &self.clipboard.session,
                &Some(id),
            )),
            Some(ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            }) => {
                let target = format!("frame:{}", layer.as_deref().unwrap_or(""));
                egui::Id::new((
                    "neoflash-actions-editor",
                    &self.clipboard.session,
                    symbol.as_str(),
                    frame,
                    target,
                ))
            }
            Some(ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            }) => {
                let target = format!("object:{element}:{event:?}");
                egui::Id::new((
                    "neoflash-actions-editor",
                    &self.clipboard.session,
                    symbol.as_str(),
                    frame,
                    target,
                ))
            }
            None => {
                if self.script_target == ScriptTarget::Source {
                    return egui::Id::new((
                        "neoflash-project-source-editor",
                        &self.clipboard.session,
                        &self.selected_source,
                    ));
                }
                let target = match self.script_target {
                    ScriptTarget::Frame => format!(
                        "frame:{}",
                        self.symbol()
                            .timeline
                            .layers
                            .get(self.layer)
                            .map_or("", |layer| layer.id.as_str())
                    ),
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
        }
    }
    pub(crate) fn actions_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Actions,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| self.actions_body(ui));
    }

    /// NF2 Actions toolbar: target select and language badge (`R-102`), object event, Check,
    /// code tools (format, hints, comments), sidebar toggle, pin script tab, Script assist and help.
    #[allow(clippy::too_many_arguments)]
    fn actions_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        script: &mut Script,
        has_stored_script: bool,
        context: &str,
        active_target: ScriptTarget,
        active_loc: &Option<ScriptLocation>,
        editable: bool,
    ) {
        let source = active_target == ScriptTarget::Source;
        let active_source_id = match active_loc {
            Some(ScriptLocation::Source(id)) => Some(id.clone()),
            _ if source => self.selected_source.clone(),
            _ => None,
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2., space::S1);
            let target_width = (ui.available_width() * 0.3).clamp(140., 280.);
            let (target_response, _) = nf::Select::new("actions_target", context)
                .icon(i16::TARGET)
                .width(target_width)
                .show(ui, |ui| {
                    let frame = format!("Frame {}", self.frame + 1);
                    let items = [
                        (ScriptTarget::Frame, frame.as_str(), true),
                        (
                            ScriptTarget::Object,
                            "Selected object",
                            self.selected_editable(),
                        ),
                        (ScriptTarget::Source, "Project files", true),
                    ];
                    for (target, label, enabled) in items {
                        let item = nf::MenuItem::new(label).checked(active_target == target);
                        if ui.add_enabled_semantic(enabled, item).clicked() {
                            self.script_target = target;
                            self.activate_current_actions_location();
                        }
                    }
                });
            nf::tooltip(target_response, context.trim_end_matches('.'), None);
            if !source {
                let proj_lang = self.project.effective_script_language();
                let skipped = has_stored_script
                    && self.project.script_disposition_for(
                        proj_lang,
                        self.project.also_compile_compatible_scripts,
                        script.language,
                    ) == ScriptDisposition::SkippedForeign;
                ui.add_space(space::S1);
                Self::script_language_badge(ui, script.language, skipped);
            }
            if active_target == ScriptTarget::Object {
                let cur_event = match active_loc {
                    Some(ScriptLocation::Object { event, .. }) => *event,
                    _ => self.script_event,
                };
                ui.add_space(space::S1);
                let label = format!("Event: {cur_event:?}");
                nf::Select::new("actions_object_event", &label)
                    .width(150.)
                    .show(ui, |ui| {
                        for event in [
                            ObjectEvent::Click,
                            ObjectEvent::Load,
                            ObjectEvent::EnterFrame,
                        ] {
                            let name = format!("{event:?}");
                            let item = nf::MenuItem::new(&name).checked(cur_event == event);
                            if ui.add(item).clicked() {
                                if let Some(ScriptLocation::Object {
                                    symbol,
                                    frame,
                                    element,
                                    ..
                                }) = active_loc
                                    && self.actions_ui.active_pinned.is_some()
                                {
                                    let next_loc = ScriptLocation::Object {
                                        symbol: symbol.clone(),
                                        frame: *frame,
                                        element: element.clone(),
                                        event,
                                    };
                                    if self.actions_ui.pinned_tabs.contains(&next_loc) {
                                        self.actions_ui.active_pinned = Some(next_loc);
                                    } else {
                                        self.open_script_location(next_loc);
                                    }
                                } else {
                                    self.script_event = event;
                                    self.activate_current_actions_location();
                                }
                            }
                        }
                    });
            }
            ui.add_space(space::S1);
            nf::divider(ui, true);
            let check_label = if source {
                "Check project"
            } else {
                "Check script"
            };
            let check = ui.add_enabled_semantic(
                !source || active_source_id.is_some(),
                nf::IconButton::new(i16::CHECK, check_label),
            );
            if check.clicked() {
                if source {
                    self.check_project_sources(true);
                } else {
                    self.check_action_source(script, true);
                }
            }
            let source_language = active_source_id.as_ref().and_then(|id| {
                self.project
                    .sources
                    .iter()
                    .find(|s| &s.id == id)
                    .map(|s| s.language)
            });
            let language = if source {
                source_language
            } else {
                Some(script.language)
            };
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
                    let mut widget = nf::IconButton::new(*icon, label);
                    if *command == CodeCommand::ShowHints {
                        widget = widget.shortcut("⌃Space");
                    }
                    let button = ui.add_enabled_semantic(enabled, widget);
                    if button.clicked() {
                        self.actions_ui.pending = Some(command.clone());
                    }
                }
                nf::divider(ui, true);
            }
            let sidebar = ui.add(
                nf::IconButton::new(i16::SIDEBAR, "Show toolbox sidebar")
                    .selected(self.actions_ui.sidebar),
            );
            if sidebar.clicked() {
                self.actions_ui.sidebar = !self.actions_ui.sidebar;
            }
            let is_pinned = active_loc
                .as_ref()
                .is_some_and(|loc| self.actions_ui.pinned_tabs.contains(loc));
            let pin_btn = ui.add_enabled_semantic(
                active_loc.is_some(),
                nf::IconButton::new(i16::PIN, "Pin script tab").selected(is_pinned),
            );
            if pin_btn.clicked()
                && let Some(loc) = active_loc.clone()
            {
                self.toggle_pin_script_tab(loc);
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
                    if self.has_ready_ai_connection() {
                        nf::menu_separator(ui);
                        if ui.add(nf::MenuItem::new("Send to AI…")).clicked() {
                            let _ = self.open_send_to_ai();
                            ui.close();
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

    /// Render the `R-102` item 7 foreign-language notice row between the toolbar separator and
    /// `Main` when viewing a stored frame/object script or project source file in another language.
    /// Returns `true` when `Mark as <project language>` was clicked and mutated the script tag.
    fn foreign_script_notice_row(
        &mut self,
        ui: &mut egui::Ui,
        active_target: ScriptTarget,
        active_loc: &Option<ScriptLocation>,
        has_stored_script: bool,
        frame_or_obj_lang: ScriptLanguage,
    ) -> bool {
        let proj_lang = self.project.effective_script_language();
        let active_foreign = match active_target {
            ScriptTarget::Source => {
                let source_id = match active_loc {
                    Some(ScriptLocation::Source(id)) => Some(id.as_str()),
                    _ => self.selected_source.as_deref(),
                };
                let unit = source_id
                    .and_then(|id| self.project.sources.iter().find(|s| s.id == id))
                    .or_else(|| self.project.sources.first());
                unit.filter(|u| u.language != proj_lang).map(|u| {
                    (
                        u.language,
                        Self::can_retag_source_unit_to(&u.path, proj_lang),
                    )
                })
            }
            ScriptTarget::Frame | ScriptTarget::Object => (has_stored_script
                && frame_or_obj_lang != proj_lang)
                .then_some((frame_or_obj_lang, true)),
        };
        let Some((foreign_lang, can_retag)) = active_foreign else {
            return false;
        };
        let disposition = self.project.script_disposition_for(
            proj_lang,
            self.project.also_compile_compatible_scripts,
            foreign_lang,
        );
        let (is_warning, notice_text) = match disposition {
            ScriptDisposition::SkippedForeign => (
                true,
                format!(
                    "This script is {}. The project uses {}, so the script is skipped when publishing.",
                    foreign_lang.display_label(),
                    proj_lang.display_label()
                ),
            ),
            ScriptDisposition::CompiledCompatibleWithWarning
            | ScriptDisposition::CompiledPrimary => (
                false,
                format!(
                    "This script is {}. The project uses {}; the script compiles with a warning.",
                    foreign_lang.display_label(),
                    proj_lang.display_label()
                ),
            ),
        };
        let mut mark_clicked = false;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(space::S3 as i8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = space::S3;
                    if can_retag {
                        let mark_label = format!("Mark as {}", proj_lang.display_label());
                        if ui.add(nf::Button::ghost(&mark_label)).clicked() {
                            mark_clicked = true;
                        }
                    }
                    let msg_w = ui.available_width();
                    let im = if is_warning {
                        nf::InlineMessage::warning(&notice_text)
                    } else {
                        nf::InlineMessage::info(&notice_text)
                    };
                    ui.add(im.width(msg_w));
                });
            });
        let notice_line_y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            notice_line_y,
            egui::Stroke::new(1.0_f32, neoflash_ui::palette(ui.ctx()).border_subtle),
        );
        if mark_clicked {
            self.mark_current_script_as_project_language();
        }
        mark_clicked
    }

    fn render_empty_state_with_pinned_tabs(
        &mut self,
        ui: &mut egui::Ui,
        empty_state: nf::EmptyState<'_>,
    ) {
        let avail = ui.available_rect_before_wrap();
        let bar_h = if self.actions_ui.pinned_tabs.is_empty() {
            0.0
        } else {
            neoflash_ui::metrics::script_tab() + 2.0
        };
        let empty_rect = egui::Rect::from_min_max(
            avail.min,
            egui::pos2(avail.right(), (avail.bottom() - bar_h).max(avail.top())),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(empty_rect), |ui| {
            let _ = empty_state.show(ui);
        });
        if bar_h > 0.0 {
            let bar = egui::Rect::from_min_max(
                egui::pos2(avail.left(), avail.bottom() - bar_h),
                avail.max,
            );
            let p = neoflash_ui::palette(ui.ctx());
            ui.painter().hline(
                bar.x_range(),
                bar.top() + 0.5,
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
            self.render_actions_script_tabs_bar(ui, bar, "");
            ui.advance_cursor_after_rect(avail);
        }
    }

    fn actions_body(&mut self, ui: &mut egui::Ui) {
        self.prune_stale_pinned_script_tabs();
        if self.actions_ui.active_pinned.is_none()
            && self.script_target == ScriptTarget::Source
            && !self
                .project
                .sources
                .iter()
                .any(|s| Some(&s.id) == self.selected_source.as_ref())
        {
            self.selected_source = self.project.sources.first().map(|s| s.id.clone());
        }
        let location_before = self.effective_actions_location();
        let active_target = match &location_before {
            Some(ScriptLocation::Source(_)) => ScriptTarget::Source,
            Some(ScriptLocation::Frame { .. }) => ScriptTarget::Frame,
            Some(ScriptLocation::Object { .. }) => ScriptTarget::Object,
            None => self.script_target,
        };
        let target_before = self.script_target;
        let event_before = self.script_event;

        let (
            sym_id,
            sym_name,
            sym_kind,
            resolved_layer_idx,
            active_layer_name,
            resolved_frame,
            active_frame_script,
            resolved_object_id,
            resolved_object_name,
            resolved_event,
            stored,
            editable,
        ) = match &location_before {
            Some(ScriptLocation::Frame {
                symbol,
                layer,
                frame,
            }) => {
                let sym = self.project.symbol(symbol).unwrap_or_else(|| self.symbol());
                let l_idx = layer
                    .as_deref()
                    .and_then(|id| sym.timeline.layers.iter().position(|l| l.id == id))
                    .or_else(|| sym.timeline.action_layer_for_frame(*frame))
                    .unwrap_or(0);
                let l_name = sym
                    .timeline
                    .layers
                    .get(l_idx)
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "Layer".to_owned());
                let fs = sym.timeline.frame_script_for_layer(l_idx, *frame).cloned();
                let st = fs.as_ref().map(|s| s.script.clone());
                (
                    sym.id.clone(),
                    sym.name.clone(),
                    sym.kind,
                    l_idx,
                    l_name,
                    *frame,
                    fs,
                    None,
                    "Object".to_owned(),
                    self.script_event,
                    st,
                    true,
                )
            }
            Some(ScriptLocation::Object {
                symbol,
                frame,
                element,
                event,
            }) => {
                let sym = self.project.symbol(symbol).unwrap_or_else(|| self.symbol());
                let el = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| &k.elements)
                    .find(|e| &e.id == element);
                let el_name = el
                    .map(|e| {
                        if e.name.is_empty() {
                            "Object".to_owned()
                        } else {
                            e.name.clone()
                        }
                    })
                    .unwrap_or_else(|| "Object".to_owned());
                let st = el
                    .and_then(|e| e.scripts.iter().find(|s| s.event == *event))
                    .map(|s| s.script.clone());
                let is_ed = if self.actions_ui.active_pinned.is_some() {
                    el.is_some()
                } else {
                    self.selected_editable()
                };
                (
                    sym.id.clone(),
                    sym.name.clone(),
                    sym.kind,
                    0,
                    "Layer".to_owned(),
                    *frame,
                    None,
                    Some(element.clone()),
                    el_name,
                    *event,
                    st,
                    is_ed,
                )
            }
            Some(ScriptLocation::Source(src_id)) => (
                self.symbol_id().to_owned(),
                self.symbol().name.clone(),
                self.symbol().kind,
                self.layer,
                "Layer".to_owned(),
                self.frame,
                None,
                None,
                "Object".to_owned(),
                self.script_event,
                None,
                self.project.sources.iter().any(|s| &s.id == src_id),
            ),
            None => (
                self.symbol_id().to_owned(),
                self.symbol().name.clone(),
                self.symbol().kind,
                self.layer,
                self.symbol()
                    .timeline
                    .layers
                    .get(self.layer)
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "Layer".to_owned()),
                self.frame,
                None,
                None,
                "Object".to_owned(),
                self.script_event,
                None,
                active_target == ScriptTarget::Frame && !self.symbol().timeline.layers.is_empty(),
            ),
        };

        let has_frame_script = active_frame_script
            .as_ref()
            .is_some_and(|script| !script.script.source.trim().is_empty());
        let proj_lang = self.project.effective_script_language();
        let has_stored_script = stored.as_ref().is_some_and(|s| !s.source.is_empty());
        let original = match &stored {
            Some(s) if !s.source.is_empty() => s.clone(),
            _ => Script {
                language: proj_lang,
                source: String::new(),
            },
        };
        let mut script = original.clone();
        let context = match active_target {
            ScriptTarget::Frame => format!(
                "{sym_name} · {active_layer_name} · Frame {}{}",
                resolved_frame + 1,
                if has_frame_script { " · script" } else { "" }
            ),
            ScriptTarget::Object => {
                format!("{sym_name} · {resolved_object_name} · {resolved_event:?}")
            }
            ScriptTarget::Source => "Project files".to_owned(),
        };
        let tab = match active_target {
            ScriptTarget::Object => format!("{resolved_object_name} : {resolved_event:?}"),
            ScriptTarget::Source => location_before
                .as_ref()
                .map(|loc| self.script_location_tab_label(loc).0)
                .unwrap_or_else(|| "Project files".to_owned()),
            ScriptTarget::Frame => format!("{sym_name} : Frame {}", resolved_frame + 1),
        };
        let saved_y_spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        // Figma Actions (docked): 44 pt toolbar = 32 pt controls + 6 pt top/bottom, 8 pt sides,
        // then a 1 pt separator above the editor (and optional 45 pt foreign-language notice row).
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(space::S2 as i8, 6))
            .show(ui, |ui| {
                self.actions_toolbar(
                    ui,
                    &mut script,
                    has_stored_script,
                    &context,
                    active_target,
                    &location_before,
                    editable,
                )
            });
        let line_y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            line_y,
            egui::Stroke::new(1.0_f32, neoflash_ui::palette(ui.ctx()).border_subtle),
        );
        if self.effective_actions_location() != location_before
            || (self.actions_ui.active_pinned.is_none()
                && (target_before != self.script_target || event_before != self.script_event))
        {
            ui.spacing_mut().item_spacing.y = saved_y_spacing;
            ui.ctx().request_repaint();
            return;
        }
        if self.foreign_script_notice_row(
            ui,
            active_target,
            &location_before,
            has_stored_script,
            script.language,
        ) {
            ui.spacing_mut().item_spacing.y = saved_y_spacing;
            ui.ctx().request_repaint();
            return;
        }
        ui.spacing_mut().item_spacing.y = saved_y_spacing;
        // Sidebar (toolbox + Script navigator) left of the script area, as in the NF2 Actions panel.
        let rest = ui.available_rect_before_wrap();
        ui.advance_cursor_after_rect(rest);
        let nav_label = location_before
            .as_ref()
            .map(|loc| self.script_location_tab_label(loc).0)
            .unwrap_or_else(|| tab.clone());
        let main = if self.actions_ui.sidebar && rest.width() >= SIDEBAR_MIN_PANEL {
            let side =
                egui::Rect::from_min_size(rest.min, egui::vec2(SIDEBAR_WIDTH, rest.height()));
            let mut side_ui = ui.new_child(
                egui::UiBuilder::new().id_salt("actions-sidebar").max_rect(
                    side.shrink(space::S2)
                        .with_max_x(side.right() - 1. - space::S2),
                ),
            );
            self.actions_sidebar_with_js_ts(&mut side_ui, script.language, &nav_label);
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
        if self.effective_actions_location() != location_before
            || (self.actions_ui.active_pinned.is_none()
                && (target_before != self.script_target || event_before != self.script_event))
        {
            ui.ctx().request_repaint();
            return;
        }
        if active_target == ScriptTarget::Source {
            self.project_sources(ui);
            return;
        }
        if active_target == ScriptTarget::Object && !editable {
            self.render_empty_state_with_pinned_tabs(
                ui,
                nf::EmptyState {
                    icon: i16::SELECT,
                    title: "No object selected",
                    body: "Select one editable object to edit its actions, or choose Frame.",
                    action: None,
                },
            );
            return;
        }
        if sym_kind == SymbolKind::Button
            && active_target == ScriptTarget::Frame
            && !has_frame_script
        {
            self.render_empty_state_with_pinned_tabs(
                ui,
                nf::EmptyState {
                    icon: i16::BUTTON_SYMBOL,
                    title: "Button states contain artwork",
                    body: "Attach actions to the named Button instance on its parent timeline, or use its linked class in Project files.",
                    action: None,
                },
            );
            return;
        }
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
            if self.playing {
                self.set_playing(false);
            }
            self.finish_edits();
            let editing_pinned = self.actions_ui.active_pinned.is_some();
            if active_target == ScriptTarget::Object
                && !editing_pinned
                && let Some(id) = resolved_object_id.clone()
            {
                self.select_single_id(id);
            }
            self.transact(|s| match active_target {
                ScriptTarget::Source => unreachable!("Project files use their own editor"),
                ScriptTarget::Frame => {
                    let Some(layer_id) = s
                        .project
                        .symbol(&sym_id)
                        .and_then(|sym| sym.timeline.layers.get(resolved_layer_idx))
                        .map(|l| l.id.clone())
                    else {
                        return;
                    };
                    if has_new_code {
                        let prev_editing = s.editing.clone();
                        if s.symbol_id() != sym_id {
                            s.editing = vec![sym_id.clone()];
                        }
                        s.ensure_frame_script_key_at(&layer_id, resolved_frame);
                        s.editing = prev_editing;
                    }
                    let Some(sym_mut) = s.project.symbol_mut(&sym_id) else {
                        return;
                    };
                    let existing_index = sym_mut
                        .timeline
                        .frame_script_index_for_layer(resolved_layer_idx, resolved_frame);
                    let scripts = &mut sym_mut.timeline.scripts;
                    if let Some(index) = existing_index {
                        scripts.remove(index);
                    }
                    if has_new_code {
                        scripts.push(FrameScript {
                            layer: Some(layer_id),
                            frame: resolved_frame,
                            script,
                        });
                        scripts.sort_by_key(|script| script.frame);
                    }
                }
                ScriptTarget::Object => {
                    let event = resolved_event;
                    if editing_pinned {
                        let Some(obj_id) = resolved_object_id.as_deref() else {
                            return;
                        };
                        if let Some(sym_mut) = s.project.symbol_mut(&sym_id) {
                            for l in &mut sym_mut.timeline.layers {
                                for k in &mut l.keyframes {
                                    if let Some(e) = k.elements.iter_mut().find(|e| e.id == obj_id)
                                    {
                                        e.scripts.retain(|os| os.event != event);
                                        if has_new_code {
                                            e.scripts.push(ObjectScript {
                                                event,
                                                script: script.clone(),
                                            });
                                        }
                                        return;
                                    }
                                }
                            }
                        }
                    } else {
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
                }
            });
        }
    }

    fn effective_sidebar_language(&self, script_language: ScriptLanguage) -> ScriptLanguage {
        if let Some(ScriptLocation::Source(id)) = self.effective_actions_location() {
            self.project
                .sources
                .iter()
                .find(|s| s.id == id)
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
            nf::tooltip(cat_row, cat_desc.trim_end_matches('.'), None);
            if !cat_open {
                continue;
            }
            for class_entry in JS_TS_API_ENTRIES.iter().filter(|e| {
                e.category == cat && (include_ts || !e.ts_only) && e.kind == JsTsEntryKind::Class
            }) {
                let class_key = format!("box:jsts:class:{}", class_entry.name);
                let (open, row) =
                    self.js_ts_folder_row(ui, &class_key, class_entry.name, i16::CLASS, 1);
                let tip = js_ts_entry_tooltip(class_entry);
                let row = nf::tooltip(row, tip.trim_end_matches('.'), None);
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
        let tip = js_ts_entry_tooltip(entry);
        let row = nf::tooltip(row.row, tip.trim_end_matches('.'), None);
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
        let tip = format!(
            "import \"./{path}\";\n[Project module]\nProject module ({path})\nDouble-click to insert ES module import"
        );
        let row = nf::tooltip(row.row, &tip, None);
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
            let icon = match self.effective_actions_location() {
                Some(ScriptLocation::Source(_)) => i16::FILE_CODE,
                Some(ScriptLocation::Object { .. }) => i16::MOVIECLIP,
                _ => i16::FRAME,
            };
            nf::TreeItem::new(current_label, TreeItemType::Symbol(icon))
                .depth(1)
                .selected(true)
                .show(ui);
        }
        let mut chosen = None;
        for (idx, scene_id) in self.project.scene_ids().into_iter().enumerate() {
            let scene_name = self
                .project
                .symbol(&scene_id)
                .map_or("Scene", |s| s.name.as_str())
                .to_owned();
            let folder_key = if idx == 0 {
                "nav:root".to_owned()
            } else {
                format!("nav:scene:{scene_id}")
            };
            if self
                .js_ts_folder_row(ui, &folder_key, &scene_name, i16::SCENE, 0)
                .0
            {
                for (label, icon, location) in self.js_ts_navigator_entries(&scene_id) {
                    chosen = chosen.or(self.js_ts_navigator_leaf(ui, &label, icon, &location, 1));
                }
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
                .filter(|s| !self.project.is_scene(&s.id))
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
        let current = self.effective_actions_location().as_ref() == Some(location);
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
        let mut frame_scripts: Vec<_> = symbol
            .timeline
            .scripts
            .iter()
            .filter(|s| !s.script.source.trim().is_empty())
            .collect();
        frame_scripts.sort_by_key(|script| script.frame);
        let mut entries: Vec<(String, Icon, ScriptLocation)> = frame_scripts
            .into_iter()
            .map(|script| {
                let layer = script.layer.clone().or_else(|| {
                    symbol
                        .timeline
                        .action_layer_for_script(script)
                        .and_then(|index| symbol.timeline.layers.get(index))
                        .map(|layer| layer.id.clone())
                });
                let layer_name = layer
                    .as_deref()
                    .and_then(|id| symbol.timeline.layers.iter().find(|item| item.id == id))
                    .map(|layer| layer.name.as_str());
                (
                    layer_name.map_or_else(
                        || format!("{} : Frame {}", symbol.name, script.frame + 1),
                        |name| format!("{name} : Frame {}", script.frame + 1),
                    ),
                    i16::FRAME,
                    ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        layer,
                        frame: script.frame,
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

    #[test]
    fn actions_live_debugger_conditional_breakpoints_watch_expressions_and_variable_editing() {
        use crate::sources::{BreakpointSpec, DebuggerScope, DebuggerVariable};

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var score:Number = 0;\nif (score >= 10) {\n    trace(score);\n}\n".into(),
            },
        });
        let mut editor = Editor::from_project(&ctx, project);
        let tab_key = format!("{} : Frame 1", editor.symbol().name);

        // 1. Configure conditional breakpoint with hit-count threshold in the open script tab
        assert!(editor.toggle_breakpoint(&tab_key, 3));
        assert!(editor.set_breakpoint_condition(&tab_key, 3, Some("score >= 10".into())));
        assert!(editor.set_breakpoint_hit_count_target(&tab_key, 3, Some(2)));
        assert_eq!(
            editor.breakpoint_spec(&tab_key, 3),
            Some(
                BreakpointSpec::new(3)
                    .with_condition("score >= 10")
                    .with_hit_count_target(2)
            )
        );

        // 2. Stepping while condition is false (`score == 5`) does not pause and does not increment hit count
        assert!(!editor.evaluate_debugger_step(
            &tab_key,
            3,
            vec![DebuggerScope {
                name: "Local".into(),
                variables: vec![
                    DebuggerVariable::scalar("score", "5", "Number"),
                    DebuggerVariable::object(
                        "player",
                        "Object",
                        vec![DebuggerVariable::object(
                            "stats",
                            "Object",
                            vec![DebuggerVariable::scalar("hp", "80", "Number")],
                        )],
                    ),
                ],
            }],
        ));
        assert_eq!(editor.breakpoint_spec(&tab_key, 3).unwrap().hit_count, 0);

        // 3. First hit where condition is true (`score == 12`) increments hit_count to 1, below target 2 -> no pause
        assert!(!editor.evaluate_debugger_step(
            &tab_key,
            3,
            vec![DebuggerScope {
                name: "Local".into(),
                variables: vec![
                    DebuggerVariable::scalar("score", "12", "Number"),
                    DebuggerVariable::object(
                        "player",
                        "Object",
                        vec![DebuggerVariable::object(
                            "stats",
                            "Object",
                            vec![DebuggerVariable::scalar("hp", "80", "Number")],
                        )],
                    ),
                ],
            }],
        ));
        assert_eq!(editor.breakpoint_spec(&tab_key, 3).unwrap().hit_count, 1);

        // 4. Second hit where condition is true (`score == 15`) reaches hit_count_target == 2 -> pauses!
        assert!(editor.evaluate_debugger_step(
            &tab_key,
            3,
            vec![DebuggerScope {
                name: "Local".into(),
                variables: vec![
                    DebuggerVariable::scalar("score", "15", "Number"),
                    DebuggerVariable::object(
                        "player",
                        "Object",
                        vec![DebuggerVariable::object(
                            "stats",
                            "Object",
                            vec![DebuggerVariable::scalar("hp", "80", "Number")],
                        )],
                    ),
                ],
            }],
        ));
        assert_eq!(editor.breakpoint_spec(&tab_key, 3).unwrap().hit_count, 2);

        // Disabling the breakpoint prevents pausing even when condition & hit target are met
        assert!(editor.set_breakpoint_enabled(&tab_key, 3, false));
        assert!(!editor.should_pause_at_breakpoint(&tab_key, 3));
        assert!(editor.set_breakpoint_enabled(&tab_key, 3, true));

        // 5. Watch expressions & live variable editing through the Actions debugger UI
        editor.set_debugger_watch_draft("player.stats.hp + score");
        editor.set_debugger_variable_edit_draft("player.stats.hp", "100");

        let render = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ed.actions_contents(ui);
                    });
                },
            )
        };

        let out = render(&mut editor, vec![]);
        let find_button_center = |output: &egui::FullOutput, label: &str| -> egui::Pos2 {
            let b = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("expected button `{label}` in Actions debugger UI"))
                .1
                .bounds()
                .unwrap();
            egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32)
        };

        // Click "Add watch" button in the Actions debugger UI
        let add_pos = find_button_center(&out, "Add watch");
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(add_pos),
                egui::Event::PointerButton {
                    pos: add_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let out = render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: add_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.debugger_watches().len(), 1);
        assert_eq!(editor.debugger_watches()[0].last_value, "95");

        // Click "Set value" button in the Actions debugger UI to mutate `player.stats.hp` to 100
        let set_pos = find_button_center(&out, "Set value");
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(set_pos),
                egui::Event::PointerButton {
                    pos: set_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: set_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            editor
                .inspect_debugger_variable_path(0, "player.stats.hp")
                .unwrap()
                .value,
            "100"
        );
        assert_eq!(editor.debugger_watches()[0].last_value, "115");
        editor.project.validate().unwrap();
    }

    #[test]
    fn actions_code_folding_and_code_assist_triggers_end_to_end() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let src = "function update():void {\n    if (true) {\n        trace(1);\n    }\n}\n";
        let mut project = Project::blank();
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: src.into(),
            },
        });
        let mut editor = Editor::from_project(&ctx, project);

        let render = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 640.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ed.actions_contents(ui);
                    });
                },
            )
        };

        let out = render(&mut editor, vec![]);
        let button_center = |output: &egui::FullOutput, label: &str| -> egui::Pos2 {
            let b = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("expected `{label}` button in Actions toolbar"))
                .1
                .bounds()
                .unwrap();
            egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32)
        };

        // 1. Click "Collapse between braces" in the Actions toolbar
        let collapse_pos = button_center(&out, "Collapse between braces");
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(collapse_pos),
                egui::Event::PointerButton {
                    pos: collapse_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let out = render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: collapse_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!editor.folded_lines().is_empty());
        assert!(editor.visible_source_lines(src).len() < src.lines().count());

        // 2. Click "Expand all" in the Actions toolbar
        let expand_pos = button_center(&out, "Expand all");
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(expand_pos),
                egui::Event::PointerButton {
                    pos: expand_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let out = render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: expand_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(editor.folded_lines().is_empty());

        // 3. Click "Show code hint" in the Actions toolbar and insert via Script assist
        let hint_pos = button_center(&out, "Show code hint");
        let _ = render(
            &mut editor,
            vec![
                egui::Event::PointerMoved(hint_pos),
                egui::Event::PointerButton {
                    pos: hint_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = render(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: hint_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            editor.insert_script_assist_snippet(crate::sources::ScriptAssistTemplate::StopTimeline)
        );
        assert!(
            editor.project.symbols[0].timeline.scripts[0]
                .script
                .source
                .ends_with("stop();\n")
        );
        editor.project.validate().unwrap();
    }

    fn key_and_text_events(ch: char) -> Vec<egui::Event> {
        let key = match ch.to_ascii_lowercase() {
            'a' => Some(egui::Key::A),
            'b' => Some(egui::Key::B),
            'c' => Some(egui::Key::C),
            'd' => Some(egui::Key::D),
            'e' => Some(egui::Key::E),
            'f' => Some(egui::Key::F),
            'g' => Some(egui::Key::G),
            'h' => Some(egui::Key::H),
            'i' => Some(egui::Key::I),
            'j' => Some(egui::Key::J),
            'k' => Some(egui::Key::K),
            'l' => Some(egui::Key::L),
            'm' => Some(egui::Key::M),
            'n' => Some(egui::Key::N),
            'o' => Some(egui::Key::O),
            'p' => Some(egui::Key::P),
            'q' => Some(egui::Key::Q),
            'r' => Some(egui::Key::R),
            's' => Some(egui::Key::S),
            't' => Some(egui::Key::T),
            'u' => Some(egui::Key::U),
            'v' => Some(egui::Key::V),
            'w' => Some(egui::Key::W),
            'x' => Some(egui::Key::X),
            'y' => Some(egui::Key::Y),
            'z' => Some(egui::Key::Z),
            '/' => Some(egui::Key::Slash),
            _ => None,
        };
        let mut out = Vec::new();
        if let Some(key) = key {
            out.push(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: if ch.is_ascii_uppercase() {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
            });
        }
        out.push(egui::Event::Text(ch.to_string()));
        out
    }

    fn esc_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn actions_esc_chords_real_input_all_50_popup_dismiss_cancellation_and_one_step_undo() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let mut editor = Editor::from_project(&ctx, project);

        let run_actions =
            |ed: &mut Editor, time: Option<f64>, events: Vec<egui::Event>| -> egui::FullOutput {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 640.0),
                        )),
                        time,
                        focused: true,
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ed.actions_contents(ui);
                        });
                        crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
                    },
                )
            };

        // Focus the Actions editor and run an initial settle frame so focus lock is registered.
        let editor_id = editor.actions_editor_id();
        ctx.memory_mut(|m| m.request_focus(editor_id));
        let _ = run_actions(&mut editor, Some(1.0), vec![]);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));

        // 1. `<ESC>` -> `f` -> `n`: keeps focus, shows prompts, leaves zero literal `"fn"`,
        //    inserts `function name ( ) {\n\t\n}`, selects `name`, and undoes in 1 step!
        let _ = run_actions(&mut editor, Some(1.1), vec![esc_event()]);
        assert!(
            ctx.memory(|m| m.has_focus(editor_id)),
            "Escape must NOT blur the Actions code editor"
        );
        assert_eq!(editor.status, crate::code_tools::ESC_CHORD_ARMED_PROMPT);
        assert!(editor.symbol().timeline.scripts.is_empty());
        assert!(!editor.history.can_undo());

        let _ = run_actions(&mut editor, Some(1.2), key_and_text_events('f'));
        assert!(ctx.memory(|m| m.has_focus(editor_id)));
        assert_eq!(
            editor.status,
            "ESC f_: fn (function), fr (for), fi (for..in), fs (fscommand)"
        );
        assert!(
            editor.symbol().timeline.scripts.is_empty(),
            "first chord key `f` must be consumed before TextEdit"
        );

        let _ = run_actions(&mut editor, Some(1.3), key_and_text_events('n'));
        assert!(ctx.memory(|m| m.has_focus(editor_id)));
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "function name ( ) {\n\t\n}"
        );
        let range = egui::text_edit::TextEditState::load(&ctx, editor_id)
            .unwrap()
            .cursor
            .char_range()
            .unwrap();
        let (sel_lo, sel_hi) = (
            range.secondary.index.min(range.primary.index),
            range.secondary.index.max(range.primary.index),
        );
        assert_eq!(
            &editor.symbol().timeline.scripts[0].script.source[sel_lo..sel_hi],
            "name"
        );

        // One-step Undo reverts the entire `<ESC>fn` insertion cleanly!
        assert!(editor.history.can_undo());
        editor.undo();
        assert!(editor.symbol().timeline.scripts.is_empty());
        assert!(!editor.history.can_undo());
        let _ = run_actions(&mut editor, Some(1.4), vec![]);

        // 2. Indented `<ESC>tr` and `<ESC>//`, plus typing over the highlighted placeholder!
        let _ = run_actions(
            &mut editor,
            Some(1.5),
            vec![egui::Event::Text("    ".into())],
        );
        let _ = run_actions(&mut editor, Some(1.6), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(1.7), key_and_text_events('t'));
        let _ = run_actions(&mut editor, Some(1.8), key_and_text_events('r'));
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "    trace ( message );"
        );
        // Typing `"\"ok\""` replaces the selected `message` placeholder directly.
        let _ = run_actions(
            &mut editor,
            Some(1.9),
            vec![egui::Event::Text("\"ok\"".into())],
        );
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "    trace ( \"ok\" );"
        );

        // 3. Invalid chord cancellation (`<ESC>fz`, `<ESC>z`, `<ESC>f<ESC>`, and >5s timeout).
        let snapshot = editor.symbol().timeline.scripts[0].script.source.clone();
        let _ = run_actions(&mut editor, Some(2.0), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(2.1), key_and_text_events('f'));
        let _ = run_actions(&mut editor, Some(2.2), key_and_text_events('z'));
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, snapshot);

        let _ = run_actions(&mut editor, Some(2.3), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(2.4), key_and_text_events('z'));
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, snapshot);

        let _ = run_actions(&mut editor, Some(2.5), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(2.6), key_and_text_events('f'));
        let _ = run_actions(&mut editor, Some(2.7), vec![esc_event()]);
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, snapshot);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));

        // Timeout expiration (> 5.0s): arming at t = 3.0 expires before t = 9.0, so `'x'` types normally.
        let _ = run_actions(&mut editor, Some(3.0), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(9.0), key_and_text_events('x'));
        assert!(
            editor.symbol().timeline.scripts[0]
                .script
                .source
                .contains('x')
        );

        // 4. Popup-dismiss-before-chord: open code hints (`ShowHints`), press `Escape` once ->
        //    dismisses hint popup while keeping focus and NOT arming `<ESC>` chord; second `Escape` arms!
        editor.transact(|ed| ed.symbol_mut().timeline.scripts.clear());
        ctx.memory_mut(|m| m.request_focus(editor_id));
        editor.actions_ui.pending = Some(CodeCommand::ShowHints);
        let _ = run_actions(&mut editor, Some(10.0), vec![]);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));

        // First Escape closes the code-hint popup without arming `<ESC>` chord.
        let _ = run_actions(&mut editor, Some(10.1), vec![esc_event()]);
        assert!(
            ctx.memory(|m| m.has_focus(editor_id)),
            "dismissing code-hint popup with Escape must retain editor focus"
        );
        let _ = run_actions(&mut editor, Some(10.2), key_and_text_events('f'));
        let _ = run_actions(&mut editor, Some(10.3), key_and_text_events('n'));
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "fn",
            "after popup-dismiss Escape, typing `fn` without a second Escape types literal `fn`"
        );

        // Second Escape (now with no popup open) arms `<ESC>` chord!
        editor.transact(|ed| ed.symbol_mut().timeline.scripts.clear());
        ctx.memory_mut(|m| m.request_focus(editor_id));
        let _ = run_actions(&mut editor, Some(10.4), vec![]);
        let _ = run_actions(&mut editor, Some(10.5), vec![esc_event()]);
        let _ = run_actions(&mut editor, Some(10.6), key_and_text_events('/'));
        let _ = run_actions(&mut editor, Some(10.7), key_and_text_events('/'));
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, "// ");

        // 5. Exercise all 50 chords end-to-end through real `egui::Context` key+text events!
        for (i, spec) in crate::code_tools::ESC_CHORDS.iter().enumerate() {
            editor.transact(|ed| ed.symbol_mut().timeline.scripts.clear());
            let id = editor.actions_editor_id();
            ctx.memory_mut(|m| m.request_focus(id));
            let base_t = 20.0 + (i as f64);
            let _ = run_actions(&mut editor, Some(base_t), vec![]);
            let _ = run_actions(&mut editor, Some(base_t + 0.1), vec![esc_event()]);
            let _ = run_actions(
                &mut editor,
                Some(base_t + 0.2),
                key_and_text_events(spec.keys[0]),
            );
            let _ = run_actions(
                &mut editor,
                Some(base_t + 0.3),
                key_and_text_events(spec.keys[1]),
            );
            assert_eq!(
                editor.symbol().timeline.scripts[0].script.source,
                spec.template,
                "end-to-end chord ESC+{}{}",
                spec.keys[0],
                spec.keys[1]
            );
        }

        // 6. Verify full `Editor::shortcuts(ctx)` + `dock_grid(ctx)` + `dialog_helpers` pipeline
        //    AND `ScriptTarget::Source` (`Project files`): `Escape` never swallows/blurs/clears
        //    Stage selection when Actions/Source `TextEdit` has focus!
        editor.project.sources.push(SourceUnit {
            id: "main-src".into(),
            path: "Main.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: String::new(),
        });
        editor.project.enable_program();
        editor.script_target = ScriptTarget::Source;
        editor.selected_source = Some("main-src".into());
        editor.show_panel(PanelId::Actions);
        let src_editor_id = editor.actions_editor_id();
        ctx.memory_mut(|m| m.request_focus(src_editor_id));

        let run_full_dock = |ed: &mut Editor, t: f64, events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    time: Some(t),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ed.shortcuts(ctx);
                    ed.prepare_docked_panel_focus(ctx);
                    ed.dock_grid(ctx);
                    crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
                },
            );
        };

        run_full_dock(&mut editor, 80.0, vec![]);
        assert!(ctx.memory(|m| m.has_focus(src_editor_id)));
        run_full_dock(&mut editor, 80.1, vec![esc_event()]);
        assert!(ctx.memory(|m| m.has_focus(src_editor_id)));
        run_full_dock(&mut editor, 80.2, key_and_text_events('f'));
        run_full_dock(&mut editor, 80.3, key_and_text_events('n'));
        assert_eq!(
            editor.project.sources[0].source,
            "function name ( ) {\n\t\n}"
        );

        // 7. Regression (`#125` #1): `⌘Z` inside focused `Actions` `TextEdit` right after
        //    un-stabilized (< 1.0s) typing + `<ESC>fn` MUST preserve the typed text instead of
        //    wiping `undoer.flux` back to `""`!
        editor.script_target = ScriptTarget::Frame;
        editor.transact(|ed| ed.symbol_mut().timeline.scripts.clear());
        let frame_editor_id = editor.actions_editor_id();
        ctx.memory_mut(|m| m.request_focus(frame_editor_id));
        // Let the cleared `""` baseline stabilize (>= 1.0s) in `TextEdit`'s `Undoer` before
        // starting the rapid `< 1.0s` typing + `<ESC>fn` + `Cmd+Z` sequence.
        run_full_dock(&mut editor, 88.5, vec![]);
        run_full_dock(&mut editor, 90.0, vec![]);
        assert!(ctx.memory(|m| m.has_focus(frame_editor_id)));

        run_full_dock(
            &mut editor,
            90.1,
            vec![egui::Event::Text("var x = 1;\n".into())],
        );
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "var x = 1;\n"
        );
        run_full_dock(&mut editor, 90.2, vec![esc_event()]);
        run_full_dock(&mut editor, 90.3, key_and_text_events('f'));
        run_full_dock(&mut editor, 90.4, key_and_text_events('n'));
        assert_eq!(
            editor.symbol().timeline.scripts[0].script.source,
            "var x = 1;\nfunction name ( ) {\n\t\n}"
        );
        assert!(ctx.memory(|m| m.has_focus(frame_editor_id)));

        let run_cmd_z = |ed: &mut Editor, t: f64| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    time: Some(t),
                    focused: true,
                    modifiers: egui::Modifiers::COMMAND,
                    events: vec![egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: Some(egui::Key::Z),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    ed.shortcuts(ctx);
                    ed.prepare_docked_panel_focus(ctx);
                    ed.dock_grid(ctx);
                    crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
                },
            );
        };

        run_cmd_z(&mut editor, 90.5);
        assert_eq!(
            editor
                .symbol()
                .timeline
                .scripts
                .first()
                .map(|s| s.script.source.as_str()),
            Some("var x = 1;\n"),
            "first Cmd+Z inside focused Actions must revert only the ESC chord and preserve <1.0s typing"
        );
        run_cmd_z(&mut editor, 90.6);
        assert!(
            editor.symbol().timeline.scripts.is_empty(),
            "second Cmd+Z reverts the pre-chord typing cleanly"
        );
        editor.project.validate().unwrap();
    }

    #[test]
    fn actions_window_level_policy_and_cursor_icon_enforcement() {
        use crate::workspace::{DockEdge, PanelPlacement};
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::rc::Rc;

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_embed_viewports(false);
        let observed_levels: Rc<RefCell<HashMap<egui::ViewportId, Option<egui::WindowLevel>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let observed_cmds: Rc<RefCell<HashMap<egui::ViewportId, Vec<egui::ViewportCommand>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let child_outputs: Rc<RefCell<HashMap<egui::ViewportId, egui::FullOutput>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let pending_child_events: Rc<RefCell<HashMap<egui::ViewportId, Vec<egui::Event>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let register_rgba_on_vp: Rc<RefCell<Option<(egui::ViewportId, egui::Id)>>> =
            Rc::new(RefCell::new(None));
        let levels_cb = observed_levels.clone();
        let cmds_cb = observed_cmds.clone();
        let outs_cb = child_outputs.clone();
        let events_cb = pending_child_events.clone();
        let rgba_cb = register_rgba_on_vp.clone();
        egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
            let id = viewport.ids.this;
            levels_cb
                .borrow_mut()
                .insert(id, viewport.builder.window_level);
            let events = events_cb.borrow_mut().remove(&id).unwrap_or_default();
            let mut input = egui::RawInput {
                viewport_id: id,
                focused: !events.is_empty(),
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 620.0),
                )),
                ..Default::default()
            };
            input.viewports.insert(
                id,
                egui::ViewportInfo {
                    parent: Some(egui::ViewportId::ROOT),
                    outer_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(80.0, 80.0),
                        egui::vec2(640.0, 620.0),
                    )),
                    inner_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(80.0, 80.0),
                        egui::vec2(640.0, 620.0),
                    )),
                    ..Default::default()
                },
            );
            let register_rgba = rgba_cb
                .borrow()
                .and_then(|(vp, popup_id)| (vp == id).then_some(popup_id));
            let child_out = ctx.run(input, |ctx| {
                if let Some(popup_id) = register_rgba {
                    crate::panels::dialog_helpers::track_rgba_popup(ctx, popup_id);
                }
                (viewport.viewport_ui_cb)(ctx);
            });
            if let Some(vo) = child_out.viewport_output.get(&id) {
                cmds_cb.borrow_mut().insert(id, vo.commands.clone());
            }
            outs_cb.borrow_mut().insert(id, child_out);
        });

        let mut editor = Editor::from_project(&ctx, Project::blank());

        // Detach Actions (workspace/code window) AND Tools + Color (compact utility palettes).
        editor.set_panel_visible(PanelId::Actions, true);
        editor.set_panel_placement(&ctx, PanelId::Actions, PanelPlacement::Detached);
        editor.set_panel_visible(PanelId::Tools, true);
        editor.set_panel_placement(&ctx, PanelId::Tools, PanelPlacement::Detached);
        editor.set_panel_visible(PanelId::Color, true);
        editor.set_panel_placement(&ctx, PanelId::Color, PanelPlacement::Detached);

        let actions_vp = egui::ViewportId::from_hash_of(PanelId::Actions.viewport_key().unwrap());
        let tools_vp = egui::ViewportId::from_hash_of(PanelId::Tools.viewport_key().unwrap());
        let color_vp = egui::ViewportId::from_hash_of(PanelId::Color.viewport_key().unwrap());

        let step_detached = |ed: &mut Editor| -> egui::FullOutput {
            observed_levels.borrow_mut().clear();
            observed_cmds.borrow_mut().clear();
            child_outputs.borrow_mut().clear();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 900.0),
                    )),
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    ed.sync_closed_detached_panels(ctx);
                    ed.detached_panels(ctx);
                },
            )
        };

        // 1. Idle state: Actions MUST be `WindowLevel::Normal` (never `AlwaysOnTop`),
        //    while compact palettes (`Tools`, `Color`) are `WindowLevel::AlwaysOnTop`.
        crate::native_windows::set_native_menu_tracking_for_test(false);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&actions_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert_eq!(
            observed_levels.borrow().get(&color_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert!(!crate::native_windows::window_should_float_for_title(Some(
            "Actions — NeoFlash"
        )));
        assert!(crate::native_windows::window_should_float_for_title(Some(
            "Tools — NeoFlash"
        )));
        assert!(crate::native_windows::window_should_float_for_title(Some(
            "Color — NeoFlash"
        )));
        assert!(!crate::native_windows::should_order_below_root_for_test());

        // 2. Opening Scripting Reference (`Help`) lowers ALL floating windows (even `Tools`/`Color`)
        //    to `Normal` AND orders them below `root`!
        crate::panels::help_manual::open_scripting_reference(&ctx);
        let out = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&actions_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert_eq!(
            observed_levels.borrow().get(&color_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        let tools_got_normal_cmd =
            out.viewport_output.get(&tools_vp).is_some_and(|vo| {
                vo.commands.contains(&egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ))
            }) || observed_cmds.borrow().get(&tools_vp).is_some_and(|cmds| {
                cmds.contains(&egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ))
            });
        assert!(tools_got_normal_cmd);
        assert!(!crate::native_windows::window_should_float_for_title(Some(
            "Tools — NeoFlash"
        )));
        assert!(crate::native_windows::should_order_below_root_for_test());

        // Closing Help restores `Tools` and `Color` to `AlwaysOnTop`, keeping `Actions` at `Normal`.
        crate::panels::help_manual::close_help_manual(&ctx);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&actions_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert!(!crate::native_windows::should_order_below_root_for_test());

        // 3. Modal dialogs and root-viewport menus lower floating palettes AND set `order_below_root`;
        //    native macOS `NSMenu` tracking lowers floating palettes WITHOUT setting `order_below_root`
        //    (`#125` #3: never bury normal-level detached `Actions` behind `root` on menu bar clicks!).
        editor.show_about = true;
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert!(crate::native_windows::should_order_below_root_for_test());
        editor.show_about = false;

        editor.clipboard.menu_open = true;
        editor.clipboard.menu_viewport = None;
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert!(crate::native_windows::should_order_below_root_for_test());

        // Context menu on a floating palette itself does NOT masquerade as a root menu!
        editor.clipboard.menu_viewport = Some(tools_vp);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert!(!crate::native_windows::should_order_below_root_for_test());
        editor.clipboard.menu_open = false;
        editor.clipboard.menu_viewport = None;

        crate::native_windows::set_native_menu_tracking_for_test(true);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert!(!crate::native_windows::window_should_float_for_title(Some(
            "Tools — NeoFlash"
        )));
        assert!(
            !crate::native_windows::should_order_below_root_for_test(),
            "native macOS NSMenu tracking must lower floating palettes without ordering detached Actions below root"
        );
        crate::native_windows::set_native_menu_tracking_for_test(false);

        // 3b. Regression (`#125` #2): Popovers opened INSIDE detached palettes (`Tools` modifier
        //     popover, `Color` RGBA popover) MUST keep `Tools` and `Color` at `AlwaysOnTop` and
        //     `order_below_root == false`, whereas root-viewport popovers lower them to `Normal`!
        editor.tool = crate::app::Tool::Pencil;
        let _ = step_detached(&mut editor);
        let pencil_mode_center = child_outputs
            .borrow()
            .get(&tools_vp)
            .and_then(|out| {
                out.platform_output
                    .accesskit_update
                    .as_ref()?
                    .nodes
                    .iter()
                    .find_map(|(_, node)| {
                        (node.label() == Some("Pencil mode"))
                            .then(|| node.bounds())
                            .flatten()
                            .map(|b| {
                                egui::pos2(
                                    ((b.x0 + b.x1) * 0.5) as f32,
                                    ((b.y0 + b.y1) * 0.5) as f32,
                                )
                            })
                    })
            })
            .expect("detached Tools viewport exposes Pencil mode modifier button");
        let click_in_tools = |pos: egui::Pos2, ed: &mut Editor| {
            pending_child_events.borrow_mut().insert(
                tools_vp,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let _ = step_detached(ed);
            pending_child_events.borrow_mut().insert(
                tools_vp,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            let _ = step_detached(ed);
        };
        click_in_tools(pencil_mode_center, &mut editor);
        assert!(
            crate::panels::tools::is_rail_popover_open(&ctx),
            "clicking Pencil mode on detached Tools opens its RailPopover"
        );
        // Next frame with detached `Tools` popover open: `Tools` & `Color` MUST stay `AlwaysOnTop`!
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert_eq!(
            observed_levels.borrow().get(&color_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert!(crate::native_windows::window_should_float_for_title(Some(
            "Tools — NeoFlash"
        )));
        assert!(!crate::native_windows::should_order_below_root_for_test());

        // If `Tools` is docked on the root window while its RailPopover is open, it IS a root-viewport
        // popover and MUST lower detached floating palettes (`Color`) to `Normal`!
        editor.set_panel_placement(&ctx, PanelId::Tools, PanelPlacement::Docked);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&color_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert!(crate::native_windows::should_order_below_root_for_test());

        // Re-detach `Tools` and close its Pencil mode popover cleanly.
        editor.set_panel_placement(&ctx, PanelId::Tools, PanelPlacement::Detached);
        let _ = step_detached(&mut editor);
        if crate::panels::tools::is_rail_popover_open(&ctx) {
            click_in_tools(pencil_mode_center, &mut editor);
        }
        assert!(!crate::panels::tools::is_rail_popover_open(&ctx));
        editor.tool = crate::app::Tool::Select;

        // Tracked RGBA colour popover inside detached `Color` vs `ViewportId::ROOT`:
        let rgba_id = egui::Id::new("test-floating-rgba");
        ctx.data_mut(|d| d.insert_temp(rgba_id.with("nf2-rgba-popup-open"), true));
        *register_rgba_on_vp.borrow_mut() = Some((color_vp, rgba_id));
        let _ = step_detached(&mut editor);
        assert!(crate::panels::dialog_helpers::is_any_tracked_rgba_popup_open(&ctx));
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::AlwaysOnTop)
        );
        assert!(!crate::native_windows::should_order_below_root_for_test());

        // Moving that RGBA popover ownership to `ViewportId::ROOT` lowers floating palettes!
        *register_rgba_on_vp.borrow_mut() = None;
        crate::panels::dialog_helpers::track_rgba_popup(&ctx, rgba_id);
        let _ = step_detached(&mut editor);
        assert_eq!(
            observed_levels.borrow().get(&tools_vp).copied().flatten(),
            Some(egui::WindowLevel::Normal)
        );
        assert!(crate::native_windows::should_order_below_root_for_test());
        ctx.data_mut(|d| d.insert_temp(rgba_id.with("nf2-rgba-popup-open"), false));
        crate::panels::dialog_helpers::record_end_of_frame_focus(&ctx);

        // 4. Dock `Actions` into the bottom dock (right below the horizontal dock splitter) and
        //    verify `CursorIcon::Text` (`IBeam`) is enforced over the editable code body even
        //    after splitter hover, stale `PointingHand` / `Grab` / `ResizeVertical` cursors, or
        //    while a popover is open on a detached palette!
        editor.set_panel_placement(&ctx, PanelId::Actions, PanelPlacement::Docked);
        editor.show_panel(PanelId::Actions);
        editor.workspace.dock.area_mut(DockEdge::Bottom).size = 260.0;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                focused: true,
                ..Default::default()
            },
            |ctx| {
                editor.dock_grid(ctx);
            },
        );

        // Hover over the bottom dock's top resize grip -> `ResizeVertical`.
        let grip_y = 800.0 - 260.0 + 2.0;
        let out_grip = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                focused: true,
                events: vec![egui::Event::PointerMoved(egui::pos2(640.0, grip_y))],
                ..Default::default()
            },
            |ctx| {
                editor.dock_grid(ctx);
            },
        );
        assert_eq!(
            out_grip.platform_output.cursor_icon,
            egui::CursorIcon::ResizeVertical
        );

        // Move pointer down into the `Actions` editable text body (and pre-poison stale cursors,
        // even while an RGBA popover is open on detached `Color`!):
        // cursor MUST be `CursorIcon::Text` both on hover and while typing!
        ctx.data_mut(|d| d.insert_temp(rgba_id.with("nf2-rgba-popup-open"), true));
        *register_rgba_on_vp.borrow_mut() = Some((color_vp, rgba_id));
        let _ = step_detached(&mut editor);
        *register_rgba_on_vp.borrow_mut() = None;

        let code_body_pos = egui::pos2(720.0, 700.0);
        for stale in [
            egui::CursorIcon::ResizeVertical,
            egui::CursorIcon::Grab,
            egui::CursorIcon::PointingHand,
        ] {
            let out_hover = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    focused: true,
                    events: vec![egui::Event::PointerMoved(code_body_pos)],
                    ..Default::default()
                },
                |ctx| {
                    ctx.set_cursor_icon(stale);
                    editor.dock_grid(ctx);
                },
            );
            assert_eq!(
                out_hover.platform_output.cursor_icon,
                egui::CursorIcon::Text,
                "stale {stale:?} must be overridden to IBeam inside Actions code body"
            );
        }
        ctx.data_mut(|d| d.insert_temp(rgba_id.with("nf2-rgba-popup-open"), false));
        crate::panels::dialog_helpers::record_end_of_frame_focus(&ctx);

        // Focus editor and type characters while pointer rests inside the code body (no PointerMoved event).
        let editor_id = editor.actions_editor_id();
        ctx.memory_mut(|m| m.request_focus(editor_id));
        let out_typing = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                focused: true,
                events: key_and_text_events('a'),
                ..Default::default()
            },
            |ctx| {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                editor.dock_grid(ctx);
            },
        );
        assert_eq!(
            out_typing.platform_output.cursor_icon,
            egui::CursorIcon::Text
        );
    }

    #[test]
    fn t364_pinnable_script_tabs_decouple_selection_switch_in_one_click_and_prune_deleted() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        // Add two editable Stage elements (`hero_mc` and `enemy_mc`), Frame 1 & Frame 2 scripts, and `Main.as`.
        project.symbols[0].timeline.frame_count = 5;
        let root_layer_id = project.symbols[0].timeline.layers[0].id.clone();
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: Some(root_layer_id.clone()),
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "stop();".into(),
            },
        });
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: Some(root_layer_id.clone()),
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "play();".into(),
            },
        });
        let mut hero = Element {
            id: "el-hero".into(),
            name: "hero_mc".into(),
            kind: ElementKind::Rectangle {
                width: 60.0,
                height: 40.0,
                fill: [40, 140, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        hero.scripts.push(ObjectScript {
            event: ObjectEvent::Load,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "var speed = 5;".into(),
            },
        });
        let mut enemy = Element {
            id: "el-enemy".into(),
            name: "enemy_mc".into(),
            kind: ElementKind::Rectangle {
                width: 60.0,
                height: 40.0,
                fill: [220, 80, 60, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        enemy.scripts.push(ObjectScript {
            event: ObjectEvent::Load,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "var hp = 10;".into(),
            },
        });
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .extend([hero, enemy]);
        project.sources.push(SourceUnit {
            id: "src-main".into(),
            path: "Main.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: "class Main {}\n".into(),
        });
        project.enable_program();
        project.upgrade_format_for_features();

        let mut editor = Editor::from_project(&ctx, project);
        editor.show_panel(PanelId::Actions);
        editor.script_target = ScriptTarget::Frame;
        editor.frame = 0;

        let step = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 620.0),
                    )),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ed.prepare_docked_panel_focus(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| ed.actions_contents(ui));
                },
            )
        };
        let find_ax_rect =
            |out: &egui::FullOutput, label: &str, min_x: f32, min_y: f32| -> egui::Rect {
                let nodes = &out.platform_output.accesskit_update.as_ref().unwrap().nodes;
                nodes
                    .iter()
                    .find_map(|(_, n)| {
                        (n.label().or(n.value()) == Some(label))
                            .then(|| n.bounds())
                            .flatten()
                            .map(|r| {
                                egui::Rect::from_min_max(
                                    egui::pos2(r.x0 as f32, r.y0 as f32),
                                    egui::pos2(r.x1 as f32, r.y1 as f32),
                                )
                            })
                            .filter(|rect| rect.left() >= min_x && rect.top() >= min_y)
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "missing AX node {label:?} (min_x={min_x}, min_y={min_y}); labels={:?}",
                            nodes
                                .iter()
                                .filter_map(|(_, n)| n.label().or(n.value()))
                                .collect::<Vec<_>>()
                        )
                    })
            };
        let click_at = |ed: &mut Editor, pos: egui::Pos2, button: egui::PointerButton| {
            let _ = step(
                ed,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let _ = step(
                ed,
                vec![egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };

        // 1. Pin `Scene 1 : Frame 1` by clicking the `📌` icon on its bottom `ScriptTab`.
        let out0 = step(&mut editor, vec![]);
        let pin_frame1 = find_ax_rect(&out0, "Pin script tab: Scene 1 : Frame 1", 250.0, 575.0);
        click_at(
            &mut editor,
            pin_frame1.center(),
            egui::PointerButton::Primary,
        );
        let frame1_loc = ScriptLocation::Frame {
            symbol: editor.project.root.clone(),
            layer: Some(root_layer_id.clone()),
            frame: 0,
        };
        assert_eq!(editor.actions_ui.pinned_tabs, vec![frame1_loc.clone()]);
        assert_eq!(editor.actions_ui.active_pinned, Some(frame1_loc.clone()));

        // 2. Selecting `hero_mc` on Stage updates `current_script_location()` (showing unpinned
        //    `hero_mc : Load` in the bottom bar) WITHOUT hijacking `Actions` away from `Scene 1 : Frame 1`!
        editor.select_single_id("el-hero".into());
        editor.script_target = ScriptTarget::Object;
        editor.script_event = ObjectEvent::Load;
        let out_after_stage_sel = step(&mut editor, vec![]);
        assert_eq!(
            editor.effective_actions_location(),
            Some(frame1_loc.clone()),
            "Stage selection must not hijack active pinned tab"
        );
        // Both the pinned `Scene 1 : Frame 1` tab AND the unpinned `hero_mc : Load` tab appear in the bottom bar!
        let hero_unpinned_tab = find_ax_rect(&out_after_stage_sel, "hero_mc : Load", 250.0, 575.0);
        let _ = find_ax_rect(&out_after_stage_sel, "Scene 1 : Frame 1", 250.0, 575.0);

        // 3. Click unpinned `hero_mc : Load` tab body -> switches to live `hero_mc : Load`, then pin via toolbar `Pin script tab`!
        click_at(
            &mut editor,
            hero_unpinned_tab.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(editor.actions_ui.active_pinned, None);
        let hero_loc = ScriptLocation::Object {
            symbol: editor.project.root.clone(),
            frame: 0,
            element: "el-hero".into(),
            event: ObjectEvent::Load,
        };
        assert_eq!(editor.effective_actions_location(), Some(hero_loc.clone()));
        let out_hero = step(&mut editor, vec![]);
        let toolbar_pin = find_ax_rect(&out_hero, "Pin script tab", 0.0, 0.0);
        assert!(toolbar_pin.bottom() < 100.0);
        click_at(
            &mut editor,
            toolbar_pin.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(
            editor.actions_ui.pinned_tabs,
            vec![frame1_loc.clone(), hero_loc.clone()]
        );
        assert_eq!(editor.actions_ui.active_pinned, Some(hero_loc.clone()));

        // 4. Open `Main.as` via `open_script_location` (`Script navigator` gesture) and pin it via
        //    right-click context menu (`Pin tab`)!
        let main_loc = ScriptLocation::Source("src-main".into());
        editor.open_script_location(main_loc.clone());
        assert_eq!(editor.actions_ui.active_pinned, None);
        let out_main = step(&mut editor, vec![]);
        let main_tab = find_ax_rect(&out_main, "Main.as", 250.0, 575.0);
        click_at(
            &mut editor,
            main_tab.center(),
            egui::PointerButton::Secondary,
        );
        let out_ctx_menu = step(&mut editor, vec![]);
        let pin_menu_item = find_ax_rect(&out_ctx_menu, "Pin tab", 0.0, 0.0);
        click_at(
            &mut editor,
            pin_menu_item.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(
            editor.actions_ui.pinned_tabs,
            vec![frame1_loc.clone(), hero_loc.clone(), main_loc.clone()]
        );
        assert_eq!(editor.actions_ui.active_pinned, Some(main_loc.clone()));

        // 5. Deselect Stage AND select `enemy_mc` on Stage — then switch among all 3 pinned tabs
        //    (`hero_mc : Load`, `Scene 1 : Frame 1`, `Main.as`) in 1 click WITHOUT altering Stage
        //    selection, edit each tab independently, and verify per-tab Undo!
        editor.select_single_id("el-enemy".into());
        editor.script_target = ScriptTarget::Object;
        editor.script_event = ObjectEvent::Load;
        let out_three_pinned = step(&mut editor, vec![]);
        // Bottom bar has 3 pinned tabs + unpinned `enemy_mc : Load`!
        let _ = find_ax_rect(&out_three_pinned, "enemy_mc : Load", 250.0, 575.0);
        let hero_tab_rect = find_ax_rect(&out_three_pinned, "hero_mc : Load", 250.0, 575.0);

        // 1-click switch to `hero_mc : Load` while `enemy_mc` stays selected on Stage!
        click_at(
            &mut editor,
            hero_tab_rect.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(editor.effective_actions_location(), Some(hero_loc.clone()));
        assert_eq!(
            editor.selected_element().map(|e| e.id.as_str()),
            Some("el-enemy"),
            "switching pinned tab must never alter Stage selection"
        );
        let hero_ed_id = editor.actions_editor_id();
        ctx.memory_mut(|m| m.request_focus(hero_ed_id));
        let _ = step(&mut editor, vec![]);
        let _ = step(
            &mut editor,
            vec![egui::Event::Text("\ntrace(speed);".into())],
        );
        let hero_src = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| e.id == "el-hero")
            .unwrap()
            .scripts[0]
            .script
            .source
            .clone();
        assert!(hero_src.contains("trace(speed);"), "hero_src={hero_src:?}");
        // `enemy_mc`'s script is completely untouched!
        let enemy_src = &editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| e.id == "el-enemy")
            .unwrap()
            .scripts[0]
            .script
            .source;
        assert_eq!(enemy_src, "var hp = 10;");

        // Even with Stage completely deselected (`targets.clear()`), `hero_mc : Load` stays editable
        // and 1-click switching to `Scene 1 : Frame 1` & `Main.as` works seamlessly!
        editor.selection.targets.clear();
        let out_deselected = step(&mut editor, vec![]);
        let frame1_tab_rect = find_ax_rect(&out_deselected, "Scene 1 : Frame 1", 250.0, 575.0);
        click_at(
            &mut editor,
            frame1_tab_rect.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(
            editor.effective_actions_location(),
            Some(frame1_loc.clone())
        );
        let frame1_ed_id = editor.actions_editor_id();
        assert_ne!(frame1_ed_id, hero_ed_id);
        ctx.memory_mut(|m| m.request_focus(frame1_ed_id));
        let _ = step(&mut editor, vec![]);
        let _ = step(
            &mut editor,
            vec![egui::Event::Text("\nnextFrame();".into())],
        );
        assert!(
            editor.symbol().timeline.scripts[0]
                .script
                .source
                .contains("nextFrame();")
        );

        let out_frame1 = step(&mut editor, vec![]);
        let main_tab_rect = find_ax_rect(&out_frame1, "Main.as", 250.0, 575.0);
        click_at(
            &mut editor,
            main_tab_rect.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(editor.effective_actions_location(), Some(main_loc.clone()));
        let main_ed_id = editor.actions_editor_id();
        assert_ne!(main_ed_id, frame1_ed_id);
        ctx.memory_mut(|m| m.request_focus(main_ed_id));
        let _ = step(&mut editor, vec![]);
        let _ = step(&mut editor, vec![egui::Event::Text("// main\n".into())]);
        assert!(editor.project.sources[0].source.contains("// main"));

        // Undo reverts `Main.as`, then `Frame 1`, then `hero_mc`, keeping project valid!
        editor.undo();
        assert_eq!(editor.project.sources[0].source, "class Main {}\n");
        editor.undo();
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, "stop();");
        editor.undo();
        assert_eq!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .find(|e| e.id == "el-hero")
                .unwrap()
                .scripts[0]
                .script
                .source,
            "var speed = 5;"
        );
        editor.project.validate().unwrap();

        // 5b. Regression (`#136` #2): `insert_script_assist_snippet` routes to active pinned
        //     `Frame` and `Object` tabs without mutating live Stage/Timeline selection!
        let assist_tmpl = crate::sources::ScriptAssistTemplate::default_menu_items()
            .into_iter()
            .next()
            .unwrap()
            .1;
        editor.select_single_id("el-enemy".into());
        editor.script_target = ScriptTarget::Object;
        editor.frame = 1;
        editor.actions_ui.active_pinned = Some(frame1_loc.clone());
        assert!(editor.insert_script_assist_snippet(assist_tmpl.clone()));
        assert_ne!(editor.symbol().timeline.scripts[0].script.source, "stop();");
        assert_eq!(
            editor.symbol().timeline.scripts[1].script.source,
            "play();",
            "live Frame 2 must not be mutated when Frame 1 is active_pinned"
        );
        assert_eq!(
            editor.selected_element().map(|e| e.id.as_str()),
            Some("el-enemy")
        );
        editor.undo();
        assert_eq!(editor.symbol().timeline.scripts[0].script.source, "stop();");

        editor.selection.targets.clear();
        editor.actions_ui.active_pinned = Some(hero_loc.clone());
        assert!(
            editor.insert_script_assist_snippet(assist_tmpl),
            "pinned Object tab accepts Script assist even when Stage selection is empty"
        );
        assert_ne!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .find(|e| e.id == "el-hero")
                .unwrap()
                .scripts[0]
                .script
                .source,
            "var speed = 5;"
        );
        assert!(editor.selection.targets.is_empty());
        editor.undo();

        // 5c. Regression (`#136` #1): Undocking + redocking `Actions` through the real
        //     `prepare_docked_panel_focus` -> `detached_panels` order MUST preserve `active_pinned`!
        editor.select_single_id("el-enemy".into());
        editor.script_target = ScriptTarget::Object;
        editor.actions_ui.active_pinned = Some(frame1_loc.clone());
        ctx.set_embed_viewports(false);
        egui::Context::set_immediate_viewport_renderer(|ctx, mut viewport| {
            let id = viewport.ids.this;
            let mut input = egui::RawInput {
                viewport_id: id,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 420.0),
                )),
                ..Default::default()
            };
            input.viewports.insert(
                id,
                egui::ViewportInfo {
                    parent: Some(egui::ViewportId::ROOT),
                    outer_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(80.0, 80.0),
                        egui::vec2(640.0, 420.0),
                    )),
                    inner_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(80.0, 80.0),
                        egui::vec2(640.0, 420.0),
                    )),
                    ..Default::default()
                },
            );
            let _ = ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx));
        });
        editor.set_panel_placement(
            &ctx,
            PanelId::Actions,
            crate::workspace::PanelPlacement::Detached,
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 620.0),
                )),
                focused: true,
                ..Default::default()
            },
            |ctx| {
                editor.prepare_docked_panel_focus(ctx);
                editor.sync_closed_detached_panels(ctx);
                editor.detached_panels(ctx);
            },
        );
        assert_eq!(
            editor.actions_ui.active_pinned,
            Some(frame1_loc.clone()),
            "undocking Actions must preserve active_pinned across prepare_docked_panel_focus + detached_panels"
        );
        editor.set_panel_placement(
            &ctx,
            PanelId::Actions,
            crate::workspace::PanelPlacement::Docked,
        );
        let _ = step(&mut editor, vec![]);
        assert_eq!(
            editor.actions_ui.active_pinned,
            Some(frame1_loc.clone()),
            "redocking Actions must also preserve active_pinned"
        );

        // 6. Explicit `open_frame_actions()` on Frame 2 switches `Actions` to Frame 2 while keeping
        //    all 3 pinned tabs intact in the bottom bar!
        editor.frame = 1;
        editor.open_frame_actions();
        let _ = step(&mut editor, vec![]);
        assert_eq!(editor.actions_ui.active_pinned, None);
        assert_eq!(editor.actions_ui.pinned_tabs.len(), 3);
        assert_eq!(
            editor.effective_actions_location(),
            Some(ScriptLocation::Frame {
                symbol: editor.project.root.clone(),
                layer: Some(root_layer_id),
                frame: 1,
            })
        );

        // 7. Right-click `hero_mc : Load` -> `Unpin other tabs` keeps ONLY `hero_mc : Load`,
        //    and deleting `el-hero` prunes the stale pinned tab cleanly!
        let out_f2 = step(&mut editor, vec![]);
        let hero_rect = find_ax_rect(&out_f2, "hero_mc : Load", 250.0, 575.0);
        click_at(
            &mut editor,
            hero_rect.center(),
            egui::PointerButton::Secondary,
        );
        let out_menu2 = step(&mut editor, vec![]);
        let unpin_others = find_ax_rect(&out_menu2, "Unpin other tabs", 0.0, 0.0);
        click_at(
            &mut editor,
            unpin_others.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(editor.actions_ui.pinned_tabs, vec![hero_loc.clone()]);

        // Click `hero_mc : Load` to make it active, then delete `el-hero` from the document:
        let out_only_hero = step(&mut editor, vec![]);
        let hero_rect2 = find_ax_rect(&out_only_hero, "hero_mc : Load", 250.0, 575.0);
        click_at(
            &mut editor,
            hero_rect2.center(),
            egui::PointerButton::Primary,
        );
        assert_eq!(editor.actions_ui.active_pinned, Some(hero_loc));
        editor.transact(|ed| {
            ed.symbol_mut().timeline.layers[0].keyframes[0]
                .elements
                .retain(|e| e.id != "el-hero");
        });
        let _ = step(&mut editor, vec![]);
        assert!(editor.actions_ui.pinned_tabs.is_empty());
        assert_eq!(editor.actions_ui.active_pinned, None);
        editor.project.validate().unwrap();
    }
}
