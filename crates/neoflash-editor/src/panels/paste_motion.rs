//! R-006: approved Paste motion special composition, existing NF2 components only.
use crate::app::Editor;
use eframe::egui;
use neoflash_ui::{TextStyle, components as c, tokens::space, typography::rich};

impl Editor {
    pub(crate) fn paste_motion_special_dialog(&mut self, ctx: &egui::Context) {
        if !self.timeline_motion.paste_special_open {
            return;
        }
        let mut options = self.timeline_motion.paste_options.clone();
        let mut cancel = false;
        let mut paste = false;
        let nf = neoflash_ui::palette(ctx);
        let modal = egui::Modal::new(egui::Id::new("paste-motion-special"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                ui.set_width(440.);
                cancel |= c::DialogHeader {
                    title: "Paste motion special",
                }
                .show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(440. - 2. * space::S5);
                        ui.spacing_mut().item_spacing.y = space::S3;
                        ui.label(rich("PROPERTIES", TextStyle::Overline).color(nf.text_secondary));
                        ui.columns(2, |columns| {
                            for (column, choices) in [
                                &[
                                    (0, "X position"),
                                    (2, "Horizontal scale"),
                                    (4, "Rotation and skew"),
                                    (6, "Filters"),
                                    (8, "Ease"),
                                ][..],
                                &[
                                    (1, "Y position"),
                                    (3, "Vertical scale"),
                                    (5, "Colour"),
                                    (7, "Blend mode"),
                                ][..],
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                columns[column].spacing_mut().item_spacing.y = 10.;
                                for &(index, label) in choices {
                                    columns[column].add(c::Checkbox::new(
                                        &mut options.properties[index],
                                        label,
                                    ));
                                }
                            }
                        });
                        ui.label(rich("OPTIONS", TextStyle::Overline).color(nf.text_secondary));
                        ui.add(c::Checkbox::new(
                            &mut options.preserve_scale,
                            "Override target scale properties",
                        ));
                        ui.add(c::Checkbox::new(
                            &mut options.preserve_rotation,
                            "Override target rotation and skew properties",
                        ));
                    });
                let mut none = false;
                c::surface::dialog_footer(
                    ui,
                    |ui| {
                        none = ui.add(c::Button::ghost("Select none")).clicked();
                    },
                    |ui| {
                        paste |= ui
                            .add_enabled(
                                options.any() && self.can_paste_motion(),
                                c::Button::primary("Paste"),
                            )
                            .clicked();
                        cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
                if none {
                    options.properties.fill(false);
                }
            });
        paste |= options.any()
            && self.can_paste_motion()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        self.timeline_motion.paste_options = options.clone();
        if cancel || modal.should_close() {
            self.timeline_motion.paste_special_open = false;
        } else if paste {
            self.timeline_motion.paste_special_open = false;
            self.paste_motion_special(&options);
        }
    }
}
