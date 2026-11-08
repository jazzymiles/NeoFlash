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
fn drawing_tools_remain_active_for_consecutive_gestures_until_explicit_switch() {
    for tool in [
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Line,
        Tool::Pen,
        Tool::Pencil,
        Tool::Brush,
        Tool::PolyStar,
        Tool::Text,
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.set_tool(tool);
        for step in 1..4 {
            stage_input_frame(&mut editor, &ctx, step as f64 * 0.1, vec![]);
        }
        let before = editor.project.clone();
        let mut after_first = None;
        for index in 0..2 {
            let time = 1. + index as f64;
            let start = page.min + egui::vec2(100. + index as f32 * 320., 100.);
            let end = start + egui::vec2(90., 45.);
            if tool == Tool::Pen {
                for (step, point) in [start, end, start + egui::vec2(20., 80.), start]
                    .into_iter()
                    .enumerate()
                {
                    pen_click(&mut editor, &ctx, time + step as f64 * 0.1, point, point);
                }
            } else if tool == Tool::Text {
                stage_input_frame(
                    &mut editor,
                    &ctx,
                    time,
                    vec![
                        egui::Event::PointerMoved(start),
                        primary_button(start, true),
                    ],
                );
                stage_input_frame(
                    &mut editor,
                    &ctx,
                    time + 0.1,
                    vec![primary_button(start, false)],
                );
            } else {
                pen_click(&mut editor, &ctx, time, start, end);
            }
            assert_eq!(editor.tool, tool, "drawing must keep {tool:?} active");
            assert!(editor.shape_draft.is_none());
            assert!(editor.vector.draft.is_none());
            let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
            assert_eq!(
                elements.len(),
                index + 1,
                "each {tool:?} gesture creates content"
            );
            let selected = editor
                .selected_element()
                .expect("new content stays selected");
            assert_eq!(selected.id, elements[index].id);
            assert!(match tool {
                Tool::Rectangle => matches!(selected.kind, ElementKind::Rectangle { .. }),
                Tool::Ellipse => matches!(selected.kind, ElementKind::Ellipse { .. }),
                Tool::Text => matches!(selected.kind, ElementKind::Text { .. }),
                _ => matches!(selected.kind, ElementKind::Path { .. }),
            });
            editor.project.validate().unwrap();
            if index == 0 {
                after_first = Some(editor.project.clone());
            }
        }
        editor.undo();
        assert_eq!(editor.project, after_first.unwrap());
        assert_eq!(editor.tool, tool);
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());

        // Holding Space pans temporarily without replacing the chosen tool.
        let start = page.center();
        let end = start + egui::vec2(30., 20.);
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        stage_input_frame(
            &mut editor,
            &ctx,
            3.,
            vec![
                key(egui::Key::Space, true),
                egui::Event::PointerMoved(start),
                primary_button(start, true),
                egui::Event::PointerMoved(end),
                primary_button(end, false),
            ],
        );
        stage_input_frame(&mut editor, &ctx, 3.1, vec![key(egui::Key::Space, false)]);
        assert_eq!(editor.pan, egui::vec2(30., 20.));
        assert_eq!(editor.tool, tool);
        assert_eq!(editor.project, before);

        // The explicit Selection shortcut still changes the tool normally.
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 900.),
                )),
                time: Some(3.2),
                events: vec![key(egui::Key::V, true)],
                ..Default::default()
            },
            |ctx| {
                editor.shortcuts(ctx);
                editor.stage(ctx);
            },
        );
        assert_eq!(editor.tool, Tool::Select);
        pen_click(&mut editor, &ctx, 3.3, start, end);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn text_click_creates_point_text_and_drag_creates_a_bounded_text_box() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Text);
    for step in 1..4 {
        stage_input_frame(&mut editor, &ctx, step as f64 * 0.01, vec![]);
    }
    let before = editor.project.clone();
    let click = page.min + egui::vec2(120., 90.);

    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(click),
            primary_button(click, true),
        ],
    );
    stage_input_frame(&mut editor, &ctx, 0.2, vec![primary_button(click, false)]);
    let point_text = editor
        .selected_element()
        .expect("point text stays selected");
    assert!(matches!(
        &point_text.kind,
        ElementKind::Text { text, .. } if text == "Your text"
    ));
    assert_eq!(
        (point_text.transform.x, point_text.transform.y),
        (120., 90.)
    );
    assert_eq!(editor.tool, Tool::Text);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);
    let start = page.min + egui::vec2(80., 70.);
    let end = page.min + egui::vec2(260., 150.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
            egui::Event::PointerMoved(end),
            primary_button(end, false),
        ],
    );
    let area_text = editor.selected_element().expect("area text stays selected");
    match &area_text.kind {
        ElementKind::TextBox {
            text,
            width,
            height,
            multiline,
            word_wrap,
            ..
        } => {
            assert_eq!(text, "Your text");
            assert_eq!((*width, *height), (180., 80.));
            assert!(*multiline);
            assert!(*word_wrap);
        }
        other => panic!("expected TextBox, got {other:?}"),
    }
    assert_eq!((area_text.transform.x, area_text.transform.y), (80., 70.));
    assert_eq!(editor.tool, Tool::Text);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn pen_close_press_commits_before_release_and_escape_finishes_the_next_open_path() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Pen);
    let before = editor.project.clone();
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(240., 110.);
    let c = page.min + egui::vec2(170., 230.);
    pen_click(&mut editor, &ctx, 0.1, a, a);
    pen_click(&mut editor, &ctx, 0.2, b, b);
    pen_click(&mut editor, &ctx, 0.3, c, c);

    // A click just outside the visible Close cursor radius must add an anchor, not close via
    // snapping or a larger hidden hit target.
    let outside_close = a + egui::vec2(9., 0.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.35,
        vec![egui::Event::PointerMoved(outside_close)],
    );
    assert_ne!(
        editor.shown_stage_cursor.map(|cursor| cursor.name),
        Some(neoflash_ui::cursors::PEN_CLOSE.name)
    );
    pen_click(&mut editor, &ctx, 0.36, outside_close, outside_close);
    assert_eq!(editor.vector.draft.as_ref().unwrap().anchors.len(), 4);

    stage_input_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![egui::Event::PointerMoved(a), primary_button(a, true)],
    );
    assert!(
        editor.vector.draft.is_none(),
        "a close click commits on press even if native release is lost"
    );
    assert!(matches!(commands(&editor).last(), Some(PathCommand::Close)));
    let after_closed = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.5, vec![primary_button(a, false)]);
    assert_eq!(editor.project, after_closed);

    let d = page.min + egui::vec2(340., 130.);
    let e = page.min + egui::vec2(430., 230.);
    pen_click(&mut editor, &ctx, 0.6, d, d);
    pen_click(&mut editor, &ctx, 0.7, e, e);
    assert_eq!(editor.vector.draft.as_ref().unwrap().anchors.len(), 2);
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 900.),
            )),
            time: Some(0.8),
            events: vec![escape],
            ..Default::default()
        },
        |ctx| editor.shortcuts(ctx),
    );
    assert!(editor.vector.draft.is_none());
    assert_eq!(editor.tool, Tool::Pen);
    assert!(!matches!(
        commands(&editor).last(),
        Some(PathCommand::Close)
    ));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        2
    );
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, after_closed);
    editor.undo();
    assert_eq!(editor.project, before);
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
    assert_eq!(editor.tool, Tool::Eyedropper);
    editor.undo();
    assert_eq!(editor.project, original);
    assert!(!editor.history.can_undo());
    editor.authoring_tools.eraser_mode = crate::authoring_tools::EraserMode::Objects;
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
    assert!(
        !editor.selection.is_empty(),
        "path element remains selected"
    );
    assert_eq!(commands(&editor).len(), 3);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, split_twice_project);
    editor.undo();
    assert_eq!(editor.project, split_once_project);
    editor.undo();
    assert_eq!(editor.project, before);

    // Closed cubic contour with coincident seam anchor: deleting seam anchor keeps valid 3-vertex contour.
    let oval_cmds =
        neoflash_core::oval_path_commands(80., 60., neoflash_core::OvalOptions::default());
    let trimmed = crate::vector::delete_anchors_from_path(&oval_cmds, &[oval_cmds.len() - 2]);
    neoflash_core::validate_path_commands(&trimmed).unwrap();
    assert_eq!(trimmed.len(), 4);
    assert!(matches!(trimmed.first(), Some(PathCommand::MoveTo { .. })));
    assert!(matches!(trimmed.last(), Some(PathCommand::Close)));
}

