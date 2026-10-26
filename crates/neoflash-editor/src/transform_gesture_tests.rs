use crate::{
    app::{Editor, Tool},
    tests::{rectangle_stage, stage_input_frame},
    transform_geometry as geometry,
};
use eframe::egui;
use neoflash_core::*;

fn shape() -> ElementKind {
    ElementKind::Rectangle {
        width: 100.,
        height: 60.,
        fill: [30, 100, 170, 255],
        stroke: [0, 0, 0, 0],
        stroke_width: 0.,
    }
}
fn clean(editor: &mut Editor) {
    editor.set_tool(Tool::Transform);
    editor.history.clear();
    editor.saved_json = serde_json::to_string(&editor.project).unwrap();
}
fn button(pos: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    }
}
fn frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 900.),
            )),
            time: Some(time),
            events,
            modifiers,
            ..Default::default()
        },
        |ctx| editor.stage(ctx),
    );
}
fn screen(page: egui::Rect, t: Transform, p: [f64; 2]) -> egui::Pos2 {
    let world = geometry::transformed_point(t, p);
    page.min + egui::vec2(world[0] as f32, world[1] as f32)
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

#[test]
fn native_corner_resize_keeps_opposite_anchor_for_rotated_mirrored_object_and_undo() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 400., 250.);
    let original = Transform {
        x: 400.,
        y: 250.,
        rotation: 27.,
        scale_x: -2.,
        scale_y: 1.2,
        alpha: 1.,
    };
    editor.selected_element_mut().unwrap().transform = original;
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let press = screen(page, original, [100., 60.]);
    let desired = Transform {
        scale_x: -3.5,
        scale_y: 2.4,
        ..original
    };
    let end = screen(page, desired, [100., 60.]);
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(press),
            button(press, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(end),
            button(end, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    let result = editor.selected_element().unwrap().transform;
    near(result.x, original.x);
    near(result.y, original.y);
    near(result.rotation, 27.);
    near(result.scale_x, -3.5);
    near(result.scale_y, 2.4);
    editor.project.validate().unwrap();
    let after = editor.project.clone();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.redo();
    assert_eq!(editor.project, after);
}

#[test]
fn native_alt_shift_resize_preserves_center_and_aspect_ratio() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 220., 180.);
    let original = Transform {
        x: 220.,
        y: 180.,
        rotation: 18.,
        scale_x: 1.5,
        scale_y: 0.8,
        alpha: 1.,
    };
    editor.selected_element_mut().unwrap().transform = original;
    clean(&mut editor);
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let center = geometry::transformed_point(original, [50., 30.]);
    let press = screen(page, original, [100., 60.]);
    let center_screen = page.min + egui::vec2(center[0] as f32, center[1] as f32);
    let end = center_screen + (press - center_screen) * 1.6;
    let modifiers = egui::Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    };
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(press),
            button(press, true, modifiers),
            egui::Event::PointerMoved(end),
            button(end, false, modifiers),
        ],
        modifiers,
    );
    let result = editor.selected_element().unwrap().transform;
    let after_center = geometry::transformed_point(result, [50., 30.]);
    near(after_center[0], center[0]);
    near(after_center[1], center[1]);
    near(result.scale_x / original.scale_x, 1.6);
    near(result.scale_y / original.scale_y, 1.6);
}

#[test]
fn native_multi_corner_uniform_scale_preserves_opposite_group_corner_and_relative_transforms() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 100., 100.);
    editor.add_element(shape(), 300., 200.);
    editor.select_all();
    clean(&mut editor);
    let initial = editor.sampled_targets();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let press = page.min + egui::vec2(400., 260.);
    let end = page.min + egui::vec2(550., 340.);
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(press),
            button(press, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(end),
            button(end, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    let expected = geometry::uniform_scale_many(
        &initial.iter().map(|(_, e)| e.transform).collect::<Vec<_>>(),
        [100., 100.],
        [400., 260.],
        [550., 340.],
    )
    .unwrap();
    for ((_, element), expected) in editor.sampled_targets().iter().zip(expected) {
        near(element.transform.x, expected.x);
        near(element.transform.y, expected.y);
        near(element.transform.scale_x, 1.5);
        near(element.transform.scale_y, 1.5);
    }
    assert_eq!(editor.selection.len(), 2);
    editor.project.validate().unwrap();
}

#[test]
fn native_rotation_handle_accumulates_two_turns_as_one_undoable_gesture() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let center = page.min + egui::vec2(200., 180.);
    let press = center - egui::vec2(0., 55.);
    let mut events = vec![
        egui::Event::PointerMoved(press),
        button(press, true, egui::Modifiers::NONE),
    ];
    for offset in [
        egui::vec2(55., 0.),
        egui::vec2(0., 55.),
        egui::vec2(-55., 0.),
        egui::vec2(0., -55.),
    ]
    .into_iter()
    .cycle()
    .take(8)
    {
        events.push(egui::Event::PointerMoved(center + offset));
    }
    events.push(button(press, false, egui::Modifiers::NONE));
    frame(&mut editor, &ctx, 0.2, events, egui::Modifiers::NONE);
    let transform = editor.selected_element().unwrap().transform;
    near(transform.rotation, 720.);
    near(transform.x, 150.);
    near(transform.y, 150.);
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn midpoint_point_handle_click_and_return_to_origin_leave_no_materialized_key() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 50. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 0],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.,
        },
        100.,
        100.,
    );
    let mut key = editor.symbol().timeline.layers[0].keyframes[0].clone();
    key.frame = 10;
    key.elements[0].transform.x = 300.;
    editor.symbol_mut().timeline.layers[0].keyframes[0].tween = Some(Easing::Linear);
    editor.symbol_mut().timeline.layers[0].keyframes.push(key);
    editor.frame = 5;
    clean(&mut editor);
    editor.set_tool(Tool::Subselect);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let press = page.min + egui::vec2(200., 100.);
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(press),
            button(press, true, egui::Modifiers::NONE),
            button(press, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
    frame(
        &mut editor,
        &ctx,
        0.3,
        vec![
            button(press, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(press + egui::vec2(40., 20.)),
            egui::Event::PointerMoved(press),
            button(press, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}
