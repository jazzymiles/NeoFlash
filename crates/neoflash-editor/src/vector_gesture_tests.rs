//! Pointer-phase regressions use the real stage event loop and history.
use crate::{
    app::{Editor, Tool},
    tests::{pen_click, primary_button, rectangle_stage, stage_input_frame},
};
use eframe::egui;
use neoflash_core::{ElementKind, PathCommand};

fn commands(editor: &Editor) -> Vec<PathCommand> {
    let ElementKind::Path { commands, .. } = &editor.selected_element().unwrap().kind else {
        panic!("expected authored path")
    };
    commands.clone()
}

fn changed_layout_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1480., 980.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| editor.stage(ctx),
    );
}

#[test]
fn edit_menu_undo_cancels_an_open_pen_path_with_empty_history() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Pen);
    let before = editor.project.clone();
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(220., 180.);
    pen_click(&mut editor, &ctx, 0.1, a, a);
    pen_click(&mut editor, &ctx, 0.2, b, b);
    let expected = editor.vector.draft.as_ref().unwrap().commands(false);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.dirty());

    fn menu_frame(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 900.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| editor.menu(ctx),
        )
    }
    fn text_center(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(label) if label.galley.job.text == text => {
                    Some(label.pos + label.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("menu should paint {text:?}"))
    }

    let edit = text_center(&menu_frame(&mut editor, &ctx, 0.3, vec![]), "Edit");
    menu_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![egui::Event::PointerMoved(edit), primary_button(edit, true)],
    );
    menu_frame(&mut editor, &ctx, 0.5, vec![primary_button(edit, false)]);
    let undo = text_center(&menu_frame(&mut editor, &ctx, 0.6, vec![]), "Undo");
    menu_frame(
        &mut editor,
        &ctx,
        0.7,
        vec![egui::Event::PointerMoved(undo), primary_button(undo, true)],
    );
    menu_frame(&mut editor, &ctx, 0.8, vec![primary_button(undo, false)]);

    assert!(
        editor.vector.draft.is_none(),
        "clicking Edit → Undo must cancel the pending path"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
    assert!(
        editor.history.can_redo(),
        "the menu action must use the same undo transaction as Cmd+Z"
    );
    editor.redo();
    let authored = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(authored.len(), 1);
    let ElementKind::Path { commands, .. } = &authored[0].kind else {
        panic!("redo must restore the authored path");
    };
    assert_eq!(*commands, expected);
    editor.project.validate().unwrap();
}

