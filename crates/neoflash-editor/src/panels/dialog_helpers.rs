//! Shared, presentation-only helpers used by editor dialogs.

use eframe::egui::{self, Ui};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

/// Overline caption of a dialog group ("OUTPUT", "RANGE").
pub(super) fn group(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    ui.label(rich(s, TextStyle::Overline).color(nf.text_tertiary));
}

/// Stage-preview placeholder (export / import dialogs): `bg/canvas` pasteboard with a `bg/stage` page.
pub(super) fn preview_box(ui: &mut Ui, width: f32, height: f32, label: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::LG as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
    ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
    ui.painter().text(
        stage.center(),
        egui::Align2::CENTER_CENTER,
        label,
        neoflash_ui::typography::font(TextStyle::Small),
        nf.text_tertiary,
    );
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
}
