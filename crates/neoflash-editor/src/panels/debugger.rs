//! Existing NF2 Debugger composition bound to a live Test Movie session.
use crate::app::Editor;
use crate::debugger::DebuggerStartupStatus;
use crate::debugger_protocol::{DebugCommand, DebugState};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};

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
                for local in snapshot.iter().flat_map(|snapshot| &snapshot.locals) {
                    // The AS compiler reserves this prefix for its own bindings.
                    // Keep JS/TS names intact, including authored __nf identifiers.
                    if actionscript && local.name.starts_with("__nf") {
                        continue;
                    }
                    ui.add(c::TableRow {
                        cells: &[&local.name, &local.value],
                        kind: c::TableRowKind::Row,
                        columns: cols,
                    });
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
                    ui.horizontal(|ui| {
                        ui.set_height(row_h);
                        ui.spacing_mut().item_spacing.x = row_gap;
                        let table_w = (ui.available_width() - row_h - row_gap - row_pad).max(0.0);
                        let value_col = if table_w > 0.0 {
                            (0.45 * full_table_w / table_w).min(0.85)
                        } else {
                            0.45
                        };
                        let row_cols = [0.0, value_col];
                        ui.allocate_ui_with_layout(
                            egui::vec2(table_w, row_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(table_w);
                                ui.add(c::TableRow {
                                    cells: &[&watch.expression, display_value],
                                    kind: c::TableRowKind::Row,
                                    columns: &row_cols,
                                });
                            },
                        );
                        if ui
                            .add(c::IconButton::new(i16::TRASH, "Remove watch").side(row_h))
                            .clicked()
                        {
                            remove_watch_idx = Some(idx);
                        }
                    });
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
}
