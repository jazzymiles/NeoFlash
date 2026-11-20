//! Existing NF2 Debugger composition bound to a live Test Movie session.
use crate::app::Editor;
use crate::debugger::{DebugEditTarget, DebugInlineEdit, DebuggerStartupStatus};
use crate::debugger_protocol::{DebugCommand, DebugInspection, DebugState};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};

const MAX_DEBUGGER_TREE_DEPTH: usize = 6;

fn column_title(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), neoflash_ui::metrics::section_header()),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(space::s3(), 0.0),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        nf.text_secondary,
    );
}

/// One pane of the debugger: fixed width, full height, its records scroll inside it.
fn column<R>(ui: &mut Ui, title: &str, width: f32, content: impl FnOnce(&mut Ui) -> R) {
    let height = ui.available_height();
    // An unbounded host (a scrolling container) lays the pane out at its content height.
    let bounded = height.is_finite();
    ui.allocate_ui_with_layout(
        egui::vec2(width, if bounded { height } else { 0.0 }),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            let body = |ui: &mut Ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                column_title(ui, title);
                content(ui);
            };
            if bounded {
                egui::ScrollArea::vertical()
                    .id_salt(("debugger-pane", title))
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, body);
            } else {
                body(ui);
            }
        },
    );
}

/// Widths of the Call stack, Variables and Watch panes for `total` points: the call stack is one
/// `width/tree`, Watch one long plus one short field, Variables takes the rest. A panel too narrow
/// for that splits 30 / 35 / 35 %, so no pane leaves the panel.
fn pane_widths(total: f32) -> [f32; 3] {
    use neoflash_ui::metrics;
    let call = metrics::tree_width();
    let watch = metrics::field_l() + metrics::field_s();
    if total >= call + watch + metrics::field_l() {
        [call, total - call - watch, watch]
    } else {
        [total * 0.30, total * 0.35, total * 0.35]
    }
}

fn text_width(ui: &Ui, text: &str, style: TextStyle) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            neoflash_ui::typography::font(style),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
}

