use super::cache_paint::*;
use super::deform_envelope::default_envelope_grid_points;
use super::*;
use crate::app::Editor;
use eframe::egui::{self, Color32, Pos2, Shape};
use neoflash_core::*;
use std::sync::Arc;

fn rectangle_commands(width: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: width, y: 0. },
        PathCommand::LineTo { x: width, y: 12. },
        PathCommand::LineTo { x: 0., y: 12. },
        PathCommand::Close,
    ]
}

fn gradient(alpha: u8) -> GradientPaint {
    GradientPaint {
        kind: GradientKind::Linear,
        matrix: [1., 0., 0., 1., 0., 0.],
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [20, 40, 60, alpha],
            },
            GradientStop {
                ratio: 255,
                color: [80, 100, 120, alpha],
            },
        ],
        focal_point_ratio: 0.,
        ..Default::default()
    }
}

fn rendered_path_passes(kind: &ElementKind) -> usize {
    let ctx = egui::Context::default();
    ctx.run(Default::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        match kind {
            ElementKind::Path {
                commands,
                fill_rule,
                fill,
                stroke,
                stroke_width,
            } => paint_path(
                &painter,
                commands,
                PathPaint {
                    rule: *fill_rule,
                    fill: Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]),
                    stroke: Color32::from_rgba_unmultiplied(
                        stroke[0], stroke[1], stroke[2], stroke[3],
                    ),
                    width: *stroke_width,
                    tolerance: 0.1,
                    fingerprint: None,
                },
                |point| point,
            ),
            ElementKind::PaintedPath {
                commands,
                fill_rule,
                fill,
                stroke,
                stroke_width,
                fill_gradient,
                stroke_gradient,
            } => paint_gradient_path(
                &painter,
                commands,
                None,
                *fill_rule,
                *stroke_width,
                (fill, fill_gradient.as_ref()),
                (stroke, stroke_gradient.as_ref()),
                |point| point,
                |color| Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
            ),
            _ => unreachable!("test cases are path renderers"),
        }
    })
    .shapes
    .iter()
    .filter(|clipped| matches!(clipped.shape, Shape::Mesh(_)))
    .count()
}

#[test]
fn vector_visibility_matches_renderer_and_stage_hit_testing() {
    let commands = rectangle_commands(20.);
    let cases = [
        (
            "solid fill",
            ElementKind::Path {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [20, 40, 60, 255],
                stroke: [0; 4],
                stroke_width: 3.,
            },
            VectorPaintVisibility {
                fill: true,
                stroke: false,
            },
            Pos2::new(10., 6.),
        ),
        (
            "painted solid fallbacks",
            ElementKind::PaintedPath {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [20, 40, 60, 255],
                stroke: [80, 100, 120, 255],
                stroke_width: 3.,
                fill_gradient: None,
                stroke_gradient: None,
            },
            VectorPaintVisibility {
                fill: true,
                stroke: true,
            },
            Pos2::new(10., 6.),
        ),
        (
            "opaque fill gradient overrides transparent solid",
            ElementKind::PaintedPath {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 3.,
                fill_gradient: Some(gradient(255)),
                stroke_gradient: None,
            },
            VectorPaintVisibility {
                fill: true,
                stroke: false,
            },
            Pos2::new(10., 6.),
        ),
        (
            "transparent fill gradient overrides opaque solid",
            ElementKind::PaintedPath {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [20, 40, 60, 255],
                stroke: [0; 4],
                stroke_width: 3.,
                fill_gradient: Some(gradient(0)),
                stroke_gradient: None,
            },
            VectorPaintVisibility {
                fill: false,
                stroke: false,
            },
            Pos2::new(10., 6.),
        ),
        (
            "opaque stroke gradient overrides transparent solid",
            ElementKind::PaintedPath {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 3.,
                fill_gradient: None,
                stroke_gradient: Some(gradient(255)),
            },
            VectorPaintVisibility {
                fill: false,
                stroke: true,
            },
            Pos2::new(0., 6.),
        ),
        (
            "transparent stroke gradient overrides opaque solid",
            ElementKind::PaintedPath {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [20, 40, 60, 255],
                stroke_width: 3.,
                fill_gradient: None,
                stroke_gradient: Some(gradient(0)),
            },
            VectorPaintVisibility {
                fill: false,
                stroke: false,
            },
            Pos2::new(0., 6.),
        ),
        (
            "zero-width stroke",
            ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [20, 40, 60, 255],
                stroke_width: 0.,
                fill_gradient: None,
                stroke_gradient: None,
            },
            VectorPaintVisibility {
                fill: false,
                stroke: false,
            },
            Pos2::new(0., 6.),
        ),
    ];
    let ctx = egui::Context::default();
    let painter = ctx.layer_painter(egui::LayerId::background());
    for (name, kind, expected, probe) in cases {
        assert_eq!(
            rendered_path_passes(&kind),
            usize::from(expected.fill) + usize::from(expected.stroke),
            "renderer passes: {name}"
        );
        let item = DrawItem {
            element_id: name.into(),
            name: String::new(),
            instance_path: vec![name.into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., 0., 0.],
            alpha: 1.,
            color_transform: Default::default(),
            bitmap_fill: None,
            mesh_fill: None,
            path_fingerprint: None,
            kind,
        };
        assert_eq!(vector_paint_visibility(&item), Some(expected), "{name}");
        assert_eq!(
            crate::stage::hit_item_with_slop(&item, &painter, probe, 1., 3.),
            expected.fill || expected.stroke,
            "stage hit: {name}"
        );
    }
}

