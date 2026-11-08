//! NF2 v2.25 frame-area geometry (`NF2/TL Grid`, `Grid Lines`, `Span`, `Marker`, `Frame Label`,
//! `Playhead`), checked against the locked Figma masters (`9:325`, `9:326`, `9:460`, `9:485`,
//! `9:499`, `9:651`) in every Frame View row height and frame width, so the painter cannot drift back
//! to row-centred, zoom-scaled glyphs.
use eframe::egui::{self, Pos2, Rect, Shape};
use neoflash_ui::components::timeline::{
    self as tl, FrameLabelKind, Geometry, MarkerKind, SpanType,
};
use neoflash_ui::{theme, typography, Theme};

/// Row heights: Compact, Short, Normal, Preview, Layer height 200 % / 300 %.
const ROWS: [f32; 6] = [14.0, 18.0, 28.0, 44.0, 56.0, 84.0];
/// Frame widths: Tiny, Small, Normal, Medium, Large.
const FRAMES: [f32; 5] = [6.0, 9.0, 12.0, 16.0, 22.0];

fn shapes(theme_: Theme, paint: impl Fn(&egui::Painter, &egui::Context)) -> Vec<Shape> {
    let ctx = egui::Context::default();
    ctx.set_fonts(typography::font_definitions());
    theme::install(&ctx, theme_);
    let mut out = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 400.0))),
            ..Default::default()
        };
        let full = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("tl-geometry"),
            ));
            paint(&painter, ctx);
        });
        out = full.shapes.into_iter().map(|c| c.shape).collect();
    }
    out
}

fn flat(shapes: Vec<Shape>) -> Vec<Shape> {
    let mut out = Vec::new();
    for s in shapes {
        match s {
            Shape::Vec(v) => out.extend(flat(v)),
            s => out.push(s),
        }
    }
    out
}

fn geo(frame_width: f32, row_height: f32) -> Geometry {
    Geometry {
        origin_x: 100.0,
        frame_width,
        row_height,
    }
}

fn row(h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(100.0, 40.0), egui::vec2(1400.0, h))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn markers_sit_on_the_glyph_line_with_fixed_sizes_in_every_view() {
    for t in [Theme::Dark, Theme::Light] {
        for h in ROWS {
            for w in FRAMES {
                let g = geo(w, h);
                let cell = g.cell(row(h), 5);
                let line = cell.bottom() - 9.0;
                let cx = cell.center().x;
                for kind in [
                    MarkerKind::Keyframe,
                    MarkerKind::BlankKeyframe,
                    MarkerKind::ActionKeyframe,
                ] {
                    let s = flat(shapes(t, |p, ctx| {
                        tl::paint_marker(p, ctx, cell, kind, false)
                    }));
                    let dot = s
                        .iter()
                        .find_map(|s| match s {
                            Shape::Circle(c) => Some(*c),
                            _ => None,
                        })
                        .expect("dot");
                    assert!(
                        close(dot.center.y, line),
                        "{kind:?} h{h} w{w}: {:?}",
                        dot.center
                    );
                    assert!(close(dot.center.x, cx));
                    let outer = dot.radius + dot.stroke.width / 2.0;
                    assert!(
                        close(outer, 3.0),
                        "{kind:?} h{h} w{w}: outer radius {outer}"
                    );
                }
                let s = flat(shapes(t, |p, ctx| {
                    tl::paint_marker(p, ctx, cell, MarkerKind::SpanEnd, false)
                }));
                let end = s
                    .iter()
                    .find_map(|s| match s {
                        Shape::Rect(r) => Some(r.rect),
                        _ => None,
                    })
                    .expect("span end");
                assert!(
                    close(end.width(), 5.0) && close(end.height(), 8.0),
                    "{end:?}"
                );
                assert!(close(end.center().y, line) && close(end.center().x, cx));
                let s = flat(shapes(t, |p, ctx| {
                    tl::paint_marker(p, ctx, cell, MarkerKind::PropertyKeyframe, true)
                }));
                let diamond = s
                    .iter()
                    .find_map(|s| match s {
                        Shape::Path(p) => Some(Rect::from_points(&p.points)),
                        _ => None,
                    })
                    .expect("diamond");
                assert!(close(diamond.width(), 7.0) && close(diamond.height(), 7.0));
                assert!(close(diamond.center().y, line));
            }
        }
    }
}

