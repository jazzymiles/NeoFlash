//! Approved v2.2 Brushes and Brush/Pencil properties (R-038).
use crate::{app::Editor, tool_preferences::PencilMode, workspace::PanelId};
use eframe::egui::{self, Color32};
use neoflash_ui::{
    components as nf,
    icons::{i16, i20},
    tokens::{TextStyle, space},
    typography::rich,
};

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(rich(text, TextStyle::Small).color(neoflash_ui::palette(ui.ctx()).text_tertiary));
}
fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.;
        ui.allocate_ui_with_layout(
            egui::vec2(60., 32.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(60.);
                ui.label(rich(label, TextStyle::Body));
            },
        );
        let width = (ui.available_width() - nf::toggle::SLIDER_FIELD - 10.).max(24.);
        ui.add(
            nf::Slider::new(value, range, label)
                .width(width)
                .value(nf::SliderValue::Field)
                .unit(unit),
        );
    });
}
fn slider_range(ui: &mut egui::Ui, minimum: &str, maximum: &str) {
    let rail_width = (ui.available_width() - 70. - nf::toggle::SLIDER_FIELD - 10.).max(24.);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.;
        ui.add_space(70.);
        ui.allocate_ui_with_layout(
            egui::vec2(rail_width, 14.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                note(ui, minimum);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    note(ui, maximum)
                });
            },
        );
    });
}
impl Editor {
    pub(crate) fn brushes_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = space::S3;
        let width = ui.available_width();
        ui.horizontal(|ui| {
            ui.add(
                nf::Search::new(&mut self.authoring_tools.brush_search, "Find brush")
                    .width((width - 40.).max(72.)),
            );
            if ui
                .add(nf::IconButton::new(i16::SETTINGS, "Brush options"))
                .clicked()
            {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("properties-tool-tab"), true));
                self.show_panel(PanelId::Properties);
            }
        });
        nf::SectionHeader::new("Basic").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            for (label, size, shape, sample) in [
                (
                    "Round",
                    12.,
                    nf::BrushShapeKind::Round,
                    nf::BrushStroke::Round,
                ),
                (
                    "Flat",
                    16.,
                    nf::BrushShapeKind::RectH,
                    nf::BrushStroke::Flat,
                ),
                (
                    "Chisel",
                    10.,
                    nf::BrushShapeKind::Slash,
                    nf::BrushStroke::Chisel,
                ),
            ] {
                if !label
                    .to_lowercase()
                    .contains(&self.authoring_tools.brush_search.to_lowercase())
                {
                    continue;
                }
                if ui
                    .add(
                        nf::BrushPreset::new(label, &format!("{size:.0}"))
                            .stroke(sample)
                            .selected(self.authoring_tools.brush_shape == shape)
                            .width(ui.available_width()),
                    )
                    .clicked()
                {
                    self.authoring_tools.brush_shape = shape;
                    self.authoring_tools.brush_size = size;
                }
            }
        });
        let mut size = self.authoring_tools.brush_size as f64;
        slider(ui, "Size", &mut size, 1. ..=200., "px");
        self.authoring_tools.brush_size = size as f32;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            ui.add_enabled(false, nf::IconButton::new(i16::PLUS, "New brush"))
                .on_hover_text("Custom brush presets are not available yet");
            ui.add_enabled(
                false,
                nf::IconButton::new(i16::FOLDERPLUS, "New brush group"),
            )
            .on_hover_text("Custom brush groups are not available yet");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled(false, nf::IconButton::new(i16::TRASH, "Delete brush"))
                    .on_hover_text("Built-in brushes cannot be deleted");
            });
        });
    }
    pub(crate) fn brush_tool_contents(&mut self, ui: &mut egui::Ui) {
        ui.add(nf::TypeHeader {
            icon: i20::TOOL_BRUSH,
            title: "Brush tool",
            subtitle: "Paints fills · B",
        });
        nf::SectionHeader::new("Brush mode").show(ui, |ui| {
            nf::Select::new("brush-mode", "Paint normal").width(ui.available_width()).show(ui, |ui| {
                ui.add(nf::MenuItem::new("Paint normal").checked(true));
                for label in ["Paint fills", "Paint behind", "Paint selection", "Paint inside"] {
                    ui.add_enabled(false, nf::MenuItem::new(label)).on_hover_text("Not available yet");
                }
            });
            note(ui, "Normal · Fills · Behind · Selection · Inside — where paint lands relative to existing shapes.");
        });
        nf::SectionHeader::new("Shape and size").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                for shape in nf::BrushShapeKind::ALL {
                    if ui
                        .add(
                            nf::BrushShape::new(shape)
                                .selected(self.authoring_tools.brush_shape == shape),
                        )
                        .clicked()
                    {
                        self.authoring_tools.brush_shape = shape;
                    }
                }
            });
            let mut size = self.authoring_tools.brush_size as f64;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                slider(ui, "Size", &mut size, 1. ..=200., "px");
                slider_range(ui, "1", "200");
            });
            self.authoring_tools.brush_size = size as f32;
            slider(
                ui,
                "Smoothing",
                &mut self.authoring_tools.brush_smoothing,
                0. ..=100.,
                "",
            );
        });
        nf::SectionHeader::new("Pen pressure and tilt").show(ui, |ui| {
            ui.horizontal(|ui| {
                neoflash_ui::icons::show(
                    ui,
                    i16::PRESSURE,
                    neoflash_ui::palette(ui.ctx()).text_secondary,
                );
                ui.add(nf::Switch::new(
                    &mut self.authoring_tools.brush_pressure,
                    "Pressure controls size",
                ));
            });
            ui.horizontal(|ui| {
                neoflash_ui::icons::show(
                    ui,
                    i16::TILT,
                    neoflash_ui::palette(ui.ctx()).text_secondary,
                );
                ui.add_enabled(false, nf::Switch::new(&mut false, "Tilt controls angle"))
                    .on_hover_text("Tablet tilt input is not available yet");
            });
        });
        nf::SectionHeader::new("Fill").show(ui, |ui| {
            ui.horizontal(|ui| {
                self.tool_color_well(ui, false);
                ui.add(nf::Checkbox::new(
                    &mut self.authoring_tools.brush_lock_fill,
                    "Lock fill",
                ));
            });
        });
        if ui
            .add(nf::Button::new("Brush library…").width(ui.available_width()))
            .clicked()
        {
            self.show_panel(PanelId::Brushes);
        }
    }
    pub(crate) fn pencil_tool_contents(&mut self, ui: &mut egui::Ui) {
        ui.add(nf::TypeHeader {
            icon: i20::TOOL_PENCIL,
            title: "Pencil tool",
            subtitle: "Draws strokes · Y",
        });
        nf::SectionHeader::new("Pencil mode").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.;
                let modes = [PencilMode::Straighten, PencilMode::Smooth, PencilMode::Ink];
                if let Some(index) = (nf::IconGroup {
                    items: &[(i16::PENCIL_STRAIGHTEN, "Straighten"), (i16::PENCIL_SMOOTH, "Smooth"), (i16::PENCIL_INK, "Ink")],
                    selected: modes.iter().position(|mode| *mode == self.workspace.tools.pencil_mode),
                }).show(ui) { self.workspace.tools.pencil_mode = modes[index]; }
                ui.label(rich(self.workspace.tools.pencil_mode.label(), TextStyle::Body));
            });
            note(ui, "Straighten turns strokes into lines and arcs, Smooth evens out the curve, Ink keeps the stroke as drawn.");
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                ui.add_enabled_ui(self.workspace.tools.pencil_mode != PencilMode::Ink, |ui| {
                    slider(ui, "Smoothing", &mut self.workspace.tools.pencil_smoothing, 0. ..=100., "");
                });
                slider_range(ui, "0", "100");
            });
        });
        nf::SectionHeader::new("Stroke").show(ui, |ui| {
            ui.horizontal(|ui| {
                self.tool_color_well(ui, true);
                ui.add(
                    nf::Field::new(&mut self.stroke_width, "Stroke width")
                        .decimals(2)
                        .step(0.25)
                        .range(0.1..=100.)
                        .presets(&[0.25, 0.5, 1., 2., 4., 8.])
                        .width(ui.available_width()),
                );
            });
            ui.add_enabled_ui(false, |ui| {
                nf::LineStyleSelect::show(
                    ui,
                    "pencil-line-style",
                    &mut nf::LineStyle::Solid,
                    ui.available_width(),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.;
                    ui.label(rich("Cap", TextStyle::Body));
                    nf::IconGroup {
                        items: &[
                            (i16::CAP_NONE, "None"),
                            (i16::CAP_ROUND, "Round"),
                            (i16::CAP_SQUARE, "Square"),
                        ],
                        selected: Some(1),
                    }
                    .show(ui);
                    ui.label(rich("Join", TextStyle::Body));
                    nf::IconGroup {
                        items: &[
                            (i16::JOIN_MITER, "Miter"),
                            (i16::JOIN_ROUND, "Round"),
                            (i16::JOIN_BEVEL, "Bevel"),
                        ],
                        selected: Some(1),
                    }
                    .show(ui);
                });
                ui.horizontal(|ui| {
                    ui.add(nf::Switch::new(&mut false, "Stroke hinting"));
                    nf::Select::new("pencil-scale", "Scale: Normal")
                        .width(ui.available_width())
                        .show(ui, |_| {});
                });
            })
            .response
            .on_hover_text("Advanced stroke styles are not available yet");
        });
    }
    fn tool_color_well(&mut self, ui: &mut egui::Ui, stroke: bool) {
        let color = if stroke { self.stroke } else { self.fill };
        if ui
            .add(nf::ColorWell {
                target: if stroke {
                    nf::ColorWellTarget::Stroke
                } else {
                    nf::ColorWellTarget::Fill
                },
                color: Some(Color32::from_rgba_unmultiplied(
                    color[0], color[1], color[2], color[3],
                )),
                active: true,
            })
            .clicked()
        {
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("color-panel-active-stroke"), stroke));
            self.authoring_modify.color_selection = false;
            self.show_panel(PanelId::Color);
        }
    }
}

#[cfg(test)]
#[path = "../tool_workflow_tests.rs"]
mod tests;
