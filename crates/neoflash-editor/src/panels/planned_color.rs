//! Planned preview of Color — Bitmap fill (`publish-misc-dialogs.png`). The working Color panel
//! (solid RGB/HSB, linear and radial gradients, stroke width) lives in `authoring_modify.rs`.
//! Colours below are sample document colours (HSV), not UI tokens.
use super::planned::{caption, num, select};
use eframe::egui::{self, Color32, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::icons::{i16, i20};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

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