#[test]
fn pen_batched_inside_press_outside_release_keeps_anchor_and_final_control() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pen;
    let before = editor.project.clone();
    let start = page.min + egui::vec2(100., 120.);
    let release = egui::pos2(-20., -20.);
    // No separate move at release: PointerButton.pos is authoritative.
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
            primary_button(release, false),
        ],
    );
    let draft = editor
        .vector
        .draft
        .as_ref()
        .expect("press began on the stage");
    assert_eq!(draft.anchors.len(), 1);
    assert_eq!(draft.anchors[0].point, egui::pos2(100., 120.));
    assert_eq!(draft.anchors[0].tangent, release - start);
    assert!(!draft.dragging);
    assert_eq!(editor.project, before);
    let second = page.min + egui::vec2(250., 230.);
    pen_click(&mut editor, &ctx, 0.2, second, second);
    editor.finish_path(false);
    assert!(matches!(commands(&editor)[1], PathCommand::CubicTo { .. }));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn pencil_batched_and_separate_outside_release_produce_the_same_path() {
    let mut results = Vec::new();
    for batched in [true, false] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = Tool::Pencil;
        let before = editor.project.clone();
        let start = page.min + egui::vec2(100., 100.);
        let middle = start + egui::vec2(50., 30.);
        let release = egui::pos2(-20., -20.);
        if batched {
            stage_input_frame(
                &mut editor,
                &ctx,
                0.1,
                vec![
                    egui::Event::PointerMoved(start),
                    primary_button(start, true),
                    egui::Event::PointerMoved(middle),
                    egui::Event::PointerMoved(release),
                    primary_button(release, false),
                ],
            );
        } else {
            stage_input_frame(
                &mut editor,
                &ctx,
                0.1,
                vec![
                    egui::Event::PointerMoved(start),
                    primary_button(start, true),
                ],
            );
            stage_input_frame(
                &mut editor,
                &ctx,
                0.2,
                vec![egui::Event::PointerMoved(middle)],
            );
            stage_input_frame(
                &mut editor,
                &ctx,
                0.3,
                vec![
                    egui::Event::PointerMoved(release),
                    primary_button(release, false),
                ],
            );
        }
        assert!(editor.vector.draft.is_none());
        let path = commands(&editor);
        let end = release - page.min;
        assert_eq!(
            path.last(),
            Some(&PathCommand::LineTo {
                x: end.x as f64,
                y: end.y as f64
            })
        );
        results.push(path);
        editor.project.validate().unwrap();
        let after = editor.project.clone();
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
        editor.redo();
        assert_eq!(editor.project, after);
    }
    assert_eq!(results[0], results[1]);

    // The added drawing tools share the same release/undo contract. Color tools
    // preserve alpha and navigation never edits authored content.
    for tool in [Tool::Line, Tool::Brush] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.set_tool(tool);
        editor.fill = [60, 80, 100, 128];
        let before = editor.project.clone();
        let start = page.min + egui::vec2(100., 100.);
        let end = start + egui::vec2(90., 35.);
        stage_input_frame(
            &mut editor,
            &ctx,
            0.1,
            vec![
                egui::Event::PointerMoved(start),
                primary_button(start, true),
                egui::Event::PointerMoved(end),
                primary_button(end, false),
            ],
        );
        let ElementKind::Path {
            fill,
            stroke,
            commands,
            ..
        } = &editor.selected_element().unwrap().kind
        else {
            panic!("expected authored vector path");
        };
        if tool == Tool::Brush {
            assert_eq!(*fill, editor.fill);
            assert!(
                commands
                    .iter()
                    .any(|command| matches!(command, PathCommand::Close))
            );
        } else {
            assert_eq!(*stroke, editor.stroke);
            assert_eq!(fill[3], 0);
        }
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.,
            height: 60.,
            fill: [10, 20, 30, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.,
        },
        100.,
        100.,
    );
    editor.history = neoflash_core::History::default();
    let original = editor.project.clone();
    let point = page.min + egui::vec2(140., 130.);
    editor.fill = [90, 50, 110, 77];
    editor.set_tool(Tool::Bucket);
    pen_click(&mut editor, &ctx, 0.2, point, point);
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::Rectangle {
            fill: [90, 50, 110, 77],
            ..
        }
    ));
    editor.set_tool(Tool::Eyedropper);
    editor.fill = [0; 4];
    pen_click(&mut editor, &ctx, 0.3, point, point);
    assert_eq!(editor.fill, [90, 50, 110, 77]);
    assert!(editor.tool == Tool::Bucket);
    editor.undo();
    assert_eq!(editor.project, original);
    assert!(!editor.history.can_undo());
    editor.set_tool(Tool::Eraser);
    pen_click(&mut editor, &ctx, 0.4, point, point);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    editor.undo();
    assert_eq!(editor.project, original);
    editor.set_tool(Tool::Hand);
    pen_click(&mut editor, &ctx, 0.5, point, point + egui::vec2(30., 20.));
    assert_eq!(editor.pan, egui::vec2(30., 20.));
    assert_eq!(editor.project, original);
    assert!(!editor.history.can_undo());

    let (mut editor, ctx, page) = rectangle_stage();
    editor.project.format_version = 5;
    editor.add_element(
        ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
                PathCommand::LineTo { x: 100., y: 80. },
                PathCommand::LineTo { x: 0., y: 80. },
                PathCommand::Close,
            ],
            fill_rule: Default::default(),
            fill: [0; 4],
            stroke: [0; 4],
            stroke_width: 0.,
            stroke_gradient: None,
            fill_gradient: Some(neoflash_core::GradientPaint {
                kind: neoflash_core::GradientKind::Linear,
                matrix: [0.06, 0., 0., 0.04, 50., 40.],
                stops: vec![
                    neoflash_core::GradientStop {
                        ratio: 0,
                        color: [255, 0, 0, 128],
                    },
                    neoflash_core::GradientStop {
                        ratio: 255,
                        color: [0, 0, 255, 255],
                    },
                ],
                focal_point_ratio: 0.,
            }),
        },
        100.,
        100.,
    );
    editor.history = neoflash_core::History::default();
    let original = editor.project.clone();
    editor.set_tool(Tool::Gradient);
    let center = page.min + egui::vec2(150., 140.);
    pen_click(&mut editor, &ctx, 0.1, center, center + egui::vec2(20., 0.));
    let ElementKind::PaintedPath {
        fill_gradient: Some(fill),
        ..
    } = &editor.selected_element().unwrap().kind
    else {
        panic!("gradient retained");
    };
    assert_eq!(fill.matrix[4], 70.);
    editor.undo();
    assert_eq!(editor.project, original);
    assert!(!editor.history.can_undo());
}

