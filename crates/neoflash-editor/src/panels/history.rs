//! History panel (NF2 `Panel / History`, PP-09 / W1.1; reference `panels-more.png`): chronological
//! undo/redo step list (`NF2/Menu Item` rows, current state checked, undone steps dimmed) and the
//! bottom Undo · Redo · Clear history icon row, plus bounded authoring command replay (`Repeat`,
//! `Replay steps`, `Copy steps`, `Save as command`).
use crate::app::Editor;
use crate::selection::SelectionTarget;
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::{
    AuthoringCommandStep, AuthoringExtensionManifest, ResourceUnit, SavedCommand,
    apply_command_steps_to_elements, new_id,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
use std::collections::{HashMap, HashSet};

pub const SAVED_COMMANDS_RESOURCE_PATH: &str = "commands/saved_commands.json";
pub const EXTENSIONS_RESOURCE_PATH: &str = "commands/extensions.json";

impl Editor {
    /// Replay a slice of `AuthoringCommandStep`s on the current stage selection in one Undo step.
    pub fn execute_command_steps(
        &mut self,
        label: &str,
        steps: &[AuthoringCommandStep],
    ) -> Result<usize, String> {
        self.finish_edits();
        let mut selected_by_layer = HashMap::<String, Vec<String>>::new();
        for target in &self.selection.targets {
            selected_by_layer
                .entry(target.layer.clone())
                .or_default()
                .push(target.element.clone());
        }
        if selected_by_layer.is_empty() {
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
        let mut resulting_targets = Vec::new();
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            let key = &mut layer.keyframes[key_idx];
            let Some(mut layer_selected_ids) = selected_by_layer.get(&layer.id).cloned() else {
                continue;
            };
            layer_selected_ids
                .retain(|id| key.elements.iter().any(|element| element.id == id.as_str()));
            if layer_selected_ids.is_empty() {
                continue;
            }
            total_applied += apply_command_steps_to_elements(
                &mut key.elements,
                &mut layer_selected_ids,
                steps,
                &valid_symbols,
            )?;
            resulting_targets.extend(layer_selected_ids.into_iter().map(|element| {
                SelectionTarget {
                    layer: layer.id.clone(),
                    element,
                }
            }));
        }
        if total_applied == 0 {
            return Err("No selected elements matched the command steps.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        let last_step = steps.last().cloned();
        self.transact_named(label, |s| {
            s.project = candidate;
            s.set_selection(resulting_targets);
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

    /// Return all `SavedCommand`s stored in the active project (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn saved_commands(&self) -> Vec<SavedCommand> {
        self.read_saved_commands().unwrap_or_default()
    }

    fn read_saved_commands(&self) -> Result<Vec<SavedCommand>, String> {
        let Some(unit) = self
            .project
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(SAVED_COMMANDS_RESOURCE_PATH))
        else {
            return Ok(Vec::new());
        };
        let bytes = unit
            .decoded_bytes()
            .map_err(|e| format!("Cannot decode saved-command registry '{}': {e}", unit.path))?;
        let commands = serde_json::from_slice::<Vec<SavedCommand>>(&bytes)
            .map_err(|e| format!("Invalid saved-command registry '{}': {e}", unit.path))?;
        for command in &commands {
            command.validate()?;
        }
        Ok(commands)
    }

    fn write_saved_commands_to_project(
        project: &mut neoflash_core::Project,
        commands: &[SavedCommand],
    ) -> Result<(), String> {
        if commands.is_empty() {
            project
                .resources
                .retain(|r| !r.path.eq_ignore_ascii_case(SAVED_COMMANDS_RESOURCE_PATH));
            return Ok(());
        }
        let bytes = serde_json::to_vec(commands).map_err(|e| e.to_string())?;
        let existing_id = project
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(SAVED_COMMANDS_RESOURCE_PATH))
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let unit = ResourceUnit::from_bytes(
            existing_id,
            SAVED_COMMANDS_RESOURCE_PATH,
            "application/json",
            &bytes,
        )
        .map_err(|e| e.to_string())?;
        if let Some(slot) = project
            .resources
            .iter_mut()
            .find(|r| r.path.eq_ignore_ascii_case(SAVED_COMMANDS_RESOURCE_PATH))
        {
            *slot = unit;
        } else {
            project.resources.push(unit);
        }
        project.format_version = project.format_version.max(3);
        Ok(())
    }

    /// Save the specified 0-based History undo steps into the project's `SavedCommand` registry (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_history_command(
        &mut self,
        name: impl Into<String>,
        step_indices: &[usize],
    ) -> Result<SavedCommand, String> {
        let cmd = self.save_history_steps_as_command(name, step_indices)?;
        self.register_saved_command(cmd.clone())?;
        Ok(cmd)
    }

    /// Insert or replace a `SavedCommand` in the project's command registry (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn register_saved_command(&mut self, command: SavedCommand) -> Result<(), String> {
        command.validate()?;
        let mut list = self.read_saved_commands()?;
        if let Some(existing) = list
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(&command.name))
        {
            *existing = command;
        } else {
            list.push(command);
        }
        let mut candidate = self.project.clone();
        Self::write_saved_commands_to_project(&mut candidate, &list)?;
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Save command", |editor| {
            editor.project = candidate;
        });
        Ok(())
    }

    /// Rename a saved command in the project's command registry (`Manage Saved Commands`, `T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rename_saved_command(
        &mut self,
        old_name: &str,
        new_name: impl Into<String>,
    ) -> Result<(), String> {
        let new_name = new_name.into();
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
            return Err("Command name must be 1..=128 non-control characters.".into());
        }
        let mut list = self.read_saved_commands()?;
        if list
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(trimmed) && !c.name.eq_ignore_ascii_case(old_name))
        {
            return Err(format!("A command named '{trimmed}' already exists."));
        }
        let Some(target) = list
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(old_name))
        else {
            return Err(format!("Saved command '{old_name}' not found."));
        };
        target.name = trimmed.to_owned();
        target.validate()?;
        let mut candidate = self.project.clone();
        Self::write_saved_commands_to_project(&mut candidate, &list)?;
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Rename command", |editor| {
            editor.project = candidate;
        });
        Ok(())
    }

    /// Delete a saved command by name (`Manage Saved Commands`, `T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn delete_saved_command(&mut self, name: &str) -> bool {
        let Ok(mut list) = self.read_saved_commands() else {
            return false;
        };
        let before = list.len();
        list.retain(|c| !c.name.eq_ignore_ascii_case(name));
        if list.len() == before {
            return false;
        }
        let mut candidate = self.project.clone();
        if Self::write_saved_commands_to_project(&mut candidate, &list).is_err() {
            return false;
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return false;
        }
        self.transact_named("Delete command", |editor| {
            editor.project = candidate;
        });
        true
    }

    /// Run a saved command by name on the current selection in one Undo step (`Run Command`, `T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn run_saved_command_by_name(&mut self, name: &str) -> Result<usize, String> {
        let cmd = self
            .read_saved_commands()?
            .into_iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("Saved command '{name}' not found."))?;
        self.run_saved_command(&cmd)
    }

    /// Return all installed `AuthoringExtensionManifest`s in the active project (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn installed_extensions(&self) -> Vec<AuthoringExtensionManifest> {
        self.read_installed_extensions().unwrap_or_default()
    }

    fn read_installed_extensions(&self) -> Result<Vec<AuthoringExtensionManifest>, String> {
        let Some(unit) = self
            .project
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(EXTENSIONS_RESOURCE_PATH))
        else {
            return Ok(Vec::new());
        };
        let bytes = unit.decoded_bytes().map_err(|e| {
            format!(
                "Cannot decode authoring-extension registry '{}': {e}",
                unit.path
            )
        })?;
        let manifests = serde_json::from_slice::<Vec<AuthoringExtensionManifest>>(&bytes)
            .map_err(|e| format!("Invalid authoring-extension registry '{}': {e}", unit.path))?;
        for manifest in &manifests {
            manifest.validate()?;
        }
        Ok(manifests)
    }

    /// Validate and install a capability-bounded `AuthoringExtensionManifest` (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn install_extension_manifest(
        &mut self,
        manifest_json: &str,
    ) -> Result<AuthoringExtensionManifest, String> {
        let manifest = AuthoringExtensionManifest::from_json(manifest_json)?;
        let mut exts = self.read_installed_extensions()?;
        if let Some(slot) = exts
            .iter_mut()
            .find(|e| e.id.eq_ignore_ascii_case(&manifest.id))
        {
            *slot = manifest.clone();
        } else {
            exts.push(manifest.clone());
        }
        let bytes = serde_json::to_vec(&exts).map_err(|e| e.to_string())?;
        let mut candidate = self.project.clone();
        let existing_id = candidate
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(EXTENSIONS_RESOURCE_PATH))
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let unit = ResourceUnit::from_bytes(
            existing_id,
            EXTENSIONS_RESOURCE_PATH,
            "application/json",
            &bytes,
        )
        .map_err(|e| e.to_string())?;
        if let Some(slot) = candidate
            .resources
            .iter_mut()
            .find(|r| r.path.eq_ignore_ascii_case(EXTENSIONS_RESOURCE_PATH))
        {
            *slot = unit;
        } else {
            candidate.resources.push(unit);
        }
        candidate.format_version = candidate.format_version.max(3);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Install extension", |editor| {
            editor.project = candidate;
        });
        Ok(manifest)
    }

    /// Execute a command from an installed `AuthoringExtensionManifest` after verifying its permissions (`T-167`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn run_extension_command(
        &mut self,
        extension_id: &str,
        command_name: &str,
    ) -> Result<usize, String> {
        let ext = self
            .read_installed_extensions()?
            .into_iter()
            .find(|e| e.id.eq_ignore_ascii_case(extension_id))
            .ok_or_else(|| format!("Extension '{extension_id}' is not installed."))?;
        ext.validate()?;
        let cmd = ext
            .commands
            .into_iter()
            .find(|c| c.name.eq_ignore_ascii_case(command_name))
            .ok_or_else(|| {
                format!("Command '{command_name}' not found in extension '{extension_id}'.")
            })?;
        self.run_saved_command(&cmd)
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
        let mut save_step_as_cmd = None;
        let mut run_saved_cmd_name = None;
        let mut delete_saved_cmd_name = None;
        let saved_cmds = self.saved_commands();

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
                        if ui
                            .add_enabled(can_replay, c::MenuItem::new("Save step as Command"))
                            .clicked()
                        {
                            save_step_as_cmd = Some((i, label.clone()));
                            ui.close();
                        }
                        if !saved_cmds.is_empty() {
                            c::menu_separator(ui);
                            for scmd in &saved_cmds {
                                if ui
                                    .add_enabled(
                                        !self.selection.is_empty(),
                                        c::MenuItem::new(&format!("Run “{}”", scmd.name)),
                                    )
                                    .clicked()
                                {
                                    run_saved_cmd_name = Some(scmd.name.clone());
                                    ui.close();
                                }
                                if ui
                                    .add(c::MenuItem::new(&format!("Delete “{}”", scmd.name)))
                                    .clicked()
                                {
                                    delete_saved_cmd_name = Some(scmd.name.clone());
                                    ui.close();
                                }
                            }
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
        if let Some((idx, lbl)) = save_step_as_cmd {
            match self.save_history_command(lbl, &[idx]) {
                Ok(cmd) => self.message(format!("Saved command “{}”.", cmd.name)),
                Err(err) => self.message(err),
            }
        }
        if let Some(name) = run_saved_cmd_name
            && let Err(err) = self.run_saved_command_by_name(&name)
        {
            self.message(err);
        }
        if let Some(name) = delete_saved_cmd_name {
            self.delete_saved_command(&name);
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