#[test]
fn pen_preview_and_precise_cursors_follow_drawing_preferences() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.precise_cursors = true;
    for tool in [
        Tool::Pen,
        Tool::Pencil,
        Tool::Brush,
        Tool::Eraser,
        Tool::Bucket,
        Tool::Ink,
        Tool::Eyedropper,
    ] {
        editor.set_tool(tool);
        stage_input_frame(
            &mut editor,
            &ctx,
            0.1,
            vec![egui::Event::PointerMoved(page.center())],
        );
        assert_eq!(
            editor.shown_stage_cursor.map(|c| c.name),
            Some("crosshair"),
            "drawing tool with Precise cursors"
        );
    }
    editor.set_tool(Tool::Select);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![egui::Event::PointerMoved(page.center())],
    );
    assert_eq!(editor.shown_stage_cursor.map(|c| c.name), Some("arrow"));

    // Pen rubber-band preview line from placed anchor to hover position.
    let anchor = page.min + egui::vec2(120., 120.);
    let hover = page.min + egui::vec2(230., 190.);
    let has_preview_line = |out: &egui::FullOutput| {
        out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::LineSegment { points, .. } => {
                points[0].distance(anchor) < 1.0 && points[1].distance(hover) < 1.0
            }
            _ => false,
        })
    };
    editor.set_tool(Tool::Pen);
    editor.workspace.drawing.pen_preview = true;
    pen_click(&mut editor, &ctx, 0.3, anchor, anchor);
    let shown = stage_input_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![egui::Event::PointerMoved(hover)],
    );
    assert!(
        has_preview_line(&shown),
        "pen preview line must paint when enabled"
    );

    editor.workspace.drawing.pen_preview = false;
    let hidden = stage_input_frame(
        &mut editor,
        &ctx,
        0.5,
        vec![egui::Event::PointerMoved(hover)],
    );
    assert!(
        !has_preview_line(&hidden),
        "pen preview line must not paint when disabled"
    );
}

