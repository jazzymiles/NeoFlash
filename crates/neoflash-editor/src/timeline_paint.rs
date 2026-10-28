use eframe::egui::{Painter, Pos2, Rect, Stroke};
pub(crate) const ROW_HEIGHT: f32 = crate::theme::ROW_HEIGHT;
pub(crate) const RULER_HEIGHT: f32 = crate::theme::ROW_HEIGHT;
pub(crate) const LAYER_CONTROL_WIDTH: f32 = 24.;
pub(crate) const LAYER_COLUMN_GAP: f32 = 4.;

/// Use 1/2/5 intervals so labels remain legible from dense overview to wide cells.
pub(crate) fn ruler_label_step(cell_width: f32, frame_count: u32) -> u32 {
    let digits = frame_count.max(1).ilog10() + 1;
    let minimum = ((digits as f32 * 6. + 10.) / cell_width.max(1.)).ceil() as u32;
    let mut decade = 1_u32;
    loop {
        for multiple in [1, 2, 5] {
            let step = decade.saturating_mul(multiple);
            if step >= minimum {
                return step;
            }
        }
        decade = decade.saturating_mul(10);
    }
}

/// Actual editable cells exclude the end gutter and the space below the layers.
pub(crate) fn timeline_cell_at(
    pos: Pos2,
    origin: Pos2,
    cell_width: f32,
    frame_count: u32,
    layer_count: usize,
) -> Option<(u32, usize)> {
    let local = pos - origin;
    if !pos.is_finite()
        || cell_width <= 0.
        || local.x < 0.
        || local.y < 0.
        || local.x >= frame_count as f32 * cell_width
        || local.y >= layer_count as f32 * ROW_HEIGHT
    {
        return None;
    }
    Some((
        (local.x / cell_width).floor() as u32,
        (local.y / ROW_HEIGHT).floor() as usize,
    ))
}

/// Once a real key drag owns the pointer, horizontal edges clamp its destination.
pub(crate) fn frame_at_x(x: f32, origin: f32, cell_width: f32, frame_count: u32) -> u32 {
    (((x - origin) / cell_width).floor().max(0.) as u32).min(frame_count.saturating_sub(1))
}

pub(crate) fn paint_tween_arrow(
    painter: &Painter,
    start: Pos2,
    end: Pos2,
    row: Rect,
    stroke: Stroke,
) {
    if !start.is_finite() || !end.is_finite() || end.x - start.x < 8. {
        return;
    }
    let p = painter.with_clip_rect(painter.clip_rect().intersect(row));
    p.line_segment([start, end], stroke);
    p.line_segment([Pos2::new(end.x - 6., end.y - 3.), end], stroke);
    p.line_segment([Pos2::new(end.x - 6., end.y + 3.), end], stroke);
}
#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Color32, Shape, Vec2};
    #[test]
    fn long_tweens_have_constant_heads_and_row_clip_at_every_scale() {
        assert_eq!(ruler_label_step(52., 500), 1);
        assert_eq!(ruler_label_step(13., 500), 5);
        assert_eq!(ruler_label_step(3., 500), 10);
        for frames in [5., 60., 500.] {
            for cell in [8., 13., 26.] {
                for dpi in [1., 1.5, 2.] {
                    let ctx = egui::Context::default();
                    ctx.set_pixels_per_point(dpi);
                    let row =
                        Rect::from_min_size(Pos2::new(-300., 50.), Vec2::new(1000., ROW_HEIGHT));
                    let end = Pos2::new(-300. + frames * cell - 8., 62.5);
                    let output = ctx.run(Default::default(), |ctx| {
                        paint_tween_arrow(
                            &ctx.layer_painter(egui::LayerId::background()),
                            Pos2::new(-288., 62.5),
                            end,
                            row,
                            Stroke::new(1.0_f32, Color32::BLACK),
                        )
                    });
                    assert_eq!(output.shapes.len(), 3);
                    for shape in &output.shapes {
                        assert!(row.contains_rect(shape.clip_rect));
                    }
                    for shape in &output.shapes[1..] {
                        let Shape::LineSegment { points, .. } = shape.shape else {
                            panic!("arrowhead must be a line")
                        };
                        assert_eq!(points[1], end);
                        assert_eq!(end.x - points[0].x, 6.);
                        assert_eq!((end.y - points[0].y).abs(), 3.);
                    }
                    // The same geometry is used after horizontal scrolling. A
                    // gutter click must not impersonate the final frame/layer,
                    // while a captured key drag can reach the exact end cell.
                    let origin = row.min;
                    let count = frames as u32;
                    let last = Pos2::new(origin.x + (frames - 0.25) * cell, origin.y + 12.);
                    assert_eq!(
                        timeline_cell_at(last, origin, cell, count, 1),
                        Some((count - 1, 0))
                    );
                    assert_eq!(
                        timeline_cell_at(
                            Pos2::new(origin.x + frames * cell, last.y),
                            origin,
                            cell,
                            count,
                            1
                        ),
                        None
                    );
                    assert_eq!(
                        timeline_cell_at(
                            Pos2::new(last.x, origin.y + ROW_HEIGHT),
                            origin,
                            cell,
                            count,
                            1
                        ),
                        None
                    );
                    assert_eq!(
                        timeline_cell_at(Pos2::new(origin.x - 1., last.y), origin, cell, count, 1),
                        None
                    );
                    assert_eq!(frame_at_x(last.x + 500., origin.x, cell, count), count - 1);
                    assert_eq!(frame_at_x(origin.x - 500., origin.x, cell, count), 0);
                }
            }
        }
    }
}
