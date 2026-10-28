//! Independent interaction regressions: real egui pointer events and authoring history.
use crate::{
    app::{Editor, Tool},
    tests::{primary_button, rectangle_stage, stage_input_frame},
};
use eframe::egui::{self, Event, Modifiers, Pos2, Rect};
use neoflash_core::*;

fn object(id: &str, x: f64, y: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 60.,
            height: 40.,
            fill: [40, 100, 180, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        scripts: vec![],
        accessibility: None,
    }
}

fn layer(id: &str, elements: Vec<Element>, tween: bool) -> Layer {
    let mut keys = vec![Keyframe {
        frame: 0,
        elements: elements.clone(),
        tween: tween.then_some(Easing::EaseIn),
        label: String::new(),
    }];
    if tween {
        let end = elements
            .into_iter()
            .map(|mut element| {
                element.transform.x += 80.;
                element.transform.y += 40.;
                element
            })
            .collect();
        keys.push(Keyframe {
            frame: 10,
            elements: end,
            tween: None,
            label: String::new(),
        });
    }
    Layer {
        properties: Default::default(),
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes: keys,
    }
}

fn fixture(frame: u32) -> (Editor, egui::Context, Rect) {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.layers = vec![
        layer(
            "first-layer",
            vec![object("first", 100., 100.), object("untouched", 600., 100.)],
            true,
        ),
        layer("second-layer", vec![object("second", 300., 200.)], true),
    ];
    editor.symbol_mut().timeline.frame_count = 11;
    editor.frame = frame;
    editor.tool = Tool::Select;
    editor.clear_selection();
    editor.history.clear();
    editor.saved_project = Some(editor.project.clone());
    editor.project.validate().unwrap();
    (editor, ctx, page)
}

fn select(editor: &mut Editor, ids: &[&str]) {
    let targets = ids
        .iter()
        .map(|id| editor.target_for_id(id).unwrap())
        .collect();
    editor.set_selection(targets);
}

fn ids(editor: &Editor) -> Vec<&str> {
    let mut values: Vec<_> = editor
        .selection
        .targets
        .iter()
        .map(|target| target.element.as_str())
        .collect();
    values.sort_unstable();
    values
}

fn sampled(editor: &Editor, id: &str) -> Element {
    editor
        .symbol()
        .timeline
        .layers
        .iter()
        .flat_map(|layer| sample_layer(layer, editor.frame))
        .find(|element| element.id == id)
        .unwrap()
}

fn center(editor: &Editor, page: Rect, id: &str) -> Pos2 {
    let element = sampled(editor, id);
    page.min
        + egui::vec2(
            element.transform.x as f32 + 30.,
            element.transform.y as f32 + 20.,
        )
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    let mut event = primary_button(pos, pressed);
    if let Event::PointerButton {
        modifiers: value, ..
    } = &mut event
    {
        *value = modifiers;
    }
    event
}

fn input(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
    modifiers: Modifiers,
) {
    if modifiers == Modifiers::NONE {
        stage_input_frame(editor, ctx, time, events);
    } else {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
                time: Some(time),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| editor.stage(ctx),
        );
    }
}

fn drag(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    start: Pos2,
    end: Pos2,
    batched: bool,
    modifiers: Modifiers,
) {
    let mut events = vec![Event::PointerMoved(start), button(start, true, modifiers)];
    if batched {
        events.extend([Event::PointerMoved(end), button(end, false, modifiers)]);
    }
    input(editor, ctx, time, events, modifiers);
    if !batched {
        input(
            editor,
            ctx,
            time + 0.1,
            vec![Event::PointerMoved(end)],
            modifiers,
        );
        input(
            editor,
            ctx,
            time + 0.2,
            vec![button(end, false, modifiers)],
            modifiers,
        );
    }
}

