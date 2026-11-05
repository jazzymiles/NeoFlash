//! Approved R-036 workspace menu and draft-only naming/confirmation dialogs.
use crate::app::Editor;
use crate::workspace::{WorkspacePrefs, WorkspacePresetError, WorkspaceTemplate};
use eframe::egui;
use neoflash_ui::{components as nf, tokens::space};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceDialog {
    Save {
        name: String,
        focus: bool,
    },
    Rename {
        original: String,
        name: String,
        focus: bool,
    },
    Delete {
        name: String,
    },
}

impl WorkspaceDialog {
    fn title(&self) -> String {
        match self {
            Self::Save { .. } => "Save workspace".into(),
            Self::Rename { original, .. } => format!("Rename “{original}”"),
            Self::Delete { name } => format!("Delete “{name}”?"),
        }
    }

    pub(crate) fn validation(&self, prefs: &WorkspacePrefs) -> Result<bool, WorkspacePresetError> {
        match self {
            Self::Save { name, .. } => prefs.check_named_preset(name, None),
            Self::Rename { original, name, .. } => prefs.check_named_preset(name, Some(original)),
            Self::Delete { name } => prefs
                .named_presets
                .contains_key(name)
                .then_some(false)
                .ok_or(WorkspacePresetError::NotFound),
        }
    }
}

fn error_label(error: WorkspacePresetError) -> &'static str {
    match error {
        WorkspacePresetError::InvalidName => {
            "Enter a name of 1–64 characters without control characters."
        }
        WorkspacePresetError::ReservedName => "This name is reserved for a built-in workspace.",
        WorkspacePresetError::AlreadyExists => "A workspace with this name already exists.",
        WorkspacePresetError::NotFound => "This workspace is no longer available.",
        WorkspacePresetError::LimitReached => {
            "You can save up to 32 workspaces. Delete one to add another."
        }
    }
}

