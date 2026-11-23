use super::*;

impl Editor {
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
        let code_prefs = self
            .preferences
            .as_ref()
            .map(|d| d.code_editor.clone())
            .unwrap_or_else(|| self.workspace.code_editor.clone());
        self.fonts.ensure_loaded(ui.ctx(), &code_prefs.font);
        let custom_palette = (!code_prefs.syntax_colors.is_empty()).then(|| {
            code_prefs
                .syntax_colors
                .resolve_palette(neoflash_ui::palette(ui.ctx()))
        });
        nf::code::set_syntax_palette(ui.ctx(), custom_palette);
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
            if matches!(
                self.effective_actions_location(),
                Some(crate::panels::actions_sidebar::ScriptLocation::Source(_))
            ) {
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
        let has_modal_dialog = self.has_modal_dialog_in(ui.ctx())
            || crate::panels::help_manual::is_help_manual_open(ui.ctx());
        let esc_before_diag = ui.ctx().input(|i| i.key_pressed(egui::Key::Escape));
        let diag_open_before = diagnostic_ui.is_open();
        let action = if !has_modal_dialog && ui.is_enabled() {
            diagnostic_ui.before_editor(ui.ctx(), editor_id, source, &current_diagnostics)
        } else {
            diagnostic_ui.close();
            None
        };
        let esc_after_diag = ui.ctx().input(|i| i.key_pressed(egui::Key::Escape));
        let diagnostic_dismissed_escape = diag_open_before && esc_before_diag && !esc_after_diag;
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
        let markers: Vec<(usize, Severity)> = feedback
            .iter()
            .flat_map(|(_, _, diagnostics)| {
                diagnostics
                    .iter()
                    .filter(|d| self.diagnostic_is_current(d))
                    .map(|d| (d.line, diagnostic_severity(d.severity)))
            })
            .collect();
        let standalone_session = self.is_standalone_script_session();
        let breakpoint_lines: Vec<usize> = if standalone_session {
            Vec::new()
        } else {
            self.breakpoints_for(&tab_key)
                .into_iter()
                .filter(|bp| bp.enabled)
                .map(|bp| bp.line)
                .collect()
        };
        let bar_h = neoflash_ui::metrics::script_tab() + 2.;
        let raw_full = ui.available_rect_before_wrap();
        let bound_right = ui.max_rect().right().min(ui.clip_rect().right());
        let bound_bottom = ui.max_rect().bottom().min(ui.clip_rect().bottom());
        let full = egui::Rect::from_min_max(
            raw_full.min,
            egui::pos2(
                bound_right.max(raw_full.left()),
                bound_bottom.max(raw_full.top()),
            ),
        );
        let show_dbg = !standalone_session && self.actions_debugger_inspector_visible(&tab_key);
        let dbg_h = if show_dbg {
            (10. * neoflash_ui::metrics::row()).min(full.height() * 0.50)
        } else {
            0.0
        };
        let bar =
            egui::Rect::from_min_max(egui::pos2(full.left(), full.bottom() - bar_h), full.max);
        let dbg_rect = egui::Rect::from_min_max(
            egui::pos2(full.left(), bar.top() - dbg_h),
            egui::pos2(full.right(), bar.top()),
        );
        let gap = if dbg_h > 0. { space::s2() } else { 0. };
        let code = egui::Rect::from_min_max(
            full.min,
            egui::pos2(
                full.right(),
                (dbg_rect.top() - gap).max(full.top() + CODE_MIN_HEIGHT),
            ),
        );
        let hint_context = || self.hint_context(language);
        let paint_diagnostics = if *source == before_command {
            current_diagnostics.as_slice()
        } else {
            &[]
        };
        let host_state = CodeEditorHostState {
            has_modal_dialog,
            has_open_popup: crate::panels::dialog_helpers::is_editor_popup_open_on_viewport(
                self,
                ui.ctx(),
                ui.ctx().viewport_id(),
            ) || (ui.ctx().viewport_id() != egui::ViewportId::ROOT
                && self.detached.context_menu_open),
            has_active_drag: self.has_active_dock_or_window_drag(ui.ctx(), Some(editor_id)),
            diagnostic_dismissed_escape,
        };
        let (mut response, caret, action, gutter_click, chord_status) = ui
            .scope_builder(egui::UiBuilder::new().max_rect(code), |ui| {
                code_editor(
                    ui,
                    editor_id,
                    source,
                    hint,
                    &markers,
                    &breakpoint_lines,
                    &hint_context,
                    show_hints,
                    &code_prefs,
                    paint_diagnostics,
                    &mut diagnostic_ui,
                    host_state,
                )
            })
            .inner;
        if let Some(status) = chord_status {
            self.status = status;
        }
        if let Some((clicked_line, shift_held)) = gutter_click {
            if shift_held || standalone_session {
                let fold_key = self.active_fold_tab_key();
                self.toggle_fold_line_for(&fold_key, clicked_line);
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
        let p = neoflash_ui::palette(ui.ctx());
        if show_dbg {
            ui.painter().hline(
                dbg_rect.x_range(),
                dbg_rect.top() + 0.5,
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(dbg_rect.shrink2(egui::vec2(space::s2(), space::s1() / 2.))),
                |ui| {
                    self.actions_debugger_inspector_ui(ui, &tab_key);
                },
            );
        }
        ui.painter().hline(
            bar.x_range(),
            bar.top() + 0.5,
            egui::Stroke::new(1.0_f32, p.border_subtle),
        );
        let min_tabs_w = neoflash_ui::metrics::field_s().min(bar.width() * 0.42);
        let status_left = (bar.left() + min_tabs_w + space::s2()).min(bar.right() - space::s2());
        let status_zone = egui::Rect::from_min_max(
            egui::pos2(status_left, bar.top()),
            egui::pos2((bar.right() - space::s2()).max(status_left), bar.bottom()),
        );
        let (status_rect, status_clicked) = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(status_zone)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    let lines = source.chars().filter(|c| *c == '\n').count() + 1;
                    let problems = feedback.as_ref().map(|(success, _, d)| {
                        let mut errs = d
                            .iter()
                            .filter(|d| d.severity == DiagnosticSeverity::Error)
                            .count();
                        let warns = d
                            .iter()
                            .filter(|d| d.severity == DiagnosticSeverity::Warning)
                            .count();
                        if !*success && errs == 0 {
                            errs = 1;
                        }
                        (errs, warns)
                    });
                    let mut status_resp =
                        nf::code::script_status(ui, caret.0, caret.1, lines, problems);
                    if let Some((success, text, diagnostics)) = &feedback {
                        let tip = Self::format_script_status_tooltip(*success, text, diagnostics);
                        status_resp = nf::tooltip(status_resp, &tip, None);
                    }
                    (status_resp.rect, status_resp.clicked())
                },
            )
            .inner;
        if status_clicked
            && let Some((_, _, diagnostics)) = &feedback
            && !diagnostics.is_empty()
        {
            self.jump_to_next_bar_diagnostic(ui.ctx(), source, caret, diagnostics);
        }
        let tabs_right = (status_rect.left() - space::s2())
            .max(bar.left() + 2. * neoflash_ui::metrics::control());
        let tabs_rect = egui::Rect::from_min_max(bar.min, egui::pos2(tabs_right, bar.bottom()));
        self.render_actions_script_tabs_bar(ui, tabs_rect, tab);
        ui.advance_cursor_after_rect(full);
        response
    }