#[test]
fn pencil_recognition_smooth_curves_lines_shapes_and_connect_lines() {
    use crate::workspace::Tolerance;

    let stroke =
        |editor: &mut Editor, ctx: &egui::Context, t: f64, page: egui::Rect, pts: &[egui::Pos2]| {
            let mut events = Vec::with_capacity(pts.len() + 2);
            let first = page.min + pts[0].to_vec2();
            events.push(egui::Event::PointerMoved(first));
            events.push(primary_button(first, true));
            for p in &pts[1..] {
                events.push(egui::Event::PointerMoved(page.min + p.to_vec2()));
            }
            let last = page.min + pts[pts.len() - 1].to_vec2();
            events.push(primary_button(last, false));
            stage_input_frame(editor, ctx, t, events);
        };

    // 1. Smooth curves: Off preserves raw samples; Rough -> Normal -> Smooth simplify progressively.
    let mut wave = Vec::new();
    for i in 0..=24 {
        let x = 80.0 + i as f32 * 8.0;
        let y = 120.0 + (i as f32 * 0.6).sin() * 14.0 + ((i % 2) as f32 - 0.5) * 1.4;
        wave.push(egui::pos2(x, y));
    }
    let mut counts = Vec::new();
    for mode in [
        Tolerance::Off,
        Tolerance::MustBeClose,
        Tolerance::Normal,
        Tolerance::CanBeDistant,
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = Tool::Pencil;
        editor.workspace.drawing.recognise_lines = Tolerance::Off;
        editor.workspace.drawing.recognise_shapes = Tolerance::Off;
        editor.workspace.drawing.connect_lines = Tolerance::Off;
        editor.workspace.drawing.smooth_curves = mode;
        stroke(&mut editor, &ctx, 0.1, page, &wave);
        counts.push(commands(&editor).len());
    }
    assert_eq!(
        counts[0],
        wave.len(),
        "Smooth curves = Off keeps raw samples"
    );
    assert!(
        counts[0] > counts[1]
            && counts[1] >= counts[2]
            && counts[2] >= counts[3]
            && counts[1] > counts[3],
        "expected progressive simplification across Off/Rough/Normal/Smooth, got {counts:?}"
    );

    // 2. Recognise lines: ±2.5 px jitter straightens at Normal (4 px), stays polyline at MustBeClose (1.5 px) and Off.
    let nearly_straight = [
        egui::pos2(100., 150.),
        egui::pos2(140., 152.4),
        egui::pos2(180., 147.8),
        egui::pos2(220., 151.9),
        egui::pos2(260., 150.),
    ];
    for (mode, expected_len) in [
        (Tolerance::Off, 5),
        (Tolerance::MustBeClose, 5),
        (Tolerance::Normal, 2),
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.tool = Tool::Pencil;
        editor.workspace.drawing.smooth_curves = Tolerance::Off;
        editor.workspace.drawing.recognise_shapes = Tolerance::Off;
        editor.workspace.drawing.recognise_lines = mode;
        stroke(&mut editor, &ctx, 0.1, page, &nearly_straight);
        assert_eq!(commands(&editor).len(), expected_len, "{mode:?}");
    }

    // 3. Recognise shapes: closed/nearly-closed rectangle -> Rectangle, circle -> Ellipse,
    // diamond -> stays Path, Off -> stays Path.
    let mut rect_pts = Vec::new();
    for i in 0..8 {
        rect_pts.push(egui::pos2(100. + i as f32 * 15., 100.));
    }
    for i in 0..6 {
        rect_pts.push(egui::pos2(220., 100. + i as f32 * 15.));
    }
    for i in 0..8 {
        rect_pts.push(egui::pos2(220. - i as f32 * 15., 190.));
    }
    for i in 0..6 {
        rect_pts.push(egui::pos2(100., 190. - i as f32 * 15.));
    }
    rect_pts.push(egui::pos2(100., 104.)); // 4 px closure gap

    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    editor.fill = [50, 140, 240, 255];
    let before = editor.project.clone();
    stroke(&mut editor, &ctx, 0.1, page, &rect_pts);
    let rec_el = editor.selected_element().unwrap();
    assert!(
        matches!(rec_el.kind, ElementKind::Rectangle { width, height, fill, .. }
            if (width - 120.).abs() < 1. && (height - 90.).abs() < 1. && fill == [50, 140, 240, 255]),
        "expected Rectangle, got {:?}",
        rec_el.kind
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);

    editor.workspace.drawing.recognise_shapes = Tolerance::Off;
    editor.workspace.drawing.connect_lines = Tolerance::Off;
    stroke(&mut editor, &ctx, 0.2, page, &rect_pts);
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::Path { .. }
    ));

    let circle_pts: Vec<egui::Pos2> = (0..=28)
        .map(|i| {
            let a = std::f32::consts::TAU * (i as f32) / 29.0;
            egui::pos2(300. + 55. * a.cos(), 220. + 45. * a.sin())
        })
        .collect();
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    stroke(&mut editor, &ctx, 0.1, page, &circle_pts);
    assert!(
        matches!(editor.selected_element().unwrap().kind, ElementKind::Ellipse { width, height, .. }
            if (width - 110.).abs() < 2. && (height - 90.).abs() < 2.),
        "expected Ellipse"
    );
    editor.project.validate().unwrap();

    // Diamond must NOT be misclassified as Rectangle or Ellipse.
    let diamond = [
        egui::pos2(200., 100.),
        egui::pos2(260., 160.),
        egui::pos2(200., 220.),
        egui::pos2(140., 160.),
        egui::pos2(200., 100.),
    ];
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    stroke(&mut editor, &ctx, 0.1, page, &diamond);
    assert!(
        matches!(
            editor.selected_element().unwrap().kind,
            ElementKind::Path { .. }
        ),
        "diamond must stay a Path"
    );

    // 4. Connect lines: self-closes triangle within 8 px at Normal (not at Off), and joins open strokes on the same layer.
    let near_closed_triangle = [
        egui::pos2(100., 100.),
        egui::pos2(200., 100.),
        egui::pos2(150., 190.),
        egui::pos2(104., 103.), // 5 px from start
    ];
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    editor.fill = [220, 80, 60, 255];
    editor.workspace.drawing.recognise_shapes = Tolerance::Off;
    editor.workspace.drawing.connect_lines = Tolerance::Off;
    stroke(&mut editor, &ctx, 0.1, page, &near_closed_triangle);
    assert!(
        !commands(&editor)
            .iter()
            .any(|c| matches!(c, PathCommand::Close))
    );

    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    editor.fill = [220, 80, 60, 255];
    editor.workspace.drawing.recognise_shapes = Tolerance::Off;
    editor.workspace.drawing.connect_lines = Tolerance::Normal;
    stroke(&mut editor, &ctx, 0.1, page, &near_closed_triangle);
    assert!(
        commands(&editor)
            .iter()
            .any(|c| matches!(c, PathCommand::Close))
    );
    if let ElementKind::Path { fill, .. } = editor.selected_element().unwrap().kind {
        assert_eq!(fill, [220, 80, 60, 255]);
    }

    // Two open strokes sharing endpoints join into one path, and a third closing stroke closes & fills it.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Pencil;
    editor.fill = [90, 200, 120, 255];
    editor.workspace.drawing.recognise_shapes = Tolerance::Off;
    editor.workspace.drawing.connect_lines = Tolerance::Normal;
    stroke(
        &mut editor,
        &ctx,
        0.1,
        page,
        &[egui::pos2(100., 100.), egui::pos2(200., 100.)],
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1
    );
    stroke(
        &mut editor,
        &ctx,
        0.2,
        page,
        &[egui::pos2(204., 102.), egui::pos2(150., 190.)],
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1,
        "second stroke joins into the first open path"
    );
    let before_third = editor.project.clone();
    stroke(
        &mut editor,
        &ctx,
        0.3,
        page,
        &[egui::pos2(152., 188.), egui::pos2(103., 102.)],
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1
    );
    assert!(
        commands(&editor)
            .iter()
            .any(|c| matches!(c, PathCommand::Close))
    );
    if let ElementKind::Path { fill, .. } = editor.selected_element().unwrap().kind {
        assert_eq!(fill, [90, 200, 120, 255]);
    }
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_third);
}

#[test]
fn paint_tools_share_drawing_gradients_and_sampled_solid_colors() {
    use crate::authoring_modify::PaintFillType;
    use neoflash_core::{GradientKind, History};

    for stroke_well in [false, true] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.workspace.drawing.object_drawing = true;
        editor.add_element(
            ElementKind::Rectangle {
                width: 80.,
                height: 60.,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 3.,
            },
            100.,
            100.,
        );
        editor.set_selection_fill_type(!stroke_well, PaintFillType::RadialGradient);
        editor.history = History::default();
        let before = editor.project.clone();
        editor.set_drawing_fill_type(stroke_well, PaintFillType::LinearGradient);
        editor.set_tool(if stroke_well { Tool::Ink } else { Tool::Bucket });
        let point = page.min + egui::vec2(140., 130.);
        pen_click(&mut editor, &ctx, 0.1, point, point);
        let ElementKind::PaintedPath {
            fill_gradient,
            stroke_gradient,
            ..
        } = &editor.selected_element().unwrap().kind
        else {
            panic!("gradient paint expected")
        };
        let (applied, preserved) = if stroke_well {
            (stroke_gradient, fill_gradient)
        } else {
            (fill_gradient, stroke_gradient)
        };
        assert_eq!(applied.as_ref().unwrap().kind, GradientKind::Linear);
        assert_eq!(preserved.as_ref().unwrap().kind, GradientKind::Radial);
        editor.project.validate().unwrap();
        assert_eq!(editor.history.undo_len(), 1);
        editor.undo();
        assert_eq!(editor.project, before);

        // Sampling the solid well replaces the previous drawing gradient. A new
        // rectangle must use the sampled solid paint, including alpha and width.
        editor.set_tool(Tool::Eyedropper);
        let sample = page.min
            + if stroke_well {
                egui::vec2(100., 130.)
            } else {
                egui::vec2(140., 130.)
            };
        pen_click(&mut editor, &ctx, 0.3, sample, sample);
        assert_eq!(editor.tool, Tool::Eyedropper);
        let gradient = if stroke_well {
            &editor.authoring_modify.drawing_stroke_gradient
        } else {
            &editor.authoring_modify.drawing_fill_gradient
        };
        assert!(gradient.is_none());
        assert_eq!(
            if stroke_well {
                editor.stroke
            } else {
                editor.fill
            },
            if stroke_well {
                [0, 0, 0, 255]
            } else {
                [255, 0, 0, 255]
            }
        );
        editor.set_tool(Tool::Rectangle);
        pen_click(
            &mut editor,
            &ctx,
            0.5,
            page.min + egui::vec2(300., 100.),
            page.min + egui::vec2(400., 180.),
        );
        assert!(matches!(
            editor.selected_element().unwrap().kind,
            ElementKind::Rectangle { .. }
        ));
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
}