#[test]
fn normal_cell_matches_the_12_by_27_master() {
    let g = geo(12.0, 28.0);
    let cell = g.cell(row(28.0), 1);
    let s = flat(shapes(Theme::Dark, |p, ctx| {
        tl::paint_marker(p, ctx, cell, MarkerKind::ActionKeyframe, false)
    }));
    let dot = s
        .iter()
        .find_map(|s| match s {
            Shape::Circle(c) => Some(*c),
            _ => None,
        })
        .unwrap();
    // Master: Dot 6 × 6 at (3, 16) → centre (6, 19).
    assert_eq!(dot.center - cell.min, egui::vec2(6.0, 19.0));
    let text = s
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) => Some(t.pos),
            _ => None,
        })
        .expect("action a");
    assert!(close(text.y - cell.top(), 1.0), "a at the top of the cell");
}

#[test]
fn ticks_are_short_and_stay_below_spans() {
    let g = geo(12.0, 28.0);
    let r = row(28.0);
    let s = flat(shapes(Theme::Dark, |p, ctx| {
        tl::paint_grid_lines(p, ctx, r, &g, egui::Color32::RED)
    }));
    let mut n = 0;
    for s in &s {
        if let Shape::LineSegment { points, .. } = s {
            n += 1;
            let (a, b) = (points[0].y.min(points[1].y), points[0].y.max(points[1].y));
            // Master tick: 1 × 4 at y 23, i.e. directly above the 1 pt row line.
            assert!(
                close(a - r.top(), 23.0) && close(b - r.top(), 27.0),
                "{points:?}"
            );
            assert!(close((points[0].x - g.origin_x + 0.5) % 12.0, 0.0));
        }
    }
    assert!(n > 100, "one tick per visible frame");
}