    /// Render the bottom `ScriptTab` bar inside `tabs_rect` (`T-364`): all pinned script tabs in
    /// order followed by the unpinned live-selection tab (when not already pinned), with disjoint
    /// `📌` pin/unpin click hit-testing and right-click context menu (`Pin tab` / `Unpin tab` /
    /// `Unpin other tabs`).
    pub(crate) fn render_actions_script_tabs_bar(
        &mut self,
        ui: &mut egui::Ui,
        tabs_rect: egui::Rect,
        fallback_tab: &str,
    ) {
        use crate::panels::actions_sidebar::ScriptLocation;
        if self.is_standalone_script_session() {
            let label = self
                .project
                .sources
                .first()
                .map(|u| u.path.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_tab);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(tabs_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(tabs_rect));
                    ui.push_id(
                        ("nf2-standalone-script-tab", &self.clipboard.session),
                        |ui| {
                            let response = (nf::ScriptTab {
                                label,
                                pinned: false,
                                active: true,
                            })
                            .show_interactive(
                                ui,
                                neoflash_ui::icons::i16::FILE_CODE,
                                false,
                            );
                            neoflash_ui::theme::focus_ring_inside(
                                ui,
                                &response.tab,
                                neoflash_ui::tokens::radius::sm(),
                            );
                        },
                    );
                },
            );
            return;
        }
        self.prune_stale_pinned_script_tabs();
        let active_loc = self.effective_actions_location();
        let live_loc = self
            .current_script_location()
            .and_then(|l| self.canonical_script_location(&l));

        let mut tab_items: Vec<(
            Option<ScriptLocation>,
            String,
            neoflash_ui::Icon,
            bool,
            bool,
        )> = Vec::new();
        for pinned_loc in self.actions_ui.pinned_tabs.clone() {
            let (label, icon) = self.script_location_tab_label(&pinned_loc);
            let is_active = active_loc.as_ref() == Some(&pinned_loc);
            tab_items.push((Some(pinned_loc), label, icon, true, is_active));
        }
        if let Some(live) = live_loc {
            if !self.actions_ui.pinned_tabs.contains(&live) {
                let (label, icon) = self.script_location_tab_label(&live);
                let is_active = self.actions_ui.active_pinned.is_none();
                tab_items.push((Some(live), label, icon, false, is_active));
            }
        } else if tab_items.is_empty() && !fallback_tab.is_empty() {
            tab_items.push((
                None,
                fallback_tab.to_owned(),
                neoflash_ui::icons::i16::FILE_CODE,
                false,
                true,
            ));
        }

        enum TabAction {
            SelectPinned(ScriptLocation),
            SelectLive,
            TogglePin(ScriptLocation),
            Pin(ScriptLocation),
            Unpin(ScriptLocation),
            UnpinOthers(ScriptLocation),
        }
        let mut action: Option<TabAction> = None;
        let pinned_snapshot = self.actions_ui.pinned_tabs.clone();
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(tabs_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(tabs_rect));
                egui::ScrollArea::horizontal()
                    .id_salt(("actions_script_tabs", &self.clipboard.session))
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (loc_opt, label, icon, pinned, active) in &tab_items {
                                let tab_salt = format!("{loc_opt:?}");
                                ui.push_id(("nf2-actions-script-tab", tab_salt, label), |ui| {
                                    let resp = nf::ScriptTab {
                                        label,
                                        pinned: *pinned,
                                        active: *active,
                                    }
                                    .show_interactive(ui, *icon, loc_opt.is_some());
                                    for control in
                                        std::iter::once(&resp.tab).chain(resp.pin.as_ref())
                                    {
                                        neoflash_ui::theme::focus_ring_inside(
                                            ui,
                                            control,
                                            neoflash_ui::tokens::radius::sm(),
                                        );
                                        if control.gained_focus()
                                            && neoflash_ui::theme::keyboard_focus(ui, control)
                                            && !ui.clip_rect().contains_rect(control.rect)
                                        {
                                            control.scroll_to_me(None);
                                        }
                                    }
                                    if let Some(loc) = loc_opt {
                                        if resp.pin.as_ref().is_some_and(egui::Response::clicked) {
                                            action = Some(TabAction::TogglePin(loc.clone()));
                                        } else if resp.tab.clicked() {
                                            action = Some(if *pinned {
                                                TabAction::SelectPinned(loc.clone())
                                            } else {
                                                TabAction::SelectLive
                                            });
                                        }
                                        let has_others = pinned_snapshot.iter().any(|p| p != loc);
                                        let build_menu =
                                            |ui: &mut egui::Ui, act: &mut Option<TabAction>| {
                                                if *pinned {
                                                    if ui
                                                        .add(
                                                            nf::MenuItem::new("Unpin tab")
                                                                .icon(neoflash_ui::icons::i16::PIN),
                                                        )
                                                        .clicked()
                                                    {
                                                        *act = Some(TabAction::Unpin(loc.clone()));
                                                        ui.close();
                                                    }
                                                } else if ui
                                                    .add(
                                                        nf::MenuItem::new("Pin tab")
                                                            .icon(neoflash_ui::icons::i16::PIN),
                                                    )
                                                    .clicked()
                                                {
                                                    *act = Some(TabAction::Pin(loc.clone()));
                                                    ui.close();
                                                }
                                                if ui
                                                    .add_enabled_semantic(
                                                        has_others,
                                                        nf::MenuItem::new("Unpin other tabs"),
                                                    )
                                                    .clicked()
                                                {
                                                    *act =
                                                        Some(TabAction::UnpinOthers(loc.clone()));
                                                    ui.close();
                                                }
                                            };
                                        nf::context_menu(&resp.tab, |ui| {
                                            build_menu(ui, &mut action)
                                        });
                                        if let Some(pin_r) = &resp.pin {
                                            nf::context_menu(pin_r, |ui| {
                                                build_menu(ui, &mut action)
                                            });
                                        }
                                    }
                                });
                            }
                        });
                    });
            },
        );
        match action {
            Some(TabAction::SelectPinned(loc)) => {
                self.actions_ui.active_pinned = Some(loc);
            }
            Some(TabAction::SelectLive) => {
                self.actions_ui.active_pinned = None;
            }
            Some(TabAction::TogglePin(loc)) => {
                self.toggle_pin_script_tab(loc);
            }
            Some(TabAction::Pin(loc)) => {
                self.pin_script_tab(loc);
            }
            Some(TabAction::Unpin(loc)) => {
                self.unpin_script_tab(&loc);
            }
            Some(TabAction::UnpinOthers(loc)) => {
                self.unpin_other_script_tabs(&loc);
            }
            None => {}
        }
    }

    /// Apply the queued toolbar/toolbox command to the visible editor. Returns `true` when code
    /// hints should open at the caret.
    pub(super) fn apply_code_command(
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
    pub(super) fn hint_context(&self, language: ScriptLanguage) -> Option<HintContext<'static>> {
        use crate::panels::actions_sidebar::ScriptLocation;
        let kind_of =
            |element: &neoflash_core::Element| ThisKind::of_element(&self.project, &element.kind);
        match self.effective_actions_location() {
            Some(ScriptLocation::Source(id)) => self
                .project
                .sources
                .iter()
                .find(|unit| unit.id == id)
                .and_then(|unit| self.source_hint_context(unit)),
            Some(ScriptLocation::Object {
                symbol, element, ..
            }) => {
                let sym = self
                    .project
                    .symbol(&symbol)
                    .unwrap_or_else(|| self.symbol());
                let this_kind = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| &k.elements)
                    .find(|e| e.id == element)
                    .map_or(ThisKind::MovieClip, kind_of);
                let mut seen = std::collections::HashSet::new();
                let instances = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|layer| &layer.keyframes)
                    .flat_map(|key| &key.elements)
                    .filter(|el| !el.name.is_empty() && seen.insert(el.name.clone()))
                    .map(|el| (el.name.clone(), kind_of(el)))
                    .collect();
                Some(
                    HintContext::new(&self.project, language, this_kind)?.with_instances(instances),
                )
            }
            Some(ScriptLocation::Frame { symbol, .. }) => {
                let sym = self
                    .project
                    .symbol(&symbol)
                    .unwrap_or_else(|| self.symbol());
                let mut seen = std::collections::HashSet::new();
                let instances = sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|layer| &layer.keyframes)
                    .flat_map(|key| &key.elements)
                    .filter(|el| !el.name.is_empty() && seen.insert(el.name.clone()))
                    .map(|el| (el.name.clone(), kind_of(el)))
                    .collect();
                Some(
                    HintContext::new(&self.project, language, ThisKind::MovieClip)?
                        .with_instances(instances),
                )
            }
            None => {
                if self.script_target == ScriptTarget::Source {
                    return self
                        .project
                        .sources
                        .iter()
                        .find(|unit| Some(&unit.id) == self.selected_source.as_ref())
                        .and_then(|unit| self.source_hint_context(unit));
                }
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
                Some(
                    HintContext::new(&self.project, language, ThisKind::MovieClip)?
                        .with_instances(instances),
                )
            }
        }
    }

    pub(super) fn source_hint_context(&self, unit: &SourceUnit) -> Option<HintContext<'static>> {
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
    pub(super) fn live_check(
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
        if matches!(
            self.effective_actions_location(),
            Some(crate::panels::actions_sidebar::ScriptLocation::Source(_))
        ) {
            self.check_project_sources(false);
        } else {
            let script = Script {
                language,
                source: source.to_owned(),
            };
            self.check_action_source(&script, false);
        }
    }
}