#[test]
fn merge_bucket_ink_and_default_eraser_use_planar_geometry_in_one_undo_step() {
    use crate::authoring_tools::{EraserMode, GapCloseMode};
    use neoflash_core::{
        Element, FillRule, FlattenOptions, History, Transform, element_world_commands,
        hit_test_path, is_merge_fill, is_merge_stroke,
    };

    let path = |id: &str, commands: Vec<PathCommand>, fill, stroke, stroke_width| Element {
        id: id.into(),
        name: id.into(),
        merge: true,
        kind: ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill,
            stroke,
            stroke_width,
        },
        transform: Transform::default(),
        ..Element::default()
    };
    let contains = |element: &Element, point: [f64; 2], fill: bool| {
        let (commands, rule) = element_world_commands(element).unwrap();
        let width = match element.kind {
            ElementKind::Path { stroke_width, .. }
            | ElementKind::PaintedPath { stroke_width, .. } => stroke_width,
            _ => 0.0,
        };
        hit_test_path(
            &commands,
            rule,
            point,
            fill,
            width,
            FlattenOptions::default(),
        )
        .unwrap()
    };

    // Four independent strokes, including a small endpoint gap, define one face. Clicking empty
    // interior space must create the fill rather than requiring one pre-existing closed object.
    let (mut editor, ctx, page) = rectangle_stage();
    let outline = [20, 30, 40, 255];
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
        path(
            "top-left",
            vec![
                PathCommand::MoveTo { x: 100., y: 100. },
                PathCommand::LineTo { x: 148., y: 100. },
            ],
            [0; 4],
            outline,
            2.,
        ),
        path(
            "top-right",
            vec![
                PathCommand::MoveTo { x: 152., y: 100. },
                PathCommand::LineTo { x: 220., y: 100. },
                PathCommand::LineTo { x: 220., y: 200. },
            ],
            [0; 4],
            outline,
            2.,
        ),
        path(
            "bottom",
            vec![
                PathCommand::MoveTo { x: 220., y: 200. },
                PathCommand::LineTo { x: 100., y: 200. },
                PathCommand::LineTo { x: 100., y: 100. },
            ],
            [0; 4],
            outline,
            2.,
        ),
    ];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before_bucket = editor.project.clone();
    editor.fill = [35, 190, 105, 255];
    editor.authoring_tools.gap_close = GapCloseMode::Small;
    editor.set_tool(Tool::Bucket);
    let centre = page.min + egui::vec2(160., 150.);
    pen_click(&mut editor, &ctx, 1., centre, centre);
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert!(
        elements
            .iter()
            .filter(|element| is_merge_fill(element))
            .any(|element| contains(element, [160., 150.], true)),
        "Bucket must create the planar face bounded by multiple strokes"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_bucket);

    // Ink Bottle may target the contour of a fill that has no current stroke. It authors a raw
    // Merge stroke and leaves the existing fill intact.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![path(
        "ink-fill",
        vec![
            PathCommand::MoveTo { x: 100., y: 100. },
            PathCommand::LineTo { x: 220., y: 100. },
            PathCommand::LineTo { x: 220., y: 200. },
            PathCommand::LineTo { x: 100., y: 200. },
            PathCommand::Close,
        ],
        [70, 110, 190, 255],
        [0; 4],
        0.,
    )];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before_ink = editor.project.clone();
    editor.stroke = [230, 45, 140, 255];
    editor.stroke_width = 5.;
    editor.set_tool(Tool::Ink);
    let edge = page.min + egui::vec2(100., 150.);
    pen_click(&mut editor, &ctx, 2., edge, edge);
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert!(elements.iter().any(is_merge_fill));
    assert!(
        elements
            .iter()
            .filter(|element| is_merge_stroke(element))
            .any(|element| contains(element, [100., 150.], false)),
        "Ink Bottle must outline a raw fill even when its previous stroke was transparent"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_ink);

    // The default Eraser cuts through raw fills and strokes; full-object removal remains a
    // separate selector mode covered by the existing option test below.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![
        path(
            "erase-fill",
            vec![
                PathCommand::MoveTo { x: 100., y: 100. },
                PathCommand::LineTo { x: 220., y: 100. },
                PathCommand::LineTo { x: 220., y: 200. },
                PathCommand::LineTo { x: 100., y: 200. },
                PathCommand::Close,
            ],
            [70, 110, 190, 255],
            [0; 4],
            0.,
        ),
        path(
            "erase-stroke",
            vec![
                PathCommand::MoveTo { x: 100., y: 250. },
                PathCommand::CubicTo {
                    cx1: 130.,
                    cy1: 225.,
                    cx2: 190.,
                    cy2: 275.,
                    x: 220.,
                    y: 250.,
                },
            ],
            [0; 4],
            outline,
            4.,
        ),
    ];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before_erase = editor.project.clone();
    assert!(editor.authoring_tools.eraser_mode == EraserMode::StrokeSegments);
    editor.authoring_tools.eraser_size = 18.;
    editor.set_tool(Tool::Eraser);
    pen_click(
        &mut editor,
        &ctx,
        3.,
        page.min + egui::vec2(160., 80.),
        page.min + egui::vec2(160., 280.),
    );
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    let fill_at = |point| {
        elements
            .iter()
            .filter(|element| is_merge_fill(element))
            .any(|element| contains(element, point, true))
    };
    let stroke_at = |point| {
        elements
            .iter()
            .filter(|element| is_merge_stroke(element))
            .any(|element| contains(element, point, false))
    };
    assert!(fill_at([120., 150.]) && fill_at([200., 150.]));
    assert!(
        !fill_at([160., 150.]),
        "eraser must cut a channel through the fill"
    );
    assert!(stroke_at([101., 250.]) && stroke_at([219., 250.]));
    assert!(
        !stroke_at([160., 250.]),
        "eraser must cut the curved stroke segment"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_erase);
}