fn begin_move(editor: &mut Editor, ctx: &egui::Context, start: Pos2) {
    input(
        editor,
        ctx,
        1.,
        vec![Event::PointerMoved(start), primary_button(start, true)],
        Modifiers::NONE,
    );
    input(
        editor,
        ctx,
        1.1,
        vec![Event::PointerMoved(start + egui::vec2(35., 20.))],
        Modifiers::NONE,
    );
    assert!(editor.object_gesture.is_some());
}

#[test]
fn multi_layer_midpoint_drag_uses_sampled_poses_and_one_undo_for_both_layers() {
    for batched in [false, true] {
        let (mut editor, ctx, page) = fixture(5);
        select(&mut editor, &["first", "second"]);
        let before = editor.project.clone();
        let poses: Vec<_> = ["first", "second", "untouched"]
            .map(|id| sampled(&editor, id))
            .into();
        let start = center(&editor, page, "first");
        drag(
            &mut editor,
            &ctx,
            1.,
            start,
            start + egui::vec2(35., 20.),
            batched,
            Modifiers::NONE,
        );
        assert_eq!(
            ids(&editor),
            ["first", "second"],
            "dragging an already selected object preserves the set"
        );
        for (index, layer) in editor.symbol().timeline.layers.iter().enumerate() {
            assert_eq!(
                layer
                    .keyframes
                    .iter()
                    .map(|key| key.frame)
                    .collect::<Vec<_>>(),
                [0, 5, 10]
            );
            assert_eq!(
                layer.keyframes[0],
                before.symbols[0].timeline.layers[index].keyframes[0]
            );
            assert_eq!(
                layer.keyframes[2],
                before.symbols[0].timeline.layers[index].keyframes[1]
            );
        }
        for original in &poses[..2] {
            let actual = sampled(&editor, &original.id);
            assert_eq!(
                (actual.transform.x, actual.transform.y),
                (original.transform.x + 35., original.transform.y + 20.)
            );
        }
        assert_eq!(sampled(&editor, "untouched"), poses[2]);
        assert!(editor.object_gesture.is_none());
        editor.project.validate().unwrap();
        let after = editor.project.clone();
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());
        editor.redo();
        assert_eq!(editor.project, after);
    }

    // The same selection/history contract also covers the Modify family.
    let (mut editor, ctx, _) = fixture(0);
    select(&mut editor, &["first", "second"]);
    let before = editor.project.clone();
    editor.align_selection(&ctx, 1, 0);
    assert_eq!(sampled(&editor, "second").transform.y, 100.);
    editor.undo();
    assert_eq!(editor.project, before);
    select(&mut editor, &["first", "second"]);
    editor.space_selection(&ctx, 0, Some(-10.));
    assert_eq!(sampled(&editor, "second").transform.x, 150.);
    editor.undo();
    assert_eq!(editor.project, before);
    select(&mut editor, &["first", "second", "untouched"]);
    editor.distribute_selection(&ctx, 0, 1);
    assert_eq!(sampled(&editor, "second").transform.x, 350.);
    editor.undo();
    assert_eq!(editor.project, before);

    select(&mut editor, &["first", "untouched"]);
    editor.group_selection();
    let grouped = editor.project.clone();
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        1
    );
    editor.clear_selection();
    select(&mut editor, &["first"]);
    assert_eq!(ids(&editor), ["first", "untouched"]);
    editor.authoring_modify.to_stage = true;
    editor.align_selection(&ctx, 0, 1);
    assert_eq!(
        sampled(&editor, "untouched").transform.x - sampled(&editor, "first").transform.x,
        500.
    );
    editor.undo();
    assert_eq!(editor.project, grouped);
    editor.authoring_modify.to_stage = false;
    editor.duplicate();
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        2
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, grouped);
    select(&mut editor, &["first"]);
    editor.ungroup_selection();
    assert!(
        editor.symbol().timeline.layers[0]
            .properties
            .groups
            .is_empty()
    );
    editor.undo();
    assert_eq!(editor.project, grouped);
    select(&mut editor, &["first"]);
    editor.affine_selection(&ctx, [-1., 0., 0., 1., 0., 0.]);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, grouped);
}

