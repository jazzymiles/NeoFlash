//! Controls for features the engine or editor does not provide yet (§1 rule 9: a planned feature is
//! shown disabled with an explicit "Not available yet" state, never as a working-looking fake).
//!
//! egui does not hover disabled widgets, so [`tooltip`](super::tooltip) never appears on them.
//! [`not_available`] shows the same `NF2/Tooltip` look from `contains_pointer` instead.
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius};
use eframe::egui::{self, Response};

/// Tooltip suffix for planned features.
pub const NOT_AVAILABLE: &str = "Not available yet";

/// Show "`label` · Not available yet" in the `NF2/Tooltip` style while the pointer rests on a
/// **disabled** control. Enabled controls are returned untouched (they keep their own tooltip).
pub fn not_available(response: Response, label: &str) -> Response {
    let ctx = response.ctx.clone();
    if response.enabled() || !response.contains_pointer() || ctx.input(|i| i.pointer.any_down()) {
        return response;
    }
    let delay = ctx.style().interaction.tooltip_delay as f64;
    let still = ctx.input(|i| i.pointer.time_since_last_movement()) as f64;
    if still < delay {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - still + 0.01));
        return response;
    }
    let p = palette(&ctx);
    let text = if label.is_empty() {
        NOT_AVAILABLE.to_owned()
    } else {
        format!("{label}  ·  {NOT_AVAILABLE}")
    };
    egui::Area::new(response.id.with("nf2-not-available"))
        .order(egui::Order::Tooltip)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(response.rect.center_bottom() + egui::vec2(0.0, crate::tokens::space::S2))
        .interactable(false)
        .constrain(true)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(p.tooltip_bg)
                .corner_radius(cr(radius::MD))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .shadow(crate::metrics::SHADOW_POPOVER.egui())
                .show(ui, |ui| {
                    ui.label(crate::typography::rich(text, TextStyle::Body).color(p.text_inverse));
                });
        });
    response
}
