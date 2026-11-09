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

/// The authored Pen anchor is the upper nib tip in the NF2 glyph. The exported cursor metadata
/// still names the lower-left outline corner as the hotspot, which makes the point land well away
/// from the visible nib (especially on Retina displays). Keep the locked asset unchanged and
/// correct only the interactive hotspot at the editor boundary.
const PEN_NIB_HOTSPOT: [f32; 2] = [11.0, 3.0];

fn authored_hotspot(mut cursor: Cursor) -> Cursor {
    if matches!(
        cursor.name,
        "pen" | "pen-add" | "pen-remove" | "pen-close" | "pen-convert"
    ) {
        cursor.hotspot = PEN_NIB_HOTSPOT;
    }
    cursor
}

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
        Tool::Rectangle | Tool::Ellipse | Tool::PolyStar | Tool::Line | Tool::Gradient => {
            nf::CROSSHAIR
        }
    }
}

/// Idle cursor respecting Preferences ▸ Drawing ▸ Precise cursors: drawing and paint tools show
/// `crosshair` instead of their tool glyph while selection, navigation and text cursors stay as-is.
pub(crate) fn tool_cursor_precise(tool: Tool, precise: bool) -> Cursor {
    if precise
        && matches!(
            tool,
            Tool::Pen
                | Tool::Pencil
                | Tool::Brush
                | Tool::Eraser
                | Tool::Bucket
                | Tool::Ink
                | Tool::Eyedropper
        )
    {
        nf::CROSSHAIR
    } else {
        tool_cursor(tool)
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
    /// Returns true if any visible detached floating panel window covers `pos` (in root viewport
    /// logical coordinates) or if a floating viewport currently owns the pointer.
    pub(crate) fn pointer_over_floating_panel(&self, ctx: &egui::Context, pos: egui::Pos2) -> bool {
        if self.panels_hidden {
            return false;
        }
        let zoom = ctx.zoom_factor().max(0.1);
        let root_origin = self
            .detached
            .root_inner
            .or_else(|| ctx.input(|i| i.viewport().inner_rect))
            .map_or(egui::Pos2::ZERO, |r| r.min);
        let global_px = (root_origin + pos.to_vec2()) * zoom;
        for window in &self.workspace.floating {
            if !window
                .panels()
                .iter()
                .any(|p| self.workspace.is_panel_visible(*p))
            {
                continue;
            }
            if let Some(geom) = window.geometry {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(geom.outer_position[0], geom.outer_position[1]),
                    egui::vec2(geom.inner_size[0], geom.inner_size[1]),
                )
                .expand(6.0 * zoom);
                if rect.contains(global_px) {
                    return true;
                }
            }
            let embedded_id = egui::Id::new(("floating-dock", window.id));
            if ctx
                .memory(|m| m.area_rect(embedded_id))
                .is_some_and(|r| r.expand(4.0).contains(pos))
            {
                return true;
            }
        }
        false
    }

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
        if self.has_modal_dialog() {
            return;
        }
        #[cfg(not(test))]
        if !ctx.input(|i| i.focused) {
            return;
        }
        // Motion path key points sit above every tool's hover target.
        if self.timeline_motion.curve_drag.is_some() || self.timeline_motion.curve_hover.is_some() {
            cursor = nf::ARROW_RESHAPE_CURVE;
        } else if self.timeline_motion.drag.is_some() || self.timeline_shape_hints.drag.is_some() {
            cursor = nf::HAND_CLOSED;
        } else if self.timeline_motion.hover.is_some() || self.timeline_shape_hints.hover.is_some()
        {
            cursor = nf::HAND_OPEN;
        }
        cursor = authored_hotspot(cursor);
        let Some(pos) = ctx.pointer_hover_pos() else {
            return;
        };
        if self.pointer_over_floating_panel(ctx, pos) {
            return;
        }
        let over_canvas = canvas.contains(pos) && ctx.layer_id_at(pos) == Some(stage_layer);
        let system_requested = ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default;
        if over_canvas && !system_requested && nf::show(ctx, canvas, cursor) {
            self.shown_stage_cursor = Some(cursor);
        }
    }

    /// Reconcile the software Stage cursor at the end of `Editor::update`, after dialogs, pills,
    /// floating panels, and notifications have rendered. Ensures the OS cursor and NF2 software
    /// cursor are never both visible simultaneously.
    pub(crate) fn reconcile_stage_cursor_after_overlays(&mut self, ctx: &egui::Context) {
        if self.shown_stage_cursor.is_none() {
            return;
        }
        let cursor_layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("nf2_cursor"));
        let Some(pos) = ctx.pointer_hover_pos() else {
            ctx.graphics_mut(|g| *g.entry(cursor_layer) = Default::default());
            self.shown_stage_cursor = None;
            return;
        };
        let current_icon = ctx.output(|o| o.cursor_icon);
        let overlay_requested_system = !matches!(
            current_icon,
            egui::CursorIcon::None | egui::CursorIcon::Default
        );
        let top_layer_changed = ctx
            .layer_id_at(pos)
            .is_some_and(|layer| layer.order != egui::Order::Background && layer != cursor_layer);
        if self.has_modal_dialog()
            || self.pointer_over_floating_panel(ctx, pos)
            || overlay_requested_system
            || top_layer_changed
        {
            ctx.graphics_mut(|g| *g.entry(cursor_layer) = Default::default());
            self.shown_stage_cursor = None;
            if current_icon == egui::CursorIcon::None {
                ctx.set_cursor_icon(egui::CursorIcon::Default);
            }
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::None);
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
            Select, Transform, Rectangle, Ellipse, PolyStar, Text, Pen, Pencil, Subselect, Line,
            Brush, Bucket, Ink, Eyedropper, Eraser, Lasso, Hand, Zoom, Gradient,
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

    #[test]
    fn precise_cursors_replace_drawing_and_paint_glyphs_with_crosshair() {
        use Tool::*;
        for tool in [Pen, Pencil, Brush, Eraser, Bucket, Ink, Eyedropper] {
            assert_ne!(tool_cursor_precise(tool, false), nf::CROSSHAIR);
            assert_eq!(tool_cursor_precise(tool, true), nf::CROSSHAIR);
        }
        for tool in [
            Select, Transform, Subselect, Text, Lasso, Hand, Zoom, Rectangle, Ellipse, PolyStar,
            Line, Gradient,
        ] {
            assert_eq!(tool_cursor_precise(tool, true), tool_cursor(tool));
        }
    }

    #[test]
    fn pen_family_hotspot_is_the_visible_nib_tip() {
        for cursor in [
            nf::PEN,
            nf::PEN_ADD,
            nf::PEN_REMOVE,
            nf::PEN_CLOSE,
            nf::PEN_CONVERT,
        ] {
            let corrected = authored_hotspot(cursor);
            assert_eq!(corrected.hotspot, PEN_NIB_HOTSPOT, "{}", cursor.name);
            assert_eq!(corrected.svg, cursor.svg);
        }
        assert_eq!(authored_hotspot(nf::PENCIL), nf::PENCIL);
        assert_eq!(authored_hotspot(nf::CROSSHAIR), nf::CROSSHAIR);
    }
}
