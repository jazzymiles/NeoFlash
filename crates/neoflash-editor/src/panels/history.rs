//! History panel (NF2 `Panel / History`, PP-09 / W1.1; reference `panels-more.png`): chronological
//! undo/redo step list (`NF2/Menu Item` rows, current state checked, undone steps dimmed) and the
//! bottom Undo · Redo · Clear history icon row, plus bounded authoring command replay (`Repeat`,
//! `Replay steps`, `Copy steps`, `Save as command`).
use crate::app::Editor;
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::{AuthoringCommandStep, SavedCommand, apply_command_steps_to_elements};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::HashSet;

impl Editor {
    /// Replay a slice of `AuthoringCommandStep`s on the current stage selection in one Undo step.
    pub fn execute_command_steps(
        &mut self,
        label: &str,
        steps: &[AuthoringCommandStep],
    ) -> Result<usize, String> {
        self.finish_edits();
        let mut selected_ids: Vec<String> = self
            .selection
            .targets
            .iter()
            .map(|t| t.element.clone())
            .collect();
        if selected_ids.is_empty() {
            return Err("Select at least one stage element first.".into());
        }
        let mut candidate = self.project.clone();
        let valid_symbols: HashSet<&str> = self
            .project
            .symbols
            .iter()
            .filter(|s| !self.project.is_scene(&s.id))
            .map(|s| s.id.as_str())
            .collect();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut total_applied = 0usize;
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            let key = &mut layer.keyframes[key_idx];
            if key.elements.iter().any(|e| selected_ids.contains(&e.id)) {
                total_applied += apply_command_steps_to_elements(
                    &mut key.elements,
                    &mut selected_ids,
                    steps,
                    &valid_symbols,
                )?;
            }
        }
        if total_applied == 0 {
            return Err("No selected elements matched the command steps.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        let last_step = steps.last().cloned();
        self.transact_named(label, |s| {
            s.project = candidate;
            let targets = selected_ids
                .iter()
                .filter_map(|id| s.target_for_id(id))
                .collect();
            s.set_selection(targets);
        });
        if let Some(step) = last_step {
            self.history.record_command_step(step);
        }
        self.normalize_selection();
        Ok(total_applied)
    }

    /// Repeat the most recent replayable History command step on the current selection in one Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn repeat_last_command(&mut self) -> Result<usize, String> {
        let Some(step) = self.history.last_command_step().cloned() else {
            return Err("No replayable command step in History.".into());
        };
        self.execute_command_steps("Repeat command", &[step])
    }

    /// Replay the specified 0-based History undo step indices on the current selection in one Undo step.
    pub fn replay_history_steps(&mut self, step_indices: &[usize]) -> Result<usize, String> {
        let steps = self.history.collect_command_steps(step_indices)?;
        self.execute_command_steps("Replay history steps", &steps)
    }

    /// Package the specified 0-based History undo steps into a validated `SavedCommand`.
    pub fn save_history_steps_as_command(
        &self,
        name: impl Into<String>,
        step_indices: &[usize],
    ) -> Result<SavedCommand, String> {
        let steps = self.history.collect_command_steps(step_indices)?;
        let cmd = SavedCommand {
            name: name.into(),
            steps,
        };
        cmd.validate()?;
        Ok(cmd)
    }

    /// Export the specified 0-based History undo steps as portable command JSON (`Copy steps`).
    pub fn copy_history_steps_json(&self, step_indices: &[usize]) -> Result<String, String> {
        let cmd = self.save_history_steps_as_command("Copied Steps", step_indices)?;
        cmd.to_json()
    }

