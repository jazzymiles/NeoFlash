//! Shape-hint authoring uses the existing normalized pairs and the real stage input pipeline.
use crate::{
    app::{Editor, Tool},
    tests::{primary_button, rectangle_stage, stage_input_frame},
};
use eframe::egui::{self, Event, Pos2};
use neoflash_core::*;
use neoflash_ui::components::canvas::ShapeHintState;

fn fixture() -> (Editor, egui::Context, egui::Rect) {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = Tool::Select;
    let shape = |x, y| Element {
        id: "shape".into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 120.,
            height: 80.,
            fill: [40, 80, 180, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ..Default::default()
    };
    let timeline = &mut editor.symbol_mut().timeline;
    timeline.frame_count = 11;
    timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Shape,
            elements: vec![shape(50., 60.)],
            ..Default::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![shape(240., 160.)],
            ..Default::default()
        },
    ];
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    (editor, ctx, page)
}

fn drag(editor: &mut Editor, ctx: &egui::Context, time: f64, from: Pos2, to: Pos2) {
    stage_input_frame(
        editor,
        ctx,
        time,
        vec![Event::PointerMoved(from), primary_button(from, true)],
    );
    stage_input_frame(
        editor,
        ctx,
        time + 0.02,
        vec![Event::PointerMoved(from.lerp(to, 0.5))],
    );
    stage_input_frame(editor, ctx, time + 0.04, vec![Event::PointerMoved(to)]);
    stage_input_frame(editor, ctx, time + 0.06, vec![primary_button(to, false)]);
    stage_input_frame(editor, ctx, time + 0.08, vec![]);
}
fn marker_screen(editor: &Editor, page: egui::Rect) -> Pos2 {
    let p = editor.shape_hint_markers()[0].position;
    page.min + egui::vec2(p[0] as f32, p[1] as f32) * editor.zoom
}

fn shortcut(editor: &mut Editor, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
    let _ = ctx.run(
        egui::RawInput {
            modifiers,
            events: vec![Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        },
        |ctx| editor.shortcuts(ctx),
    );
}

#[test]
fn shape_hint_pairs_drag_remove_cancel_and_round_trip_with_single_undo() {
    let (mut editor, ctx, page) = fixture();
    let empty = editor.project.clone();
    assert!(editor.can_add_shape_hint());
    assert_eq!(editor.add_shape_hint(), Some('a'));
    assert_eq!(editor.project.format_version, 7);
    assert_eq!(
        editor.shape_hint_markers()[0].state,
        ShapeHintState::Unplaced
    );
    let added = editor.project.clone();
    let from = marker_screen(&editor, page);
    let steps = editor.history.undo_len();
    stage_input_frame(
        &mut editor,
        &ctx,
        0.01,
        vec![Event::PointerMoved(from), primary_button(from, true)],
    );
    stage_input_frame(&mut editor, &ctx, 0.03, vec![primary_button(from, false)]);
    assert_eq!(
        editor.project, added,
        "clicking a marker does not place it or add history"
    );
    stage_input_frame(&mut editor, &ctx, 0.05, vec![]);
    drag(
        &mut editor,
        &ctx,
        0.1,
        from,
        page.min + egui::vec2(50., 60.),
    );
    assert_eq!(editor.history.undo_len(), steps + 1);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].shape_hints[0].start_uv,
        [0., 0.]
    );
    assert_eq!(
        editor.shape_hint_markers()[0].state,
        ShapeHintState::Unplaced,
        "other endpoint has not been placed"
    );
    editor.project.validate().unwrap();
    let start_placed = editor.project.clone();
    editor.frame = 10;
    let from = marker_screen(&editor, page);
    drag(
        &mut editor,
        &ctx,
        0.3,
        from,
        page.min + egui::vec2(360., 240.),
    );
    assert_eq!(editor.shape_hint_markers()[0].state, ShapeHintState::End);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].shape_hints[0].end_uv,
        [1., 1.]
    );
    let both = editor.project.clone();
    let restored = Project::from_json(&editor.project.to_json().unwrap()).unwrap();
    assert_eq!(restored, both);
    editor.undo();
    assert_eq!(editor.project, start_placed, "moving end is one Undo");
    editor.redo();
    assert_eq!(editor.project, both);
    editor.frame = 0;
    assert_eq!(editor.shape_hint_markers()[0].state, ShapeHintState::Start);

    assert!(editor.begin_shape_hint_drag('a'));
    editor.drag_shape_hint_to([170., 140.]);
    assert_ne!(editor.project, both);
    shortcut(&mut editor, &ctx, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(editor.timeline_shape_hints.drag.is_none());
    assert_eq!(
        editor.project, both,
        "Escape restores the draft without history"
    );
    let from = marker_screen(&editor, page);
    drag(
        &mut editor,
        &ctx,
        0.5,
        from,
        page.min - egui::vec2(10., 10.),
    );
    assert!(!editor.can_remove_shape_hints());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, both, "off-stage removal is one Undo");
    editor.frame = 10;
    assert!(
        editor.remove_all_shape_hints(),
        "remove from end key edits start's pair"
    );
    editor.undo();
    assert_eq!(editor.project, both);
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, added);
    editor.undo();
    assert_eq!(editor.project, empty);
}

