//! Shared NF2 panel and dialog form/footer helpers.
//!
//! All 13 former `PlannedPanel` previews have been promoted to live editor panels,
//! modal dialogs, or the standalone Test Movie preview window.
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

pub(crate) fn select(ui: &mut Ui, label: &str, width: f32) {
    c::Select::new(ui.next_auto_id(), label)
        .width(width)
        .show(ui, |_| {});
}

pub(crate) fn switch(ui: &mut Ui, on: bool, label: &str) {
    let mut v = on;
    ui.add(c::Switch::new(&mut v, label));
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
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, metrics::TOGGLE_ROW), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        s,
        neoflash_ui::typography::font(TextStyle::Body),
        nf.text_secondary,
    );
}

/// Properties type header: 48 × 48 type-icon tile, title, subtitle (Properties panels).
pub(crate) fn type_header(ui: &mut Ui, icon: neoflash_ui::Icon, title: &str, subtitle: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal(|ui| {
        let (tile, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        ui.painter().rect_filled(
            tile,
            egui::CornerRadius::same(radius::LG as u8),
            nf.bg_field,
        );
        neoflash_ui::icons::paint(ui.painter(), ui.ctx(), tile, icon, nf.text_secondary);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(space::S1);
            ui.label(rich(title, TextStyle::BodyStrong).color(nf.text_primary));
            ui.label(rich(subtitle, TextStyle::Small).color(nf.text_secondary));
        });
    });
}

/// Footer button clicked in [`footer_clicked`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FooterClick {
    Ghost,
    /// Index into `buttons`.
    Button(usize),
}

/// Dialog footer with a left caption or ghost action and right-aligned buttons (last = primary).
pub(crate) fn footer_clicked(
    ui: &mut Ui,
    left: &str,
    ghost: Option<(&str, bool)>,
    buttons: &[&str],
) -> Option<FooterClick> {
    let specs: Vec<(&str, bool)> = buttons.iter().copied().map(|b| (b, true)).collect();
    footer_clicked_with_enabled(ui, left, ghost, &specs)
}

/// [`footer_clicked`] where each trailing button carries its own enabled flag.
pub(crate) fn footer_clicked_with_enabled(
    ui: &mut Ui,
    left: &str,
    ghost: Option<(&str, bool)>,
    buttons: &[(&str, bool)],
) -> Option<FooterClick> {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut clicked = None;
    ui.add_space(space::S2);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.x_range(),
        r.top() - space::S1,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
    );
    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        if let Some((g, enabled)) = ghost
            && ui.add_enabled(enabled, c::Button::ghost(g)).clicked()
        {
            clicked = Some(FooterClick::Ghost);
        }
        if !left.is_empty() {
            caption(ui, left);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, (b, enabled)) in buttons.iter().rev().enumerate() {
                let widget = if i == 0 {
                    c::Button::primary(b)
                } else {
                    c::Button::new(b)
                };
                let response = ui.add_enabled(*enabled, widget);
                if response.clicked() {
                    clicked = Some(FooterClick::Button(buttons.len() - 1 - i));
                }
            }
        });
    });
    clicked
}