#[test]
fn shift_click_toggles_and_shift_drag_preserves_the_selected_set() {
    let (mut editor, ctx, page) = fixture(0);
    let a = center(&editor, page, "first");
    let b = center(&editor, page, "second");
    drag(&mut editor, &ctx, 1., a, a, true, Modifiers::NONE);
    drag(&mut editor, &ctx, 2., b, b, true, Modifiers::SHIFT);
    assert_eq!(ids(&editor), ["first", "second"]);
    drag(&mut editor, &ctx, 3., a, a, true, Modifiers::SHIFT);
    assert_eq!(ids(&editor), ["second"]);
    drag(&mut editor, &ctx, 4., a, a, true, Modifiers::SHIFT);
    assert!(
        !editor.history.can_undo(),
        "selection alone must not create document history"
    );
    drag(
        &mut editor,
        &ctx,
        5.,
        a,
        a + egui::vec2(30., 10.),
        false,
        Modifiers::SHIFT,
    );
    assert_eq!(ids(&editor), ["first", "second"]);
    assert_eq!(
        (
            sampled(&editor, "first").transform.x,
            sampled(&editor, "first").transform.y
        ),
        (130., 100.)
    );
    assert_eq!(
        (
            sampled(&editor, "second").transform.x,
            sampled(&editor, "second").transform.y
        ),
        (330., 200.)
    );
}

#[test]
fn marquee_requires_full_bounds_and_shift_marquee_adds_to_the_initial_selection() {
    let (mut editor, ctx, page) = fixture(0);
    select(&mut editor, &["untouched"]);
    let before = editor.project.clone();
    drag(
        &mut editor,
        &ctx,
        1.,
        page.min + egui::vec2(80., 80.),
        page.min + egui::vec2(150., 160.),
        true,
        Modifiers::NONE,
    );
    assert!(
        editor.selection.is_empty(),
        "partial overlap is insufficient"
    );
    drag(
        &mut editor,
        &ctx,
        2.,
        page.min + egui::vec2(80., 80.),
        page.min + egui::vec2(180., 160.),
        false,
        Modifiers::NONE,
    );
    assert_eq!(ids(&editor), ["first"]);
    drag(
        &mut editor,
        &ctx,
        3.,
        page.min + egui::vec2(380., 260.),
        page.min + egui::vec2(280., 180.),
        true,
        Modifiers::SHIFT,
    );
    assert_eq!(ids(&editor), ["first", "second"]);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(!editor.dirty());
}