impl Editor {
    pub(crate) fn debugger_contents(&mut self, ui: &mut Ui) -> bool {
        let snapshot = self.debugger.snapshot();
        let startup = self.debugger.startup_status.clone();
        let idle_ready = snapshot.is_none() && startup.is_none();
        let nf = neoflash_ui::palette(ui.ctx());
        let paused = snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.state == DebugState::Paused && !snapshot.command_pending
        });
        let has_paused_frame = snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.state == DebugState::Paused
                && snapshot
                    .selected_frame
                    .is_some_and(|index| index < snapshot.frames.len())
        });
        let inspection = self.debugger.inspection();
        self.debugger.sync_ui_pause_state(
            has_paused_frame,
            inspection.pause_epoch,
            inspection.pause_generation,
        );
        let selected_frame = snapshot
            .as_ref()
            .and_then(|s| s.selected_frame)
            .unwrap_or(0);
        let actionscript = snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .selected_frame
                    .and_then(|index| snapshot.frames.get(index))
            })
            .and_then(|frame| frame.location.as_ref())
            .is_some_and(|location| self.debug_source_is_actionscript(&location.source));
        // Toolbar, one row: Continue (the primary), Stop · the three steps · breakpoints, then the
        // session status at the right edge. "Remove all breakpoints" keeps its label while the row
        // has room and becomes an icon button otherwise; a status that does not fit is shortened
        // and keeps the full text in its tooltip.
        const REMOVE_ALL: &str = "Remove all breakpoints";
        let gap = space::s2();
        let button = neoflash_ui::metrics::control();
        let divider = 1.0 + space::s2();
        let remove_label =
            text_width(ui, REMOVE_ALL, TextStyle::Body) + 2.0 * c::button::button_padding();
        // Six icon buttons, two dividers, Remove all and one gap after each of them.
        let left = |remove: f32| 6.0 * button + 2.0 * divider + remove + 9.0 * gap;
        let badge_width = |ui: &Ui, text: &str| text_width(ui, text, TextStyle::Small) + 12.0;
        let (status_full, status_short, status_kind) = match &snapshot {
            None => (
                "Not running".to_owned(),
                "Not running".to_owned(),
                c::BadgeType::Neutral,
            ),
            Some(snapshot) => {
                let short = format!("{:?}", snapshot.state);
                let full = if has_paused_frame {
                    snapshot
                        .frames
                        .get(snapshot.selected_frame.unwrap_or(0))
                        .and_then(|frame| frame.location.as_ref())
                        .map(|location| {
                            format!(
                                "Paused · {} · line {} ({})",
                                location.source, location.line, snapshot.reason
                            )
                        })
                        .unwrap_or_else(|| "Paused".into())
                } else {
                    short.clone()
                };
                let kind = match snapshot.state {
                    DebugState::Running => c::BadgeType::Accent,
                    DebugState::Paused => c::BadgeType::Warning,
                    _ => c::BadgeType::Neutral,
                };
                (full, short, kind)
            }
        };
        let available = ui.available_width();
        let remove_as_text = left(remove_label) + badge_width(ui, &status_short) <= available;
        let left_width = left(if remove_as_text { remove_label } else { button });
        let status_room = (available - left_width).max(0.0);
        ui.horizontal(|ui| {
            ui.set_height(button);
            ui.spacing_mut().item_spacing.x = gap;
            if ui
                .add_enabled_semantic(
                    paused || idle_ready,
                    c::IconButton::new(i16::DEBUG_CONTINUE, "Continue")
                        .kind(c::IconButtonType::Primary)
                        .shortcut("F8"),
                )
                .clicked()
            {
                if snapshot.is_some() {
                    self.debug_command(DebugCommand::Resume);
                } else {
                    self.start_debug_movie(ui.ctx());
                }
            }
            if ui
                .add_enabled_semantic(!idle_ready, c::IconButton::new(i16::STOP, "Stop debugging"))
                .clicked()
            {
                self.debugger.stop();
            }
            c::divider(ui, true);
            for (icon, label, shortcut, command) in [
                (i16::STEP_OVER, "Step over", "F10", DebugCommand::StepOver),
                (i16::STEP_INTO, "Step in", "F11", DebugCommand::StepInto),
                (i16::STEP_OUT, "Step out", "⇧F11", DebugCommand::StepOut),
            ] {
                if ui
                    .add_enabled_semantic(
                        paused,
                        c::IconButton::new(icon, label).shortcut(shortcut),
                    )
                    .clicked()
                {
                    self.debug_command(command);
                }
            }
            c::divider(ui, true);
            if ui
                .add_enabled_semantic(
                    snapshot.is_some() || idle_ready,
                    c::IconButton::new(i16::BREAKPOINT, "Toggle breakpoint").shortcut("⌘B"),
                )
                .clicked()
            {
                self.toggle_debug_breakpoint(ui.ctx());
            }
            let can_remove =
                snapshot.is_some() || (idle_ready && !self.debugger.breakpoints.is_empty());
            let remove = if remove_as_text {
                ui.add_enabled_semantic(can_remove, c::Button::ghost(REMOVE_ALL))
            } else {
                ui.add_enabled_semantic(can_remove, c::IconButton::new(i16::TRASH, REMOVE_ALL))
            };
            if remove.clicked() {
                self.clear_all_session_breakpoints();
                if snapshot.is_some() {
                    self.debug_command(DebugCommand::SetBreakpoints(Vec::new()));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // A message next to its action button keeps the rest of the row.
                let message_width = |ui: &Ui, action: &str| {
                    (status_room
                        - text_width(ui, action, TextStyle::Body)
                        - 2.0 * c::button::button_padding()
                        - gap)
                        .max(neoflash_ui::metrics::field_s())
                };
                if let Some(startup) = &startup {
                    match startup {
                        DebuggerStartupStatus::NotAvailable(reason) => {
                            ui.add(c::Badge::new("Not available", c::BadgeType::Neutral));
                            let width = (status_room - badge_width(ui, "Not available") - gap)
                                .max(neoflash_ui::metrics::field_s());
                            let message = ui.add(c::InlineMessage::info(reason).width(width));
                            c::tooltip(message, reason.trim_end_matches('.'), None);
                        }
                        DebuggerStartupStatus::Failed(reason) => {
                            let width = message_width(ui, "Retry");
                            if ui.add(c::Button::new("Retry")).clicked() {
                                self.start_debug_movie(ui.ctx());
                            }
                            let message = ui.add(c::InlineMessage::error(reason).width(width));
                            c::tooltip(message, reason.trim_end_matches('.'), None);
                        }
                    }
                    return;
                }
                let Some(snapshot) = &snapshot else {
                    ui.add(c::Badge::new(&status_full, status_kind));
                    return;
                };
                if snapshot.state == DebugState::Failed {
                    let width = message_width(ui, "Retry");
                    if ui.add(c::Button::new("Retry")).clicked() {
                        self.start_debug_movie(ui.ctx());
                    }
                    let reason = if snapshot.reason.is_empty() {
                        "Test Movie debugger failed."
                    } else {
                        &snapshot.reason
                    };
                    let message = ui.add(c::InlineMessage::error(reason).width(width));
                    c::tooltip(message, reason.trim_end_matches('.'), None);
                    return;
                }
                let mut room = status_room;
                if snapshot.state == DebugState::Disconnected {
                    room -= text_width(ui, "Restart", TextStyle::Body)
                        + 2.0 * c::button::button_padding()
                        + gap;
                    if ui.add(c::Button::new("Restart")).clicked() {
                        self.start_debug_movie(ui.ctx());
                    }
                }
                if badge_width(ui, &status_full) <= room {
                    ui.add(c::Badge::new(&status_full, status_kind));
                } else {
                    let badge = ui.add(c::Badge::new(&status_short, status_kind));
                    c::tooltip(badge, &status_full, None);
                }
                if snapshot.state == DebugState::Connecting {
                    c::spinner(ui);
                }
            });
        });
        ui.add_space(space::s2());
        let [call_width, variables_width, watch_width] = pane_widths(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            column(ui, "Call stack", call_width, |ui| {
                if !has_paused_frame {
                    ui.horizontal(|ui| {
                        ui.add_space(space::s3());
                        ui.label(
                            neoflash_ui::typography::rich(
                                "Pause to view the call stack.",
                                TextStyle::Small,
                            )
                            .color(nf.text_tertiary),
                        );
                    });
                }
                for (index, frame) in snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.frames)
                    .enumerate()
                    .filter(|_| has_paused_frame)
                {
                    let label = frame
                        .location
                        .as_ref()
                        .map(|location| {
                            format!(
                                "{} · {}:{}",
                                frame.function_name, location.source, location.line
                            )
                        })
                        .unwrap_or_else(|| {
                            format!(
                                "{} · {}:{}",
                                frame.function_name, frame.generated_url, frame.generated_line
                            )
                        });
                    let selected = snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.selected_frame == Some(index));
                    let icon = if selected {
                        i16::EXEC_ARROW
                    } else {
                        i16::CALLSTACK
                    };
                    if c::TreeItem::new(&label, c::TreeItemType::Symbol(icon))
                        .selected(selected)
                        .show(ui)
                        .row
                        .clicked()
                    {
                        let frame_location = frame.location.clone();
                        self.debug_command(DebugCommand::SelectFrame(index));
                        if let Some(location) = frame_location {
                            self.navigate_actions_to_debug_location(ui.ctx(), &location, false);
                        }
                    }
                }
            });
            column(ui, "Variables", variables_width, |ui| {
                if !has_paused_frame {
                    ui.horizontal(|ui| {
                        ui.add_space(space::s3());
                        ui.label(
                            neoflash_ui::typography::rich(
                                "Pause to inspect variables.",
                                TextStyle::Small,
                            )
                            .color(nf.text_tertiary),
                        );
                    });
                    return;
                }
                let cols = &[0.0, 0.45];
                ui.add(c::TableRow {
                    cells: &["Name", "Value"],
                    kind: c::TableRowKind::Header,
                    columns: cols,
                });
                if !inspection.locals.is_empty() {
                    for local in &inspection.locals {
                        // The AS compiler reserves this prefix for its own bindings.
                        // Keep JS/TS names intact, including authored __nf identifiers.
                        if actionscript && local.name.starts_with("__nf") {
                            continue;
                        }
                        let expandable = local.object_id.is_some();
                        let row_key = format!(
                            "frame:{selected_frame}:local:{}:{}",
                            local.scope_number, local.name
                        );
                        let expanded = expandable && self.debugger.is_tree_expanded(&row_key);
                        let name_cell = if expandable {
                            format!("{} {}", if expanded { "▾" } else { "▸" }, local.name)
                        } else {
                            local.name.clone()
                        };
                        let edit_target = (paused && local.writable && !expandable).then(|| {
                            DebugEditTarget::ScopeVariable {
                                scope_number: local.scope_number,
                                variable_name: local.name.clone(),
                            }
                        });
                        self.render_debugger_value_row(
                            ui,
                            &row_key,
                            &name_cell,
                            &local.value,
                            cols,
                            expandable,
                            edit_target,
                        );
                        if expandable
                            && self.debugger.is_tree_expanded(&row_key)
                            && let Some(oid) = local.object_id.as_deref()
                        {
                            self.ensure_debug_object_inspected(
                                inspection.pause_generation,
                                oid,
                                inspection.object_properties.contains_key(oid),
                            );
                            let mut ancestors = vec![oid.to_owned()];
                            self.render_debugger_object_children(
                                ui,
                                &row_key,
                                oid,
                                1,
                                &mut ancestors,
                                actionscript,
                                paused,
                                &inspection,
                                cols,
                                None,
                            );
                        }
                    }
                } else {
                    for local in snapshot.iter().flat_map(|snapshot| &snapshot.locals) {
                        if actionscript && local.name.starts_with("__nf") {
                            continue;
                        }
                        ui.add(c::TableRow {
                            cells: &[&local.name, &local.value],
                            kind: c::TableRowKind::Row,
                            columns: cols,
                        });
                    }
                }
            });
            column(ui, "Watch", watch_width, |ui| {
                let mut watch_draft = self.debugger_watch_draft();
                let watches = self.debugger_watches();
                let mut add_watch_now = false;
                let mut refresh_now = false;
                let control = neoflash_ui::metrics::control();
                let row_h = neoflash_ui::metrics::row();
                let pad = space::s3();
                ui.add_space(space::s1());
                ui.horizontal(|ui| {
                    ui.set_height(control);
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.add_space(pad);
                    let expr_width =
                        (ui.available_width() - pad - control - control - gap - gap).max(0.0);
                    let tf = ui.add(
                        c::TextField::new(&mut watch_draft, "Watch expression")
                            .prefix("Expr")
                            .hint("Expression")
                            .mono()
                            .width(expr_width),
                    );
                    if tf.changed() {
                        self.set_debugger_watch_draft(watch_draft.clone());
                    }
                    let mut tf_edit = tf.clone();
                    tf_edit.id = c::TextField::edit_id(tf.id);
                    if (tf_edit.has_focus() || tf_edit.lost_focus())
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !watch_draft.trim().is_empty()
                    {
                        add_watch_now = true;
                    }
                    if ui
                        .add_enabled_semantic(
                            !watch_draft.trim().is_empty(),
                            c::IconButton::new(i16::PLUS, "Add watch"),
                        )
                        .clicked()
                    {
                        add_watch_now = true;
                    }
                    if ui
                        .add(c::IconButton::new(i16::REFRESH, "Evaluate watches"))
                        .clicked()
                    {
                        refresh_now = true;
                    }
                });
                if add_watch_now {
                    let expr = watch_draft.trim().to_owned();
                    if !expr.is_empty() {
                        self.add_debugger_watch(expr);
                        self.set_debugger_watch_draft(String::new());
                    }
                }
                if refresh_now {
                    self.refresh_debugger_watches();
                }
                ui.add_space(space::s2());
                let full_table_w = ui.available_width();
                let header_cols = &[0.0, 0.45];
                ui.add(c::TableRow {
                    cells: &["Name", "Value"],
                    kind: c::TableRowKind::Header,
                    columns: header_cols,
                });
                let mut remove_watch_idx = None;
                let row_gap = space::s1();
                let row_pad = space::s2();
                for (idx, watch) in watches.iter().enumerate() {
                    let display_value = if snapshot.is_some() && !has_paused_frame {
                        "<not paused>"
                    } else {
                        watch.last_value.as_str()
                    };
                    let watch_oid = has_paused_frame
                        .then(|| {
                            inspection
                                .watches
                                .iter()
                                .find(|w| w.index == idx && w.expression == watch.expression)
                                .and_then(|w| w.object_id.as_deref())
                        })
                        .flatten();
                    let expandable = watch_oid.is_some();
                    let row_key = format!("watch:{idx}:{}", watch.expression);
                    let expanded = expandable && self.debugger.is_tree_expanded(&row_key);
                    let name_cell = if expandable {
                        format!("{} {}", if expanded { "▾" } else { "▸" }, watch.expression)
                    } else {
                        watch.expression.clone()
                    };
                    let table_w = (ui.available_width() - row_h - row_gap - row_pad).max(0.0);
                    let value_col = if table_w > 0.0 {
                        (0.45 * full_table_w / table_w).min(0.85)
                    } else {
                        0.45
                    };
                    let row_cols = [0.0, value_col];
                    ui.horizontal(|ui| {
                        ui.set_height(row_h);
                        ui.spacing_mut().item_spacing.x = row_gap;
                        ui.allocate_ui_with_layout(
                            egui::vec2(table_w, row_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(table_w);
                                self.render_debugger_value_row(
                                    ui,
                                    &row_key,
                                    &name_cell,
                                    display_value,
                                    &row_cols,
                                    expandable,
                                    None,
                                );
                            },
                        );
                        if ui
                            .add(c::IconButton::new(i16::TRASH, "Remove watch").side(row_h))
                            .clicked()
                        {
                            remove_watch_idx = Some(idx);
                        }
                    });
                    if expandable
                        && self.debugger.is_tree_expanded(&row_key)
                        && let Some(oid) = watch_oid
                    {
                        self.ensure_debug_object_inspected(
                            inspection.pause_generation,
                            oid,
                            inspection.object_properties.contains_key(oid),
                        );
                        let mut ancestors = vec![oid.to_owned()];
                        self.render_debugger_object_children(
                            ui,
                            &row_key,
                            oid,
                            1,
                            &mut ancestors,
                            actionscript,
                            paused,
                            &inspection,
                            &row_cols,
                            Some(table_w),
                        );
                    }
                }
                if let Some(idx) = remove_watch_idx {
                    self.remove_debugger_watch(idx);
                }
                column_title(ui, "Breakpoints");
                let requested_breakpoints: Vec<crate::debugger_protocol::AuthoredBreakpoint> =
                    if let Some(snapshot) = &snapshot {
                        snapshot
                            .breakpoints
                            .iter()
                            .map(|breakpoint| breakpoint.requested.clone())
                            .collect()
                    } else {
                        self.debugger.breakpoints.clone()
                    };
                for requested in requested_breakpoints {
                    let label = format!("{} · line {}", requested.source, requested.line);
                    // One `row()` per breakpoint.
                    let row = egui::vec2(ui.available_width(), neoflash_ui::metrics::row());
                    ui.allocate_ui_with_layout(
                        row,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = space::s2();
                            ui.add_space(space::s3());
                            icons::show(ui, i16::BREAKPOINT, nf.syntax_breakpoint);
                            let mut enabled = true;
                            if ui.add(c::Checkbox::new(&mut enabled, &label)).changed() && !enabled
                            {
                                self.remove_session_breakpoint(&requested.source, requested.line);
                                if snapshot.is_some() {
                                    self.debug_command(DebugCommand::SetBreakpoints(
                                        self.debugger.breakpoints.clone(),
                                    ));
                                }
                            }
                        },
                    );
                }
            });
        });
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn render_debugger_value_row(
        &mut self,
        ui: &mut Ui,
        row_key: &str,
        name_cell: &str,
        value_cell: &str,
        columns: &[f32],
        expandable: bool,
        edit_target: Option<DebugEditTarget>,
    ) {
        let is_editing = self
            .debugger
            .inline_edit
            .as_ref()
            .is_some_and(|edit| edit.row_key == row_key);
        if is_editing {
            super::dialog_helpers::mark_inline_edit_active(ui.ctx());
            let row_resp = ui.add(c::TableRow {
                cells: &[name_cell, ""],
                kind: c::TableRowKind::Selected,
                columns,
            });
            let val_start = columns.get(1).copied().unwrap_or(0.45);
            let x0 = row_resp.rect.left() + row_resp.rect.width() * val_start + space::s2();
            let x1 = (row_resp.rect.right() - space::s1()).max(x0);
            let edit_rect = egui::Rect::from_min_max(
                egui::pos2(x0, row_resp.rect.top()),
                egui::pos2(x1, row_resp.rect.bottom()),
            );
            let mut commit_now = false;
            let mut cancel_now = false;
            if let Some(draft) = self.debugger.inline_edit.as_mut() {
                let mut slot = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("debugger-inline-value", row_key))
                        .max_rect(edit_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Min)),
                );
                let mut field = slot.add(
                    c::TextField::new(&mut draft.draft, "Edit value")
                        .hint("JSON value")
                        .mono()
                        .compact(edit_rect.height())
                        .text_style(TextStyle::MonoSmall)
                        .width(edit_rect.width()),
                );
                field.id = c::TextField::edit_id(field.id);
                if draft.focus {
                    field.request_focus();
                    if let Some(mut state) =
                        egui::text_edit::TextEditState::load(ui.ctx(), field.id)
                    {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(draft.draft.chars().count()),
                            )));
                        state.store(ui.ctx(), field.id);
                    }
                    draft.focus = false;
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                {
                    cancel_now = true;
                    field.surrender_focus();
                } else if field.lost_focus()
                    || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    let _ =
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    commit_now = true;
                    field.surrender_focus();
                }
            }
            if cancel_now {
                self.debugger.inline_edit = None;
            } else if commit_now && let Some(edit) = self.debugger.inline_edit.take() {
                let _ = self.commit_debug_inline_edit(edit);
            }
            return;
        }

        let response = ui.add(c::TableRow {
            cells: &[name_cell, value_cell],
            kind: c::TableRowKind::Row,
            columns,
        });
        if response.clicked() {
            if expandable {
                self.debugger.toggle_tree_expanded(row_key);
            } else if let Some(target) = edit_target {
                let initial = if value_cell == "undefined" {
                    String::new()
                } else {
                    value_cell.to_owned()
                };
                self.debugger.inline_edit = Some(DebugInlineEdit {
                    row_key: row_key.to_owned(),
                    target,
                    draft: initial,
                    focus: true,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_debugger_object_children(
        &mut self,
        ui: &mut Ui,
        parent_key: &str,
        parent_object_id: &str,
        depth: usize,
        ancestor_ids: &mut Vec<String>,
        actionscript: bool,
        paused: bool,
        inspection: &DebugInspection,
        columns: &[f32],
        constrained_width: Option<f32>,
    ) {
        if depth > MAX_DEBUGGER_TREE_DEPTH {
            return;
        }
        let row_h = neoflash_ui::metrics::row();
        let indent = "  ".repeat(depth);
        let Some(children) = inspection.object_properties.get(parent_object_id) else {
            let loading_name = format!("{indent}  …");
            if let Some(table_w) = constrained_width {
                ui.horizontal(|ui| {
                    ui.set_height(row_h);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(table_w, row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(table_w);
                            ui.add(c::TableRow {
                                cells: &[&loading_name, "Loading…"],
                                kind: c::TableRowKind::Row,
                                columns,
                            });
                        },
                    );
                });
            } else {
                ui.add(c::TableRow {
                    cells: &[&loading_name, "Loading…"],
                    kind: c::TableRowKind::Row,
                    columns,
                });
            }
            return;
        };

        for prop in children {
            if actionscript && prop.name.starts_with("__nf") {
                continue;
            }
            let is_cycle = prop
                .object_id
                .as_ref()
                .is_some_and(|oid| ancestor_ids.iter().any(|seen| seen == oid));
            let can_expand =
                prop.object_id.is_some() && !is_cycle && depth < MAX_DEBUGGER_TREE_DEPTH;
            let child_key = format!("{parent_key}.{}", prop.name);
            let child_expanded = can_expand && self.debugger.is_tree_expanded(&child_key);
            let glyph = if can_expand {
                if child_expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let name_cell = format!("{indent}{glyph}{}", prop.name);
            let value_cell = if is_cycle {
                format!("<circular> {}", prop.value)
            } else {
                prop.value.clone()
            };
            let edit_target = (paused && prop.writable && prop.object_id.is_none()).then(|| {
                DebugEditTarget::ObjectProperty {
                    object_id: parent_object_id.to_owned(),
                    property_name: prop.name.clone(),
                }
            });
            if let Some(table_w) = constrained_width {
                ui.horizontal(|ui| {
                    ui.set_height(row_h);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(table_w, row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(table_w);
                            self.render_debugger_value_row(
                                ui,
                                &child_key,
                                &name_cell,
                                &value_cell,
                                columns,
                                can_expand,
                                edit_target,
                            );
                        },
                    );
                });
            } else {
                self.render_debugger_value_row(
                    ui,
                    &child_key,
                    &name_cell,
                    &value_cell,
                    columns,
                    can_expand,
                    edit_target,
                );
            }
            if can_expand
                && self.debugger.is_tree_expanded(&child_key)
                && let Some(child_oid) = prop.object_id.as_deref()
            {
                self.ensure_debug_object_inspected(
                    inspection.pause_generation,
                    child_oid,
                    inspection.object_properties.contains_key(child_oid),
                );
                ancestor_ids.push(child_oid.to_owned());
                self.render_debugger_object_children(
                    ui,
                    &child_key,
                    child_oid,
                    depth + 1,
                    ancestor_ids,
                    actionscript,
                    paused,
                    inspection,
                    columns,
                    constrained_width,
                );
                ancestor_ids.pop();
            }
        }
    }
}
