//! Docking feedback painted while a panel is dragged between native windows: `NF2/Dock Zone`
//! candidates (idle and hovered) and a `NF2/Drag Ghost` chip at an explicit position (the pointer
//! of a cross-window drag is not this viewport's hover position, so `surface::drag_ghost` cannot be used).
use crate::icons::{self, i16};
use crate::theme::{cr, palette};
use crate::tokens::{Palette, TextStyle, radius, space};
use eframe::egui::{self, Rect, Stroke, StrokeKind};

/// A dock zone candidate. `hovered` = the zone that receives the drop (full `NF2/Dock Zone` look);
/// other candidates show a faint tint with their caption so every target is discoverable.
pub fn dock_target(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    label: &str,
    hovered: bool,
) {
    let p = palette(ctx);
    if hovered {
        super::surface::dock_zone(painter, ctx, rect, label);
        return;
    }
    painter.rect_filled(rect, cr(radius::lg()), Palette::tint(p.accent_subtle, 0.5));
    painter.rect_stroke(
        rect,
        cr(radius::lg()),
        Stroke::new(1.0_f32, Palette::tint(p.accent_default, 0.6)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        crate::typography::font(TextStyle::Small),
        p.text_secondary,
    );
}

/// `NF2/Drag Ghost` at `pos` (top-left): grip + label on `bg/raised` with an accent border.
pub fn drag_ghost_at(painter: &egui::Painter, ctx: &egui::Context, pos: egui::Pos2, label: &str) {
    let p = palette(ctx);
    let g = painter.layout_no_wrap(
        label.to_owned(),
        crate::typography::font(TextStyle::Body),
        p.text_primary,
    );
    let size = egui::vec2(
        space::s3() + 16.0 + space::s2() + g.size().x + space::s3(),
        crate::metrics::row(),
    );
    let rect = Rect::from_min_size(pos, size);
    painter.add(
        super::surface::fitted_shadow(crate::metrics::SHADOW_POPOVER, rect.height())
            .as_shape(rect, cr(radius::md())),
    );
    painter.rect_filled(rect, cr(radius::md()), p.bg_raised);
    painter.rect_stroke(
        rect,
        cr(radius::md()),
        Stroke::new(1.0_f32, p.accent_default),
        StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + space::s3() + 8.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    icons::paint(painter, ctx, icon, i16::GRIP, p.text_tertiary);
    painter.galley(
        egui::pos2(
            icon.right() + space::s2(),
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        p.text_primary,
    );
}
