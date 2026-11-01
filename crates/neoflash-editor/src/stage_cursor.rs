//! NF2 stage cursors (spec ST-07). Stage subsystems propose a cursor in `Editor::stage_cursor`
//! during the frame; [`Editor::show_stage_cursor`] decides once, after all stage input and chrome.
//!
//! `neoflash_ui::cursors::show` paints the 24 pt NF2 glyph on the tooltip layer and hides the system
//! cursor, so it is used only while the pointer is over the canvas itself: not over rulers, floating
//! stage chrome, popovers, menus or dialogs (they own other layers), and not when stage code already
//! asked egui for a system cursor this frame (guides, dock grips). Cursors are the same in both
//! themes: black glyphs with a white outline.
use crate::app::{Editor, Tool};
use eframe::egui::{self, Rect};
use neoflash_ui::cursors::{self as nf, Cursor};

/// Idle cursor of a stage tool, before any hover target refines it.
pub(crate) fn tool_cursor(tool: Tool) -> Cursor {
    match tool {
        Tool::Select | Tool::Transform => nf::ARROW,
        Tool::Subselect => nf::SUBSELECT,
        Tool::Text => nf::TEXT_IBEAM,
        Tool::Pen => nf::PEN,
        Tool::Pencil => nf::PENCIL,
        Tool::Brush => nf::BRUSH,
        Tool::Bucket => nf::BUCKET,
        Tool::Ink => nf::INK_BOTTLE,
        Tool::Eyedropper => nf::EYEDROPPER,
        Tool::Eraser => nf::ERASER,
        Tool::Lasso => nf::LASSO,
        Tool::Hand => nf::HAND_OPEN,
        Tool::Zoom => nf::ZOOM_IN,
        Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Gradient => nf::CROSSHAIR,
    }
}

/// Resize cursor for a handle at screen direction `dir` from the box centre (y grows downwards):
/// horizontal → `resize-h`, vertical → `resize-v`, NW/SE → `resize-d1`, NE/SW → `resize-d2`.
pub(crate) fn resize_cursor(dir: egui::Vec2) -> Cursor {
    let angle = dir.y.atan2(dir.x).to_degrees().rem_euclid(180.);
    match angle {
        a if !(22.5..157.5).contains(&a) => nf::RESIZE_H,
        a if a < 67.5 => nf::RESIZE_D1,
        a if a < 112.5 => nf::RESIZE_V,
        _ => nf::RESIZE_D2,
    }
}

impl Editor {
    /// Paint the frame's NF2 cursor over `canvas` (the stage's own layer `stage_layer`) and forget it.
    pub(crate) fn show_stage_cursor(
        &mut self,
        ctx: &egui::Context,
        canvas: Rect,
        stage_layer: egui::LayerId,
    ) {
        self.shown_stage_cursor = None;
        let Some(mut cursor) = self.stage_cursor.take() else {
            return;
        };
        // Motion path key points sit above every tool's hover target.
        if self.timeline_motion.drag.is_some() {
            cursor = nf::HAND_CLOSED;
        } else if self.timeline_motion.hover.is_some() {
            cursor = nf::HAND_OPEN;
        }
        let Some(pos) = ctx.pointer_hover_pos() else {
            return;
        };
        let over_canvas = canvas.contains(pos) && ctx.layer_id_at(pos) == Some(stage_layer);
        let system_requested = ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default;
        if over_canvas && !system_requested && nf::show(ctx, canvas, cursor) {
            self.shown_stage_cursor = Some(cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_designed_cursor() {
        use Tool::*;
        for tool in [
            Select, Transform, Rectangle, Ellipse, Text, Pen, Pencil, Subselect, Line, Brush,
            Bucket, Ink, Eyedropper, Eraser, Lasso, Hand, Zoom, Gradient,
        ] {
            let cursor = tool_cursor(tool);
            assert!(nf::ALL.contains(&cursor), "{}", cursor.name);
        }
    }

    #[test]
    fn resize_cursor_follows_the_handle_direction() {
        let cases = [
            (egui::vec2(1., 0.), "resize-h"),
            (egui::vec2(-1., 0.), "resize-h"),
            (egui::vec2(0., -1.), "resize-v"),
            (egui::vec2(1., 1.), "resize-d1"),
            (egui::vec2(-1., -1.), "resize-d1"),
            (egui::vec2(1., -1.), "resize-d2"),
            (egui::vec2(-1., 1.), "resize-d2"),
        ];
        for (dir, name) in cases {
            assert_eq!(resize_cursor(dir).name, name, "{dir:?}");
        }
    }
}
