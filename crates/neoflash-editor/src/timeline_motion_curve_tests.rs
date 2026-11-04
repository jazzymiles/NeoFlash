//! Spatial curve gestures, stable tangents, document validation and a single Undo boundary.
use crate::{
    app::{Editor, Tool},
    selection::SelectionTarget,
    tests::{primary_button, rectangle_stage, stage_input_frame},
    timeline_motion_curve::CurveTarget,
};
use eframe::egui::{self, Event};
use neoflash_core::*;

fn fixture() -> (Editor, egui::Context, egui::Rect) {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Select;
    let timeline = &mut editor.symbol_mut().timeline;
    timeline.frame_count = 30;
    timeline.layers[0].keyframes = (0..3)
        .map(|i| Keyframe {
            frame: i * 10,
            tween: (i == 0).then_some(Easing::Linear),
            tween_kind: if i == 0 {
                TweenKind::Motion
            } else {
                TweenKind::Classic
            },
            keyed_properties: (i > 0).then_some(vec![TweenProperty::X, TweenProperty::Y]),
            elements: vec![Element {
                id: "moving".into(),
                transform: Transform {
                    x: 50. + i as f64 * 100.,
                    y: 100.,
                    ..Default::default()
                },
                kind: ElementKind::Rectangle {
                    width: 20.,
                    height: 20.,
                    fill: [30, 60, 190, 255],
                    stroke: [0; 4],
                    stroke_width: 0.,
                },
                ..Default::default()
            }],
            ..Default::default()
        })
        .collect();
    editor.project.upgrade_format_for_features();
    let layer = editor.symbol().timeline.layers[0].id.clone();
    editor.set_selection(vec![SelectionTarget {
        layer,
        element: "moving".into(),
    }]);
    editor.project.validate().unwrap();
    (editor, ctx, page)
}
fn curve(editor: &Editor) -> Vec<PathCommand> {
    editor.symbol().timeline.layers[0].keyframes[0]
        .motion_path
        .clone()
        .unwrap()
}
fn pose(editor: &Editor, frame: u32) -> Transform {
    sample_layer(&editor.symbol().timeline.layers[0], frame)[0].transform
}
fn handle(path: &[PathCommand], index: usize, out: bool) -> [f64; 2] {
    match path[index] {
        PathCommand::CubicTo {
            cx1, cy1, cx2, cy2, ..
        } => {
            if out {
                [cx1, cy1]
            } else {
                [cx2, cy2]
            }
        }
        _ => panic!("expected cubic"),
    }
}
fn shortcut(editor: &mut Editor, ctx: &egui::Context, key: egui::Key) {
    let _ = ctx.run(
        egui::RawInput {
            events: vec![Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| editor.shortcuts(ctx),
    );
}

#[test]
fn motion_curve_bend_handle_smooth_corner_cancel_and_undo() {
    let (mut editor, ctx, page) = fixture();
    let before = editor.project.clone();
    let from = page.min + egui::vec2(100., 100.);
    let to = from + egui::vec2(0., 60.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![Event::PointerMoved(from), primary_button(from, true)],
    );
    assert!(editor.timeline_motion.curve_drag.is_some());
    assert!(
        editor.object_gesture.is_none(),
        "curve consumed the stage press"
    );
    stage_input_frame(&mut editor, &ctx, 0.2, vec![Event::PointerMoved(to)]);
    assert!(pose(&editor, 5).y > 150., "live preview follows bent curve");
    stage_input_frame(&mut editor, &ctx, 0.3, vec![primary_button(to, false)]);
    let output = stage_input_frame(&mut editor, &ctx, 0.4, vec![]);
    assert_eq!(editor.project.format_version, 8);
    editor.project.validate().unwrap();
    assert!(
        output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Circle(c) if c.radius>2.5&&c.radius<4.)),
        "approved handle painter is present"
    );
    let bent = editor.project.clone();
    editor.undo();
    assert_eq!(editor.project, before);
    editor.redo();
    assert_eq!(editor.project, bent);

    let original = curve(&editor);
    let incoming = handle(&original, 1, false);
    assert!(editor.begin_motion_curve_drag(CurveTarget::Handle(1, false), incoming));
    editor.drag_motion_curve_to([incoming[0], incoming[1] + 20.], false);
    editor.finish_motion_curve_drag();
    let smooth = curve(&editor);
    let a = handle(&smooth, 1, false);
    let b = handle(&smooth, 2, true);
    assert!(
        (a[0] + b[0] - 300.).abs() < 1e-6 && (a[1] + b[1] - 200.).abs() < 1e-6,
        "mirrored about middle key"
    );
    let smooth_project = editor.project.clone();
    assert!(editor.begin_motion_curve_drag(CurveTarget::Handle(1, false), a));
    editor.drag_motion_curve_to([a[0] + 10., a[1] + 15.], true);
    editor.finish_motion_curve_drag();
    assert_eq!(
        handle(&curve(&editor), 2, true),
        b,
        "Alt breaks the opposite handle"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, smooth_project);
    assert!(editor.begin_motion_curve_drag(CurveTarget::Segment(2), [200., 100.]));
    editor.drag_motion_curve_to([200., 180.], false);
    shortcut(&mut editor, &ctx, egui::Key::Escape);
    assert!(editor.timeline_motion.curve_drag.is_none());
    assert_eq!(editor.project, smooth_project);

    // Alt-click the middle diamond toggles a corner and back without starting an object drag.
    let at = page.min + egui::vec2(150., 100.);
    for time in [0.6, 0.9] {
        stage_input_frame(
            &mut editor,
            &ctx,
            time,
            vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::ALT,
                },
            ],
        );
        stage_input_frame(
            &mut editor,
            &ctx,
            time + 0.05,
            vec![primary_button(at, false)],
        );
        stage_input_frame(&mut editor, &ctx, time + 0.1, vec![]);
        assert!(editor.object_gesture.is_none());
        editor.project.validate().unwrap();
    }
    assert_ne!(handle(&curve(&editor), 1, false), [150., 100.]);
}

