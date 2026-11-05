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
