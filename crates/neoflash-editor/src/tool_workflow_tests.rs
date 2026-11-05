use crate::{
    app::{Editor, Tool},
    panels::customize_tools::CustomizeToolsDraft,
    tests::{primary_button, rectangle_stage, stage_input_frame},
    tool_preferences::{PencilMode, ToolPreferences},
    workspace::{WorkspacePrefs, WorkspaceTemplate},
};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::BrushShapeKind;

fn bounds(commands: &[PathCommand]) -> egui::Rect {
    let points: Vec<_> = flatten_path(commands, FlattenOptions::default())
        .unwrap()
        .iter()
        .flat_map(|p| {
            p.points
                .iter()
                .map(|p| egui::pos2(p[0] as f32, p[1] as f32))
        })
        .collect();
    egui::Rect::from_points(&points)
}

#[test]
fn brush_properties_outline_repeats_without_seams_and_keeps_one_undo() {
    fn properties(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640., 1600.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.properties(ui, ctx));
            },
        )
    }
    fn edit(editor: &mut Editor, ctx: &egui::Context, time: &mut f64, label: &str, text: &str) {
        *time += 0.1;
        let output = properties(editor, ctx, *time, vec![]);
        let point = named_bounds(&output, label)
            .unwrap_or_else(|| panic!("missing {label}"))
            .center();
        *time += 0.1;
        properties(
            editor,
            ctx,
            *time,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
                primary_button(point, false),
            ],
        );
        *time += 0.1;
        properties(
            editor,
            ctx,
            *time,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ctrl: true,
                        mac_cmd: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text(text.into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        ctx.enable_accesskit();
        editor.set_theme(&ctx, theme);
        editor.workspace.drawing.object_drawing = true;
        editor.set_tool(Tool::Brush);
        editor.authoring_tools.brush_size = 24.;
        let from = page.min + egui::vec2(100., 150.);
        let to = from + egui::vec2(150., 0.);
        stage_input_frame(
            &mut editor,
            &ctx,
            0.1,
            vec![
                egui::Event::PointerMoved(from),
                primary_button(from, true),
                egui::Event::PointerMoved(to),
                primary_button(to, false),
            ],
        );
        let first = editor.selected_element().unwrap().clone();
        assert!(
            matches!(
                first.kind,
                ElementKind::Path {
                    stroke_width: 0.,
                    ..
                }
            ),
            "default Brush stays fill-only"
        );
        let mut time = 1.;
        edit(&mut editor, &ctx, &mut time, "Stroke hex", "#3E8640");
        edit(&mut editor, &ctx, &mut time, "Stroke alpha", "75");
        edit(&mut editor, &ctx, &mut time, "Stroke width", "2.5");
        editor.finish_edits();
        assert_eq!(editor.stroke, [62, 134, 64, 191]);
        assert_eq!(editor.stroke_width, 2.5);
        assert!(editor.authoring_tools.brush_outline);
        let styled = editor.selected_element().unwrap().clone();
        let (commands, _) = styled.to_path_commands().unwrap();
        assert!(
            !hit_test_path(
                &commands,
                FillRule::NonZero,
                [175., 150.],
                false,
                2.5,
                FlattenOptions::default()
            )
            .unwrap(),
            "no interior triangle strokes"
        );
        assert!(
            hit_test_path(
                &commands,
                FillRule::NonZero,
                [175., 138.],
                false,
                2.5,
                FlattenOptions::default()
            )
            .unwrap(),
            "external outline remains"
        );
        let saved = editor.project.clone();
        let undo = editor.history.undo_len();
        for (i, shape) in [BrushShapeKind::Round, BrushShapeKind::Square]
            .into_iter()
            .enumerate()
        {
            editor.authoring_tools.brush_shape = shape;
            let from = from + egui::vec2(0., 120. + i as f32 * 100.);
            let to = from + egui::vec2(150., 0.);
            time += 0.2;
            stage_input_frame(&mut editor, &ctx, time, vec![]);
            time += 0.1;
            stage_input_frame(
                &mut editor,
                &ctx,
                time,
                vec![
                    egui::Event::PointerMoved(from),
                    primary_button(from, true),
                    egui::Event::PointerMoved(to),
                    primary_button(to, false),
                ],
            );
            let new = editor.selected_element().unwrap();
            assert!(
                matches!(&new.kind,ElementKind::Path { stroke,stroke_width,.. } if *stroke == [62,134,64,191] && *stroke_width == 2.5)
            );
            editor.project.validate().unwrap();
            assert_eq!(editor.history.undo_len(), undo + 1);
            let reopened: Project =
                serde_json::from_str(&serde_json::to_string(&editor.project).unwrap()).unwrap();
            reopened.validate().unwrap();
            let reopened_path = reopened.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .find(|e| e.id == new.id)
                .unwrap();
            assert!(
                matches!(&reopened_path.kind,ElementKind::Path { stroke,stroke_width,.. }
                if *stroke == [62,134,64,191] && *stroke_width == 2.5)
            );
            let (before, _) = new.to_path_commands().unwrap();
            let (after, _) = reopened_path.to_path_commands().unwrap();
            let before = flatten_path(&before, FlattenOptions::default()).unwrap();
            let after = flatten_path(&after, FlattenOptions::default()).unwrap();
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(&after) {
                assert_eq!(before.points.len(), after.points.len());
                for (a, b) in before.points.iter().zip(&after.points) {
                    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
                }
            }
            editor.undo();
            assert_eq!(editor.project, saved);
            assert_eq!(
                editor.stroke_width, 2.5,
                "Undo artwork keeps chosen tool settings"
            );
        }
        editor.workspace.drawing.object_drawing = false;
        let from = page.min + egui::vec2(100., 500.);
        let to = from + egui::vec2(150., 0.);
        time += 0.2;
        stage_input_frame(&mut editor, &ctx, time, vec![]);
        time += 0.1;
        stage_input_frame(
            &mut editor,
            &ctx,
            time,
            vec![
                egui::Event::PointerMoved(from),
                primary_button(from, true),
                egui::Event::PointerMoved(to),
                primary_button(to, false),
            ],
        );
        let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        assert_eq!(elements.iter().filter(|e| is_merge_fill(e)).count(), 1);
        let stroke = elements.iter().find(|e| is_merge_stroke(e)).unwrap();
        assert!(
            matches!(&stroke.kind,ElementKind::Path { stroke,stroke_width,.. } if *stroke == [62,134,64,191] && *stroke_width == 2.5)
        );
        assert_eq!(editor.history.undo_len(), undo + 1);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, saved);
        editor.set_tool(Tool::Select);
        let mut edit = styled.clone();
        if let ElementKind::Path { stroke, .. } = &mut edit.kind {
            *stroke = [4, 5, 6, 255];
        }
        assert!(editor.remember_brush_outline_edit(&styled, &mut edit));
        assert_eq!(
            editor.stroke,
            [62, 134, 64, 191],
            "ordinary selection edits stay local"
        );
    }
    // The same extraction handles holes, duplicated vertex IDs, and rejects unsafe mesh indices.
    let path = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: 40., y: 0. },
        PathCommand::LineTo { x: 40., y: 40. },
        PathCommand::LineTo { x: 0., y: 40. },
        PathCommand::Close,
        PathCommand::MoveTo { x: 10., y: 10. },
        PathCommand::LineTo { x: 10., y: 30. },
        PathCommand::LineTo { x: 30., y: 30. },
        PathCommand::LineTo { x: 30., y: 10. },
        PathCommand::Close,
    ];
    let outline = crate::authoring_tools::brush_outer_contours(&path, FillRule::NonZero);
    assert!(
        !hit_test_path(
            &outline,
            FillRule::NonZero,
            [20., 20.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        hit_test_path(
            &outline,
            FillRule::NonZero,
            [5., 5.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert_eq!(
        outline
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count(),
        2
    );
    assert!(fill_mesh_boundary(&[[0., 0.]], &[0, 1, 2]).is_none());
    let square = fill_mesh_boundary(
        &[[0., 0.], [1., 0.], [1., 1.], [0., 0.], [1., 1.], [0., 1.]],
        &[0, 1, 2, 3, 4, 5],
    )
    .unwrap();
    assert_eq!(square.len(), 5, "welded triangle seam is removed");
    for shape in [BrushShapeKind::Round, BrushShapeKind::Square] {
        let points: Vec<_> = (0..256)
            .map(|i| egui::pos2(i as f32 * 2., (i as f32 * 0.12).sin() * 25.))
            .collect();
        let pressure: Vec<_> = (0..256).map(|i| 0.2 + 0.8 * (i as f32 / 255.)).collect();
        let ink = crate::authoring_tools::shaped_brush_outline(&points, &pressure, 24., shape);
        let boundary = crate::authoring_tools::brush_outer_contours(&ink, FillRule::NonZero);
        assert!(
            !boundary.is_empty(),
            "dense pressure stroke resolves its perimeter"
        );
        validate_path_commands(&boundary).unwrap();
        assert_eq!(
            boundary
                .iter()
                .filter(|c| matches!(c, PathCommand::Close))
                .count(),
            1
        );
    }
}
#[test]
fn tool_workflows_document_units_shapes_pressure_and_pencil_modes_keep_one_undo() {
    for shape in BrushShapeKind::ALL {
        for pressure in [0.1, 1.] {
            let commands = crate::authoring_tools::shaped_brush_outline(
                &[egui::pos2(50., 50.)],
                &[pressure],
                40.,
                shape,
            );
            validate_path_commands(&commands).unwrap();
            let actual = bounds(&commands);
            let tip = shape.footprint(40. * pressure);
            // Cursor display polygons choose a variable segment count that need not contain
            // the cardinal extrema. Authored size is checked against the analytic footprint.
            let expected = if tip.ellipse {
                egui::Rect::from_center_size(egui::pos2(50., 50.), tip.half * 2.)
            } else {
                tip.bounds(egui::pos2(50., 50.))
            };
            assert!(
                (actual.width() - expected.width()).abs() < 0.2,
                "{shape:?}: {actual:?} {expected:?}"
            );
            assert!(
                (actual.height() - expected.height()).abs() < 0.2,
                "{shape:?}: {actual:?} {expected:?}"
            );
        }
    }
    for zoom in [0.5, 1., 2.] {
        let (mut editor, ctx, _) = rectangle_stage();
        editor.zoom = zoom;
        editor.set_tool(Tool::Brush);
        editor.authoring_tools.brush_size = 40.;
        editor.authoring_tools.brush_shape = BrushShapeKind::Square;
        let output = stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
        let page = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r)
                    if r.fill == egui::Color32::WHITE
                        && (r.rect.width() - 960. * zoom).abs() < 0.1 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .unwrap();
        let before = editor.project.clone();
        let point = page.center();
        stage_input_frame(
            &mut editor,
            &ctx,
            0.2,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
                primary_button(point, false),
            ],
        );
        let (commands, _) = editor
            .selected_element()
            .unwrap()
            .to_path_commands()
            .unwrap();
        assert!((bounds(&commands).width() - 40.).abs() < 0.1, "zoom {zoom}");
        assert_eq!(editor.tool, Tool::Brush);
        assert_eq!(editor.history.undo_len(), 1);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
    let mut results = vec![];
    for mode in [PencilMode::Straighten, PencilMode::Smooth, PencilMode::Ink] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.set_tool(Tool::Pencil);
        editor.workspace.tools.pencil_mode = mode;
        let before = editor.project.clone();
        let points: Vec<_> = (0..30)
            .map(|i| {
                page.min + egui::vec2(100. + i as f32 * 5., 150. + (i as f32 * 0.3).sin() * 2.)
            })
            .collect();
        let mut events = vec![
            egui::Event::PointerMoved(points[0]),
            primary_button(points[0], true),
        ];
        events.extend(points[1..].iter().copied().map(egui::Event::PointerMoved));
        events.push(primary_button(*points.last().unwrap(), false));
        stage_input_frame(&mut editor, &ctx, 0.2, events);
        let (commands, _) = editor
            .selected_element()
            .unwrap()
            .to_path_commands()
            .unwrap();
        results.push(commands);
        assert_eq!(editor.tool, Tool::Pencil);
        assert_eq!(editor.history.undo_len(), 1);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
    assert_eq!(results[0].len(), 2, "Straighten recognises a near-line");
    assert!(
        results[1]
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. })),
        "Smooth produces editable curves"
    );
    assert!(results[2].len() > results[1].len(), "Ink keeps raw samples");
    assert!(
        results[2]
            .iter()
            .all(|c| matches!(c, PathCommand::MoveTo { .. } | PathCommand::LineTo { .. }))
    );

    // An integral current width must not round fractional dropdown presets to zero.
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.set_theme(&ctx, theme);
        editor.set_tool(Tool::Pencil);
        editor.stroke_width = 1.;
        let before = editor.project.clone();
        let frame = |editor: &mut Editor, time, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320., 800.),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| editor.pencil_tool_contents(ui));
                },
            )
        };
        frame(&mut editor, 1., vec![]);
        let output = frame(&mut editor, 1.1, vec![]);
        let point = named_bounds(&output, "Presets").unwrap().center();
        frame(
            &mut editor,
            1.2,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
                primary_button(point, false),
            ],
        );
        let output = frame(&mut editor, 1.3, vec![]);
        let quarter = named_bounds(&output, "0.25").expect("quarter-width preset label");
        assert!(
            named_bounds(&output, "0.50").is_some(),
            "half-width preset label"
        );
        let point = quarter.center();
        frame(
            &mut editor,
            1.4,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
                primary_button(point, false),
            ],
        );
        assert_eq!(editor.stroke_width, 0.25);
        assert_eq!(editor.tool, Tool::Pencil);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}
