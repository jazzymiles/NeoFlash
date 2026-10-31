//! Planned previews of the Color panel fill types the engine cannot edit yet: radial / linear gradient and
//! solid mixer (`panel-color-radial-gradient.png`, `color-type-menu-open.png`, `panel-color-solid.png`) and
//! bitmap fill (`publish-misc-dialogs.png`). The working Color panel (solid RGB, stroke width) lives in
//! `authoring_modify.rs`. Colours below are sample document colours (HSV), not UI tokens.
use super::planned::{PlannedPanel, caption, check, num, select, set_open};
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::{Hsb, hsv};
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

/// Sample fill: FF9900 (orange) as HSV.
fn orange() -> Hsb {
    hsv(36.0 / 360.0, 1.0, 1.0, 1.0)
}

/// Scope row, Stroke / Fill wells and colour actions shared by every fill type.
fn top(ui: &mut Ui, fill: Color32) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        let mut scope = true;
        c::Segmented::show(
            ui,
            &mut scope,
            &[(true, "Selection"), (false, "Drawing")],
            false,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(rich("1 object", TextStyle::Body).color(nf.text_secondary));
        });
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add(c::ColorWell {
            target: c::ColorWellTarget::Stroke,
            color: Some(hsv(0.0, 0.0, 0.0, 1.0).into()),
            active: false,
        });
        ui.add(c::ColorWell {
            target: c::ColorWellTarget::Fill,
            color: Some(fill),
            active: true,
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.add(c::IconButton::new(i20::COLOR_SWAP, "Swap colours").side(28.0));
            let _ = ui.add(c::IconButton::new(i20::COLOR_NONE, "No colour").side(28.0));
            let _ = ui.add(c::IconButton::new(i20::COLOR_DEFAULT, "Black and white").side(28.0));
        });
    });
}

/// Fill-type `NF2/Select`. Preview navigation: Solid / Linear / Radial switch the preview page, Bitmap fill
/// opens the bitmap-fill preview; "None" is not part of the preview.
fn fill_type(ui: &mut Ui, label: &str, solid: &mut bool) {
    let w = ui.available_width();
    let ctx = ui.ctx().clone();
    c::Select::new("planned-color-type", label)
        .width(w)
        .show(ui, |ui| {
            ui.add_enabled(false, c::MenuItem::new("None"));
            if ui
                .add(c::MenuItem::new("Solid colour").checked(*solid))
                .clicked()
            {
                *solid = true;
            }
            if ui.add(c::MenuItem::new("Linear gradient")).clicked() {
                *solid = false;
            }
            if ui
                .add(c::MenuItem::new("Radial gradient").checked(!*solid))
                .clicked()
            {
                *solid = false;
            }
            if ui.add(c::MenuItem::new("Bitmap fill")).clicked() {
                set_open(&ctx, PlannedPanel::BitmapFill, true);
            }
        });
}

/// Colour mixer block: picker, RGB / HSB, R G B fields, hex + alpha.
fn mixer(ui: &mut Ui, color: Hsb) {
    let mut hsva = color;
    ui.add(c::ColorPicker { color: &mut hsva });
    let mut model = 0;
    c::Segmented::show(ui, &mut model, &[(0, "RGB"), (1, "HSB")], false);
    let rgb: Color32 = color.into();
    ui.horizontal(|ui| {
        num(ui, rgb.r() as f64, "Red", "R", "", 0, 80.0);
        num(ui, rgb.g() as f64, "Green", "G", "", 0, 80.0);
        num(ui, rgb.b() as f64, "Blue", "B", "", 0, 80.0);
    });
    let mut field = rgb;
    let w = ui.available_width();
    c::ColorField::new(&mut field, "Fill colour")
        .width(w)
        .show(ui);
}

/// "STROKE" group: width + line style.
fn stroke(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.x_range(),
        r.top(),
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    ui.add_space(space::S2);
    ui.label(rich("Stroke", TextStyle::Overline).color(nf.text_tertiary));
    let half = (ui.available_width() - space::S2) / 2.0;
    ui.horizontal(|ui| {
        num(ui, 1.0, "Stroke width", "Width", "px", 1, half);
        select(ui, "Solid", half);
    });
}

/// Panel / Color — Radial gradient (`solid = false`) or Solid colour (`solid = true`, the state shown with the
/// type menu open). The type select stays live as preview navigation; everything else is disabled.
pub(super) fn color_gradient(ui: &mut Ui, solid: &mut bool) {
    let fill = if *solid {
        hsv(48.0 / 360.0, 1.0, 1.0, 1.0)
    } else {
        orange()
    };
    ui.scope(|ui| {
        ui.disable();
        top(ui, fill.into());
    });
    fill_type(
        ui,
        if *solid {
            "Solid colour"
        } else {
            "Radial gradient"
        },
        solid,
    );
    ui.scope(|ui| {
        ui.disable();
        if !*solid {
            ui.horizontal(|ui| {
                let nf = neoflash_ui::palette(ui.ctx());
                ui.label(rich("Flow", TextStyle::Body).color(nf.text_secondary));
                c::IconGroup {
                    items: &[
                        (i16::FLOW_EXTEND, "Extend"),
                        (i16::FLOW_REFLECT, "Reflect"),
                        (i16::FLOW_REPEAT, "Repeat"),
                    ],
                    selected: Some(0),
                }
                .show(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    check(ui, false, "Linear RGB");
                });
            });
        }
        mixer(ui, fill);
        if !*solid {
            let h = fill.h;
            let mut stops: Vec<(f32, Color32)> = vec![
                (0.0, hsv(h, 0.0, 1.0, 1.0).into()),
                (0.5, hsv(h, 0.6, 1.0, 1.0).into()),
                (1.0, fill.into()),
            ];
            let mut selected = 1;
            ui.add(c::GradientBar {
                stops: &mut stops,
                selected: &mut selected,
                width: 232.0,
            });
        }
        stroke(ui);
    });
}

/// Panel / Color — Bitmap fill.
pub(super) fn bitmap_fill(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    let tile = |h: f32, s: f32| -> (Color32, Color32) {
        (hsv(h, s, 0.85, 1.0).into(), hsv(h, s, 0.35, 1.0).into())
    };
    top(ui, hsv(48.0 / 360.0, 1.0, 1.0, 1.0).into());
    select(ui, "Bitmap fill", ui.available_width());
    let bitmaps = [
        tile(0.07, 0.7),
        tile(0.64, 0.55),
        tile(0.38, 0.55),
        tile(0.0, 0.0),
        tile(0.96, 0.65),
        tile(0.62, 0.15),
    ];
    for row in bitmaps.chunks(4) {
        ui.horizontal(|ui| {
            for (from, to) in row {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::hover());
                c::swatches::paint_gradient(
                    ui.painter(),
                    rect,
                    c::GradientSwatch {
                        from: *from,
                        to: *to,
                        radial: false,
                    },
                );
                let first = std::ptr::eq(from, &bitmaps[0].0);
                let (w, color) = if first {
                    (2.0_f32, nf.accent_default)
                } else {
                    (1.0, nf.border_subtle)
                };
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(radius::MD as u8),
                    egui::Stroke::new(w, color),
                    egui::StrokeKind::Inside,
                );
            }
        });
    }
    ui.horizontal(|ui| {
        let _ = ui.add(c::Button::new("Import…").icon(i16::PLUS));
        let _ = ui.add(c::IconButton::new(i16::LOCK_FILL, "Lock fill").selected(true));
        caption(ui, "Lock fill");
    });
    caption(ui, "texture.png · 512×512 · tiles across selection");
    stroke(ui);
}
