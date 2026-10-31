//! Timeline hit-testing geometry shared by painting (panels/timeline.rs) and gestures
//! (timeline_edit.rs). Rows, ruler and frame widths are NF2 Timeline 2.0 metrics.
use eframe::egui::Pos2;
/// `NF2/TL Layer` row height (regular rows; Short rows use `LAYER_ROW_SHORT`).
#[cfg(test)]
pub(crate) const ROW_HEIGHT: f32 = neoflash_ui::metrics::LAYER_ROW;
/// `NF2/TL Ruler` / `NF2/TL Layers Header` height.
pub(crate) const RULER_HEIGHT: f32 = neoflash_ui::metrics::TIMELINE_RULER;

/// Actual editable cells exclude the end gutter and the space below the layers.
pub(crate) fn timeline_cell_at(
    pos: Pos2,
    origin: Pos2,
    cell_width: f32,
    frame_count: u32,
    layer_count: usize,
    row_height: f32,
) -> Option<(u32, usize)> {
    let local = pos - origin;
    if !pos.is_finite()
        || cell_width <= 0.
        || local.x < 0.
        || local.y < 0.
        || local.x >= frame_count as f32 * cell_width
        || local.y >= layer_count as f32 * row_height
    {
        return None;
    }
    Some((
        (local.x / cell_width).floor() as u32,
        (local.y / row_height).floor() as usize,
    ))
}

/// Once a real key drag owns the pointer, horizontal edges clamp its destination.
pub(crate) fn frame_at_x(x: f32, origin: f32, cell_width: f32, frame_count: u32) -> u32 {
    (((x - origin) / cell_width).floor().max(0.) as u32).min(frame_count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Rect, Vec2};
    #[test]
    fn cell_hit_testing_excludes_gutters_at_every_frame_view() {
        for frames in [5., 60., 500.] {
            for view in neoflash_ui::metrics::FrameView::ALL {
                let cell = view.frame_width();
                let row = Rect::from_min_size(Pos2::new(-300., 50.), Vec2::new(1000., ROW_HEIGHT));
                // The same geometry is used after horizontal scrolling. A
                // gutter click must not impersonate the final frame/layer,
                // while a captured key drag can reach the exact end cell.
                let origin = row.min;
                let count = frames as u32;
                let last = Pos2::new(origin.x + (frames - 0.25) * cell, origin.y + 12.);
                assert_eq!(
                    timeline_cell_at(last, origin, cell, count, 1, ROW_HEIGHT),
                    Some((count - 1, 0))
                );
                assert_eq!(
                    timeline_cell_at(
                        Pos2::new(origin.x + frames * cell, last.y),
                        origin,
                        cell,
                        count,
                        1,
                        ROW_HEIGHT
                    ),
                    None
                );
                assert_eq!(
                    timeline_cell_at(
                        Pos2::new(last.x, origin.y + ROW_HEIGHT),
                        origin,
                        cell,
                        count,
                        1,
                        ROW_HEIGHT
                    ),
                    None
                );
                assert_eq!(
                    timeline_cell_at(
                        Pos2::new(origin.x - 1., last.y),
                        origin,
                        cell,
                        count,
                        1,
                        ROW_HEIGHT
                    ),
                    None
                );
                assert_eq!(frame_at_x(last.x + 500., origin.x, cell, count), count - 1);
                assert_eq!(frame_at_x(origin.x - 500., origin.x, cell, count), 0);
            }
        }
    }
}
