//! Existing NF2 Debugger composition bound to a live Test Movie session.
use crate::app::Editor;
use crate::debugger_protocol::{DebugCommand, DebugState};
use crate::theme::DesignUi as _;
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, space};

fn column_title(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(space::S3, 0.0),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        nf.text_secondary,
    );
}

fn column<R>(ui: &mut Ui, width: f32, content: impl FnOnce(&mut Ui) -> R) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            content(ui);
        },
    );
}

impl Editor {
    pub(crate) fn debugger_contents(&mut self, ui: &mut Ui) -> bool {
        let Some(snapshot) = self.debugger.snapshot() else {
            return true;
        };
        let mut close = false;
        let nf = neoflash_ui::palette(ui.ctx());
        let paused = snapshot.state == DebugState::Paused && !snapshot.command_pending;
        let has_paused_frame = snapshot.state == DebugState::Paused
            && snapshot
                .selected_frame
                .is_some_and(|index| index < snapshot.frames.len());
        let actionscript = snapshot
            .selected_frame
            .and_then(|index| snapshot.frames.get(index))
            .and_then(|frame| frame.location.as_ref())
            .is_some_and(|location| self.debug_source_is_actionscript(&location.source));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui
                .add_enabled_semantic(
                    paused,
                    c::IconButton::new(i16::DEBUG_CONTINUE, "Continue")
                        .kind(c::IconButtonType::Primary)
                        .shortcut("F8"),
                )
                .clicked()
            {
                self.debug_command(DebugCommand::Resume);
            }
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
            if ui
                .add(c::IconButton::new(i16::STOP, "Stop debugging"))
                .clicked()
            {
                self.debugger.stop();
                close = true;
            }
            if ui
                .add(c::IconButton::new(i16::BREAKPOINT, "Toggle breakpoint").shortcut("⌘B"))
                .clicked()
            {
                self.toggle_debug_breakpoint(ui.ctx());
            }
            if ui.add(c::Button::ghost("Remove all breakpoints")).clicked() {
                self.debugger.breakpoints.clear();
                self.debug_command(DebugCommand::SetBreakpoints(Vec::new()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if snapshot.state == DebugState::Disconnected
                    && ui.add(c::Button::new("Restart")).clicked()
                {
                    self.start_debug_movie(ui.ctx());
                }
                let status = if has_paused_frame {
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
                    format!("{:?}", snapshot.state)
                };
                let badge = match snapshot.state {
                    DebugState::Connecting | DebugState::Disconnected => c::BadgeType::Neutral,
                    DebugState::Running => c::BadgeType::Accent,
                    // The approved Error line still needs the design-owned inline-message API.
                    DebugState::Paused | DebugState::Failed => c::BadgeType::Warning,
                };
                ui.add(c::Badge::new(&status, badge));
                if snapshot.state == DebugState::Connecting {
                    c::spinner(ui);
                }
            });
        });
        let total = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            column(ui, 240.0, |ui| {
                column_title(ui, "Call stack");
                if !has_paused_frame {
                    ui.label(
                        neoflash_ui::typography::rich(
                            "Pause to view the call stack.",
                            TextStyle::Small,
                        )
                        .color(nf.text_tertiary),
                    );
                }
                for (index, frame) in snapshot
                    .frames
                    .iter()
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
                    let selected = snapshot.selected_frame == Some(index);
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
                        self.debug_command(DebugCommand::SelectFrame(index));
                    }
                }
            });
            column(ui, 380.0, |ui| {
                column_title(ui, "Variables");
                if !has_paused_frame {
                    ui.label(
                        neoflash_ui::typography::rich(
                            "Pause to inspect variables.",
                            TextStyle::Small,
                        )
                        .color(nf.text_tertiary),
                    );
                    return;
                }
                let cols = &[0.0, 0.45];
                ui.add(c::TableRow {
                    cells: &["Name", "Value"],
                    kind: c::TableRowKind::Header,
                    columns: cols,
                });
                for local in &snapshot.locals {
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
            column(ui, (total - 620.0).max(240.0), |ui| {
                column_title(ui, "Watch");
                let cols = &[0.0, 0.5];
                ui.add(c::TableRow {
                    cells: &["Expression", "Value"],
                    kind: c::TableRowKind::Header,
                    columns: cols,
                });
                column_title(ui, "Breakpoints");
                ui.spacing_mut().item_spacing.y = space::S1;
                for breakpoint in &snapshot.breakpoints {
                    let label = format!(
                        "{} · line {}",
                        breakpoint.requested.source, breakpoint.requested.line
                    );
                    ui.horizontal(|ui| {
                        ui.add_space(space::S3);
                        icons::show(ui, i16::BREAKPOINT, nf.syntax_breakpoint);
                        let mut enabled = true;
                        if ui.add(c::Checkbox::new(&mut enabled, &label)).changed() && !enabled {
                            self.debugger
                                .breakpoints
                                .retain(|p| p != &breakpoint.requested);
                            self.debug_command(DebugCommand::SetBreakpoints(
                                self.debugger.breakpoints.clone(),
                            ));
                        }
                    });
                }
            });
        });
        close
    }
}