#[test]
fn eraser_sweeps_actual_geometry_respects_holes_masks_and_locked_layers() {
    use crate::authoring_tools::EraserMode;
    use neoflash_core::{Element, FillRule, History, Keyframe, Layer, LayerRole, Transform};
    let rectangle = |width, height| ElementKind::Rectangle {
        width,
        height,
        fill: [240, 20, 40, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    // A small shape wholly inside the eraser circle, away from its axes.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.authoring_tools.eraser_mode = EraserMode::Objects;
    editor.add_element(rectangle(8., 8.), 235., 235.);
    editor.history = History::default();
    let before = editor.project.clone();
    editor.authoring_tools.eraser_size = 128.;
    editor.set_tool(Tool::Eraser);
    let point = page.min + egui::vec2(200., 200.);
    pen_click(&mut editor, &ctx, 0.1, point, point);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);

    // Sparse pointer events must still hit a thin transformed stroke between them.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.authoring_tools.eraser_mode = EraserMode::Objects;
    editor.add_element_transformed(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 80., y: 0. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 40, 200, 255],
            stroke_width: 2.,
        },
        Transform {
            x: 250.,
            y: 160.,
            rotation: 90.,
            scale_y: 2.,
            ..Default::default()
        },
    );
    editor.history = History::default();
    let before = editor.project.clone();
    editor.set_tool(Tool::Eraser);
    pen_click(
        &mut editor,
        &ctx,
        0.1,
        page.min + egui::vec2(200., 200.),
        page.min + egui::vec2(300., 200.),
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    editor.symbol_mut().timeline.layers[0].locked = true;
    let locked = editor.project.clone();
    pen_click(
        &mut editor,
        &ctx,
        0.3,
        page.min + egui::vec2(200., 200.),
        page.min + egui::vec2(300., 200.),
    );
    assert_eq!(editor.project, locked);

    // Empty space in a vector hole is not an eraser target.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.authoring_tools.eraser_mode = EraserMode::Objects;
    let mut commands = Vec::new();
    for (low, high) in [(0., 100.), (25., 75.)] {
        commands.extend([
            PathCommand::MoveTo { x: low, y: low },
            PathCommand::LineTo { x: high, y: low },
            PathCommand::LineTo { x: high, y: high },
            PathCommand::LineTo { x: low, y: high },
            PathCommand::Close,
        ]);
    }
    editor.add_element(
        ElementKind::Path {
            commands,
            fill_rule: FillRule::EvenOdd,
            fill: [240, 20, 40, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        100.,
        100.,
    );
    editor.history = History::default();
    let before = editor.project.clone();
    editor.set_tool(Tool::Eraser);
    let hole = page.min + egui::vec2(150., 150.);
    pen_click(&mut editor, &ctx, 0.1, hole, hole);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    let edge = page.min + egui::vec2(110., 150.);
    pen_click(&mut editor, &ctx, 0.3, edge, edge);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    editor.undo();
    assert_eq!(editor.project, before);

    // Artwork outside a locked mask remains untouched; visible artwork erases.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.authoring_tools.eraser_mode = EraserMode::Objects;
    editor.add_element(rectangle(200., 100.), 100., 100.);
    let mut mask = Layer {
        id: "eraser-mask".into(),
        name: "Mask".into(),
        visible: true,
        locked: true,
        properties: Default::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "eraser-mask-fill".into(),
                kind: rectangle(100., 100.),
                transform: Transform {
                    x: 100.,
                    y: 100.,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    mask.properties.role = LayerRole::Mask;
    editor.symbol_mut().timeline.layers[0].properties.mask = Some(mask.id.clone());
    editor.symbol_mut().timeline.layers.push(mask);
    editor.project.format_version = editor.project.format_version.max(4);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before = editor.project.clone();
    editor.set_tool(Tool::Eraser);
    let outside = page.min + egui::vec2(250., 150.);
    pen_click(&mut editor, &ctx, 0.1, outside, outside);
    assert_eq!(editor.project, before);
    let inside = page.min + egui::vec2(150., 150.);
    pen_click(&mut editor, &ctx, 0.3, inside, inside);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .len(),
        1
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn properties_paint_fields_replace_rgb_and_preserve_gradients_for_alpha() {
    use crate::authoring_modify::PaintFillType;
    use neoflash_core::History;
    fn panel_frame(
        editor: &mut Editor,
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 1200.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.properties(ui, ctx));
                editor.end_property_interaction(ctx);
            },
        )
    }
    for (stroke_well, alpha_only) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut editor, ctx, _) = rectangle_stage();
        ctx.enable_accesskit();
        editor.workspace.drawing.object_drawing = true;
        editor.add_element(
            ElementKind::Rectangle {
                width: 80.,
                height: 60.,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 3.,
            },
            100.,
            100.,
        );
        editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
        editor.set_selection_fill_type(true, PaintFillType::RadialGradient);
        editor.history = History::default();
        let before = editor.project.clone();
        let before_kind = editor.selected_element().unwrap().kind.clone();
        panel_frame(&mut editor, &ctx, 0.1, vec![]);
        let output = panel_frame(&mut editor, &ctx, 0.2, vec![]);
        let name = match (stroke_well, alpha_only) {
            (false, false) => "Fill hex",
            (true, false) => "Stroke hex",
            (false, true) => "Fill alpha",
            (true, true) => "Stroke alpha",
        };
        let bounds = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(name))
                    .then(|| node.bounds())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("missing inspector field {name}"));
        let point = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        panel_frame(
            &mut editor,
            &ctx,
            0.3,
            vec![
                egui::Event::PointerMoved(point),
                primary_button(point, true),
            ],
        );
        panel_frame(&mut editor, &ctx, 0.4, vec![primary_button(point, false)]);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        panel_frame(
            &mut editor,
            &ctx,
            0.5,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text(if alpha_only { "50" } else { "#123456" }.into()),
            ],
        );
        panel_frame(
            &mut editor,
            &ctx,
            0.6,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        panel_frame(&mut editor, &ctx, 0.7, vec![]);
        let ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            ..
        } = &editor.selected_element().unwrap().kind
        else {
            panic!("painted path remains")
        };
        let (color, replaced, preserved) = if stroke_well {
            (stroke, stroke_gradient, fill_gradient)
        } else {
            (fill, fill_gradient, stroke_gradient)
        };
        if alpha_only {
            assert_eq!(color[3], 128);
            let ElementKind::PaintedPath {
                fill_gradient: old_fill,
                stroke_gradient: old_stroke,
                ..
            } = before_kind
            else {
                unreachable!()
            };
            let (mut expected, opposite) = if stroke_well {
                (old_stroke, old_fill)
            } else {
                (old_fill, old_stroke)
            };
            for stop in &mut expected.as_mut().unwrap().stops {
                stop.color[3] = 128;
            }
            assert_eq!(
                *replaced, expected,
                "alpha preserves every gradient stop color and geometry"
            );
            assert_eq!(*preserved, opposite);
        } else {
            assert_eq!(*color, [0x12, 0x34, 0x56, 255]);
            assert!(replaced.is_none());
            assert!(preserved.is_some());
        }
        editor.project.validate().unwrap();
        assert_eq!(editor.history.undo_len(), 1);
        editor.undo();
        assert_eq!(editor.project, before);
    }
}

#[test]
fn merge_brush_ignores_line_in_empty_corner_of_its_bounds() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = false;
    editor.fill = [40, 120, 200, 255];
    editor.stroke = [220, 40, 40, 255];
    editor.stroke_width = 2.;
    editor.authoring_tools.brush_size = 12.;
    editor.set_tool(Tool::Brush);
    let points = [
        egui::vec2(100., 100.),
        egui::vec2(200., 100.),
        egui::vec2(200., 200.),
    ]
    .map(|point| page.min + point);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(points[0]),
            primary_button(points[0], true),
            egui::Event::PointerMoved(points[1]),
            egui::Event::PointerMoved(points[2]),
            primary_button(points[2], false),
        ],
    );
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elements.len(), 1, "one continuous Merge brush fill");
    let brush = elements[0].clone();
    editor.project.validate().unwrap();
    let before_line = editor.project.clone();
    editor.history.clear();
    editor.set_tool(Tool::Line);
    pen_click(
        &mut editor,
        &ctx,
        0.3,
        page.min + egui::vec2(130., 150.),
        page.min + egui::vec2(170., 150.),
    );
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(
        elements.len(),
        2,
        "untouched brush and separate line, without triangle fragments"
    );
    assert_eq!(
        elements.iter().find(|element| element.id == brush.id),
        Some(&brush)
    );
    assert_eq!(
        elements
            .iter()
            .filter(|element| matches!(element.kind,
        ElementKind::Path { fill, .. } | ElementKind::PaintedPath { fill, .. } if fill[3] > 0))
            .count(),
        1
    );
    assert_eq!(elements.iter().filter(|element| matches!(element.kind,
        ElementKind::Path { stroke, stroke_width, .. } | ElementKind::PaintedPath { stroke, stroke_width, .. }
        if stroke[3] > 0 && stroke_width > 0.)).count(), 1);
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before_line);
}