#[test]
fn motion_curve_anchors_follow_key_edits_without_moving_other_handles() {
    let (mut editor, _, _) = fixture();
    editor.begin_motion_curve_drag(CurveTarget::Segment(1), [100., 100.]);
    editor.drag_motion_curve_to([100., 160.], false);
    editor.finish_motion_curve_drag();
    let before = editor.project.clone();
    let path = curve(&editor);
    assert!(editor.begin_motion_key_drag(10, [150., 100.]));
    editor.drag_motion_key_to([180., 120.]);
    assert_eq!(
        [pose(&editor, 10).x, pose(&editor, 10).y],
        [180., 120.],
        "anchor follows during held drag"
    );
    editor.finish_motion_key_drag();
    let moved = curve(&editor);
    assert_eq!(
        handle(&moved, 1, true),
        handle(&path, 1, true),
        "distant anchor's handle stays put"
    );
    let old = handle(&path, 1, false);
    assert_eq!(handle(&moved, 1, false), [old[0] + 30., old[1] + 20.]);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    editor.transact_named("Move position key", |editor| {
        let p = &mut editor.symbol_mut().timeline.layers[0].keyframes[2].elements[0].transform;
        p.x += 25.;
        p.y += 40.;
    });
    assert_eq!(
        [pose(&editor, 20).x, pose(&editor, 20).y],
        [275., 140.],
        "ordinary property edits rebase at commit"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    // Turning a non-position property key into a path anchor preserves the original cubic.
    let sampled = sample_layer(&editor.symbol().timeline.layers[0], 5);
    editor.transact_named("Add alpha key", |editor| {
        let layer = &mut editor.symbol_mut().timeline.layers[0];
        layer.keyframes.push(Keyframe {
            frame: 5,
            keyed_properties: Some(vec![TweenProperty::Alpha]),
            elements: sampled,
            ..Default::default()
        });
        layer.keyframes.sort_by_key(|key| key.frame);
    });
    let with_alpha = editor.project.clone();
    let at = pose(&editor, 5);
    assert!(editor.begin_motion_key_drag(5, [at.x, at.y]));
    editor.drag_motion_key_to([at.x + 20., at.y - 10.]);
    editor.finish_motion_key_drag();
    let moved = pose(&editor, 5);
    assert!((moved.x - at.x - 20.).abs() < 0.1 && (moved.y - at.y + 10.).abs() < 0.1);
    assert_eq!(
        curve(&editor).len(),
        4,
        "new position anchor subdivides retained cubic"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, with_alpha);

    // Deleting a temporal waypoint retains its geometric node. Later endpoint edits must
    // still be visible, and dragging a non-position key splits that retained curve as needed.
    editor.transact_named("Remove middle position key", |editor| {
        editor.symbol_mut().timeline.layers[0]
            .keyframes
            .retain(|key| key.frame != 10);
    });
    let removed = editor.project.clone();
    let endpoint_before = pose(&editor, 20);
    assert!(editor.begin_motion_key_drag(20, [endpoint_before.x, endpoint_before.y]));
    editor.drag_motion_key_to([endpoint_before.x + 30., endpoint_before.y + 20.]);
    editor.finish_motion_key_drag();
    let endpoint_after = pose(&editor, 20);
    assert!(
        (endpoint_after.x - endpoint_before.x - 30.).abs() < 1e-6
            && (endpoint_after.y - endpoint_before.y - 20.).abs() < 1e-6
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, removed);
    let middle_before = pose(&editor, 5);
    assert!(editor.begin_motion_key_drag(5, [middle_before.x, middle_before.y]));
    editor.drag_motion_key_to([middle_before.x + 25., middle_before.y - 15.]);
    editor.finish_motion_key_drag();
    let middle_after = pose(&editor, 5);
    assert!(
        (middle_after.x - middle_before.x - 25.).abs() < 0.1
            && (middle_after.y - middle_before.y + 15.).abs() < 0.1
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, removed);

    let restored = Project::from_json(&before.to_json().unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.format_version, before.format_version);
    for frame in 0..30 {
        let before_pose = sample_layer(&before.symbols[0].timeline.layers[0], frame)[0].transform;
        let restored_pose =
            sample_layer(&restored.symbols[0].timeline.layers[0], frame)[0].transform;
        assert!(
            (before_pose.x - restored_pose.x).abs() < 1e-9
                && (before_pose.y - restored_pose.y).abs() < 1e-9,
            "JSON round trip preserves sampled geometry at frame {frame}"
        );
    }
}

#[test]
fn secondary_motion_tracks_only_expose_matching_curve_handles() {
    let (mut editor, _, _) = fixture();
    let layer = editor.symbol().timeline.layers[0].id.clone();
    for key in &mut editor.symbol_mut().timeline.layers[0].keyframes {
        let mut secondary = key.elements[0].clone();
        secondary.id = "secondary".into();
        secondary.transform.x += if key.frame == 20 { 45. } else { 20. };
        secondary.transform.y += 10.;
        key.elements.push(secondary);
    }
    editor.set_selection(vec![SelectionTarget {
        layer: layer.clone(),
        element: "secondary".into(),
    }]);
    let before = editor.project.clone();
    assert!(!editor.begin_motion_curve_drag(CurveTarget::Segment(1), [120., 110.]));
    assert_eq!(editor.project, before);
    assert!(
        editor.begin_motion_key_drag(10, [170., 110.]),
        "secondary position keys still edit"
    );
    editor.cancel_motion_drags();
    editor.set_selection(vec![SelectionTarget {
        layer: layer.clone(),
        element: "moving".into(),
    }]);
    assert!(editor.begin_motion_curve_drag(CurveTarget::Segment(1), [100., 100.]));
    editor.cancel_motion_drags();
    editor.symbol_mut().timeline.layers[0].keyframes[2].elements[1]
        .transform
        .x -= 25.;
    editor.set_selection(vec![SelectionTarget {
        layer,
        element: "secondary".into(),
    }]);
    assert!(
        editor.begin_motion_curve_drag(CurveTarget::Segment(1), [120., 110.]),
        "constant offsets share exact curve geometry"
    );
    editor.cancel_motion_drags();
    editor.project.validate().unwrap();
}

#[test]
fn deleting_or_reordering_a_curve_reference_preserves_surviving_animation() {
    for (changing_offset, remove_reference) in
        [(false, true), (false, false), (true, true), (true, false)]
    {
        let (mut editor, _, _) = fixture();
        for key in &mut editor.symbol_mut().timeline.layers[0].keyframes {
            let mut secondary = key.elements[0].clone();
            secondary.id = "secondary".into();
            secondary.transform.x += if changing_offset {
                20. + key.frame as f64
            } else {
                20.
            };
            secondary.transform.y += 10.;
            key.elements.push(secondary);
            if key.frame == 10 {
                key.label = "middle".into();
            }
        }
        editor.begin_motion_curve_drag(CurveTarget::Segment(1), [100., 100.]);
        editor.drag_motion_curve_to([100., 160.], false);
        editor.finish_motion_curve_drag();
        let before = editor.project.clone();
        let original = editor.symbol().timeline.layers[0].clone();
        editor.transact_named("Change span reference", |editor| {
            let key = &mut editor.symbol_mut().timeline.layers[0].keyframes[0];
            if remove_reference {
                key.elements.remove(0);
            } else {
                key.elements.swap(0, 1);
            }
        });
        editor.project.validate().unwrap();
        let changed = &editor.symbol().timeline.layers[0];
        assert_eq!(changed.keyframes[0].motion_path.is_none(), changing_offset);
        assert_eq!(
            changed
                .keyframes
                .iter()
                .find(|key| key.frame == 10)
                .unwrap()
                .label,
            "middle"
        );
        for frame in 0..30 {
            let old = sample_layer(&original, frame);
            let now = sample_layer(changed, frame);
            assert_eq!(now.len(), if remove_reference { 1 } else { 2 });
            assert_eq!(now[0].id, "secondary");
            for element in now {
                let expected = old.iter().find(|old| old.id == element.id).unwrap();
                assert!(
                    (element.transform.x - expected.transform.x).abs() < 1e-7,
                    "x at {frame}"
                );
                assert!(
                    (element.transform.y - expected.transform.y).abs() < 1e-7,
                    "y at {frame}"
                );
            }
        }
        editor.undo();
        assert_eq!(editor.project, before);
    }
}