#[test]
fn supplied_path_fingerprint_matches_fallback_bounds_and_tessellation_keys() {
    let commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 20.,
            cy: 30.,
            x: 40.,
            y: 0.,
        },
        PathCommand::LineTo { x: 0., y: 0. },
        PathCommand::Close,
    ];
    let fingerprint = path_commands_fingerprint(&commands);

    assert_eq!(
        bounds_cache_key(&commands, None),
        bounds_cache_key(&commands, Some(fingerprint))
    );
    assert_eq!(
        path_bounds_cached(&commands, None),
        path_bounds_cached(&commands, Some(fingerprint))
    );
    for width in [None, Some(3.5)] {
        assert_eq!(
            tess_cache_key(&commands, None, FillRule::EvenOdd, width, 0.123),
            tess_cache_key(
                &commands,
                Some(fingerprint),
                FillRule::EvenOdd,
                width,
                0.123
            )
        );
    }

    let fallback = tessellate_cached(&commands, FillRule::EvenOdd, None, 0.123).unwrap();
    let supplied = tessellate_cached_with_fingerprint(
        &commands,
        Some(fingerprint),
        FillRule::EvenOdd,
        None,
        0.123,
    )
    .unwrap();
    assert!(Arc::ptr_eq(&fallback, &supplied));
}