#[test]
fn fast_and_sequential_move_both_apply_release_position_outside_the_canvas() {
    for batched in [false, true] {
        let (mut editor, ctx, page) = fixture(5);
        select(&mut editor, &["first"]);
        let before = editor.project.clone();
        let original = sampled(&editor, "first");
        let start = center(&editor, page, "first");
        let end = Pos2::new(1320., 930.);
        drag(&mut editor, &ctx, 1., start, end, batched, Modifiers::NONE);
        let actual = sampled(&editor, "first");
        assert_eq!(
            (actual.transform.x, actual.transform.y),
            (
                original.transform.x + f64::from(end.x - start.x),
                original.transform.y + f64::from(end.y - start.y)
            )
        );
        assert!(editor.object_gesture.is_none());
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn click_and_return_to_origin_leave_no_midpoint_keys_dirty_flag_or_history() {
    for return_to_origin in [false, true] {
        let (mut editor, ctx, page) = fixture(5);
        select(&mut editor, &["first", "second"]);
        let before = editor.project.clone();
        let start = center(&editor, page, "first");
        let mut events = vec![Event::PointerMoved(start), primary_button(start, true)];
        if return_to_origin {
            events.push(Event::PointerMoved(start + egui::vec2(50., 25.)));
        }
        events.extend([Event::PointerMoved(start), primary_button(start, false)]);
        input(&mut editor, &ctx, 1., events, Modifiers::NONE);
        assert_eq!(editor.project, before);
        assert!(!editor.dirty());
        assert!(!editor.history.can_undo());
        assert!(editor.object_gesture.is_none());
    }
}

#[test]
fn nested_hit_selects_the_outer_placement_and_skips_a_locked_foreground() {
    let (mut editor, ctx, page) = fixture(0);
    let mut inside = layer("inside-layer", vec![object("inside", 0., 0.)], false);
    inside.locked = true;
    editor.project.symbols.push(Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: "nested-symbol".into(),
        name: "Nested".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![inside],
            scripts: vec![],
        },
    });
    let mut instance = object("outer", 100., 100.);
    instance.kind = ElementKind::Instance {
        symbol_id: "nested-symbol".into(),
        playback: InstancePlayback::Independent,
    };
    let mut foreground = layer("locked-overlay", vec![object("overlay", 100., 100.)], false);
    foreground.locked = true;
    editor.symbol_mut().timeline.layers =
        vec![layer("outer-layer", vec![instance], false), foreground];
    editor.clear_selection();
    let pos = page.min + egui::vec2(130., 120.);
    drag(&mut editor, &ctx, 1., pos, pos, true, Modifiers::NONE);
    let target = editor
        .selection
        .single()
        .expect("outer instance should be editable below locked overlay");
    assert_eq!((&*target.layer, &*target.element), ("outer-layer", "outer"));
    let before = editor.project.clone();
    drag(
        &mut editor,
        &ctx,
        2.,
        pos,
        pos + egui::vec2(25., 15.),
        true,
        Modifiers::NONE,
    );
    assert_eq!(
        (
            sampled(&editor, "outer").transform.x,
            sampled(&editor, "outer").transform.y
        ),
        (125., 115.)
    );
    assert_eq!(
        editor.project.symbol("nested-symbol"),
        before.symbol("nested-symbol")
    );
    assert_eq!(
        editor.symbol().timeline.layers[1],
        before.symbols[0].timeline.layers[1]
    );
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn escape_restores_all_layers_and_late_release_cannot_mutate_another_selection() {
    let (mut editor, ctx, page) = fixture(5);
    select(&mut editor, &["first", "second"]);
    let before = editor.project.clone();
    let start = center(&editor, page, "first");
    begin_move(&mut editor, &ctx, start);
    assert_ne!(editor.project, before);
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
            time: Some(1.15),
            events: vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.stage(ctx);
        },
    );
    assert_eq!(
        ids(&editor),
        ["first", "second"],
        "Escape cancels the gesture before clearing its original selection"
    );
    editor.select_single_id("untouched".into());
    input(
        &mut editor,
        &ctx,
        1.2,
        vec![
            Event::PointerMoved(start + egui::vec2(90., 80.)),
            primary_button(start + egui::vec2(90., 80.), false),
        ],
        Modifiers::NONE,
    );
    assert_eq!(editor.project, before);
    assert_eq!(ids(&editor), ["untouched"]);
    assert!(!editor.history.can_undo());
}

#[test]
fn changing_frame_during_a_move_cancels_before_touching_another_authored_key() {
    let (mut editor, ctx, page) = fixture(5);
    select(&mut editor, &["first", "second"]);
    let before = editor.project.clone();
    let start = center(&editor, page, "first");
    begin_move(&mut editor, &ctx, start);
    editor.frame = 10;
    input(
        &mut editor,
        &ctx,
        1.2,
        vec![
            Event::PointerMoved(start + egui::vec2(90., 80.)),
            primary_button(start + egui::vec2(90., 80.), false),
        ],
        Modifiers::NONE,
    );
    assert_eq!(editor.project, before);
    assert_eq!(editor.frame, 10);
    assert!(!editor.history.can_undo());
    assert!(editor.object_gesture.is_none());
}

