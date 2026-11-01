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

/// Units select options, in menu order.
pub const UNITS: [&str; 5] = [
    "Units: Pixels",
    "Units: Points",
    "Units: Inches",
    "Units: Centimetres",
    "Units: Millimetres",
];

/// Snap accuracy select options, from the tightest to the loosest.
pub const SNAP_ACCURACY: [&str; 3] = [
    "Snap: Must be close",
    "Snap: Normal",
    "Snap: Can be distant",
];

/// Default grid colour: `color/stage-grid` at 14 % (reference "FFFFFF · 14 %" in Dark).
pub fn default_grid_color(p: &Palette) -> Color32 {
    let [r, g, b, _] = p.stage_grid.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, 36)
}

/// Where the popover sits relative to the ⚙ button of `NF2/Zoom Control`
/// (editor-view-settings-open.png: popover right = button right + 25, bottom = button bottom + 38).
pub const POPOVER_OFFSET: egui::Vec2 = egui::vec2(25.0, 38.0);
/// Content width inside the popover (outline rows are 288 wide).
const CONTENT_W: f32 = 288.0;
/// Popover size (`NF2/View Settings` 320 × 542).
pub const POPOVER_SIZE: egui::Vec2 = egui::vec2(320.0, 542.0);

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
            grid_color: default_grid_color(p),
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

    /// The popover itself: `Surface::Popover` (radius 8, border, shadow) with 16 pt padding, placed by
    /// [`POPOVER_OFFSET`] from `button` (the ⚙ rect) and kept inside `bounds`. Esc or a click outside
    /// the popover (and outside `button`) reports `close`.
    pub fn popover(
        self,
        ctx: &egui::Context,
        id: egui::Id,
        button: egui::Rect,
        bounds: egui::Rect,
    ) -> ViewSettingsResponse {
        let max = button.max + POPOVER_OFFSET;
        let mut min = max - POPOVER_SIZE;
        min.x = min.x.clamp(
            bounds.left(),
            (bounds.right() - POPOVER_SIZE.x).max(bounds.left()),
        );
        min.y = min.y.clamp(
            bounds.top(),
            (bounds.bottom() - POPOVER_SIZE.y).max(bounds.top()),
        );
        let inner = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(min)
            .show(ctx, |ui| {
                // Reference frame: `border/default` stroke, 16 pt sides, 13 pt top, 16 pt bottom
                // (egui draws the 1 pt stroke inside the frame, so it is taken off the margin).
                super::Surface::Popover
                    .frame(ctx)
                    .stroke(egui::Stroke::new(1.0_f32, palette(ctx).border_default))
                    .inner_margin(egui::Margin {
                        left: 15,
                        right: 15,
                        top: 12,
                        bottom: 15,
                    })
                    .show(ui, |ui| {
                        ui.set_width(CONTENT_W);
                        self.show(ui)
                    })
                    .inner
            });
        let mut out = inner.inner;
        // A click into an open Select list (its own popup layer) is not "outside".
        let outside = !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|pos| {
                        !inner.response.rect.contains(pos) && !button.contains(pos)
                    })
            });
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        out.close |= outside || escape;
        out
    }

    /// Layout from the outline `NF2/View Settings` (288 pt content): Header · Rulers · Guides · Grid ·
    /// Footer, 12 pt apart with a 1 pt separator between the three sections; rows 8 pt apart.
    fn contents(self, ui: &mut Ui) -> ViewSettingsResponse {
        let p = palette(ui.ctx());
        let s = self.state;
        let mut out = ViewSettingsResponse::default();
        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);
        let w = ui.available_width();
        let section = |ui: &mut Ui, body: &mut dyn FnMut(&mut Ui)| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                // Rows hug their content (16 pt captions, 20 pt checkboxes) instead of the 32 pt
                // control minimum; fields and selects keep their own 32 pt height.
                ui.spacing_mut().interact_size.y = 16.0;
                body(ui);
            });
        };
        let separator = |ui: &mut Ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                egui::Stroke::new(1.0_f32, p.border_subtle),
            );
        };
        ui.horizontal(|ui| {
            ui.set_height(CONTROL);
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
        section(ui, &mut |ui| {
            heading(ui, i16::RULER, "Rulers");
            ui.add(Checkbox::new(&mut s.show_rulers, "Show rulers"));
            Select::new("nf2-view-units", s.units)
                .width(w)
                .show(ui, |ui| {
                    for u in UNITS {
                        if ui
                            .add(super::MenuItem::new(u).checked(s.units == u))
                            .clicked()
                        {
                            s.units = u;
                        }
                    }
                });
            let row = egui::vec2(w, 34.0); // `Label interval` row: Segmented Control is 34 tall
            ui.allocate_ui_with_layout(
                row,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        crate::typography::rich("Labels", TextStyle::Body).color(p.text_secondary),
                    );
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
                            .width(72.0),
                    );
                },
            );
            // Line break as in the reference (egui's metrics would wrap one word earlier).
            ui.add(
                egui::Label::new(
                    crate::typography::rich(
                        "Auto picks 10 / 50 / 100 / 500 units so labels never\noverlap at the current zoom.",
                        TextStyle::Small,
                    )
                    .color(p.text_tertiary)
                    .line_height(Some(16.0)),
                )
                .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
        separator(ui);
        section(ui, &mut |ui| {
            heading(ui, i16::GUIDES, "Guides");
            ui.horizontal(|ui| {
                ui.add(Checkbox::new(&mut s.guides_show, "Show"));
                ui.add(Checkbox::new(&mut s.guides_snap, "Snap"));
                ui.add(Checkbox::new(&mut s.guides_lock, "Lock"));
            });
            ui.horizontal(|ui| {
                ColorField::new(&mut s.guide_color, "Guide colour")
                    .width(COLOR_W)
                    .show(ui);
                accuracy(
                    ui,
                    "nf2-view-guide-snap",
                    &mut s.guide_snap,
                    w - COLOR_W - space::S2,
                );
            });
        });
        separator(ui);
        section(ui, &mut |ui| {
            heading(ui, i16::GRID, "Grid");
            ui.horizontal(|ui| {
                ui.add(Checkbox::new(&mut s.grid_show, "Show"));
                ui.add(Checkbox::new(&mut s.grid_snap, "Snap"));
                ui.add(Checkbox::new(&mut s.grid_over_objects, "Over objects"));
            });
            ui.horizontal(|ui| {
                let fw = (w - CONTROL - 2.0 * space::S2) / 2.0;
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
                    .width(COLOR_W)
                    .show(ui);
                accuracy(
                    ui,
                    "nf2-view-grid-snap",
                    &mut s.grid_snap_accuracy,
                    w - COLOR_W - space::S2,
                );
            });
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

/// `NF2/Color Field` width in the guide and grid rows (the Snap select takes the rest of 288).
const COLOR_W: f32 = 152.0;
/// Control height (fields, selects, buttons, icon buttons).
const CONTROL: f32 = 32.0;

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
        for a in SNAP_ACCURACY {
            if ui
                .add(super::MenuItem::new(a).checked(*value == a))
                .clicked()
            {
                *value = a;
            }
        }
    });
}