    /// Execute a `SavedCommand` on the current selection in one Undo step named after the command.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn run_saved_command(&mut self, command: &SavedCommand) -> Result<usize, String> {
        command.validate()?;
        self.execute_command_steps(&command.name, &command.steps)
    }

    pub(crate) fn history_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::History, "History");
        let undo_labels: Vec<String> = self.history.undo_labels().map(str::to_owned).collect();
        let redo_labels: Vec<String> = self.history.redo_labels().map(str::to_owned).collect();
        let undo_len = undo_labels.len();
        let footer_h = crate::theme::control_h() + space::S2;
        let list_h = (ui.available_height() - footer_h).max(crate::theme::control_h());
        let mut target_step = None;
        let mut replay_step = None;
        let mut copy_step = None;

        egui::ScrollArea::vertical()
            .id_salt("history-steps")
            .auto_shrink([false, false])
            .max_height(list_h)
            .themed()
            .show(ui, |ui| {
                c::menu::menu_style(ui.style_mut());
                if undo_labels.is_empty() && redo_labels.is_empty() {
                    ui.add_enabled(false, c::MenuItem::new("Open").checked(true));
                    return;
                }
                let open_row = ui.add(c::MenuItem::new("Open").checked(undo_len == 0));
                if open_row.clicked() && undo_len > 0 {
                    target_step = Some(0);
                }
                for (i, label) in undo_labels.iter().enumerate() {
                    let is_current = i + 1 == undo_len;
                    let can_replay = self.history.step_command(i).is_some();
                    let response = ui.add(c::MenuItem::new(label).checked(is_current));
                    if response.clicked() && !is_current {
                        target_step = Some(i + 1);
                    }
                    c::context_menu(&response, |ui| {
                        if ui
                            .add_enabled(
                                can_replay && !self.selection.is_empty(),
                                c::MenuItem::new("Replay step on selection"),
                            )
                            .clicked()
                        {
                            replay_step = Some(i);
                            ui.close();
                        }
                        if ui
                            .add_enabled(can_replay, c::MenuItem::new("Copy step JSON"))
                            .clicked()
                        {
                            copy_step = Some(i);
                            ui.close();
                        }
                    });
                }
                for (offset, label) in redo_labels.iter().enumerate() {
                    let step = undo_len + offset + 1;
                    // Undone steps render in NF2 disabled state (`panels-more.png`) while still
                    // letting the user click any row to step forward without discarding redo.
                    let shown = ui.add_enabled(false, c::MenuItem::new(label));
                    let click = ui.interact(
                        shown.rect,
                        ui.id().with(("history-redo-step", step)),
                        egui::Sense::click(),
                    );
                    if ui.is_enabled() && click.clicked() {
                        target_step = Some(step);
                    }
                }
            });

        if let Some(step) = target_step {
            self.step_history_to(step);
        }
        if let Some(idx) = replay_step
            && let Err(err) = self.replay_history_steps(&[idx])
        {
            self.message(err);
        }
        if let Some(idx) = copy_step {
            match self.copy_history_steps_json(&[idx]) {
                Ok(json) => {
                    ui.ctx().copy_text(json);
                    self.message("Copied command step JSON to clipboard.");
                }
                Err(err) => self.message(err),
            }
        }

        ui.add_space(space::S2);
        let undo_sc = self.tooltip_sc(crate::commands::CommandId::EditUndo);
        let redo_sc = self.tooltip_sc(crate::commands::CommandId::EditRedo);
        ui.horizontal(|ui| {
            let mut undo_btn = c::IconButton::new(i16::UNDO, "Undo");
            if let Some(sc) = undo_sc.as_deref() {
                undo_btn = undo_btn.shortcut(sc);
            }
            if ui
                .add_enabled(
                    self.history.can_undo() || self.vector.draft.is_some(),
                    undo_btn,
                )
                .clicked()
            {
                self.undo();
            }
            let mut redo_btn = c::IconButton::new(i16::REDO, "Redo");
            if let Some(sc) = redo_sc.as_deref() {
                redo_btn = redo_btn.shortcut(sc);
            }
            if ui.add_enabled(self.history.can_redo(), redo_btn).clicked() {
                self.redo();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.history.is_empty(),
                        c::IconButton::new(i16::TRASH, "Clear history"),
                    )
                    .clicked()
                {
                    self.clear_history();
                }
            });
        });
    }
}