#[test]
fn shape_hint_overlay_visibility_paint_and_guide_priority_are_shared_across_themes() {
    let (mut editor, ctx, page) = fixture();
    editor.add_shape_hint();
    let document = editor.project.clone();
    let history = editor.history.undo_len();
    let danger = neoflash_ui::theme::Theme::Dark.palette().danger_default;
    for dark in [true, false] {
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let painted = stage_input_frame(&mut editor, &ctx, if dark { 0.1 } else { 0.2 }, vec![]);
        assert!(
            painted
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.fill == danger)),
            "unplaced NF2 marker in both themes"
        );
    }
    editor.frame = 5;
    assert!(
        editor.shape_hint_markers().is_empty(),
        "no markers on in-between frames"
    );
    editor.frame = 0;
    shortcut(
        &mut editor,
        &ctx,
        egui::Key::H,
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
    );
    assert!(editor.shape_hint_markers().is_empty());
    assert!(!editor.begin_shape_hint_drag('a'));
    shortcut(
        &mut editor,
        &ctx,
        egui::Key::H,
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
    );
    assert_eq!(editor.project, document);
    assert_eq!(
        editor.history.undo_len(),
        history,
        "view state is not document history"
    );
    editor.symbol_mut().timeline.layers[0].locked = true;
    assert!(!editor.can_add_shape_hint());
    assert!(!editor.can_remove_shape_hints());
    assert!(editor.shape_hint_markers().is_empty());
    editor.symbol_mut().timeline.layers[0].locked = false;

    // A guide crossing a marker owns the press; the hint must not start a competing gesture.
    let from = marker_screen(&editor, page);
    let hint_y = editor.shape_hint_markers()[0].position[1];
    editor.symbol_mut().timeline.guides.push(Guide {
        axis: GuideAxis::Horizontal,
        position: hint_y,
    });
    editor.project.upgrade_format_for_features();
    let before_guide = editor.project.clone();
    drag(&mut editor, &ctx, 0.4, from, from + egui::vec2(0., 30.));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].shape_hints,
        before_guide.symbols[0].timeline.layers[0].keyframes[0].shape_hints
    );
    assert_ne!(editor.guides()[0].position, hint_y);
    editor.undo();
    assert_eq!(editor.project, before_guide);
}

