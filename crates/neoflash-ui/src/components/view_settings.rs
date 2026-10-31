//! `NF2/View Settings` — rulers, guides & grid popover (320 × 542), opened from ⚙ in the canvas
//! `NF2/Zoom Control` and from View ▸ Rulers, guides & grid….
use super::{Button, Checkbox, ColorField, Field, IconButton, Segmented, Select};
use crate::icons::{self, Icon, i16};
use crate::theme::palette;
use crate::tokens::{Palette, TextStyle, space};
use eframe::egui::{self, Color32, Ui};

/// Values shown by the popover. Defaults follow the reference (guide colour = `color/guide`,
/// grid colour = `color/stage-grid`, 20 px linked grid, labels Auto).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSettingsState {
    pub show_rulers: bool,
    pub units: &'static str,
    pub labels_auto: bool,
    pub label_every: f64,
    pub guides_show: bool,
    pub guides_snap: bool,
    pub guides_lock: bool,
    pub guide_color: Color32,
    pub guide_snap: &'static str,
    pub grid_show: bool,
    pub grid_snap: bool,
    pub grid_over_objects: bool,
    pub grid_w: f64,
    pub grid_h: f64,
    pub grid_linked: bool,
    pub grid_color: Color32,
    pub grid_snap_accuracy: &'static str,
}

impl ViewSettingsState {
    pub fn defaults(p: &Palette) -> Self {
        Self {
            show_rulers: true,
            units: "Units: Pixels",
            labels_auto: true,
            label_every: 100.0,
            guides_show: true,
            guides_snap: true,
            guides_lock: false,
            guide_color: p.guide,
            guide_snap: "Snap: Normal",
            grid_show: true,
            grid_snap: false,
            grid_over_objects: false,
            grid_w: 20.0,
            grid_h: 20.0,
            grid_linked: true,
            grid_color: p.stage_grid,
            grid_snap_accuracy: "Snap: Normal",
        }
    }
}

/// What the user did with the popover this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewSettingsResponse {
    pub close: bool,
    pub clear_guides: bool,
    pub save_default: bool,
}

/// `NF2/View Settings`. Lay it out inside a [`super::Surface::Popover`] frame, 320 wide.
pub struct ViewSettings<'a> {
    pub state: &'a mut ViewSettingsState,
}

impl ViewSettings<'_> {
    pub fn show(self, ui: &mut Ui) -> ViewSettingsResponse {
        ui.vertical(|ui| self.contents(ui)).inner
    }

    fn contents(self, ui: &mut Ui) -> ViewSettingsResponse {
        let p = palette(ui.ctx());
        let s = self.state;
        let mut out = ViewSettingsResponse::default();
        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
        let w = ui.available_width();
        ui.horizontal(|ui| {
            ui.label(
                crate::typography::rich("Rulers, guides & grid", TextStyle::BodyStrong)
                    .color(p.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                out.close = ui
                    .add(IconButton::new(i16::X, "Close").shortcut("Esc"))
                    .clicked();
            });
        });
        heading(ui, i16::RULER, "Rulers");
        ui.add(Checkbox::new(&mut s.show_rulers, "Show rulers"));
        Select::new("nf2-view-units", s.units)
            .width(w)
            .show(ui, |ui| {
                for u in [
                    "Units: Pixels",
                    "Units: Points",
                    "Units: Inches",
                    "Units: Centimetres",
                    "Units: Millimetres",
                ] {
                    if ui
                        .add(super::MenuItem::new(u).checked(s.units == u))
                        .clicked()
                    {
                        s.units = u;
                    }
                }
            });
        ui.horizontal(|ui| {
            ui.label(crate::typography::rich("Labels", TextStyle::Body).color(p.text_secondary));
            Segmented::show(
                ui,
                &mut s.labels_auto,
                &[(true, "Auto"), (false, "Every")],
                false,
            );
            ui.add_enabled(
                !s.labels_auto,
                Field::new(&mut s.label_every, "Label interval")
                    .prefix("px")
                    .range(1.0..=10_000.0)
                    .width(96.0),
            );
        });
        ui.label(
            crate::typography::rich(
                "Auto picks 10 / 50 / 100 / 500 units so labels never overlap at the current zoom.",
                TextStyle::Small,
            )
            .color(p.text_tertiary),
        );
        super::divider(ui, false);
        heading(ui, i16::GUIDES, "Guides");
        ui.horizontal(|ui| {
            ui.add(Checkbox::new(&mut s.guides_show, "Show"));
            ui.add(Checkbox::new(&mut s.guides_snap, "Snap"));
            ui.add(Checkbox::new(&mut s.guides_lock, "Lock"));
        });
        ui.horizontal(|ui| {
            ColorField::new(&mut s.guide_color, "Guide colour")
                .width((w - space::S2) * 0.55)
                .show(ui);
            accuracy(
                ui,
                "nf2-view-guide-snap",
                &mut s.guide_snap,
                ui.available_width(),
            );
        });
        super::divider(ui, false);
        heading(ui, i16::GRID, "Grid");
        ui.horizontal(|ui| {
            ui.add(Checkbox::new(&mut s.grid_show, "Show"));
            ui.add(Checkbox::new(&mut s.grid_snap, "Snap"));
            ui.add(Checkbox::new(&mut s.grid_over_objects, "Over objects"));
        });
        ui.horizontal(|ui| {
            let fw = (w - 32.0 - 2.0 * space::S2) / 2.0;
            let before = s.grid_w;
            ui.add(
                Field::new(&mut s.grid_w, "Grid spacing horizontal")
                    .prefix("↔")
                    .unit("px")
                    .range(1.0..=1000.0)
                    .width(fw),
            );
            if s.grid_linked && s.grid_w != before {
                s.grid_h = s.grid_w;
            }
            let before = s.grid_h;
            ui.add(
                Field::new(&mut s.grid_h, "Grid spacing vertical")
                    .prefix("↕")
                    .unit("px")
                    .range(1.0..=1000.0)
                    .width(fw),
            );
            if s.grid_linked && s.grid_h != before {
                s.grid_w = s.grid_h;
            }
            if ui
                .add(IconButton::new(i16::LINK, "Link spacing").selected(s.grid_linked))
                .clicked()
            {
                s.grid_linked = !s.grid_linked;
            }
        });
        ui.horizontal(|ui| {
            ColorField::new(&mut s.grid_color, "Grid colour")
                .width((w - space::S2) * 0.55)
                .show(ui);
            accuracy(
                ui,
                "nf2-view-grid-snap",
                &mut s.grid_snap_accuracy,
                ui.available_width(),
            );
        });
        ui.horizontal(|ui| {
            out.clear_guides = ui.add(Button::ghost("Clear guides")).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                out.save_default = ui.add(Button::new("Save as default")).clicked();
            });
        });
        out
    }
}

/// Section caption: 16 px icon + overline label.
fn heading(ui: &mut Ui, icon: Icon, label: &str) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        icons::show(ui, icon, p.text_tertiary);
        ui.label(crate::typography::rich(label, TextStyle::Overline).color(p.text_tertiary));
    });
}

fn accuracy(ui: &mut Ui, id: &str, value: &mut &'static str, width: f32) {
    Select::new(id, value).width(width).show(ui, |ui| {
        for a in [
            "Snap: Must be close",
            "Snap: Normal",
            "Snap: Can be distant",
        ] {
            if ui
                .add(super::MenuItem::new(a).checked(*value == a))
                .clicked()
            {
                *value = a;
            }
        }
    });
}