#[test]
fn retained_screen_mesh_reuses_arc_and_replaces_changed_draw_slot() {
    SCREEN_MESH_CACHE.with(|cache| {
        *cache.borrow_mut() = ScreenMeshCache::new(
            MAX_SCREEN_MESH_CACHE_ENTRIES,
            MAX_SCREEN_MESH_CACHE_ELEMENTS,
        );
    });
    let commands = rectangle_commands(20.);
    let fingerprint = path_commands_fingerprint(&commands);
    let mut item = DrawItem {
        element_id: "retained-path".into(),
        name: String::new(),
        instance_path: vec!["outer".into(), "retained-path".into()],
        layer_id: "layer".into(),
        transform: [1., 0., 0., 1., 5., 7.],
        alpha: 1.,
        color_transform: Default::default(),
        bitmap_fill: None,
        mesh_fill: None,
        path_fingerprint: Some(fingerprint),
        kind: ElementKind::Path {
            commands: commands.clone(),
            fill_rule: FillRule::NonZero,
            fill: [220, 40, 20, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    };
    let origin = Pos2::new(30., 40.);
    let color = Color32::from_rgb(220, 40, 20);
    let build = |item: &DrawItem, commands: &[PathCommand], color, style| {
        retained_screen_mesh(
            item,
            commands,
            item.path_fingerprint,
            FillRule::NonZero,
            None,
            0.1,
            color,
            origin,
            1.5,
            style,
            ScreenMeshPass::Fill,
        )
        .unwrap()
    };

    let first = build(&item, &commands, color, RetainedPathStyle::Artwork);
    let warm = build(&item, &commands, color, RetainedPathStyle::Artwork);
    assert!(Arc::ptr_eq(&first, &warm));

    item.transform[4] += 10.;
    let moved = build(&item, &commands, color, RetainedPathStyle::Artwork);
    assert!(!Arc::ptr_eq(&warm, &moved));
    let recolored = build(
        &item,
        &commands,
        Color32::from_rgb(20, 80, 230),
        RetainedPathStyle::Artwork,
    );
    assert!(!Arc::ptr_eq(&moved, &recolored));

    let changed_commands = rectangle_commands(28.);
    item.path_fingerprint = Some(path_commands_fingerprint(&changed_commands));
    let reshaped = build(
        &item,
        &changed_commands,
        Color32::from_rgb(20, 80, 230),
        RetainedPathStyle::Artwork,
    );
    assert!(!Arc::ptr_eq(&recolored, &reshaped));
    let outlined = build(
        &item,
        &changed_commands,
        Color32::from_rgb(20, 80, 230),
        RetainedPathStyle::Outline,
    );
    assert!(!Arc::ptr_eq(&reshaped, &outlined));

    SCREEN_MESH_CACHE.with(|cache| {
        let cache = cache.borrow();
        assert_eq!(
            cache.entries.len(),
            1,
            "changed drag positions replace one slot"
        );
    });
}

#[test]
fn retained_screen_mesh_cache_is_bounded_by_entries_and_elements() {
    let commands = rectangle_commands(20.);
    let fingerprint = path_commands_fingerprint(&commands);
    let (tessellation, _) =
        tess_cache_key(&commands, Some(fingerprint), FillRule::NonZero, None, 0.1);
    let key = ScreenMeshKey {
        tessellation,
        transform_bits: [0; 6],
        origin_bits: [0; 2],
        zoom_bits: 1.0_f32.to_bits(),
        color: [10, 20, 30, 255],
        style: RetainedPathStyle::Artwork,
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(Pos2::ZERO, Color32::WHITE);
    let mesh = Arc::new(mesh);
    let slot = |identity| ScreenMeshSlot {
        identity,
        pass: ScreenMeshPass::Fill,
    };

    let mut by_entries = ScreenMeshCache::new(2, usize::MAX);
    for identity in 0..3 {
        by_entries.insert(slot(identity), key, mesh.clone());
    }
    assert!(by_entries.entries.len() <= 2);

    let mut by_elements = ScreenMeshCache::new(10, 2);
    for identity in 0..3 {
        by_elements.insert(slot(identity), key, mesh.clone());
    }
    assert!(by_elements.total_elements <= 2);
    assert!(by_elements.entries.len() <= 2);
}

mod vector_t233_tests {
    use super::*;

    #[test]
    fn envelope_mesh_handle_drag_and_bake_with_one_step_undo_and_round_trip() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "shape-env".into(),
                name: "Shape".into(),
                transform: Transform {
                    x: 100.0,
                    y: 50.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 200.0,
                    height: 100.0,
                    fill: [60, 120, 240, 255],
                    stroke: [10, 20, 40, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            });
        editor.select_single_id("shape-env".into());

        // Set a 2x2 EnvelopeWarp grid in 1 Undo step
        let grid = EnvelopeWarp {
            preset: EnvelopeWarpPreset::CustomGrid,
            rows: 2,
            cols: 2,
            control_points: default_envelope_grid_points(2, 2),
            ..EnvelopeWarp::default()
        };
        assert!(editor.set_selected_envelope_warp(Some(grid)));
        let handles = editor.selected_envelope_stage_handles();
        assert_eq!(handles.len(), 9);
        assert_eq!(
            editor.hit_test_envelope_stage_handle([100.0, 50.0], 8.0),
            Some((0, 0))
        );

        // Drag center handle (1, 1) in Stage space -> updates control_points[4] in 1 Undo step
        assert!(editor.drag_selected_envelope_handle_stage(1, 1, [220.0, 130.0]));
        let updated_warp = editor.selected_envelope_warp().expect("envelope warp");
        assert_eq!(updated_warp.preset, EnvelopeWarpPreset::CustomGrid);
        assert!((updated_warp.control_points[4][0] - 0.6).abs() < 1e-6);
        assert!((updated_warp.control_points[4][1] - 0.8).abs() < 1e-6);

        // Undo handle drag -> restores original (0.5, 0.5) center control point
        editor.undo();
        let restored_warp = editor.selected_envelope_warp().expect("restored warp");
        assert!((restored_warp.control_points[4][0] - 0.5).abs() < 1e-6);
        assert!((restored_warp.control_points[4][1] - 0.5).abs() < 1e-6);

        // Redo handle drag, save/reopen, and validate
        editor.redo();
        editor.project.validate().expect("valid project");
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("reopened valid");
    }

    #[test]
    fn perspective_grid_handle_drag_plane_switch_snap_and_undo() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert!(editor.active_perspective_grid().is_none());

        // Enable perspective grid
        assert!(editor.toggle_perspective_grid());
        let g = editor.active_perspective_grid().expect("perspective grid");
        assert!(g.enabled);

        // Drag left vanishing point and horizon in 1 Undo step
        assert!(editor.drag_perspective_grid_handle(
            PerspectiveGridHandle::LeftVanishingPoint,
            [-320.0, 180.0],
        ));
        let g2 = editor.active_perspective_grid().expect("updated grid");
        assert!((g2.left_vanishing_x - (-320.0)).abs() < 1e-6);
        assert!((g2.horizon_y - 180.0).abs() < 1e-6);
        assert_eq!(
            editor.hit_test_perspective_grid_handle([-320.0, 180.0], 10.0),
            Some(PerspectiveGridHandle::LeftVanishingPoint)
        );

        // Switch active plane to RightGrid
        assert!(editor.set_perspective_active_plane(PerspectivePlane::RightGrid));
        let g3 = editor.active_perspective_grid().expect("plane switched");
        assert_eq!(g3.active_plane, PerspectivePlane::RightGrid);

        // Snap a Stage point onto the active perspective grid
        let snapped = g3.snap_point([415.0, 205.0], 24.0);
        assert!(snapped[0].is_finite() && snapped[1].is_finite());

        // 1-step Undo restores previous plane
        editor.undo();
        assert_eq!(
            editor.active_perspective_grid().expect("grid").active_plane,
            PerspectivePlane::LeftGrid
        );

        editor.project.validate().expect("valid");
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("reopened valid");
        assert!(reopened.symbols[0].timeline.perspective_grid().is_some());
    }
}