#[test]
fn shape_hints_respect_transformed_geometry_creation_limit_and_endpoint_scope() {
    let (mut editor, _, _) = fixture();
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0]
        .transform
        .rotation = 90.;
    editor.add_shape_hint();
    assert!(editor.begin_shape_hint_drag('a'));
    editor.drag_shape_hint_to([50., 60.]);
    editor.finish_shape_hint_drag(false);
    let p = editor.shape_hint_markers()[0].position;
    assert!((p[0] - 50.).abs() < 1e-6 && (p[1] - 60.).abs() < 1e-6);
    for letter in 'b'..='z' {
        assert_eq!(editor.add_shape_hint(), Some(letter));
    }
    assert!(!editor.can_add_shape_hint());
    assert_eq!(editor.add_shape_hint(), None);
    editor.project.validate().unwrap();
    editor.layer = usize::MAX;
    assert!(editor.shape_hint_markers().is_empty());
    assert!(!editor.can_remove_shape_hints());
}

#[test]
fn transformed_shape_hint_overlay_and_drag_share_the_same_mapping() {
    let (mut editor, ctx, _) = fixture();
    editor.zoom = 1.0;
    editor.add_shape_hint();
    let mapping =
        crate::stage_mapping::StageMapping::try_new([1.1, 0.35, -0.25, 0.8, 60., 25.]).unwrap();
    let origin = egui::pos2(90., 70.);
    let marker = editor.shape_hint_markers()[0].position;
    let from = mapping.edit_to_screen(
        origin,
        editor.zoom,
        Pos2::new(marker[0] as f32, marker[1] as f32),
    );
    let target = [50., 60.];
    let to = mapping.edit_to_screen(
        origin,
        editor.zoom,
        Pos2::new(target[0] as f32, target[1] as f32),
    );

    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(900., 700.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.paint_shape_hints(ui.painter(), origin, mapping);
            });
        },
    );
    assert!(output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Circle(circle) if circle.center.distance(from) < 1e-3)
    }));

    let mut frame = |time: f64, events: Vec<Event>| {
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(900., 700.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let canvas = ui.max_rect();
                    editor.shape_hint_input(ui, canvas, canvas, origin, mapping);
                });
            },
        );
    };
    frame(
        0.1,
        vec![Event::PointerMoved(from), primary_button(from, true)],
    );
    frame(0.12, vec![Event::PointerMoved(to)]);
    frame(0.14, vec![primary_button(to, false)]);

    let stored = editor.symbol().timeline.layers[0].keyframes[0].shape_hints[0].start_uv;
    assert!(
        stored[0].abs() < 1e-6 && stored[1].abs() < 1e-6,
        "{stored:?}"
    );
    assert!(editor.timeline_shape_hints.drag.is_none());
}