impl Editor {
    pub(crate) fn named_workspace_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        nf::menu_section(ui, "Built-in");
        for template in WorkspaceTemplate::ALL {
            let checked = self.workspace.active_named_preset.is_none()
                && !self.workspace.unsaved_layout
                && self.workspace.template == template;
            if ui
                .add(nf::MenuItem::new(template.label()).checked(checked))
                .on_hover_text(template.description())
                .clicked()
            {
                self.apply_workspace_template(ctx, template);
                ui.close();
            }
        }
        nf::menu_section(ui, "My workspaces");
        let names: Vec<_> = self.workspace.named_presets.keys().cloned().collect();
        for name in names {
            let checked = self.workspace.active_named_preset.as_deref() == Some(&name);
            if ui.add(nf::MenuItem::new(&name).checked(checked)).clicked() {
                self.apply_named_workspace(ctx, &name);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if ui.add(nf::MenuItem::new("Save workspace…")).clicked() {
            self.workspace_dialog = Some(WorkspaceDialog::Save {
                name: String::new(),
                focus: true,
            });
            ui.close();
        }
        let active = self.workspace.active_named_preset.clone();
        let label = self.workspace.active_workspace_label().to_owned();
        if ui
            .add_enabled(
                active.is_some(),
                nf::MenuItem::new(&format!("Update “{label}”")),
            )
            .clicked()
        {
            if let Some(name) = &active {
                let _ = self.save_named_workspace(name, true);
            }
            ui.close();
        }
        if ui
            .add_enabled(
                active.is_some(),
                nf::MenuItem::new(&format!("Rename “{label}”…")),
            )
            .clicked()
        {
            if let Some(name) = &active {
                self.workspace_dialog = Some(WorkspaceDialog::Rename {
                    original: name.clone(),
                    name: name.clone(),
                    focus: true,
                });
            }
            ui.close();
        }
        if ui
            .add_enabled(
                active.is_some(),
                nf::MenuItem::new(&format!("Delete “{label}”…")),
            )
            .clicked()
        {
            if let Some(name) = &active {
                self.workspace_dialog = Some(WorkspaceDialog::Delete { name: name.clone() });
            }
            ui.close();
        }
        nf::menu_separator(ui);
        if ui
            .add_enabled(
                !self.workspace.unsaved_layout,
                nf::MenuItem::new(&format!("Reset “{label}”")),
            )
            .clicked()
        {
            self.reset_active_workspace(ctx);
            ui.close();
        }
    }

    pub(crate) fn reset_active_workspace(&mut self, ctx: &egui::Context) {
        if let Some(name) = self.workspace.active_named_preset.clone() {
            self.apply_named_workspace(ctx, &name);
        } else if !self.workspace.unsaved_layout {
            self.apply_workspace_template(ctx, self.workspace.template);
        }
    }

    /// Apply a validated dialog. Save replacement is an explicit click on Replace; Rename never replaces.
    pub(crate) fn commit_workspace_dialog(
        &mut self,
        draft: &WorkspaceDialog,
    ) -> Result<(), WorkspacePresetError> {
        let replace = draft.validation(&self.workspace)?;
        match draft {
            WorkspaceDialog::Save { name, .. } => self.save_named_workspace(name, replace),
            WorkspaceDialog::Rename { original, name, .. } => {
                self.workspace.rename_named_preset(original, name, false)
            }
            WorkspaceDialog::Delete { name } => {
                self.workspace.delete_named_preset(name);
                Ok(())
            }
        }
    }

    pub(crate) fn workspace_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.workspace_dialog.take() else {
            return;
        };
        let mut close = false;
        let mut submit = false;
        let title = draft.title();
        let modal = egui::Modal::new(egui::Id::new("workspace-management"))
            .frame(nf::Surface::Dialog.frame(ctx))
            .backdrop_color(neoflash_ui::palette(ctx).overlay)
            .show(ctx, |ui| {
                ui.set_width(440.);
                close |= nf::DialogHeader { title: &title }.show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: space::S5 as i8,
                        right: space::S5 as i8,
                        top: 0,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        if let WorkspaceDialog::Save { name, focus }
                        | WorkspaceDialog::Rename { name, focus, .. } = &mut draft
                        {
                            let field =
                                ui.add(nf::TextField::new(name, "Workspace name").prefix("Name"));
                            if *focus {
                                ui.memory_mut(|m| {
                                    m.request_focus(nf::TextField::edit_id(field.id))
                                });
                                *focus = false;
                            }
                        }
                        match draft.validation(&self.workspace) {
                            Err(error) => {
                                ui.add_space(space::S2);
                                ui.add(nf::InlineMessage::error(error_label(error)));
                            }
                            Ok(true) if matches!(draft, WorkspaceDialog::Save { .. }) => {
                                ui.add_space(space::S2);
                                ui.add(nf::InlineMessage::warning(
                                    "A workspace with this name exists. Replace its saved layout?",
                                ));
                            }
                            _ => {}
                        }
                    });
                let validation = draft.validation(&self.workspace);
                let primary = match &draft {
                    WorkspaceDialog::Save { .. } if validation == Ok(true) => "Replace",
                    WorkspaceDialog::Save { .. } => "Save",
                    WorkspaceDialog::Rename { .. } => "Rename",
                    WorkspaceDialog::Delete { .. } => "Delete",
                };
                nf::surface::dialog_footer(
                    ui,
                    |_| {},
                    |ui| {
                        let button = if matches!(draft, WorkspaceDialog::Delete { .. }) {
                            nf::Button::danger(primary)
                        } else {
                            nf::Button::primary(primary)
                        };
                        submit |= ui
                            .add_enabled(validation.is_ok(), button.shortcut("↵"))
                            .clicked();
                        close |= ui.add(nf::Button::new("Cancel").shortcut("Esc")).clicked();
                    },
                );
            });
        close |= modal.should_close();
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        submit |= enter && draft.validation(&self.workspace).is_ok();
        if !close && submit && self.commit_workspace_dialog(&draft).is_ok() {
            return;
        }
        if !close {
            self.workspace_dialog = Some(draft);
        }
    }
}