#[test]
fn customize_tools_drafts_persist_globally_without_document_or_layout_changes() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.set_tool(Tool::Brush);
    let project = editor.project.clone();
    let original = editor.workspace.tools.clone();
    let mut draft = CustomizeToolsDraft::new(&original);
    draft.selected_slot = 6;
    draft.current = Some(1);
    draft.remove();
    assert_eq!(draft.available.as_deref(), Some("Brush"));
    assert_eq!(
        editor.workspace.tools, original,
        "draft cannot mutate saved preferences"
    );
    draft.selected_slot = 0;
    draft.add();
    draft.apply(&mut editor.workspace.tools);
    assert_eq!(
        editor.workspace.tools.slots[0].last().map(String::as_str),
        Some("Brush")
    );
    let prefs = editor.workspace.tools.clone();
    editor
        .workspace
        .store_named_preset("Drawing", false)
        .unwrap();
    editor.workspace.apply_template(WorkspaceTemplate::Animator);
    assert_eq!(editor.workspace.tools, prefs);
    editor.workspace.apply_named_preset("Drawing");
    assert_eq!(editor.workspace.tools, prefs);
    let loaded = WorkspacePrefs::from_json(&serde_json::to_string(&editor.workspace).unwrap());
    assert_eq!(loaded.tools, prefs);
    assert_eq!(editor.project, project);
    assert!(!editor.history.can_undo());
    assert_eq!(editor.tool, Tool::Brush);
    let mut dirty = ToolPreferences {
        slots: vec![
            vec!["Brush".into(), "Deco".into()],
            vec!["Brush".into()],
            vec![],
        ],
        ..Default::default()
    };
    dirty.sanitize();
    assert_eq!(dirty.slots, vec![vec!["Brush"]]);
    ctx.enable_accesskit();
    editor.open_customize_tools(&ctx);
    assert!(editor.customize_tools_open(&ctx));
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        editor.set_theme(&ctx, theme);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920., 1080.),
                )),
                ..Default::default()
            },
            |ctx| editor.customize_tools_dialog(ctx),
        );
        assert_eq!(editor.workspace.tools, prefs);
    }
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| editor.customize_tools_dialog(ctx),
    );
    assert!(!editor.customize_tools_open(&ctx));
    assert_eq!(editor.workspace.tools, prefs);

    // Actual shared row clicks must select without closing the modal; Add/Remove edit only
    // the draft, and OK persists the changed slot without selecting a different tool.
    editor.open_customize_tools(&ctx);
    modal_frame(&mut editor, &ctx, 1., vec![]);
    let mut output = modal_frame(&mut editor, &ctx, 1.1, vec![]);
    for (step, label) in ["Brush tool", "‹ Remove", "Slot 2", "Brush tool", "Add ›"]
        .into_iter()
        .enumerate()
    {
        let point = named_bounds(&output, label)
            .unwrap_or_else(|| panic!("missing {label}"))
            .center();
        modal_frame(
            &mut editor,
            &ctx,
            1.2 + step as f64 * 0.2,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
                primary_button(point, false),
            ],
        );
        output = modal_frame(&mut editor, &ctx, 1.3 + step as f64 * 0.2, vec![]);
        assert!(
            editor.customize_tools_open(&ctx),
            "{label} must keep the draft open"
        );
        assert_eq!(editor.workspace.tools, prefs);
    }
    let point = named_bounds(&output, "OK").unwrap().center();
    modal_frame(
        &mut editor,
        &ctx,
        3.,
        vec![
            egui::Event::PointerMoved(point),
            primary_button(point, true),
            primary_button(point, false),
        ],
    );
    assert!(!editor.customize_tools_open(&ctx));
    assert_eq!(
        editor.workspace.tools.slots[1].last().map(String::as_str),
        Some("Brush")
    );
    assert_eq!(editor.project, project);
    assert!(!editor.history.can_undo());
    assert_eq!(editor.tool, Tool::Brush);
}
fn modal_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920., 1080.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| editor.customize_tools_dialog(ctx),
    )
}
fn named_bounds(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if node.label() != Some(label) {
                return None;
            }
            node.bounds().map(|r| {
                egui::Rect::from_min_max(
                    egui::pos2(r.x0 as f32, r.y0 as f32),
                    egui::pos2(r.x1 as f32, r.y1 as f32),
                )
            })
        })
}