#[test]
fn drawing_pressure_gap_and_partial_stroke_gestures_preserve_content_and_undo() {
    use crate::authoring_tools::{EraserMode, GapCloseMode};
    use neoflash_core::{
        AuthoringGroup, Element, FillRule, FlattenOptions, History, Keyframe, Layer, LayerRole,
        Transform, hit_test_path,
    };
    // Exercise the actual NF2 selector before the gesture: options are transient
    // tool state, while only the resulting artwork belongs in project history.
    fn choose_option(
        editor: &mut Editor,
        ctx: &egui::Context,
        start: f64,
        current: &str,
        choice: &str,
    ) {
        ctx.enable_accesskit();
        let mut time = start;
        let mut frame = |editor: &mut Editor, events| {
            time += 0.001;
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
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(280.);
                        editor.authoring_tool_options(ui);
                    });
                },
            )
        };
        let locate = |output: &egui::FullOutput, label: &str| {
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some(label))
                        .then(|| node.bounds())
                        .flatten()
                        .map(|r| egui::pos2((r.x0 + r.x1) as f32 / 2., (r.y0 + r.y1) as f32 / 2.))
                })
                .unwrap_or_else(|| panic!("tool option {label:?} must be visible"))
        };
        let before = editor.project.clone();
        let undo = editor.history.undo_len();
        let output = frame(editor, vec![]);
        let at = locate(&output, current);
        frame(
            editor,
            vec![egui::Event::PointerMoved(at), primary_button(at, true)],
        );
        frame(editor, vec![primary_button(at, false)]);
        let output = frame(editor, vec![]);
        let at = locate(&output, choice);
        frame(
            editor,
            vec![egui::Event::PointerMoved(at), primary_button(at, true)],
        );
        frame(editor, vec![primary_button(at, false)]);
        frame(editor, vec![]);
        assert_eq!(editor.project, before, "options never mutate artwork");
        assert_eq!(editor.history.undo_len(), undo);
        // Retire the popup hit layer before the following real stage gesture.
        stage_input_frame(editor, ctx, start + 0.01, vec![]);
    }

    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        for (mode, label) in [
            (EraserMode::Objects, "Whole objects"),
            (EraserMode::Fills, "Whole fills"),
            (EraserMode::Strokes, "Whole strokes"),
        ] {
            let (mut editor, ctx, page) = rectangle_stage();
            editor.set_theme(&ctx, theme);
            editor.workspace.drawing.object_drawing = true;
            editor.add_element(
                ElementKind::Rectangle {
                    width: 80.,
                    height: 80.,
                    fill: [80, 120, 180, 255],
                    stroke: [20, 30, 40, 255],
                    stroke_width: 4.,
                },
                100.,
                100.,
            );
            editor.history = History::default();
            let before = editor.project.clone();
            editor.set_tool(Tool::Eraser);
            assert!(
                editor.authoring_tools.eraser_mode == EraserMode::StrokeSegments,
                "cut-through is the safe default; whole-object erasing stays explicit"
            );
            choose_option(&mut editor, &ctx, 0.03, "Cut strokes", label);
            assert!(editor.authoring_tools.eraser_mode == mode);
            let point = page.min + egui::vec2(100., 140.);
            pen_click(&mut editor, &ctx, 0.1, point, point);
            let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
            if mode == EraserMode::Objects {
                assert!(elements.is_empty());
            } else {
                let ElementKind::Rectangle {
                    fill,
                    stroke,
                    stroke_width,
                    ..
                } = elements[0].kind
                else {
                    panic!("whole modes retain the original geometry");
                };
                assert_eq!(fill[3] == 0, mode == EraserMode::Fills);
                assert_eq!(
                    stroke[3] == 0 && stroke_width == 0.,
                    mode == EraserMode::Strokes
                );
            }
            editor.project.validate().unwrap();
            assert_eq!(editor.history.undo_len(), 1);
            editor.undo();
            assert_eq!(editor.project, before);
        }
    }
    let touch = |phase, pos, force| egui::Event::Touch {
        device_id: egui::TouchDeviceId(7),
        id: egui::TouchId(9),
        phase,
        pos,
        force: Some(force),
    };
    let contains = |commands: &[PathCommand], point| {
        hit_test_path(
            commands,
            FillRule::NonZero,
            point,
            true,
            0.,
            FlattenOptions::default(),
        )
        .unwrap()
    };
    // Real stage events: low/high pressure and pen-up zero; draft is pure and the
    // emitted artwork is an ordinary fill, retained by save/open and one Undo.
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.set_theme(&ctx, theme);
        editor.workspace.drawing.object_drawing = true;
        editor.set_tool(Tool::Brush);
        editor.authoring_tools.brush_size = 40.;
        let before = editor.project.clone();
        let a = page.min + egui::vec2(100., 150.);
        let b = page.min + egui::vec2(200., 150.);
        let output = stage_input_frame(
            &mut editor,
            &ctx,
            0.1,
            vec![
                touch(egui::TouchPhase::Start, a, 0.2),
                egui::Event::PointerMoved(a),
                primary_button(a, true),
                touch(egui::TouchPhase::Move, b, 1.),
                egui::Event::PointerMoved(b),
            ],
        );
        assert_eq!(editor.project, before, "pressure ink remains a draft");
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() > 10)
        ));
        stage_input_frame(
            &mut editor,
            &ctx,
            0.2,
            vec![
                touch(egui::TouchPhase::End, b, 0.),
                primary_button(b, false),
            ],
        );
        let ink = commands(&editor);
        assert!(contains(&ink, [100., 153.]));
        assert!(!contains(&ink, [100., 160.]));
        assert!(contains(&ink, [200., 165.]));
        assert!(!contains(&ink, [200., 172.]));
        assert_eq!(editor.tool, Tool::Brush);
        assert_eq!(editor.history.undo_len(), 1);
        editor.project.validate().unwrap();
        let encoded = serde_json::to_string(&editor.project).unwrap();
        let decoded: neoflash_core::Project = serde_json::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
        let reopened = &decoded.symbols[0].timeline.layers[0].keyframes[0].elements[0];
        assert_eq!(reopened.id, editor.selected_element().unwrap().id);
        let (reopened_commands, _) = reopened.to_path_commands().unwrap();
        let original_points = neoflash_core::flatten_path(&ink, FlattenOptions::default()).unwrap();
        let reopened_points =
            neoflash_core::flatten_path(&reopened_commands, FlattenOptions::default()).unwrap();
        assert_eq!(original_points.len(), reopened_points.len());
        for (original, reopened) in original_points.iter().zip(&reopened_points) {
            assert_eq!(original.points.len(), reopened.points.len());
            for (a, b) in original.points.iter().zip(&reopened.points) {
                assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
            }
        }
        if let Ok(path) = std::env::var("NEOFLASH_DRAWING_FIXTURE") {
            std::fs::write(path, encoded).unwrap();
        }
        editor.undo();
        assert_eq!(editor.project, before);
        // Cancellation does not leak pressure into the next mouse stroke.
        stage_input_frame(
            &mut editor,
            &ctx,
            0.3,
            vec![
                touch(egui::TouchPhase::Start, a, 0.1),
                primary_button(a, true),
                touch(egui::TouchPhase::Cancel, b, 0.),
                primary_button(b, false),
            ],
        );
        assert_eq!(editor.project, before);
        pen_click(&mut editor, &ctx, 0.4, a, b);
        assert!(contains(&commands(&editor), [150., 165.]));
        editor.undo();
        assert_eq!(editor.project, before);
    }
    // Approved gap choices drive the real Bucket path. The screen-space
    // threshold honours element scale and closes separate contours.
    for (scale, mode, label, changed) in [
        (1., GapCloseMode::None, "Don’t close gaps", false),
        (1., GapCloseMode::Small, "Close small gaps", true),
        (2., GapCloseMode::Small, "Close small gaps", false),
        (2., GapCloseMode::Medium, "Close medium gaps", true),
        (4., GapCloseMode::Large, "Close large gaps", true),
    ] {
        let (mut editor, ctx, page) = rectangle_stage();
        editor.workspace.drawing.object_drawing = true;
        let open = vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: 80., y: 0. },
            PathCommand::LineTo { x: 80., y: 80. },
            PathCommand::LineTo { x: 0., y: 80. },
            PathCommand::LineTo { x: 0., y: 5. },
        ];
        editor.add_element_transformed(
            ElementKind::Path {
                commands: open,
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [20, 30, 40, 255],
                stroke_width: 2.,
            },
            Transform {
                x: 100.,
                y: 100.,
                scale_x: scale,
                scale_y: scale,
                ..Default::default()
            },
        );
        editor.history = History::default();
        let before = editor.project.clone();
        editor.set_tool(Tool::Bucket);
        choose_option(&mut editor, &ctx, 0.01, "Gap size: Don’t close gaps", label);
        assert_eq!(editor.authoring_tools.gap_close, mode);
        let point = page.min + egui::vec2(120., 120.);
        pen_click(&mut editor, &ctx, 0.1, point, point);
        assert_eq!(editor.project != before, changed, "{scale} {mode:?}");
        editor.project.validate().unwrap();
        if changed {
            assert_eq!(editor.history.undo_len(), 1);
            assert!(matches!(commands(&editor).last(), Some(PathCommand::Close)));
            editor.undo();
            assert_eq!(editor.project, before);
        }
    }
    // Gap closing may not fill a masked-out interior. Partial stroke erasing is
    // explicitly refused even when the cursor touches the visible masked stroke.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 80., y: 0. },
                PathCommand::LineTo { x: 80., y: 80. },
                PathCommand::LineTo { x: 0., y: 80. },
                PathCommand::LineTo { x: 0., y: 5. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 30, 40, 255],
            stroke_width: 4.,
        },
        100.,
        100.,
    );
    let mut mask = Layer {
        id: "drawing-gap-mask".into(),
        name: "Mask".into(),
        visible: true,
        locked: true,
        properties: Default::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "drawing-gap-mask-fill".into(),
                kind: ElementKind::Rectangle {
                    width: 20.,
                    height: 90.,
                    fill: [0, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.,
                },
                transform: Transform {
                    x: 100.,
                    y: 95.,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    mask.properties.role = LayerRole::Mask;
    editor.symbol_mut().timeline.layers[0].properties.mask = Some(mask.id.clone());
    editor.symbol_mut().timeline.layers.push(mask);
    editor.project.upgrade_format_for_features();
    editor.project.format_version = editor.project.format_version.max(4);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before = editor.project.clone();
    editor.set_tool(Tool::Bucket);
    editor.authoring_tools.gap_close = GapCloseMode::Small;
    let outside = page.min + egui::vec2(160., 140.);
    pen_click(&mut editor, &ctx, 0.1, outside, outside);
    assert_eq!(editor.project, before, "gap closing respects the mask");
    assert!(!editor.history.can_undo());
    editor.set_tool(Tool::Eraser);
    assert!(editor.authoring_tools.eraser_mode == EraserMode::StrokeSegments);
    editor.authoring_tools.eraser_size = 10.;
    let visible_stroke = page.min + egui::vec2(110., 100.);
    pen_click(&mut editor, &ctx, 0.3, visible_stroke, visible_stroke);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(
        editor
            .status
            .contains("Partial stroke erasing on masked artwork")
    );
    editor.project.validate().unwrap();

    // Sparse pointer motion cuts the interior of a long stroke, preserving both
    // remaining pieces and the original filled contour/gradient in one undo step.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    let outline = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: 200., y: 0. },
        PathCommand::LineTo { x: 200., y: 80. },
        PathCommand::LineTo { x: 0., y: 80. },
        PathCommand::Close,
    ];
    let gradient = neoflash_core::GradientPaint {
        kind: neoflash_core::GradientKind::Linear,
        stops: vec![
            neoflash_core::GradientStop {
                ratio: 0,
                color: [200, 20, 30, 255],
            },
            neoflash_core::GradientStop {
                ratio: 255,
                color: [20, 30, 200, 255],
            },
        ],
        matrix: [0.1, 0., 0., 0.1, 80., 40.],
        focal_point_ratio: 0.,
    };
    editor.add_element_transformed(
        ElementKind::PaintedPath {
            commands: outline.clone(),
            fill_rule: FillRule::NonZero,
            fill: [80, 120, 180, 255],
            stroke: [0; 4],
            stroke_width: 2.,
            fill_gradient: Some(gradient.clone()),
            stroke_gradient: Some(gradient.clone()),
        },
        Transform {
            x: 100.,
            y: 200.,
            ..Default::default()
        },
    );
    let original_id = editor.selected_element().unwrap().id.clone();
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 10., y: 0. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 30, 40, 255],
            stroke_width: 2.,
        },
        400.,
        200.,
    );
    let remote_id = editor.selected_element().unwrap().id.clone();
    editor.symbol_mut().timeline.layers[0]
        .properties
        .groups
        .push(AuthoringGroup {
            id: "partial-stroke-group".into(),
            name: "Artwork".into(),
            members: vec![original_id.clone(), remote_id.clone()],
        });
    editor.project.upgrade_format_for_features();
    editor.project.format_version = editor.project.format_version.max(5);
    editor.project.validate().unwrap();
    editor.history = History::default();
    let before = editor.project.clone();
    editor.authoring_tools.eraser_mode = EraserMode::StrokeSegments;
    editor.authoring_tools.eraser_size = 20.;
    editor.set_tool(Tool::Eraser);
    pen_click(
        &mut editor,
        &ctx,
        0.1,
        page.min + egui::vec2(140., 170.),
        page.min + egui::vec2(140., 225.),
    );
    editor.project.validate().unwrap();
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0].id, original_id);
    let ElementKind::PaintedPath {
        commands: fill_commands,
        fill_gradient,
        stroke_gradient,
        ..
    } = &elements[0].kind
    else {
        panic!()
    };
    assert_eq!(fill_commands, &outline);
    assert_eq!(fill_gradient, &Some(gradient.clone()));
    assert_eq!(stroke_gradient, &None);
    let ElementKind::PaintedPath {
        commands: cut,
        stroke_gradient,
        ..
    } = &elements[1].kind
    else {
        panic!()
    };
    assert_eq!(stroke_gradient, &Some(gradient));
    let stroked = |x| {
        hit_test_path(
            cut,
            FillRule::NonZero,
            [x, 0.],
            false,
            2.,
            FlattenOptions::default(),
        )
        .unwrap()
    };
    assert!(stroked(10.));
    assert!(!stroked(40.));
    assert!(stroked(80.));
    assert_eq!(elements[2].id, remote_id);
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups[0].members,
        vec![original_id.clone(), elements[1].id.clone(), remote_id]
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);
    // Removing the complete remote stroke prunes the now-single-member group
    // before validation; Undo restores both its member and the group itself.
    let remote_stroke = page.min + egui::vec2(405., 200.);
    pen_click(&mut editor, &ctx, 0.4, remote_stroke, remote_stroke);
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1
    );
    assert!(
        editor.symbol().timeline.layers[0]
            .properties
            .groups
            .is_empty()
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);
    // A combined fill/stroke with object effects must not be split: doing so
    // changes compositing. Refusal keeps the original identity and Undo stack.
    for filtered in [false, true] {
        let element = &mut editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0];
        if filtered {
            element.blend_mode = neoflash_core::BlendMode::Normal;
            element.filters = vec![neoflash_core::FilterSpec::Blur {
                blur_x: 4.,
                blur_y: 4.,
                quality: 1,
            }];
        } else {
            element.blend_mode = neoflash_core::BlendMode::Multiply;
        }
        editor.project.upgrade_format_for_features();
        let protected = editor.project.clone();
        pen_click(
            &mut editor,
            &ctx,
            if filtered { 1.1 } else { 0.7 },
            page.min + egui::vec2(140., 170.),
            page.min + egui::vec2(140., 225.),
        );
        assert_eq!(editor.project, protected);
        assert!(!editor.history.can_undo());
        assert!(editor.status.contains("combined fill/stroke"));
        editor.project.validate().unwrap();
        // A stroke collected before the forbidden target must also remain
        // intact: refusal cancels the entire gesture, not just one target.
        pen_click(
            &mut editor,
            &ctx,
            if filtered { 1.3 } else { 0.9 },
            page.min + egui::vec2(405., 200.),
            page.min + egui::vec2(140., 200.),
        );
        assert_eq!(
            editor.project, protected,
            "refusal is atomic across targets"
        );
        assert!(!editor.history.can_undo());
        assert!(editor.status.contains("combined fill/stroke"));
    }

    // The guided pose differs from the stored/interpolated pose. Erasing uses
    // the displayed matrix (including orientation), while Auto keyframe off
    // keeps the authored span-key transform and guide motion intact.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.workspace.drawing.object_drawing = true;
    editor.workspace.timeline_prefs.auto_keyframe = false;
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 80., y: 0. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 30, 40, 255],
            stroke_width: 2.,
        },
        100.,
        100.,
    );
    let target_id = editor.selected_element().unwrap().id.clone();
    let timeline = &mut editor.symbol_mut().timeline;
    timeline.frame_count = 11;
    let layer = &mut timeline.layers[0];
    layer.properties.motion_guide = Some("eraser-guide".into());
    layer.keyframes[0].tween = Some(neoflash_core::Easing::Linear);
    layer.keyframes[0].orient_to_path = true;
    let mut end = layer.keyframes[0].clone();
    end.frame = 10;
    end.tween = None;
    end.elements[0].transform.x = 300.;
    layer.keyframes.push(end);
    timeline.layers.push(Layer {
        id: "eraser-guide".into(),
        name: "Guide".into(),
        visible: true,
        locked: true,
        properties: neoflash_core::LayerProperties {
            role: LayerRole::Guide,
            ..Default::default()
        },
        keyframes: vec![Keyframe {
            elements: vec![Element {
                id: "eraser-guide-path".into(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 100., y: 100. },
                        PathCommand::LineTo { x: 100., y: 200. },
                        PathCommand::LineTo { x: 300., y: 200. },
                        PathCommand::LineTo { x: 300., y: 100. },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [20, 30, 40, 255],
                    stroke_width: 1.,
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
    });
    editor.frame = 5;
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    editor.history = History::default();
    editor.set_tool(Tool::Eraser);
    editor.authoring_tools.eraser_mode = EraserMode::StrokeSegments;
    editor.authoring_tools.eraser_size = 20.;
    let before = editor.project.clone();
    let shown = neoflash_core::sample_layer_in(
        &editor.symbol().timeline,
        &editor.symbol().timeline.layers[0],
        editor.frame,
    );
    assert_eq!((shown[0].transform.x, shown[0].transform.y), (200., 200.));
    assert_eq!(shown[0].transform.rotation, -90.);
    // Inserting a guided key cannot preserve this pose yet. Refuse instead of
    // silently moving the artwork, then allow the existing-span edit below.
    editor.workspace.timeline_prefs.auto_keyframe = true;
    pen_click(
        &mut editor,
        &ctx,
        0.1,
        page.min + egui::vec2(185., 160.),
        page.min + egui::vec2(215., 160.),
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(
        editor
            .status
            .contains("cannot insert a keyframe inside guided motion")
    );
    editor.project.validate().unwrap();
    editor.workspace.timeline_prefs.auto_keyframe = false;
    pen_click(
        &mut editor,
        &ctx,
        0.3,
        page.min + egui::vec2(185., 160.),
        page.min + egui::vec2(215., 160.),
    );
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes.len(), 2, "Auto keyframe stays off");
    let cut = &layer.keyframes[0].elements[0];
    assert_eq!(cut.id, target_id);
    assert_eq!(
        cut.transform,
        before.symbols[0].timeline.layers[0].keyframes[0].elements[0].transform
    );
    let (cut_commands, _) = cut.to_path_commands().unwrap();
    let remaining = |x| {
        hit_test_path(
            &cut_commands,
            FillRule::NonZero,
            [x, 0.],
            false,
            2.,
            FlattenOptions::default(),
        )
        .unwrap()
    };
    assert!(remaining(10.));
    assert!(!remaining(40.));
    assert!(remaining(70.));
    let after = neoflash_core::sample_layer_in(&editor.symbol().timeline, layer, editor.frame);
    assert_eq!(after[0].transform, shown[0].transform);
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);
}