#[test]
fn pen_view_change_ends_drag_preserving_all_previous_anchors_and_tangents() {
    for change in ["pan", "zoom", "layout"] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = Tool::Pen;
        let before = editor.project.clone();
        let a = page.min + egui::vec2(80., 90.);
        let b = page.min + egui::vec2(220., 180.);
        pen_click(&mut editor, &ctx, 0.1, a, a + egui::vec2(20., 10.));
        stage_input_frame(
            &mut editor,
            &ctx,
            0.2,
            vec![
                egui::Event::PointerMoved(b),
                primary_button(b, true),
                egui::Event::PointerMoved(b + egui::vec2(35., 25.)),
            ],
        );
        let saved = editor.vector.draft.as_ref().unwrap().commands(false);
        assert!(editor.vector.draft.as_ref().unwrap().dragging);
        let events = vec![
            egui::Event::PointerMoved(b + egui::vec2(110., 90.)),
            primary_button(b + egui::vec2(120., 100.), false),
        ];
        match change {
            "pan" => {
                editor.pan += egui::vec2(75., 35.);
                stage_input_frame(&mut editor, &ctx, 0.3, events);
            }
            "zoom" => {
                editor.zoom = 1.8;
                stage_input_frame(&mut editor, &ctx, 0.3, events);
            }
            _ => changed_layout_frame(&mut editor, &ctx, 0.3, events),
        }
        let draft = editor
            .vector
            .draft
            .as_ref()
            .expect("Pen draft must survive a view change");
        assert_eq!(draft.commands(false), saved, "{change}");
        assert!(!draft.dragging);
        assert!(!draft.closing);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn pencil_view_change_finishes_old_samples_using_frozen_simplification_zoom() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    let before = editor.project.clone();
    let a = page.min + egui::vec2(100., 100.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(a),
            primary_button(a, true),
            egui::Event::PointerMoved(a + egui::vec2(10., 0.7)),
            egui::Event::PointerMoved(a + egui::vec2(20., 0.)),
        ],
    );
    assert_eq!(editor.vector.draft.as_ref().unwrap().anchors.len(), 3);
    editor.zoom = 4.;
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(a + egui::vec2(60., 60.)),
            primary_button(a + egui::vec2(80., 80.), false),
        ],
    );
    assert!(editor.vector.draft.is_none());
    assert_eq!(
        commands(&editor),
        vec![
            PathCommand::MoveTo { x: 100., y: 100. },
            PathCommand::LineTo { x: 120., y: 100. }
        ]
    );
    let after = editor.project.clone();
    stage_input_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![egui::Event::PointerMoved(a + egui::vec2(100., 100.))],
    );
    assert_eq!(editor.project, after);
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn explicit_end_phase_is_idempotent_and_pen_requires_a_new_press() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pen;
    let a = page.min + egui::vec2(100., 100.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(a),
            primary_button(a, true),
            egui::Event::PointerMoved(a + egui::vec2(30., 40.)),
        ],
    );
    editor.end_vector_pointer_phase();
    editor.end_vector_pointer_phase();
    let saved = editor.vector.draft.as_ref().unwrap().anchors[0];
    editor.pan += egui::vec2(30., 20.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(a + egui::vec2(150., 140.)),
            primary_button(a + egui::vec2(150., 140.), false),
        ],
    );
    let draft = editor.vector.draft.as_ref().unwrap();
    assert_eq!(draft.anchors[0].point, saved.point);
    assert_eq!(draft.anchors[0].tangent, saved.tangent);
    let next = a + editor.pan + egui::vec2(100., 100.);
    pen_click(&mut editor, &ctx, 0.3, next, next + egui::vec2(10., 5.));
    let draft = editor.vector.draft.as_ref().unwrap();
    assert_eq!(draft.anchors.len(), 2);
    assert_eq!(draft.anchors[0].tangent, saved.tangent);
    assert_eq!(draft.anchors[1].point, egui::pos2(200., 200.));
    assert_eq!(draft.anchors[1].tangent, egui::vec2(10., 5.));
}

