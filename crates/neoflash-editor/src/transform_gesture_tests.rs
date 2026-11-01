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
    editor.saved_project = Some(editor.project.clone());
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
/// The `NF2/Transform Box` corner (screen) that drives the placement's local corner `local`: the box
/// is the axis-aligned stage bounds, and each box corner drives the nearest local corner.
fn box_corner(page: egui::Rect, t: Transform, local: [f64; 2]) -> egui::Pos2 {
    let corners = [[0., 0.], [100., 0.], [100., 60.], [0., 60.]].map(|c| screen(page, t, c));
    let frame = egui::Rect::from_points(&corners);
    let target = screen(page, t, local);
    let nearest = |at: egui::Pos2| {
        corners
            .into_iter()
            .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
            .unwrap()
    };
    [
        frame.left_top(),
        frame.right_top(),
        frame.right_bottom(),
        frame.left_bottom(),
    ]
    .into_iter()
    .find(|c| nearest(*c) == target)
    .expect("a box corner drives this local corner")
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
        ..Default::default()
    };
    editor.selected_element_mut().unwrap().transform = original;
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let press = box_corner(page, original, [100., 60.]);
    let desired = Transform {
        scale_x: -3.5,
        scale_y: 2.4,
        ..original
    };
    // Only pointer displacement resizes, so the box handle moves like the local corner would.
    let end = press + (screen(page, desired, [100., 60.]) - screen(page, original, [100., 60.]));
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
        ..Default::default()
    };
    editor.selected_element_mut().unwrap().transform = original;
    clean(&mut editor);
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let center = geometry::transformed_point(original, [50., 30.]);
    let press = box_corner(page, original, [100., 60.]);
    let center_screen = page.min + egui::vec2(center[0] as f32, center[1] as f32);
    let end = press + (screen(page, original, [100., 60.]) - center_screen) * 0.6;
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
fn native_rotate_zone_accumulates_two_turns_as_one_undoable_gesture() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let center = page.min + egui::vec2(200., 180.);
    // Flash layout: the rotate zone is just outside a corner of the transform box.
    let press = page.min + egui::vec2(150., 150.) - egui::vec2(8., 8.);
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
    events.push(egui::Event::PointerMoved(press));
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

#[test]
fn top_edge_drag_skews_about_the_bottom_edge_as_one_undo() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    // Top edge between the corner and middle handles.
    let press = page.min + egui::vec2(170., 150.);
    let end = press + egui::vec2(30., 0.);
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
    let t = editor.selected_element().unwrap().transform;
    let top_left = geometry::transformed_point(t, [0., 0.]);
    let bottom_left = geometry::transformed_point(t, [0., 60.]);
    near(top_left[0], 180.);
    near(top_left[1], 150.);
    near(bottom_left[0], 150.);
    near(bottom_left[1], 210.);
    near(t.rotation, 0.);
    near(t.scale_x, 1.);
    assert!(t.skew > 20. && t.skew < 30., "skew {}", t.skew);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn left_edge_drag_skews_vertically_about_the_right_edge() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let press = page.min + egui::vec2(150., 165.);
    let end = press + egui::vec2(0., 20.);
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
    let t = editor.selected_element().unwrap().transform;
    let top_left = geometry::transformed_point(t, [0., 0.]);
    let top_right = geometry::transformed_point(t, [100., 0.]);
    near(top_left[0], 150.);
    near(top_left[1], 170.);
    near(top_right[0], 250.);
    near(top_right[1], 150.);
    editor.project.validate().unwrap();
}

