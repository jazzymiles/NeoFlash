use neoflash_core::{bitmap_trace::*, *};
fn image(w: u32, h: u32, pixel: impl Fn(u32, u32) -> Color) -> DecodedImage {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend(pixel(x, y));
        }
    }
    DecodedImage {
        width: w,
        height: h,
        rgba: rgba.into(),
    }
}
fn exact() -> TraceOptions {
    TraceOptions {
        paths: 100.,
        corners: 100.,
        noise: 1,
        method: TraceMethod::Abutting,
        ..Default::default()
    }
}
fn paint(trace: &BitmapTrace, x: f64, y: f64) -> Color {
    let mut colour = [0; 4];
    for shape in &trace.shapes {
        if hit_test_path(
            &shape.commands,
            shape.fill_rule,
            [x, y],
            true,
            0.,
            FlattenOptions::default(),
        )
        .unwrap()
        {
            colour = shape.colour;
        }
    }
    colour
}
#[test]
fn trace_preserves_holes_islands_alpha_and_method_composition() {
    let source = image(12, 10, |x, y| {
        if x == 0 || y == 0 || x == 11 || y == 9 || (4..8).contains(&x) && (3..7).contains(&y) {
            [0; 4]
        } else if x == 2 && y == 2 {
            [0, 90, 220, 128]
        } else if x > 8 {
            [20, 180, 80, 255]
        } else {
            [230, 80, 40, 255]
        }
    });
    for method in [TraceMethod::Abutting, TraceMethod::Overlapping] {
        let options = TraceOptions {
            method,
            palette: TracePalette::Full,
            ..exact()
        };
        let mut last = 0.;
        let trace = trace_bitmap(&source, &options, |p| {
            assert!(p >= last);
            last = p;
            true
        })
        .unwrap()
        .unwrap();
        assert!(trace.path_count >= 4);
        for y in 0..source.height {
            for x in 0..source.width {
                let expected: Color = source.rgba[((y * source.width + x) * 4) as usize..][..4]
                    .try_into()
                    .unwrap();
                assert_eq!(
                    paint(&trace, x as f64 + 0.5, y as f64 + 0.5),
                    expected,
                    "{method:?} at {x},{y}"
                );
            }
        }
        for s in &trace.shapes {
            validate_path_commands(&s.commands).unwrap();
        }
        // Diagonal contacts create several valid continuations at one pixel corner.
        let mosaic = image(18, 16, |x, y| {
            [
                [0; 4],
                [10, 20, 30, 255],
                [100, 80, 40, 255],
                [20, 150, 250, 128],
            ][((x * 7 + y * 13 + x * y) % 4) as usize]
        });
        let traced = trace_bitmap(&mosaic, &options, |_| true).unwrap().unwrap();
        for y in 0..mosaic.height {
            for x in 0..mosaic.width {
                let expected: Color = mosaic.rgba[((y * mosaic.width + x) * 4) as usize..][..4]
                    .try_into()
                    .unwrap();
                assert_eq!(
                    paint(&traced, x as f64 + 0.5, y as f64 + 0.5),
                    expected,
                    "diagonal {method:?} {x},{y}"
                );
            }
        }
    }
}
#[test]
fn trace_controls_change_geometry_palette_and_preserve_cancellation() {
    let source = image(36, 32, |x, y| {
        if (x as i32 - 18).pow(2) + (y as i32 - 16).pow(2) < 180 {
            [x as u8 * 7, y as u8 * 7, 80, 255]
        } else {
            [255; 4]
        }
    });
    let options = TraceOptions {
        colours: 6,
        ..exact()
    };
    let detailed = trace_bitmap(&source, &options, |_| true).unwrap().unwrap();
    assert!(detailed.shapes.len() <= 6);
    let smooth = trace_bitmap(
        &source,
        &TraceOptions {
            paths: 10.,
            corners: 0.,
            ..options.clone()
        },
        |_| true,
    )
    .unwrap()
    .unwrap();
    assert!(
        smooth
            .shapes
            .iter()
            .flat_map(|s| &s.commands)
            .any(|c| matches!(c, PathCommand::QuadraticTo { .. }))
    );
    assert!(
        smooth
            .shapes
            .iter()
            .map(|s| s.commands.len())
            .sum::<usize>()
            < detailed
                .shapes
                .iter()
                .map(|s| s.commands.len())
                .sum::<usize>()
    );
    for mode in [TraceMode::Greyscale, TraceMode::BlackAndWhite] {
        let trace = trace_bitmap(
            &source,
            &TraceOptions {
                mode,
                ignore_white: true,
                ..options.clone()
            },
            |_| true,
        )
        .unwrap()
        .unwrap();
        assert!(
            trace
                .shapes
                .iter()
                .all(|s| s.colour[0] == s.colour[1] && s.colour[1] == s.colour[2])
        );
        assert_eq!(paint(&trace, 0.5, 0.5), [0; 4]);
    }
    let noisy = image(12, 12, |x, y| {
        if x == 5 && y == 5 {
            [0, 0, 0, 255]
        } else {
            [240, 240, 240, 255]
        }
    });
    let cleaned = trace_bitmap(
        &noisy,
        &TraceOptions {
            noise: 2,
            ..exact()
        },
        |_| true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        paint(&cleaned, 5.5, 5.5),
        [240, 240, 240, 255],
        "noise merges into adjacent paint without punching a hole"
    );
    assert!(
        trace_bitmap(&source, &options, |p| p < 0.5)
            .unwrap()
            .is_none()
    );
    assert!(
        trace_bitmap(
            &source,
            &TraceOptions {
                fills: false,
                strokes: false,
                ..options
            },
            |_| true
        )
        .is_err()
    );
    assert!(
        trace_bitmap(
            &DecodedImage {
                width: 4097,
                height: 1,
                rgba: vec![255; 4097 * 4].into()
            },
            &exact(),
            |_| true
        )
        .is_err()
    );
}