#[test]
fn tween_line_and_arrowhead_follow_the_span_master() {
    for h in ROWS {
        let g = geo(12.0, h);
        // Classic tween over frames 1–15: 180 pt span, next keyframe at 180.
        let span = g.span(row(h), 1, 15);
        let line_y = span.bottom() - 9.0 - 0.5;
        for kind in [SpanType::ClassicTween, SpanType::ShapeTween] {
            let s = flat(shapes(Theme::Dark, |p, ctx| {
                tl::paint_span(p, ctx, span, kind, true)
            }));
            let line = s
                .iter()
                .find_map(|s| match s {
                    Shape::LineSegment { points, stroke } => Some((*points, stroke.width)),
                    _ => None,
                })
                .expect("tween line");
            assert!(close(line.1, 1.0));
            assert!(close(line.0[0].y, line_y) && close(line.0[1].y, line_y));
            assert!(
                close(line.0[0].x - span.left(), 11.0) && close(line.0[1].x - span.left(), 167.0)
            );
            let head = s
                .iter()
                .find_map(|s| match s {
                    Shape::Path(p) => Some(Rect::from_points(&p.points)),
                    _ => None,
                })
                .expect("arrowhead");
            assert!(
                close(head.width(), 4.0) && close(head.height(), 5.0),
                "{head:?}"
            );
            assert!(close(head.right() - span.left(), 171.0));
            assert!(close(head.center().y, span.bottom() - 9.0));
            // The band is one piece: no vertical edges or ticks inside a tween span.
            assert!(!s.iter().any(|s| matches!(s, Shape::LineSegment { points, .. } if close(points[0].x, points[1].x))));
        }
        let s = flat(shapes(Theme::Dark, |p, ctx| {
            tl::paint_span(p, ctx, span, SpanType::BrokenTween, true)
        }));
        assert!(
            !s.iter().any(|s| matches!(s, Shape::Path(_))),
            "broken tween has no arrowhead"
        );
        let dashes: Vec<_> = s
            .iter()
            .filter_map(|s| match s {
                Shape::LineSegment { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        assert!(dashes.len() > 10);
        assert!(dashes.iter().all(|d| close(d[0].y, line_y)));
        assert!(dashes.iter().all(|d| d[1].x <= span.right() - 9.0 + 0.01));
    }
}

#[test]
fn static_spans_close_with_one_point_edges_only() {
    let g = geo(12.0, 28.0);
    let span = g.span(row(28.0), 1, 12);
    for kind in [SpanType::Static, SpanType::MotionTween, SpanType::Selected] {
        let s = flat(shapes(Theme::Light, |p, ctx| {
            tl::paint_span(p, ctx, span, kind, true)
        }));
        let edges: Vec<f32> = s
            .iter()
            .filter_map(|s| match s {
                Shape::LineSegment { points, .. } if close(points[0].x, points[1].x) => {
                    Some(points[0].x - span.left())
                }
                _ => None,
            })
            .collect();
        assert_eq!(edges, vec![0.5, 143.5], "{kind:?}");
    }
}

#[test]
fn frame_labels_sit_at_the_top_after_the_keyframe_cell() {
    for h in [18.0, 28.0, 44.0] {
        for w in [9.0, 12.0, 22.0] {
            let g = geo(w, h);
            let cell = g.cell(row(h), 1);
            for (kind, text_x) in [
                (FrameLabelKind::Label, 11.0),
                (FrameLabelKind::Comment, 11.0),
                (FrameLabelKind::Anchor, 12.0),
            ] {
                let s = flat(shapes(Theme::Dark, |p, ctx| {
                    tl::paint_frame_label(p, ctx, cell, kind, "intro", 1400.0)
                }));
                let text = s
                    .iter()
                    .find_map(|s| match s {
                        Shape::Text(t) => Some(t.pos),
                        _ => None,
                    })
                    .expect("label text");
                // Instance at keyframe x + 13 (Normal), text at (11, 1) / Anchor (12, 1).
                assert!(
                    close(text.x - (cell.right() + 1.0), text_x),
                    "{kind:?} w{w} h{h}"
                );
                assert!(close(text.y - cell.top(), 1.0));
                let icon_top = s
                    .iter()
                    .filter_map(|s| match s {
                        Shape::Path(p) => p.points.iter().map(|p| p.y).reduce(f32::min),
                        Shape::LineSegment { points, .. } => Some(points[0].y.min(points[1].y)),
                        Shape::Circle(c) => Some(c.center.y - c.radius),
                        Shape::CubicBezier(b) => b.points.iter().map(|p| p.y).reduce(f32::min),
                        _ => None,
                    })
                    .reduce(f32::min)
                    .expect("icon");
                assert!(
                    (icon_top - cell.top() - 3.0).abs() <= 1.0,
                    "{kind:?}: icon top {}",
                    icon_top - cell.top()
                );
                let icon_left = s
                    .iter()
                    .filter_map(|s| match s {
                        Shape::Path(p) => p.points.iter().map(|p| p.x).reduce(f32::min),
                        Shape::LineSegment { points, .. } => Some(points[0].x.min(points[1].x)),
                        Shape::CubicBezier(b) => b.points.iter().map(|p| p.x).reduce(f32::min),
                        _ => None,
                    })
                    .reduce(f32::min)
                    .expect("icon left");
                assert!(icon_left - cell.right() >= 1.0 && icon_left - cell.right() <= 3.5);
            }
        }
    }
}

#[test]
fn playhead_chip_shares_the_ruler_digit_line() {
    let g = geo(12.0, 28.0);
    let ruler = Rect::from_min_size(Pos2::new(100.0, 0.0), egui::vec2(1400.0, 32.0));
    let full = Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), 300.0));
    for time in [Some("0.29s"), None] {
        let digits = tl::ruler_digits_top(ruler, time.is_some());
        let s = flat(shapes(Theme::Dark, |p, ctx| {
            tl::paint_playhead(p, ctx, full, ruler, &g, g.frame_x(8), 8, time)
        }));
        let chip = s
            .iter()
            .find_map(|s| match s {
                Shape::Rect(r) if close(r.rect.height(), 16.0) => Some(r.rect),
                _ => None,
            })
            .expect("frame chip");
        assert!(chip.width() >= 20.0);
        assert!(close(chip.center().x, g.frame_x(8) + 6.0));
        let label = s.iter().find_map(|s| match s {
            Shape::Text(t) if (t.pos.y - digits).abs() < 0.5 => Some(t.pos),
            _ => None,
        });
        assert!(
            label.is_some(),
            "frame number on the digits' top line ({time:?})"
        );
        assert!(chip.bottom() <= ruler.bottom());
    }
}