/// ST-07: the NF2 cursor follows the Free Transform zone or Selection target under the pointer and
/// replaces the system cursor over the canvas.
#[test]
fn stage_cursor_follows_transform_zones_and_selection_targets() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let at = |x: f32, y: f32| page.min + egui::vec2(x, y);
    let mut time = 0.2;
    let mut hover = |editor: &mut Editor, pos: egui::Pos2| {
        time += 0.1;
        let out = stage_input_frame(editor, &ctx, time, vec![egui::Event::PointerMoved(pos)]);
        let shown = editor.shown_stage_cursor.map(|c| c.name);
        if shown.is_some() {
            assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::None);
        }
        shown
    };
    let cases = [
        (at(150., 150.), "resize-d1"),
        (at(250., 210.), "resize-d1"),
        (at(250., 150.), "resize-d2"),
        (at(200., 150.), "resize-v"),
        (at(250., 180.), "resize-h"),
        (at(170., 150.), "skew-h"),
        (at(150., 195.), "skew-v"),
        (at(142., 142.), "rotate"),
        (at(200., 180.), "move"),
        (at(400., 400.), "arrow"),
    ];
    for (pos, name) in cases {
        assert_eq!(
            hover(&mut editor, pos),
            Some(name),
            "Free Transform at {pos:?}"
        );
    }
    editor.set_tool(Tool::Select);
    assert_eq!(hover(&mut editor, at(200., 180.)), Some("arrow-move"));
    assert_eq!(hover(&mut editor, at(400., 400.)), Some("arrow"));
    editor.set_tool(Tool::Brush);
    assert_eq!(hover(&mut editor, at(400., 400.)), Some("brush"));
}

#[test]
fn custom_transformation_point_drags_rotates_alt_resizes_and_resets_with_undo() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(shape(), 150., 150.);
    clean(&mut editor);
    let before = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);

    // Default pivot is box center (200, 180); drag it to the top-left corner (150, 150).
    let default_pivot = page.min + egui::vec2(200., 180.);
    let custom_pivot = page.min + egui::vec2(150., 150.);
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(default_pivot),
            button(default_pivot, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(default_pivot.lerp(custom_pivot, 0.5)),
            egui::Event::PointerMoved(custom_pivot),
            button(custom_pivot, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    let after_pivot = editor.selected_element().unwrap().transform;
    assert_eq!(editor.project.format_version, 7);
    let pivot_local = after_pivot.pivot.expect("custom local pivot persisted");
    near(pivot_local[0], 0.);
    near(pivot_local[1], 0.);
    near(after_pivot.x, 150.);
    near(after_pivot.y, 150.);
    editor.project.validate().unwrap();
    let pivot_project = editor.project.clone();

    // Rotate 90° clockwise around the custom pivot (150, 150): top-left stays fixed at (150, 150).
    let rot_start = page.min + egui::vec2(250., 150.) + egui::vec2(8., -8.);
    let rot_vec = rot_start - custom_pivot;
    let rot_end = custom_pivot + egui::vec2(-rot_vec.y, rot_vec.x);
    frame(
        &mut editor,
        &ctx,
        0.3,
        vec![
            egui::Event::PointerMoved(rot_start),
            button(rot_start, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(rot_end),
            button(rot_end, false, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    let rotated = editor.selected_element().unwrap().transform;
    let world_pivot = geometry::transformed_point(rotated, rotated.pivot.unwrap());
    near(world_pivot[0], 150.);
    near(world_pivot[1], 150.);
    near(rotated.rotation, 90.);
    editor.project.validate().unwrap();

    // Undo rotation back to unrotated custom pivot state, then Alt-resize around custom pivot.
    editor.undo();
    assert_eq!(editor.project, pivot_project);
    stage_input_frame(&mut editor, &ctx, 0.35, vec![]);

    let br_corner = page.min + egui::vec2(250., 210.);
    let br_end = page.min + egui::vec2(300., 240.);
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };
    frame(
        &mut editor,
        &ctx,
        0.4,
        vec![
            egui::Event::PointerMoved(br_corner),
            button(br_corner, true, alt),
            egui::Event::PointerMoved(br_end),
            button(br_end, false, alt),
        ],
        alt,
    );
    let resized = editor.selected_element().unwrap().transform;
    let resized_pivot = geometry::transformed_point(resized, resized.pivot.unwrap());
    near(resized_pivot[0], 150.);
    near(resized_pivot[1], 150.);
    near(resized.scale_x, 1.5);
    near(resized.scale_y, 1.5);
    editor.project.validate().unwrap();

    // Resetting the transformation point restores default None pivot in one Undo.
    editor.reset_transformation_point();
    assert!(editor.selected_element().unwrap().transform.pivot.is_none());
    editor.project.validate().unwrap();
    editor.undo();
    assert!(editor.selected_element().unwrap().transform.pivot.is_some());
    editor.undo();
    assert_eq!(editor.project, pivot_project);
    editor.undo();
    assert_eq!(editor.project, before);
}