#[test]
fn explicit_end_pencil_commits_once_and_unmoved_press_has_no_history() {
    for moved in [false, true] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = Tool::Pencil;
        let before = editor.project.clone();
        let a = page.min + egui::vec2(100., 100.);
        let mut events = vec![egui::Event::PointerMoved(a), primary_button(a, true)];
        if moved {
            events.push(egui::Event::PointerMoved(a + egui::vec2(40., 25.)));
        }
        stage_input_frame(&mut editor, &ctx, 0.1, events);
        editor.end_vector_pointer_phase();
        let after = editor.project.clone();
        editor.end_vector_pointer_phase();
        editor.zoom = 2.;
        stage_input_frame(
            &mut editor,
            &ctx,
            0.2,
            vec![primary_button(a + egui::vec2(100., 100.), false)],
        );
        assert!(editor.vector.draft.is_none());
        assert_eq!(editor.project, after);
        assert_eq!(editor.history.can_undo(), moved);
        if moved {
            editor.undo();
        }
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn foreground_overlay_prevents_pen_or_pencil_press() {
    for tool in [Tool::Pen, Tool::Pencil] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = tool;
        let a = page.min + egui::vec2(100., 100.);
        for (time, events) in [
            (0.1, vec![]),
            (
                0.2,
                vec![
                    egui::Event::PointerMoved(a),
                    primary_button(a, true),
                    primary_button(a, false),
                ],
            ),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 900.),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::Area::new(egui::Id::new("vector-overlay"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(a - egui::vec2(20., 20.))
                        .show(ctx, |ui| {
                            ui.allocate_exact_size(
                                egui::vec2(100., 100.),
                                egui::Sense::click_and_drag(),
                            );
                        });
                    editor.stage(ctx);
                },
            );
        }
        assert!(editor.vector.draft.is_none());
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn polystar_stage_drag_and_modal_settings_author_valid_closed_paths() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.open_polystar_settings();
    assert!(editor.authoring_tools.polystar_dialog.is_some());
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800., 600.),
            )),
            time: Some(0.05),
            ..Default::default()
        },
        |ctx| editor.polystar_settings_modal(ctx),
    );
    editor.authoring_tools.polystar_dialog = None;
    stage_input_frame(&mut editor, &ctx, 0.08, vec![]);
    editor.authoring_tools.polystar_star = true;
    editor.authoring_tools.polystar_sides = 6;
    editor.authoring_tools.polystar_point_size = 0.45;
    editor.set_tool(Tool::PolyStar);
    editor.history.clear();
    let before = editor.project.clone();

    let center = page.min + egui::vec2(200., 180.);
    let outer = center + egui::vec2(50., 0.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(center),
            primary_button(center, true),
            egui::Event::PointerMoved(outer),
            primary_button(outer, false),
        ],
    );
    let cmds = commands(&editor);
    // 6-point star = 12 vertices: MoveTo + 11 LineTo + Close = 13 commands.
    assert_eq!(cmds.len(), 13);
    assert!(matches!(cmds.last(), Some(PathCommand::Close)));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn pen_and_subselect_insert_and_delete_anchor_points_preserving_curve_and_undo() {
    let (mut editor, ctx, page) = rectangle_stage();
    let cubic = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::CubicTo {
            cx1: 0.,
            cy1: 100.,
            cx2: 100.,
            cy2: 100.,
            x: 100.,
            y: 0.,
        },
    ];
    let original_bounds = neoflash_core::path_bounds(&cubic).unwrap().unwrap();
    editor.add_element(
        ElementKind::Path {
            commands: cubic,
            fill_rule: neoflash_core::FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.,
        },
        100.,
        100.,
    );
    editor.history.clear();
    let before = editor.project.clone();

    // At t = 0.5, the cubic midpoint in local coordinates is (50, 75) -> stage (150, 175).
    let mid_screen = page.min + egui::vec2(150., 175.);
    editor.set_tool(Tool::Pen);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![egui::Event::PointerMoved(mid_screen)],
    );
    assert_eq!(
        editor.shown_stage_cursor.map(|c| c.name),
        Some("pen-add"),
        "Pen hovering an existing path segment shows PEN_ADD"
    );
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(mid_screen),
            primary_button(mid_screen, true),
            primary_button(mid_screen, false),
        ],
    );
    assert!(
        editor.vector.draft.is_none(),
        "subdividing must not start a new draft"
    );
    let after_pen_split = commands(&editor);
    assert_eq!(after_pen_split.len(), 3);
    let split_bounds = neoflash_core::path_bounds(&after_pen_split)
        .unwrap()
        .unwrap();
    assert!((split_bounds.y_max - original_bounds.y_max).abs() < 1e-4);
    editor.project.validate().unwrap();
    let split_once_project = editor.project.clone();

    // Switch to Subselect, Alt-click the right half at u=0.75 (t=0.5 of segment 2) -> local (84.375, 56.25).
    editor.set_tool(Tool::Subselect);
    let right_screen = page.min + egui::vec2(184.375, 156.25);
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 900.),
            )),
            time: Some(0.3),
            modifiers: alt,
            events: vec![
                egui::Event::PointerMoved(right_screen),
                egui::Event::PointerButton {
                    pos: right_screen,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: alt,
                },
                egui::Event::PointerButton {
                    pos: right_screen,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: alt,
                },
            ],
            ..Default::default()
        },
        |ctx| editor.stage(ctx),
    );
    assert_eq!(commands(&editor).len(), 4);
    editor.project.validate().unwrap();
    let split_twice_project = editor.project.clone();

    // Newly inserted anchor is selected; delete_selected() removes ONLY that anchor, keeping the path!
    editor.delete_selected();
    assert!(!editor.selection.is_empty(), "path element remains selected");
    assert_eq!(commands(&editor).len(), 3);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, split_twice_project);
    editor.undo();
    assert_eq!(editor.project, split_once_project);
    editor.undo();
    assert_eq!(editor.project, before);

    // Closed cubic contour with coincident seam anchor: deleting seam anchor keeps valid 3-vertex contour.
    let oval_cmds = neoflash_core::oval_path_commands(80., 60., neoflash_core::OvalOptions::default());
    let trimmed = crate::vector::delete_anchors_from_path(&oval_cmds, &[oval_cmds.len() - 2]);
    neoflash_core::validate_path_commands(&trimmed).unwrap();
    assert_eq!(trimmed.len(), 4);
    assert!(matches!(trimmed.first(), Some(PathCommand::MoveTo { .. })));
    assert!(matches!(trimmed.last(), Some(PathCommand::Close)));
}
