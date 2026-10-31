//! `NF2 Cursor/*`, 24 × 24, black glyph with white outline, generated from
//! `interface/design-system/exports/cursors` with hotspots from `docs/data/figma-dump.json`.
//!
//! egui (winit) can only show system cursors, so stage tools hide the system cursor and paint the
//! NF2 cursor on the tooltip layer with [`show`]. Use it only over the stage/canvas; chrome keeps
//! system cursors ([`system_fallback`]).
use eframe::egui::{self, Color32, Rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    pub name: &'static str,
    /// Hotspot inside the 24 × 24 box.
    pub hotspot: [f32; 2],
    pub svg: &'static str,
}

pub const SIZE: f32 = 24.0;

include!(concat!(env!("OUT_DIR"), "/cursors.rs"));

impl Cursor {
    pub fn by_name(name: &str) -> Option<Self> {
        ALL.iter().copied().find(|c| c.name == name)
    }
}

/// Paint `cursor` at the pointer (if it is over `area`) and hide the system cursor.
/// Returns `false` when the pointer is elsewhere and nothing was drawn.
pub fn show(ctx: &egui::Context, area: Rect, cursor: Cursor) -> bool {
    let Some(pos) = ctx.pointer_hover_pos() else {
        return false;
    };
    if !area.contains(pos) {
        return false;
    }
    let Some(tex) = crate::icons::texture(ctx, cursor.name, cursor.svg, SIZE, false) else {
        ctx.set_cursor_icon(system_fallback(cursor));
        return true;
    };
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let min = pos - egui::vec2(cursor.hotspot[0], cursor.hotspot[1]);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("nf2-cursor"),
    ));
    painter.image(
        tex.id(),
        Rect::from_min_size(min, egui::vec2(SIZE, SIZE)),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    true
}

/// Closest system cursor, for chrome and for platforms where custom painting is not wanted.
pub fn system_fallback(cursor: Cursor) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match cursor.name {
        "text-ibeam" => C::Text,
        "hand-open" => C::Grab,
        "hand-closed" => C::Grabbing,
        "move" | "arrow-move" => C::Move,
        "crosshair" | "pen" | "pen-add" | "pen-remove" | "pen-close" | "pen-convert" | "pencil"
        | "brush" | "bucket" | "ink-bottle" | "eyedropper" | "lasso" | "eraser" | "brush-size" => {
            C::Crosshair
        }
        "zoom-in" => C::ZoomIn,
        "zoom-out" => C::ZoomOut,
        "resize-h" | "col-resize" => C::ResizeHorizontal,
        "resize-v" | "row-resize" => C::ResizeVertical,
        "resize-d1" => C::ResizeNwSe,
        "resize-d2" => C::ResizeNeSw,
        "not-allowed" => C::NotAllowed,
        "wait" => C::Wait,
        "drag-copy" => C::Copy,
        "drag-link" => C::Alias,
        "rotate" | "skew-h" | "skew-v" => C::AllScroll,
        _ => C::Default,
    }
}