#[test]
fn shape_hints_handle_chained_spans_in_between_jump_single_removal_vertex_snap_and_f1_search() {
    let (mut editor, ctx, page) = fixture();

    // SH-08: F1 function search & shortcut registry discover all 3 shape hint commands.
    let search_hits = crate::panels::function_search::function_search_results("shape hint");
    assert!(search_hits.contains(&crate::commands::CommandId::ModifyAddShapeHint));
    assert!(search_hits.contains(&crate::commands::CommandId::ViewShowShapeHints));
    let remove_hits = crate::panels::function_search::function_search_results("remove all hints");
    assert!(remove_hits.contains(&crate::commands::CommandId::ModifyRemoveAllShapeHints));
    assert_eq!(
        editor.sc(crate::commands::CommandId::ModifyAddShapeHint),
        "⇧⌘H"
    );
    assert_eq!(
        editor.sc(crate::commands::CommandId::ViewShowShapeHints),
        "⌥⌘H"
    );

    // SH-09: Adding a shape hint from an in-between frame jumps the playhead to the start keyframe
    // and reveals the new marker immediately.
    editor.frame = 5;
    editor.timeline_shape_hints.show = false;
    assert_eq!(editor.add_shape_hint(), Some('a'));
    assert_eq!(editor.frame, 0);
    assert!(editor.timeline_shape_hints.show);
    assert_eq!(editor.shape_hint_markers().len(), 1);

    // SH-05: Deleting 'a' after adding 'b' and re-adding 'a' assigns deterministic label-indexed UV
    // [1/27, 0.5] so 'a' and 'b' never stack at [2/27, 0.5].
    assert_eq!(editor.add_shape_hint(), Some('b'));
    assert!(editor.remove_shape_hint('a'));
    assert_eq!(editor.add_shape_hint(), Some('a'));
    let hints = &editor.symbol().timeline.layers[0].keyframes[0].shape_hints;
    let uv_a = hints.iter().find(|h| h.label == 'a').unwrap().start_uv;
    let uv_b = hints.iter().find(|h| h.label == 'b').unwrap().start_uv;
    assert!((uv_a[0] - 1.0 / 27.0).abs() < 1e-9);
    assert!((uv_b[0] - 2.0 / 27.0).abs() < 1e-9);
    assert_ne!(uv_a, uv_b);

    // SH-06: Magnetic corner snapping snaps a pointer within 5px of the rectangle's (170, 60) corner
    // directly to the exact corner UV [1.0, 0.0].
    assert!(editor.begin_shape_hint_drag('a'));
    editor.drag_shape_hint_to([166.5, 62.0]);
    editor.finish_shape_hint_drag(false);
    let snapped_a = editor.symbol().timeline.layers[0].keyframes[0]
        .shape_hints
        .iter()
        .find(|h| h.label == 'a')
        .unwrap()
        .start_uv;
    assert!(
        (snapped_a[0] - 1.0).abs() < 1e-9 && snapped_a[1].abs() < 1e-9,
        "expected magnetic corner snap to [1.0, 0.0], got {snapped_a:?}"
    );

    // SH-07: Pressing Backspace while hovering hint 'b' removes only 'b' in one Undo step.
    let marker_b = editor
        .shape_hint_markers()
        .into_iter()
        .find(|m| m.label == 'b')
        .unwrap();
    let pos_b = page.min + egui::vec2(marker_b.position[0] as f32, marker_b.position[1] as f32);
    stage_input_frame(&mut editor, &ctx, 1.0, vec![Event::PointerMoved(pos_b)]);
    assert_eq!(editor.timeline_shape_hints.hover, Some('b'));
    stage_input_frame(
        &mut editor,
        &ctx,
        1.02,
        vec![
            Event::PointerMoved(pos_b),
            Event::Key {
                key: egui::Key::Backspace,
                physical_key: Some(egui::Key::Backspace),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .shape_hints
            .len(),
        1
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].shape_hints[0].label,
        'a'
    );
    editor.undo();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .shape_hints
            .len(),
        2
    );

    // SH-10: Chained Shape Tweens K0 (0) -> K1 (10) -> K2 (20).
    // At frame 10 (K1), adding a hint adds 'a' to K1 -> K2 (not 'c' to K0 -> K1),
    // and both K0 -> K1's End hints and K1 -> K2's Start hints are visible at frame 10.
    {
        let timeline = &mut editor.symbol_mut().timeline;
        timeline.frame_count = 21;
        timeline.layers[0].keyframes[1].tween = Some(Easing::Linear);
        timeline.layers[0].keyframes[1].tween_kind = TweenKind::Shape;
        let k2_elem = timeline.layers[0].keyframes[0].elements.clone();
        timeline.layers[0].keyframes.push(Keyframe {
            frame: 20,
            elements: k2_elem,
            ..Default::default()
        });
    }
    editor.frame = 10;
    assert_eq!(editor.shape_hint_add_span(), Some((1, 2)));
    assert_eq!(editor.add_shape_hint(), Some('a'));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[1]
            .shape_hints
            .len(),
        1
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[1].shape_hints[0].label,
        'a'
    );
    let markers_at_k1 = editor.shape_hint_markers();
    assert!(
        markers_at_k1
            .iter()
            .any(|m| m.start_frame == 0 && m.end && m.label == 'a'),
        "incoming K0->K1 end hint 'a' must remain visible at K1"
    );
    assert!(
        markers_at_k1
            .iter()
            .any(|m| m.start_frame == 10 && !m.end && m.label == 'a'),
        "outgoing K1->K2 start hint 'a' must be visible at K1"
    );
    editor.project.validate().unwrap();
}