#[test]
fn switching_tool_finishes_the_move_and_does_not_draw_or_retarget_on_release() {
    let (mut editor, ctx, page) = fixture(5);
    select(&mut editor, &["first", "second"]);
    let before = editor.project.clone();
    let start = center(&editor, page, "first");
    begin_move(&mut editor, &ctx, start);
    editor.set_tool(Tool::Rectangle);
    let moved = editor.project.clone();
    editor.select_single_id("untouched".into());
    input(
        &mut editor,
        &ctx,
        1.2,
        vec![
            Event::PointerMoved(start + egui::vec2(100., 90.)),
            primary_button(start + egui::vec2(100., 90.), false),
        ],
        Modifiers::NONE,
    );
    assert_eq!(editor.project, moved);
    assert!(editor.shape_draft.is_none());
    assert!(editor.object_gesture.is_none());
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn layer_lock_or_hide_is_separate_from_the_finished_move_transaction() {
    for hide in [false, true] {
        let (mut editor, ctx, page) = fixture(5);
        select(&mut editor, &["first", "second"]);
        let before = editor.project.clone();
        let start = center(&editor, page, "first");
        begin_move(&mut editor, &ctx, start);
        editor.finish_edits();
        let moved = editor.project.clone();
        editor.transact(|editor| {
            if hide {
                editor.symbol_mut().timeline.layers[0].visible = false;
            } else {
                editor.symbol_mut().timeline.layers[0].locked = true;
            }
        });
        let locked = editor.project.clone();
        input(
            &mut editor,
            &ctx,
            1.2,
            vec![
                Event::PointerMoved(start + egui::vec2(100., 90.)),
                primary_button(start + egui::vec2(100., 90.), false),
            ],
            Modifiers::NONE,
        );
        assert_eq!(editor.project, locked);
        editor.undo();
        assert_eq!(editor.project, moved);
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn panning_during_a_drag_cancels_the_frozen_view_transaction() {
    let (mut editor, ctx, page) = fixture(5);
    select(&mut editor, &["first", "second"]);
    let before = editor.project.clone();
    let start = center(&editor, page, "first");
    begin_move(&mut editor, &ctx, start);
    editor.pan += egui::vec2(30., 20.);
    input(
        &mut editor,
        &ctx,
        1.2,
        vec![
            Event::PointerMoved(start + egui::vec2(100., 90.)),
            primary_button(start + egui::vec2(100., 90.), false),
        ],
        Modifiers::NONE,
    );
    assert_eq!(editor.project, before);
    assert!(editor.object_gesture.is_none());
    assert!(!editor.history.can_undo());
}

#[test]
fn timeline_key_drop_uses_final_release_column_and_keeps_the_captured_layer() {
    let (mut editor, ctx, _) = fixture(0);
    let before = editor.project.clone();
    let captured_layer = crate::layer_controls::visible_layer_indices(&editor.symbol().timeline)[0];
    let mut timeline_input = |time, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                editor.timeline(ctx);
                editor.stage(ctx);
            },
        )
    };
    timeline_input(0.1, vec![]);
    let output = timeline_input(0.2, vec![]);
    let start = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle)
                if circle.radius == 3.
                    && circle.fill == crate::theme::UiPalette::current(&ctx).timeline_key
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
        .expect("timeline must paint the first authored key");
    let third = start + egui::vec2(13. * 3., 0.);
    let eighth_other_row = start + egui::vec2(13. * 8., crate::timeline_paint::ROW_HEIGHT);
    timeline_input(
        1.,
        vec![Event::PointerMoved(start), primary_button(start, true)],
    );
    timeline_input(1.1, vec![Event::PointerMoved(third)]);
    timeline_input(
        1.2,
        vec![
            Event::PointerMoved(eighth_other_row),
            primary_button(eighth_other_row, false),
        ],
    );
    assert_eq!(editor.frame, 8);
    assert_eq!(
        editor.layer, captured_layer,
        "moving a key keeps its captured layer despite releasing over another row"
    );
    assert_eq!(
        editor.symbol().timeline.layers[captured_layer]
            .keyframes
            .iter()
            .map(|key| key.frame)
            .collect::<Vec<_>>(),
        [8, 10]
    );
    assert_eq!(
        editor.symbol().timeline.layers[1 - captured_layer],
        before.symbols[0].timeline.layers[1 - captured_layer]
    );
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}