#[test]
fn ruler_frame_numbers_are_tick_centred_and_frame_one_is_edge_safe() {
    let ruler = Rect::from_min_size(Pos2::new(100.0, 0.0), egui::vec2(1400.0, 32.0));
    for theme in [Theme::Dark, Theme::Light] {
        for (width, labelled_frames) in [
            (6.0, &[20_u32, 100][..]),
            (9.0, &[5_u32, 10, 100][..]),
            (12.0, &[5_u32, 10, 100][..]),
        ] {
            let g = geo(width, 28.0);
            let s = flat(shapes(theme, |p, ctx| {
                let clipped = p.with_clip_rect(ruler);
                tl::paint_ruler(&clipped, ctx, ruler, &g, 24.0, false);
            }));
            let label = |wanted: &str| {
                s.iter()
                    .find_map(|shape| match shape {
                        Shape::Text(text) if text.galley.job.text == wanted => Some(text),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing ruler label {wanted}"))
            };

            // Major labels stay optically centred on their half-pixel tick regardless of digit
            // count and frame zoom.
            for &frame in labelled_frames {
                let text = label(&frame.to_string());
                let centre = text.pos.x + text.galley.size().x / 2.0;
                assert!(
                    close(centre, g.frame_x(frame) + 0.5),
                    "{theme:?} width {width} frame {frame}: label centre {centre}, tick {}",
                    g.frame_x(frame) + 0.5
                );
            }

            // The first tick is at the left clip edge, so its label is shifted just enough to
            // remain completely visible instead of losing the left half of the glyph.
            let first = label("1");
            assert!(
                first.pos.x >= ruler.left() - 0.01,
                "{theme:?} width {width}: frame 1 starts at {}, clip starts at {}",
                first.pos.x,
                ruler.left()
            );
            assert!(
                first.pos.x + first.galley.size().x <= ruler.right() + 0.01,
                "{theme:?} width {width}: frame 1 leaves the ruler"
            );
        }
    }
}

#[test]
fn compact_layer_rows_fit_their_content() {
    // `NF2/TL Layer` presets (R-052): Normal 28 pt NF2/Body, Short 22 pt NF2/Small and Compact 16 pt
    // NF2/Caption labels; Short and Compact use 12 pt icons.
    for h in [16.0_f32, 22.0, 28.0] {
        let ctx = egui::Context::default();
        ctx.set_fonts(typography::font_definitions());
        theme::install(&ctx, Theme::Dark);
        let mut row_rect = Rect::NOTHING;
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 200.0))),
                ..Default::default()
            };
            let full = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = tl::LayerRow {
                        label: "Guided",
                        kind: tl::LayerKind::FolderOpen,
                        selected: false,
                        indent: 0,
                        editing: true,
                        visible: true,
                        locked: true,
                        outline: false,
                        color: egui::Color32::LIGHT_BLUE,
                    }
                    .show(ui, 280.0, h);
                    row_rect = r.row.rect;
                });
            });
            shapes = flat(full.shapes.into_iter().map(|c| c.shape).collect());
        }
        let label = shapes
            .iter()
            .find_map(|s| match s {
                Shape::Text(t) if t.galley.job.text == "Guided" => Some(t.clone()),
                _ => None,
            })
            .expect("label");
        let size = label.galley.job.sections[0].format.font_id.size;
        let expect = typography::font(if h < 22.0 {
            neoflash_ui::TextStyle::Caption
        } else if h < 28.0 {
            neoflash_ui::TextStyle::Small
        } else {
            neoflash_ui::TextStyle::Body
        })
        .size;
        assert!(close(size, expect), "h{h}: label {size}");
        assert!(close(
            tl::layer_row_icon(h),
            if h < 28.0 { 12.0 } else { 16.0 }
        ));
        let glyph_h = label.galley.size().y;
        assert!(glyph_h <= h + 2.5, "h{h}: label line {glyph_h}");
        for s in &shapes {
            if let Shape::Mesh(m) = s {
                let r = Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
                assert!(
                    r.top() >= row_rect.top() - 0.5 && r.bottom() <= row_rect.bottom() + 0.5,
                    "h{h}: icon {r:?} leaves row {row_rect:?}"
                );
            }
        }
    }
}
