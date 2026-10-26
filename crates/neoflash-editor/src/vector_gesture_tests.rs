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
    let undo = text_center(&menu_frame(&mut editor, &ctx, 0.6, vec![]), "Undo     ⌘Z");
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
