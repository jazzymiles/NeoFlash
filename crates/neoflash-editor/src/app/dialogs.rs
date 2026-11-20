use super::*;

impl Editor {
    pub(crate) fn has_modal_dialog(&self) -> bool {
        self.confirm.is_some()
            || self.show_about
            || self.workspace_dialog.is_some()
            || self.bitmap_trace.is_open()
            || self.symbol_conversion.is_some()
            || self.timeline_motion.paste_special_open
            || self.document_settings.is_some()
            || self.publish_settings.is_some()
            || self.export_lottie.is_some()
            || self.export_video.is_some()
            || self.preferences.is_some()
            || self.ai_connection_modal.is_some()
            || self.ai_session.has_open_modal()
            || self.edit_envelope.is_some()
            || self.authoring_modify.scale_and_rotate_dialog.is_some()
            || self.authoring_tools.polystar_dialog.is_some()
            || self.authoring_tools.magic_wand_dialog.is_some()
            || self.find_replace.is_some()
            || self.beta_feedback.open
            || self.confirm_revert
            || self.has_external_change_prompt()
            || (self.recovery.has_prompt() && (!self.dirty() || !self.sessions.is_empty()))
    }
    pub(crate) fn has_modal_dialog_in(&self, ctx: &egui::Context) -> bool {
        self.has_modal_dialog()
            || self.customize_tools_open(ctx)
            || crate::bitmaps::has_bitmap_properties_dialog(ctx)
            || crate::panels::export_audio::has_export_audio_dialog(ctx)
            || crate::panels::export_image::has_export_image_dialog(ctx)
            || crate::panels::figma_import::has_figma_import_dialog(ctx)
            || crate::panels::import_video::has_import_video_dialog(ctx)
            || crate::panels::stroke_style::has_stroke_style_dialog(ctx)
            || crate::panels::font_embedding::has_font_embedding_dialog(ctx)
            || crate::panels::sound_properties::has_sound_properties_dialog(ctx)
            || crate::panels::layer_properties::has_layer_properties_dialog(ctx)
            || crate::panels::productivity_tools::has_productivity_modal(ctx)
            || crate::panels::help_manual::is_help_manual_open(ctx)
            || crate::panels::ui_studio::is_studio_open(ctx)
    }

