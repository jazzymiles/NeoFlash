//! R-006: Paste motion special (v3 `D2.pasteMotion`): property checkboxes in two equal columns,
//! two override options, `Select none` as the ghost button of the footer.
use super::dialog_helpers::{FormDialog, dialog_width, form_line, group_caption};
use crate::app::Editor;
use eframe::egui;
use neoflash_ui::{components as c, metrics, tokens::space};

impl Editor {
    pub(crate) fn paste_motion_special_dialog(&mut self, ctx: &egui::Context) {
        if !self.timeline_motion.paste_special_open {
            return;
        }
        let mut options = self.timeline_motion.paste_options.clone();
        let mut cancel = false;
        let mut paste = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let nf = neoflash_ui::palette(ctx);
        let width = dialog_width(ctx, metrics::dialog_m());
        let can_paste = options.any() && self.can_paste_motion();
        let modal = egui::Modal::new(egui::Id::new("paste-motion-special"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Paste motion special", width, None);
                cancel |= dialog.close;
                dialog.page(ui, "paste-motion-special", |ui, _| {
                    group_caption(ui, "Properties");
                    // Nine property checkboxes in two equal columns, one pair per row.
                    let cell = ((ui.available_width() - space::s2()) / 2.0).floor();
                    for pair in [
                        [(0, "X position"), (1, "Y position")],
                        [(2, "Horizontal scale"), (3, "Vertical scale")],
                        [(4, "Rotation and skew"), (5, "Colour")],
                        [(6, "Filters"), (7, "Blend mode")],
                    ] {
                        form_line(ui, |ui| {
                            for (index, label) in pair {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(cell, metrics::control()),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(cell);
                                        ui.add(c::Checkbox::new(
                                            &mut options.properties[index],
                                            label,
                                        ));
                                    },
                                );
                            }
                        });
                    }
                    form_line(ui, |ui| {
                        ui.add(c::Checkbox::new(&mut options.properties[8], "Ease"));
                    });
                    group_caption(ui, "Options");
                    form_line(ui, |ui| {
                        ui.add(c::Checkbox::new(
                            &mut options.preserve_scale,
                            "Override target scale properties",
                        ));
                    });
                    form_line(ui, |ui| {
                        ui.add(c::Checkbox::new(
                            &mut options.preserve_rotation,
                            "Override target rotation and skew properties",
                        ));
                    });
                });
                let mut none = false;
                dialog.footer(
                    ui,
                    |ui| {
                        none = ui.add(c::Button::ghost("Select none")).clicked();
                    },
                    |ui| {
                        paste |= ui
                            .add_enabled(can_paste, c::Button::primary("Paste"))
                            .clicked();
                        cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
                if none {
                    options.properties.fill(false);
                }
            });
        if !cancel && !paste {
            match kb.resolve(ctx, &modal, options.any() && self.can_paste_motion()) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => paste = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        self.timeline_motion.paste_options = options.clone();
        if cancel {
            self.timeline_motion.paste_special_open = false;
        } else if paste {
            self.timeline_motion.paste_special_open = false;
            self.paste_motion_special(&options);
        }
    }
}
