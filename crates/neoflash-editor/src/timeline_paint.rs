use eframe::egui::{Painter, Pos2, Rect, Stroke};
pub(crate) const ROW_HEIGHT: f32 = 25.;
pub(crate) const RULER_HEIGHT: f32 = 25.;
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
                }
            }
        }
    }
}
