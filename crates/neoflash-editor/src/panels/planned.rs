//! Shared panel and dialog text / form-label helpers (dialog footers live in `dialog_helpers`).
//!
//! All 13 former `PlannedPanel` previews have been promoted to live editor panels,
//! modal dialogs, or the standalone Test Movie preview window.
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::TextStyle;
use neoflash_ui::typography::rich;

pub(crate) fn select(ui: &mut Ui, label: &str, width: f32) {
    c::Select::new(ui.next_auto_id(), label)
        .width(width)
        .show(ui, |_| {});
}

pub(crate) fn caption(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::Small).color(nf.text_secondary));
}

pub(crate) fn mono(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::MonoSmall).color(nf.text_secondary));
}

pub(crate) fn body_text(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.label(rich(s, TextStyle::Body).color(nf.text_primary));
}

/// Label column of a dialog form row (Preferences, Document settings).
pub(crate) fn form_label(ui: &mut Ui, s: &str, width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, metrics::toggle_row()),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::Body),
        nf.text_secondary,
    );
}