    pub fn draw_dialogs(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        self.drain_converted_entry_triggers(ctx);
        self.document_settings_dialog(ctx);
        self.publish_settings_dialog(ctx);
        self.export_lottie_modal(ctx);
        self.export_video_modal(ctx);
        if let Some(font) = self
            .preferences
            .as_ref()
            .map(|draft| draft.code_editor.font.clone())
        {
            self.fonts.ensure_loaded(ctx, &font);
        }
        self.preferences_dialog(ctx);
        self.ai_dialogs(ctx);
        self.find_replace_dialog(ctx);
        self.edit_envelope_dialog(ctx);
        self.paste_motion_special_dialog(ctx);
        self.sound_properties_dialog(ctx);
        if crate::bitmaps::has_bitmap_properties_dialog(ctx) {
            self.bitmap_properties_dialog(ctx);
        }
        self.export_audio_dialog(ctx);
        self.export_image_dialog(ctx);
        self.figma_import_dialog(ctx);
        self.import_video_dialog(ctx);
        self.stroke_style_dialog(ctx);
        self.font_embedding_dialog(ctx);
        self.layer_properties_dialog(ctx);
        self.polystar_settings_modal(ctx);
        self.scale_and_rotate_modal(ctx);
        self.help_manual_dialog(ctx);
        self.beta_feedback_dialog(ctx);
        use crate::panels::dialog_helpers::{FormDialog, confirm_message, dialog_width};
        use neoflash_ui::{
            TextStyle,
            components::{Button, Surface, logo},
            metrics,
            tokens::space,
            typography::rich,
        };
        let p = neoflash_ui::palette(ctx);
        // Confirmations: the question is the title, one warning line, `dialog_s` wide.
        let confirm_width = dialog_width(ctx, metrics::dialog_s());
        if self.show_about {
            let mut close = false;
            let mut open_manual = false;
            let width = dialog_width(ctx, metrics::dialog_m());
            let modal = egui::Modal::new(egui::Id::new("about-neoflash"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    let dialog = FormDialog::show(ui, "About NeoFlash", width, None);
                    close |= dialog.close;
                    dialog.page(ui, "about-page", |ui, _| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::s3();
                            logo(ui, 40.);
                            let id = crate::build_info::BuildIdentity::current();
                            ui.vertical(|ui| {
                                #[cfg(target_os = "macos")]
                                let edition = "macOS Beta";
                                #[cfg(target_os = "windows")]
                                let edition = "Windows Technical Preview";
                                #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
                                let edition = "Linux Technical Preview";
                                ui.label(
                                    rich(
                                        format!("NeoFlash {} — {edition}", id.version),
                                        TextStyle::Title,
                                    )
                                    .color(p.text_primary),
                                );
                                ui.label(
                                    rich(id.about_subtitle(), TextStyle::Small)
                                        .color(p.text_secondary),
                                );
                            });
                        });
                        for line in [
                            "Classic Flash workflow. Rust core. HTML5 publishing.",
                            "This is an early authoring foundation, not complete Flash compatibility.",
                            "See docs/COMPATIBILITY.md and docs/ROADMAP.md for the exact scope.",
                        ] {
                            ui.add(
                                egui::Label::new(
                                    rich(line, TextStyle::DialogBody).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    });
                    dialog.footer(
                        ui,
                        |ui| open_manual |= ui.add(Button::ghost("Help & Reference…")).clicked(),
                        |ui| close |= ui.add(Button::primary("Close").shortcut("↵")).clicked(),
                    );
                });
            let enter = !crate::panels::dialog_helpers::blocks_local_dialog_enter(ctx)
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if open_manual {
                self.show_about = false;
                crate::panels::help_manual::open_help_manual(ctx);
            } else if close || enter || modal.should_close() {
                self.show_about = false;
            }
        }
        if self.confirm_revert {
            // 0 = Revert, 1 = Cancel.
            let mut action = None;
            let name = self.project.name.clone();
            let modal = egui::Modal::new(egui::Id::new("revert-changes"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    let dialog =
                        FormDialog::show(ui, "Revert to saved version?", confirm_width, None);
                    let (mut revert, mut cancel) = (false, dialog.close);
                    dialog.page(ui, "revert-page", |ui, _| {
                        confirm_message(
                            ui,
                            &format!(
                                "Your unsaved changes to “{name}” will be lost and the last saved version on disk will be reloaded."
                            ),
                        );
                    });
                    dialog.footer(
                        ui,
                        |_| {},
                        |ui| {
                            revert = ui.add(Button::danger("Revert").shortcut("↵")).clicked();
                            cancel |= ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
                        },
                    );
                    if revert {
                        action = Some(0);
                    } else if cancel {
                        action = Some(1);
                    }
                });
            if action.is_none() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(0);
                } else if modal.should_close() {
                    action = Some(1);
                }
            }
            if let Some(action) = action {
                self.confirm_revert = false;
                if action == 0 {
                    self.revert_to_saved(ctx);
                }
            }
        }
        if self.confirm.is_some() {
            // 0 = Save, 1 = Discard, 2 = Cancel.
            let mut action = None;
            let name = self.project.name.clone();
            let modal = egui::Modal::new(egui::Id::new("unsaved-changes"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    let dialog = FormDialog::show(
                        ui,
                        "Save changes before continuing?",
                        confirm_width,
                        None,
                    );
                    let (mut discard, mut save, mut cancel) = (false, false, dialog.close);
                    dialog.page(ui, "unsaved-page", |ui, _| {
                        confirm_message(
                            ui,
                            &format!("Your changes to “{name}” will be lost if you discard them."),
                        );
                    });
                    dialog.footer(
                        ui,
                        |ui| discard = ui.add(Button::danger("Discard")).clicked(),
                        |ui| {
                            save = ui.add(Button::primary("Save").shortcut("↵")).clicked();
                            cancel |= ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
                        },
                    );
                    if save {
                        action = Some(0);
                    } else if discard {
                        action = Some(1);
                    } else if cancel {
                        action = Some(2);
                    }
                });
            if action.is_none() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(0);
                } else if modal.should_close() {
                    action = Some(2);
                }
            }
            if let Some(action) = action {
                if action == 2 {
                    self.cancel_confirmation(ctx);
                } else if action == 1 {
                    self.continue_confirmation(ctx);
                } else if action == 0 {
                    let saved = self.save(false);
                    self.resolve_confirmation_save(ctx, saved);
                }
            }
        }
        if self.confirm.is_none()
            && let Some((title, summary, can_reload)) = self.external_change_prompt_summary()
        {
            // 0 = Reload, 1 = Keep editing, 2 = Save As.
            let mut action = None;
            let dirty = self.dirty();
            let mut dismiss = false;
            let modal = egui::Modal::new(egui::Id::new("external-file-change"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    let dialog = FormDialog::show(ui, title, confirm_width, None);
                    dismiss |= dialog.close;
                    dialog.page(ui, "external-change-page", |ui, _| {
                        confirm_message(ui, &summary)
                    });
                    let (mut reload, mut keep, mut save_as) = (false, false, false);
                    dialog.footer(
                        ui,
                        |ui| {
                            if can_reload {
                                reload = ui
                                    .add(if dirty {
                                        Button::danger("Reload")
                                    } else {
                                        Button::primary("Reload").shortcut("↵")
                                    })
                                    .clicked();
                            }
                        },
                        |ui| {
                            // Right edge inwards: Keep editing, then Save As….
                            keep = ui
                                .add(if dirty || !can_reload {
                                    Button::primary("Keep editing").shortcut("↵")
                                } else {
                                    Button::new("Keep editing").shortcut("Esc")
                                })
                                .clicked();
                            save_as = ui.add(Button::new("Save As…")).clicked();
                        },
                    );
                    if reload {
                        action = Some(0);
                    } else if keep {
                        action = Some(1);
                    } else if save_as {
                        action = Some(2);
                    }
                });
            if action.is_none() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(if !dirty && can_reload { 0 } else { 1 });
                } else if dismiss || modal.should_close() {
                    self.dismiss_external_change_prompt();
                }
            }
            match action {
                Some(0) => {
                    let _ = self.reload_external_modification(ctx);
                }
                Some(1) => {
                    let _ = self.keep_editing_external_change();
                }
                Some(2) => {
                    let _ = self.save(true);
                }
                _ => {}
            }
        }
        if self.confirm.is_none()
            && !self.has_external_change_prompt()
            && (!self.dirty() || !self.sessions.is_empty())
            && let Some(summary) = self
                .recovery
                .current_prompt_summary(crate::recovery::unix_now_secs())
        {
            // 0 = Restore, 1 = Discard.
            let mut action = None;
            let modal = egui::Modal::new(egui::Id::new("recovery-prompt"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    // No close button: the prompt is answered with Restore or Discard.
                    let dialog = FormDialog::prompt(ui, "Recover unsaved work?", confirm_width);
                    dialog.page(ui, "recovery-page", |ui, _| confirm_message(ui, &summary));
                    let (mut discard, mut restore) = (false, false);
                    dialog.footer(
                        ui,
                        |ui| discard = ui.add(Button::danger("Discard")).clicked(),
                        |ui| {
                            restore = ui.add(Button::primary("Restore").shortcut("↵")).clicked();
                        },
                    );
                    if restore {
                        action = Some(0);
                    } else if discard {
                        action = Some(1);
                    }
                });
            let _ = modal;
            if action.is_none()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                action = Some(0);
            }
            match action {
                Some(0) => self.restore_recovery_prompt(ctx),
                Some(1) => self.discard_recovery_prompt(),
                _ => {}
            }
        }
    }
}
