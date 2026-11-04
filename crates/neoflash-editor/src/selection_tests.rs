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
        ..Element::default()
    }
}

fn layer(id: &str, elements: Vec<Element>, tween: bool) -> Layer {
    let mut keys = vec![Keyframe {
        frame: 0,
        elements: elements.clone(),
        tween: tween.then_some(Easing::EaseIn),
        label: String::new(),
        ..Default::default()
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
            ..Default::default()
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
            guides: vec![],
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
    // NF2/TL Marker Kind=Keyframe: a filled dot on the row centre (radius = frame width / 4).
    let start = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle)
                if (1.5..=3.5).contains(&circle.radius)
                    && circle.fill == neoflash_ui::palette(&ctx).timeline_marker
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
        .expect("timeline must paint the first authored key");
    // Normal frame view: 12 pt per frame (FrameView::Normal).
    let cell = neoflash_ui::metrics::FrameView::Normal.frame_width();
    let third = start + egui::vec2(cell * 3., 0.);
    let eighth_other_row = start + egui::vec2(cell * 8., crate::timeline_paint::ROW_HEIGHT);
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

#[test]
fn color_panel_creates_linear_and_radial_gradients_edits_stops_and_undoes() {
    use crate::authoring_modify::PaintFillType;
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(
        ElementKind::Rectangle {
            width: 120.,
            height: 80.,
            fill: [30, 110, 210, 255],
            stroke: [10, 10, 10, 255],
            stroke_width: 2.,
        },
        100.,
        90.,
    );
    editor.history.clear();
    let solid_before = editor.project.clone();

    // Convert solid Rectangle fill to LinearGradient via Color panel command.
    editor.authoring_modify.color_selection = true;
    editor.set_selection_fill_type(false, PaintFillType::LinearGradient);
    assert!(editor.project.format_version >= 5);
    editor.project.validate().unwrap();
    let ElementKind::PaintedPath {
        fill_gradient: Some(ref linear),
        ..
    } = editor.selected_element().unwrap().kind
    else {
        panic!("expected PaintedPath with LinearGradient");
    };
    assert_eq!(linear.kind, GradientKind::Linear);
    assert_eq!(linear.stops.len(), 2);
    let after_linear = editor.project.clone();

    // Edit stops (unsorted input is normalized and sorted) and switch to RadialGradient.
    editor.set_selection_gradient_stops(
        false,
        vec![
            GradientStop {
                ratio: 255,
                color: [10, 20, 240, 255],
            },
            GradientStop {
                ratio: 0,
                color: [250, 80, 20, 255],
            },
            GradientStop {
                ratio: 128,
                color: [255, 220, 40, 255],
            },
        ],
        None,
    );
    editor.set_selection_fill_type(false, PaintFillType::RadialGradient);
    editor.project.validate().unwrap();
    let ElementKind::PaintedPath {
        fill_gradient: Some(ref radial),
        ..
    } = editor.selected_element().unwrap().kind
    else {
        panic!("expected PaintedPath with RadialGradient");
    };
    assert_eq!(radial.kind, GradientKind::Radial);
    assert_eq!(radial.stops.len(), 3);
    assert_eq!(radial.stops[0].ratio, 0);
    assert_eq!(radial.stops[1].ratio, 128);
    assert_eq!(radial.stops[2].ratio, 255);

    // Render Color panel UI in both selection and drawing modes without error.
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(360., 720.))),
            time: Some(0.1),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| editor.color_panel_ui(ui));
        },
    );

    // Undo radial -> stops -> linear -> solid Rectangle.
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, after_linear);
    editor.undo();
    assert_eq!(editor.project, solid_before);

    // Drawing mode gradient applies automatically to newly drawn shapes on the stage.
    editor.clear_selection();
    editor.authoring_modify.color_selection = false;
    editor.set_drawing_fill_type(false, PaintFillType::LinearGradient);
    editor.set_tool(Tool::Rectangle);
    let p0 = page.min + egui::vec2(260., 120.);
    let p1 = p0 + egui::vec2(90., 60.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            Event::PointerMoved(p0),
            primary_button(p0, true),
            Event::PointerMoved(p1),
            primary_button(p1, false),
        ],
    );
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::PaintedPath {
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                ..
            }),
            ..
        }
    ));
    editor.project.validate().unwrap();
}

#[test]
fn oval_drag_preview_follows_geometry_without_editing_the_document() {
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        for case in 0..4 {
            let (mut editor, ctx, _) = rectangle_stage();
            theme.apply(&ctx);
            editor.zoom = if case >= 2 { 1.5 } else { 1. };
            editor.pan = egui::vec2(27., -19.);
            editor.stage_view.prefs.grid_show = false;
            editor.workspace.drawing.object_drawing = true;
            editor.set_tool(Tool::Ellipse);
            let options = if case == 3 {
                OvalOptions {
                    start_angle: 0.,
                    end_angle: 180.,
                    inner_radius: 40.,
                    close_path: false,
                }
            } else {
                OvalOptions::default()
            };
            editor.authoring_tools.oval_options = options;
            let layout = stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
            let page = layout
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.fill == egui::Color32::WHITE => Some(rect.rect),
                    _ => None,
                })
                .unwrap();
            let start = page.min + egui::vec2(400., 300.) * editor.zoom;
            let delta = if case == 2 {
                egui::vec2(-90., -60.)
            } else {
                egui::vec2(160., 100.)
            };
            let end = start + delta * editor.zoom;
            let modifiers = Modifiers {
                shift: case == 1 || case == 2,
                alt: case == 2,
                ..Modifiers::NONE
            };
            let expected = match case {
                1 => Rect::from_min_size(start, egui::vec2(160., 160.) * editor.zoom),
                2 => Rect::from_center_size(start, egui::vec2(180., 180.) * editor.zoom),
                _ => Rect::from_two_pos(start, end),
            };
            let before = editor.project.clone();
            input(
                &mut editor,
                &ctx,
                0.2,
                vec![Event::PointerMoved(start), button(start, true, modifiers)],
                modifiers,
            );
            let preview = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
                    time: Some(0.3),
                    events: vec![Event::PointerMoved(end)],
                    modifiers,
                    ..Default::default()
                },
                |ctx| editor.stage(ctx),
            );
            let overlay = crate::theme::UiPalette::current(&ctx).overlay;
            let points: Vec<_> = preview
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.vertices.iter().all(|v| v.color == overlay) => {
                        Some(mesh)
                    }
                    _ => None,
                })
                .flat_map(|mesh| mesh.vertices.iter().map(|v| v.pos))
                .collect();
            assert!(
                points.len() > 20,
                "oval preview must paint curved geometry, case {case}"
            );
            let radii = expected.size() * 0.5;
            for point in &points {
                let normalized = (*point - expected.center()) / radii;
                let radius = normalized.length();
                assert!(
                    (radius - 1.).abs() < 0.04 || (case == 3 && (radius - 0.4).abs() < 0.04),
                    "preview must follow the oval, not its bounding rectangle: {point:?}, case {case}"
                );
                if case == 3 {
                    assert!(
                        normalized.y >= -0.04,
                        "open arc must not draw the missing half"
                    );
                }
            }
            assert!(
                points
                    .iter()
                    .any(|p| p.distance(expected.center_bottom()) < 2.),
                "outer contour is visible"
            );
            if case == 3 {
                let inner_bottom = expected.center() + egui::vec2(0., radii.y * 0.4);
                assert!(
                    points.iter().any(|p| p.distance(inner_bottom) < 2.),
                    "inner contour is visible"
                );
            }
            assert_eq!(editor.project, before, "preview is transient");
            assert!(!editor.history.can_undo());
            input(
                &mut editor,
                &ctx,
                0.4,
                vec![button(end, false, modifiers)],
                modifiers,
            );
            let element = editor.selected_element().unwrap();
            assert_eq!(element.oval_options(), options);
            let ElementKind::Ellipse { width, height, .. } = element.kind else {
                panic!("oval committed");
            };
            assert!((width - (expected.width() / editor.zoom) as f64).abs() < 0.001);
            assert!((height - (expected.height() / editor.zoom) as f64).abs() < 0.001);
            assert!(
                (element.transform.x - ((expected.min.x - page.min.x) / editor.zoom) as f64).abs()
                    < 0.001
            );
            assert!(
                (element.transform.y - ((expected.min.y - page.min.y) / editor.zoom) as f64).abs()
                    < 0.001
            );
            assert!(editor.shape_draft.is_none());
            editor.project.validate().unwrap();
            editor.undo();
            assert_eq!(editor.project, before);
            assert!(!editor.history.can_undo());
        }
    }
}

#[test]
fn parametric_primitives_render_inspector_break_apart_to_paths_and_undo() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.authoring_tools.rect_options = RectangleOptions {
        corner_radii: [18., 8., 18., 8.],
        corners_locked: false,
    };
    editor.set_tool(Tool::Rectangle);
    let r0 = page.min + egui::vec2(80., 80.);
    let r1 = r0 + egui::vec2(120., 70.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            Event::PointerMoved(r0),
            primary_button(r0, true),
            Event::PointerMoved(r1),
            primary_button(r1, false),
        ],
    );
    assert_eq!(editor.project.format_version, 7);
    assert_eq!(
        editor.selected_element().unwrap().rectangle_options(),
        RectangleOptions {
            corner_radii: [18., 8., 18., 8.],
            corners_locked: false,
        }
    );

    editor.authoring_tools.oval_options = OvalOptions {
        start_angle: 45.,
        end_angle: 315.,
        inner_radius: 40.,
        close_path: false,
    };
    editor.set_tool(Tool::Ellipse);
    let o0 = page.min + egui::vec2(240., 80.);
    let o1 = o0 + egui::vec2(100., 100.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            Event::PointerMoved(o0),
            primary_button(o0, true),
            Event::PointerMoved(o1),
            primary_button(o1, false),
        ],
    );
    editor.project.validate().unwrap();

    // Exercise the 2-line Properties hook on both parametric elements.
    let mut sample_oval = editor.selected_element().unwrap().clone();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(320., 600.))),
            time: Some(0.3),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = editor.primitive_options_properties_ui(ui, &mut sample_oval);
            });
        },
    );

    editor.select_all();
    let before_break = editor.project.clone();
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(elements.len(), 2);
    for el in elements {
        assert!(el.transform.primitive.is_none());
        assert!(matches!(el.kind, ElementKind::Path { .. }));
    }
    if let ElementKind::Path { fill, commands, .. } = &elements[1].kind {
        assert_eq!(*fill, [0, 0, 0, 0], "open oval Break Apart clears fill");
        assert!(!commands.iter().any(|c| matches!(c, PathCommand::Close)));
    }
    editor.undo();
    assert_eq!(editor.project, before_break);
}

#[test]
fn click_accuracy_controls_selection_slop_and_exact_hit_takes_priority() {
    use crate::workspace::Tolerance;
    let (mut editor, ctx, page) = fixture(0);
    // Replace layers with two touching 60×60 rectangles (`lower` at x=100..160, `upper` at x=160..220)
    // plus an open horizontal vector stroke from (100, 300) to (200, 300) with stroke_width = 1.
    let mut lower = object("lower", 100., 100.);
    lower.kind = ElementKind::Rectangle {
        width: 60.,
        height: 60.,
        fill: [40, 120, 220, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut upper = object("upper", 160., 100.);
    upper.kind = ElementKind::Rectangle {
        width: 60.,
        height: 60.,
        fill: [220, 90, 40, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut wire = object("wire", 0., 0.);
    wire.kind = ElementKind::Path {
        commands: vec![
            PathCommand::MoveTo { x: 100., y: 300. },
            PathCommand::LineTo { x: 200., y: 300. },
        ],
        fill_rule: FillRule::NonZero,
        fill: [0; 4],
        stroke: [20, 20, 20, 255],
        stroke_width: 1.,
    };
    editor.symbol_mut().timeline.layers = vec![layer("l", vec![lower, upper, wire], false)];
    editor.clear_selection();

    let click_at = |editor: &mut Editor, t: f64, stage_pt: egui::Vec2| -> Vec<String> {
        editor.clear_selection();
        let pos = page.min + stage_pt;
        drag(editor, &ctx, t, pos, pos, true, Modifiers::NONE);
        ids(editor).into_iter().map(str::to_owned).collect()
    };

    // 3.5 px below the 1 px stroke center (3.0 px outside stroke edge):
    editor.workspace.drawing.click_accuracy = Tolerance::Off;
    assert!(click_at(&mut editor, 1.0, egui::vec2(150., 303.5)).is_empty());

    editor.workspace.drawing.click_accuracy = Tolerance::MustBeClose;
    assert!(click_at(&mut editor, 1.2, egui::vec2(150., 303.5)).is_empty());
    assert_eq!(
        click_at(&mut editor, 1.4, egui::vec2(150., 301.2)),
        ["wire"]
    );

    editor.workspace.drawing.click_accuracy = Tolerance::Normal;
    assert_eq!(
        click_at(&mut editor, 1.6, egui::vec2(150., 303.5)),
        ["wire"]
    );
    assert!(click_at(&mut editor, 1.8, egui::vec2(150., 306.5)).is_empty());

    editor.workspace.drawing.click_accuracy = Tolerance::CanBeDistant;
    assert_eq!(
        click_at(&mut editor, 2.0, egui::vec2(150., 306.5)),
        ["wire"]
    );

    // Adjacent shapes: clicking at x = 158 (inside `lower`, 2 px from `upper` on top) must pick
    // `lower` by direct hit even when `CanBeDistant` (8 px) is active, while clicking empty space
    // at x = 226 (6 px right of `upper`) picks `upper` via slop.
    assert_eq!(
        click_at(&mut editor, 2.2, egui::vec2(158., 130.)),
        ["lower"]
    );
    assert_eq!(
        click_at(&mut editor, 2.4, egui::vec2(226., 130.)),
        ["upper"]
    );
}

#[test]
fn span_based_selection_clicks_spans_supports_drag_shift_command_and_single_frame_mode() {
    let (mut editor, ctx, _) = fixture(0);
    editor.symbol_mut().timeline.frame_count = 20;
    let top_layer = crate::layer_controls::visible_layer_indices(&editor.symbol().timeline)[0];
    // Untweened spans on top_layer: [0..=9] and [10..=19].
    editor.symbol_mut().timeline.layers[top_layer].keyframes[0].tween = None;
    editor.project.validate().unwrap();

    fn timeline_input(
        ctx: &egui::Context,
        editor: &mut Editor,
        time: f64,
        events: Vec<Event>,
        modifiers: Modifiers,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
                time: Some(time),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                editor.timeline(ctx);
                editor.stage(ctx);
            },
        )
    }
    timeline_input(&ctx, &mut editor, 0.1, vec![], Modifiers::NONE);
    let output = timeline_input(&ctx, &mut editor, 0.2, vec![], Modifiers::NONE);
    let key0 = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle)
                if (1.5..=3.5).contains(&circle.radius)
                    && circle.fill == neoflash_ui::palette(&ctx).timeline_marker
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
        .expect("timeline must paint the first authored key");
    let cell = neoflash_ui::metrics::FrameView::Normal.frame_width();
    let cell_pos = |frame: u32| key0 + egui::vec2(cell * frame as f32, 0.);
    let click_cell = |editor: &mut Editor, t: f64, frame: u32, mods: Modifiers| {
        let p = cell_pos(frame);
        timeline_input(
            &ctx,
            editor,
            t,
            vec![Event::PointerMoved(p), button(p, true, mods)],
            mods,
        );
        timeline_input(&ctx, editor, t + 0.05, vec![button(p, false, mods)], mods);
    };

    // Default span_selection = true: clicking in-between frame 3 selects span 0..=9.
    assert!(editor.workspace.timeline_prefs.span_selection);
    click_cell(&mut editor, 1.0, 3, Modifiers::NONE);
    assert_eq!(editor.frame, 3);
    assert_eq!(editor.selected_frame_range(), Some((0, 9, 1)));

    // Shift-clicking frame 14 extends to the union of both spans (0..=19).
    click_cell(&mut editor, 1.2, 14, Modifiers::SHIFT);
    assert_eq!(editor.selected_frame_range(), Some((0, 19, 1)));

    // Plain click without dragging inside the selected multi-span range narrows to clicked span 10..=19.
    click_cell(&mut editor, 1.4, 12, Modifiers::NONE);
    assert_eq!(editor.selected_frame_range(), Some((10, 19, 1)));

    // Command-click selects an individual frame even when span_selection is on.
    click_cell(&mut editor, 1.6, 13, Modifiers::COMMAND);
    assert_eq!(editor.selected_frame_range(), Some((13, 13, 1)));

    // Drag-to-select across in-between frames selects the exact dragged sub-range (2..=6).
    let p2 = cell_pos(2);
    let p6 = cell_pos(6);
    timeline_input(
        &ctx,
        &mut editor,
        1.8,
        vec![Event::PointerMoved(p2), button(p2, true, Modifiers::NONE)],
        Modifiers::NONE,
    );
    timeline_input(
        &ctx,
        &mut editor,
        1.85,
        vec![Event::PointerMoved(p6)],
        Modifiers::NONE,
    );
    timeline_input(
        &ctx,
        &mut editor,
        1.9,
        vec![Event::PointerMoved(p6), button(p6, false, Modifiers::NONE)],
        Modifiers::NONE,
    );
    assert_eq!(editor.selected_frame_range(), Some((2, 6, 1)));

    // span_selection = false: clicking in-between frame 3 selects only single cell 3..=3.
    editor.workspace.timeline_prefs.span_selection = false;
    click_cell(&mut editor, 2.1, 3, Modifiers::NONE);
    assert_eq!(editor.selected_frame_range(), Some((3, 3, 1)));
    click_cell(&mut editor, 2.3, 14, Modifiers::SHIFT);
    assert_eq!(editor.selected_frame_range(), Some((3, 14, 1)));
}

#[test]
fn auto_keyframe_off_edits_span_keyframe_keeps_motion_property_keys_and_f6_inserts() {
    let (mut editor, ctx, page) = fixture(5);
    for l in &mut editor.symbol_mut().timeline.layers {
        l.keyframes[0].tween = None;
    }
    editor.history.clear();
    editor.workspace.timeline_prefs.auto_keyframe = false;
    select(&mut editor, &["first"]);
    let before = editor.project.clone();
    let orig = sampled(&editor, "first").transform;
    let start = center(&editor, page, "first");
    let end = start + egui::vec2(45., 25.);

    // Dragging on in-between frame 5 with auto_keyframe = false edits keyframe 0 instead of splitting.
    input(
        &mut editor,
        &ctx,
        1.0,
        vec![Event::PointerMoved(start), primary_button(start, true)],
        Modifiers::NONE,
    );
    input(
        &mut editor,
        &ctx,
        1.1,
        vec![Event::PointerMoved(end), primary_button(end, false)],
        Modifiers::NONE,
    );
    let first_layer = &editor.symbol().timeline.layers[0];
    assert_eq!(
        first_layer
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 10],
        "auto_keyframe = false must not create a keyframe on transform"
    );
    let moved_k0 = first_layer.keyframes[0]
        .elements
        .iter()
        .find(|e| e.id == "first")
        .unwrap()
        .transform;
    assert_eq!((moved_k0.x, moved_k0.y), (orig.x + 45., orig.y + 25.));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());

    // F6 make_key is NOT gated by auto_keyframe = false.
    editor.layer = 0;
    editor.frame = 5;
    editor.make_key(false);
    assert_eq!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 5, 10]
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);

    // Inside a motion tween span, auto_keyframe = false still creates ◆ property keyframes.
    let (mut motion_ed, mctx, mpage) = rectangle_stage();
    motion_ed.symbol_mut().timeline.frame_count = 16;
    motion_ed.add_element(
        ElementKind::Text {
            text: "Hero".into(),
            font_size: 24.,
            color: [255, 255, 255, 255],
            font_family: String::new(),
        },
        120.,
        100.,
    );
    motion_ed.frame = 0;
    motion_ed.layer = 0;
    motion_ed.create_tween(TweenKind::Motion);
    motion_ed.history.clear();
    motion_ed.workspace.timeline_prefs.auto_keyframe = false;
    let before_motion = motion_ed.project.clone();
    motion_ed.frame = 6;
    motion_ed.set_tool(Tool::Select);
    let text_id = motion_ed.symbol().timeline.layers[0].keyframes[0].elements[0]
        .id
        .clone();
    motion_ed.select_single_id(text_id);
    stage_input_frame(&mut motion_ed, &mctx, 0.1, vec![]);
    let p0 = mpage.min + egui::vec2(130., 105.);
    let p1 = p0 + egui::vec2(60., 30.);
    stage_input_frame(
        &mut motion_ed,
        &mctx,
        0.2,
        vec![
            Event::PointerMoved(p0),
            primary_button(p0, true),
            Event::PointerMoved(p1),
            primary_button(p1, false),
        ],
    );
    let prop_key = motion_ed.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 6)
        .expect("motion span must still record a ◆ property keyframe");
    assert_eq!(
        prop_key.keyed_properties,
        Some(vec![TweenProperty::X, TweenProperty::Y])
    );
    // Motion span selection covers 0..=15 even across ◆ property key at frame 6.
    assert_eq!(motion_ed.layer_span_range(0, 6, 16), (0, 15));
    motion_ed.project.validate().unwrap();
    motion_ed.undo();
    assert_eq!(motion_ed.project, before_motion);
    assert!(!motion_ed.history.can_undo());
}

#[test]
fn tween_type_on_drag_creates_classic_or_motion_tween_in_one_undo_step() {
    use crate::workspace::TweenOnDrag;

    // 1. Classic tween on drag uses workspace.default_ease in a single Undo step.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.frame_count = 20;
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.,
            height: 50.,
            fill: [50, 130, 220, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        100.,
        100.,
    );
    let rect_id = editor.selected_element().unwrap().id.clone();
    editor.history.clear();
    editor.workspace.timeline_prefs.tween_on_drag = TweenOnDrag::Classic;
    editor.workspace.default_ease = Easing::Linear;
    let before_classic = editor.project.clone();

    editor.frame = 8;
    editor.set_tool(Tool::Select);
    editor.select_single_id(rect_id.clone());
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let p0 = page.min + egui::vec2(120., 120.);
    let p1 = p0 + egui::vec2(80., 40.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            Event::PointerMoved(p0),
            primary_button(p0, true),
            Event::PointerMoved(p1),
            primary_button(p1, false),
        ],
    );
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(
        layer.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 8]
    );
    assert_eq!(layer.keyframes[0].tween, Some(Easing::Linear));
    assert_eq!(layer.keyframes[0].tween_kind, TweenKind::Classic);
    assert_eq!(layer.keyframes[1].tween, None);
    let mid = sample_layer(layer, 4);
    assert!((mid[0].transform.x - 140.).abs() < 1e-6);
    assert!((mid[0].transform.y - 120.).abs() < 1e-6);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_classic);
    assert!(!editor.history.can_undo());

    // 2. Motion tween on drag creates a Motion tween on prev keyframe + ◆ property key in one Undo step.
    let (mut motion_ed, mctx, mpage) = rectangle_stage();
    motion_ed.symbol_mut().timeline.frame_count = 20;
    motion_ed.add_element(
        ElementKind::Text {
            text: "Fly".into(),
            font_size: 24.,
            color: [255, 255, 255, 255],
            font_family: String::new(),
        },
        100.,
        100.,
    );
    let text_id = motion_ed.selected_element().unwrap().id.clone();
    motion_ed.history.clear();
    motion_ed.workspace.timeline_prefs.tween_on_drag = TweenOnDrag::Motion;
    let before_motion = motion_ed.project.clone();

    motion_ed.frame = 10;
    motion_ed.set_tool(Tool::Select);
    motion_ed.select_single_id(text_id);
    stage_input_frame(&mut motion_ed, &mctx, 0.1, vec![]);
    let q0 = mpage.min + egui::vec2(110., 105.);
    let q1 = q0 + egui::vec2(100., 60.);
    stage_input_frame(
        &mut motion_ed,
        &mctx,
        0.2,
        vec![
            Event::PointerMoved(q0),
            primary_button(q0, true),
            Event::PointerMoved(q1),
            primary_button(q1, false),
        ],
    );
    let mlayer = &motion_ed.symbol().timeline.layers[0];
    assert_eq!(mlayer.keyframes[0].tween, Some(Easing::Linear));
    assert_eq!(mlayer.keyframes[0].tween_kind, TweenKind::Motion);
    assert_eq!(
        mlayer.keyframes[1].frame, 10,
        "drag frame becomes a ◆ property keyframe inside the new motion span"
    );
    assert_eq!(
        mlayer.keyframes[1].keyed_properties,
        Some(vec![TweenProperty::X, TweenProperty::Y])
    );
    assert!(motion_ed.project.format_version >= 6);
    motion_ed.project.validate().unwrap();
    motion_ed.undo();
    assert_eq!(motion_ed.project, before_motion);
    assert!(!motion_ed.history.can_undo());
}

#[test]
fn onion_opacity_falloff_is_linear_to_fifteen_percent() {
    use crate::stage::onion_ghost_opacity;

    let approx = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "{a} != {b}");
    approx(onion_ghost_opacity(70.0, 1, 3), 0.70);
    approx(onion_ghost_opacity(70.0, 2, 3), 0.425);
    approx(onion_ghost_opacity(70.0, 3, 3), 0.15);
    approx(onion_ghost_opacity(85.0, 1, 1), 0.85);

    let (mut editor, _, _) = fixture(6);
    editor.symbol_mut().timeline.frame_count = 15;
    editor.workspace.timeline_prefs.onion_falloff = 75.0;
    editor.onion_before = 4;
    editor.onion_after = 2;
    approx(editor.onion_frame_opacity(5), 0.75);
    approx(editor.onion_frame_opacity(4), 0.55);
    approx(editor.onion_frame_opacity(3), 0.35);
    approx(editor.onion_frame_opacity(2), 0.15);
    approx(editor.onion_frame_opacity(7), 0.75);
    approx(editor.onion_frame_opacity(8), 0.15);

    editor.onion_anchor = true;
    editor.anchored_range = (3, 9);
    approx(editor.onion_frame_opacity(5), 0.75);
    approx(editor.onion_frame_opacity(4), 0.45);
    approx(editor.onion_frame_opacity(3), 0.15);
    approx(editor.onion_frame_opacity(9), 0.15);
}

#[test]
fn alt_drag_duplicates_selection_and_alt_shift_constrains_axis_in_one_undo_step() {
    let (mut editor, ctx, page) = fixture(0);
    let before = editor.project.clone();
    let start = center(&editor, page, "first");
    let end = start + egui::vec2(55., 35.);

    drag(&mut editor, &ctx, 1.0, start, end, false, Modifiers::ALT);
    editor.project.validate().unwrap();
    assert_eq!(
        sampled(&editor, "first").transform.x,
        100.0,
        "original placement must stay untouched after Option-drag"
    );
    assert_eq!(sampled(&editor, "first").transform.y, 100.0);
    assert_eq!(editor.selection.len(), 1);
    let copy = editor.selected_element().unwrap().clone();
    assert_ne!(copy.id, "first");
    assert_eq!(copy.transform.x, 155.0);
    assert_eq!(copy.transform.y, 135.0);

    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());

    // Option + Shift constrains duplication to the dominant axis.
    let constrained_end = start + egui::vec2(60., 14.);
    drag(
        &mut editor,
        &ctx,
        2.0,
        start,
        constrained_end,
        true,
        Modifiers::ALT | Modifiers::SHIFT,
    );
    editor.project.validate().unwrap();
    let constrained_copy = editor.selected_element().unwrap();
    assert_ne!(constrained_copy.id, "first");
    assert_eq!(constrained_copy.transform.x, 160.0);
    assert_eq!(constrained_copy.transform.y, 100.0);
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn arrow_keys_nudge_selected_objects_by_1px_and_shift_arrow_by_10px() {
    let (mut editor, ctx, _page) = fixture(2);
    let press_key = |editor: &mut Editor, time: f64, key: egui::Key, modifiers: Modifiers| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280., 900.))),
                time: Some(time),
                modifiers,
                events: vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| {
                editor.stage(ctx);
                editor.shortcuts(ctx);
            },
        );
    };

    // With no selection, ArrowRight still steps the playhead.
    assert!(editor.selection.is_empty());
    press_key(&mut editor, 0.1, egui::Key::ArrowRight, Modifiers::NONE);
    assert_eq!(editor.frame, 3);

    // With stage selection and timeline unfocused, arrows nudge 1px and Shift+arrows 10px.
    editor.timeline_edit.focused = false;
    select(&mut editor, &["first"]);
    let before = editor.project.clone();
    let initial = sampled(&editor, "first").transform;
    press_key(&mut editor, 0.2, egui::Key::ArrowRight, Modifiers::NONE);
    assert_eq!(editor.frame, 3, "arrow nudge must not scrub playhead");
    assert_eq!(sampled(&editor, "first").transform.x, initial.x + 1.0);
    assert_eq!(sampled(&editor, "first").transform.y, initial.y);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);

    select(&mut editor, &["first"]);
    press_key(&mut editor, 0.3, egui::Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(sampled(&editor, "first").transform.x, initial.x);
    assert_eq!(sampled(&editor, "first").transform.y, initial.y + 10.0);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn break_apart_unpacks_groups_first_and_splits_text_into_positioned_characters() {
    let (mut editor, _ctx, _page) = fixture(0);
    // 1. Grouping two paths then pressing Break Apart unpacks the group on the first step.
    select(&mut editor, &["first", "untouched"]);
    editor.break_apart_selection(); // Rectangle -> Path
    select(&mut editor, &["first", "untouched"]);
    editor.group_selection();
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        1
    );
    let grouped_snapshot = editor.project.clone();
    editor.break_apart_selection();
    assert!(
        editor.symbol().timeline.layers[0]
            .properties
            .groups
            .is_empty(),
        "first Break Apart on a group must unpack the group"
    );
    assert_eq!(ids(&editor), ["first", "untouched"]);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, grouped_snapshot);

    // 2. TextBox Break Apart splits visible characters with horizontal & line offsets.
    editor.layer = 1;
    let tb_id = new_id("object");
    let format = TextFormat {
        size: 24.0,
        letter_spacing: 2.0,
        ..TextFormat::default()
    };
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .push(Element {
            id: tb_id.clone(),
            name: "headline".into(),
            transform: Transform {
                x: 50.0,
                y: 80.0,
                ..Transform::default()
            },
            kind: ElementKind::TextBox {
                text: "A B\nC".into(),
                width: 220.0,
                height: 90.0,
                format,
                word_wrap: false,
                multiline: true,
            },
            ..Element::default()
        });
    let before_text_break = editor.project.clone();
    select(&mut editor, &[tb_id.as_str()]);
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    assert_eq!(
        editor.selection.len(),
        3,
        "spaces/newlines are skipped; 3 letters remain"
    );
    let letters: Vec<Element> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, el)| el)
        .collect();
    let chars: Vec<&str> = letters
        .iter()
        .filter_map(|el| match &el.kind {
            ElementKind::TextBox { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chars, ["A", "B", "C"]);
    assert!((letters[0].transform.x - 50.0).abs() < 1e-6);
    assert!((letters[0].transform.y - 80.0).abs() < 1e-6);
    assert!(
        letters[1].transform.x > letters[0].transform.x + 10.0,
        "B must be shifted right past A and space"
    );
    assert!((letters[1].transform.y - letters[0].transform.y).abs() < 1e-6);
    assert!((letters[2].transform.x - 50.0).abs() < 1e-6);
    assert!(
        letters[2].transform.y > letters[0].transform.y + 20.0,
        "C on line 2 must be shifted down by line advance"
    );
    editor.undo();
    assert_eq!(editor.project, before_text_break);

    // 3. Point Text (`ElementKind::Text`) Break Apart splits letters horizontally.
    let pt_id = new_id("object");
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .push(Element {
            id: pt_id.clone(),
            name: String::new(),
            transform: Transform {
                x: 120.0,
                y: 40.0,
                ..Transform::default()
            },
            kind: ElementKind::Text {
                text: "NEO".into(),
                font_family: "Helvetica".into(),
                font_size: 20.0,
                color: [255, 255, 255, 255],
            },
            ..Element::default()
        });
    let before_pt = editor.project.clone();
    select(&mut editor, &[pt_id.as_str()]);
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    let pt_letters: Vec<Element> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, el)| el)
        .collect();
    assert_eq!(pt_letters.len(), 3);
    assert!(pt_letters[0].transform.x < pt_letters[1].transform.x);
    assert!(pt_letters[1].transform.x < pt_letters[2].transform.x);
    editor.undo();
    assert_eq!(editor.project, before_pt);
}

#[test]
fn merge_drawing_mode_punches_slices_splits_and_combines_objects() {
    let (mut editor, ctx, page) = fixture(0);
    editor.layer = 1; // clean empty layer `top`
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .clear();

    // 1. Default mode is Object Drawing (`object_drawing == true`): drawn shape stays 1 element.
    assert!(editor.workspace.drawing.object_drawing);
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.0,
            height: 60.0,
            fill: [40, 120, 220, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.0,
        },
        10.0,
        10.0,
    );
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .len(),
        1
    );
    assert!(!editor.symbol().timeline.layers[1].keyframes[0].elements[0].merge);
    editor.undo();

    // 2. Switch to Merge Drawing mode (`object_drawing = false`):
    // Drawing a stroked rectangle separates fill and stroke into 2 independent merge elements,
    // and double-clicking the fill selects both the fill and its adjacent stroke.
    editor.workspace.drawing.object_drawing = false;
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.0,
            height: 100.0,
            fill: [40, 120, 220, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.0,
        },
        0.0,
        0.0,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.project.format_version, 7);
    {
        let elements = &editor.symbol().timeline.layers[1].keyframes[0].elements;
        assert_eq!(elements.len(), 2);
        assert!(is_merge_fill(&elements[0]));
        assert!(is_merge_stroke(&elements[1]));
        let fill_id = elements[0].id.clone();
        editor.select_single_id(fill_id);
    }
    let single_fill_target = editor.selection.single().unwrap().clone();
    editor.expand_merge_double_click_selection(&single_fill_target);
    assert_eq!(
        editor.selection.len(),
        2,
        "double-clicking a Merge fill must select its adjacent stroke too"
    );

    // 3. Overlapping different-color fill in Merge mode subtracts from the underlying fill.
    editor.add_element(
        ElementKind::Rectangle {
            width: 20.0,
            height: 140.0,
            fill: [220, 50, 50, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        40.0,
        -20.0,
    );
    editor.project.validate().unwrap();
    {
        let elements = &editor.symbol().timeline.layers[1].keyframes[0].elements;
        let fills: Vec<&Element> = elements.iter().filter(|e| is_merge_fill(e)).collect();
        // Original blue fill split into left and right halves + new red fill = 3 fills.
        assert_eq!(fills.len(), 3);
    }

    // 4. Drawing a line across a Merge fill slices the fill into 2 independent fills.
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .clear();
    editor.add_element(
        ElementKind::Rectangle {
            width: 100.0,
            height: 100.0,
            fill: [50, 180, 90, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        0.0,
        0.0,
    );
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: -10.0, y: 50.0 },
                PathCommand::LineTo { x: 110.0, y: 50.0 },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        0.0,
        0.0,
    );
    editor.project.validate().unwrap();
    {
        let elements = &editor.symbol().timeline.layers[1].keyframes[0].elements;
        let fills: Vec<&Element> = elements.iter().filter(|e| is_merge_fill(e)).collect();
        let strokes: Vec<&Element> = elements.iter().filter(|e| is_merge_stroke(e)).collect();
        assert_eq!(
            fills.len(),
            2,
            "line across fill must slice it into 2 fills"
        );
        assert_eq!(
            strokes.len(),
            3,
            "line must split at the 2 fill boundary crossings"
        );
    }

    // 5. Modify ▸ Combine objects (Union & Punch) on Object Drawing shapes.
    editor.workspace.drawing.object_drawing = true;
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .clear();
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.0,
            height: 80.0,
            fill: [100, 100, 240, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        0.0,
        0.0,
    );
    let id_a = editor.symbol().timeline.layers[1].keyframes[0].elements[0]
        .id
        .clone();
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.0,
            height: 80.0,
            fill: [240, 100, 100, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        40.0,
        0.0,
    );
    let id_b = editor.symbol().timeline.layers[1].keyframes[0].elements[1]
        .id
        .clone();
    let layer_id = editor.symbol().timeline.layers[1].id.clone();
    editor.set_selection(vec![
        crate::selection::SelectionTarget {
            layer: layer_id.clone(),
            element: id_a,
        },
        crate::selection::SelectionTarget {
            layer: layer_id,
            element: id_b,
        },
    ]);
    assert!(editor.can_combine_objects());
    let before_combine = editor.project.clone();
    editor.combine_objects_selection(BooleanOp::Union, false);
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .len(),
        1
    );
    editor.undo();
    assert_eq!(editor.project, before_combine);

    // Grouped Merge artwork is isolated both from drawing and from dropping raw shapes onto it.
    editor.symbol_mut().timeline.layers = vec![layer("group-layer", vec![], false)];
    editor.symbol_mut().timeline.frame_count = 1;
    editor.layer = 0;
    editor.clear_selection();
    editor.history.clear();
    editor.workspace.drawing.object_drawing = false;
    let kind = ElementKind::Rectangle {
        width: 50.,
        height: 40.,
        fill: [40, 120, 220, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    editor.add_element(kind.clone(), 200., 100.);
    editor.add_element(kind, 300., 100.);
    editor.select_all();
    editor.group_selection();
    let grouped = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .clone();
    let before_cover = editor.project.clone();
    let cover = ElementKind::Rectangle {
        width: 200.,
        height: 80.,
        fill: [220, 40, 40, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    editor.add_element(cover.clone(), 180., 80.);
    editor.project.validate().unwrap();
    assert_eq!(
        &editor.symbol().timeline.layers[0].keyframes[0].elements[..2],
        grouped
    );
    editor.undo();
    assert_eq!(editor.project, before_cover);

    editor.add_element(cover, 500., 300.);
    let before_move = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 10., vec![]);
    drag(
        &mut editor,
        &ctx,
        11.,
        page.min + egui::vec2(600., 340.),
        page.min + egui::vec2(280., 120.),
        false,
        Modifiers::NONE,
    );
    editor.project.validate().unwrap();
    assert_ne!(
        editor.project, before_move,
        "the raw shape was actually moved"
    );
    assert_eq!(
        &editor.symbol().timeline.layers[0].keyframes[0].elements[..2],
        grouped
    );
    editor.undo();
    assert_eq!(editor.project, before_move);
}

#[test]
fn merge_selection_two_tone_hatch_covers_geometry_lasso_symbols_and_onion() {
    for theme in [
        crate::theme::ThemeMode::Dark,
        crate::theme::ThemeMode::Light,
    ] {
        merge_selection_hatch_paths_in_theme(theme);
    }
}

fn merge_selection_hatch_paths_in_theme(theme: crate::theme::ThemeMode) {
    fn meshes_from(ctx: &egui::Context, out: egui::FullOutput) -> Vec<egui::epaint::Mesh> {
        ctx.tessellate(out.shapes, out.pixels_per_point)
            .into_iter()
            .filter_map(|p| {
                if let egui::epaint::Primitive::Mesh(mesh) = p.primitive {
                    Some(mesh)
                } else {
                    None
                }
            })
            .collect()
    }
    fn hatch_vertices(meshes: &[egui::epaint::Mesh]) -> Vec<&egui::epaint::Vertex> {
        let tones = [
            egui::Color32::from_white_alpha(230),
            egui::Color32::from_black_alpha(191),
        ];
        let vertices: Vec<_> = meshes
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .filter(|vertex| tones.contains(&vertex.color))
            .collect();
        for tone in tones {
            assert!(
                vertices
                    .iter()
                    .filter(|vertex| vertex.color == tone)
                    .count()
                    >= 3,
                "selection output must retain both NF2 hatch tones: missing {tone:?}"
            );
        }
        vertices
    }
    fn hatch_signature(meshes: &[egui::epaint::Mesh]) -> Vec<(u32, u32, [u8; 4])> {
        let mut result: Vec<_> = hatch_vertices(meshes)
            .into_iter()
            .map(|vertex| {
                (
                    vertex.pos.x.to_bits(),
                    vertex.pos.y.to_bits(),
                    vertex.color.to_array(),
                )
            })
            .collect();
        result.sort_unstable();
        result.dedup();
        result
    }
    let ctx = egui::Context::default();
    let mut theme_editor = Editor::from_project(&ctx, Project::blank());
    theme_editor.set_theme(&ctx, theme);
    let rect_commands = |min: f64, max: f64| {
        vec![
            PathCommand::MoveTo { x: min, y: min },
            PathCommand::LineTo { x: max, y: min },
            PathCommand::LineTo { x: max, y: max },
            PathCommand::LineTo { x: min, y: max },
            PathCommand::Close,
        ]
    };
    let mut ring = rect_commands(0., 100.);
    ring.extend(rect_commands(30., 70.));
    let cases = [
        ElementKind::Ellipse {
            width: 100.,
            height: 60.,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ElementKind::Path {
            commands: ring,
            fill_rule: FillRule::EvenOdd,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 10., y: 10. },
                PathCommand::LineTo { x: 90., y: 90. },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 8.,
        },
    ];
    for (case, kind) in cases.into_iter().enumerate() {
        let item = DrawItem {
            element_id: "selected".into(),
            name: String::new(),
            instance_path: vec!["selected".into()],
            layer_id: "layer".into(),
            transform: transform_matrix(Transform {
                x: 35.,
                y: 20.,
                rotation: 24.,
                scale_x: 1.3,
                skew: 12.,
                ..Default::default()
            }),
            alpha: 1.,
            color_transform: Default::default(),
            kind,
        };
        let origin = egui::pos2(100., 100.);
        let zoom = 1.7;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.))),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let rect = crate::stage::world_bounds(&item, &painter);
                let screen_rect = Rect::from_min_max(
                    origin + rect.min.to_vec2() * zoom,
                    origin + rect.max.to_vec2() * zoom,
                );
                crate::stage_clipping::paint_shape_selection(
                    &painter,
                    &item,
                    origin,
                    zoom,
                    screen_rect,
                    None,
                )
                .unwrap();
            },
        );
        let meshes = meshes_from(&ctx, out);
        hatch_vertices(&meshes);
        assert!(
            meshes.iter().any(|mesh| !mesh.indices.is_empty()),
            "selection must remain visible"
        );
        for mesh in meshes {
            for indices in mesh.indices.as_chunks::<3>().0 {
                let tri = indices.map(|i| {
                    let stage = ((mesh.vertices[i as usize].pos - origin) / zoom).to_pos2();
                    crate::stage::inverse(item.transform, stage).unwrap()
                });
                for point in tri.into_iter().chain([((tri[0].to_vec2()
                    + tri[1].to_vec2()
                    + tri[2].to_vec2())
                    / 3.)
                    .to_pos2()])
                {
                    match case {
                        0 => assert!(
                            ((point.x - 50.) / 50.).powi(2) + ((point.y - 30.) / 30.).powi(2)
                                <= 1.01,
                            "hatch outside ellipse: {point:?}"
                        ),
                        1 => assert!(
                            !(point.x > 30.01
                                && point.x < 69.99
                                && point.y > 30.01
                                && point.y < 69.99),
                            "hatch fills a hole: {point:?}"
                        ),
                        _ => {
                            let t = ((point.x + point.y - 20.) / 160.).clamp(0., 1.);
                            assert!(
                                point.distance(egui::pos2(10. + 80. * t, 10. + 80. * t)) <= 4.01,
                                "hatch outside stroke: {point:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    // Ordinary freehand strokes must retain their hatch across the whole path, without exhausting
    // the mask-work budget. The point counts cover short and longer editing gestures, not timing.
    for count in [100, 300] {
        let commands = (0..count)
            .map(|i| {
                let x = i as f64 * 2.;
                let y = 60. + (i as f64 * 0.13).sin() * 40.;
                if i == 0 {
                    PathCommand::MoveTo { x, y }
                } else {
                    PathCommand::LineTo { x, y }
                }
            })
            .collect();
        let item = DrawItem {
            element_id: "long-stroke".into(),
            name: String::new(),
            instance_path: vec!["long-stroke".into()],
            layer_id: "layer".into(),
            transform: transform_matrix(Transform::default()),
            alpha: 1.,
            color_transform: Default::default(),
            kind: ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [0, 0, 0, 255],
                stroke_width: 12.,
            },
        };
        let origin = egui::pos2(30., 40.);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.))),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let rect = crate::stage::world_bounds(&item, &painter).translate(origin.to_vec2());
                crate::stage_clipping::paint_shape_selection(
                    &painter, &item, origin, 1., rect, None,
                )
                .unwrap_or_else(|error| panic!("{count}-point stroke selection: {error}"));
            },
        );
        let meshes = meshes_from(&ctx, out);
        hatch_vertices(&meshes);
        assert!(meshes.iter().any(|mesh| !mesh.indices.is_empty()));
        let vertices: Vec<_> = meshes.iter().flat_map(|mesh| &mesh.vertices).collect();
        assert!(vertices.iter().any(|v| v.pos.x < origin.x + 8.));
        assert!(
            vertices
                .iter()
                .any(|v| v.pos.x > origin.x + (count - 1) as f32 * 2. - 8.),
            "long-stroke hatch was truncated"
        );
    }

    // Exercise the real selection_stage lookup: its selected draw must carry the authored mask
    // IDs into hatch clipping. Calling only the geometry helper would miss that wiring.
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.set_theme(&ctx, theme);
    let mut selected = object("masked-selection", 20., 20.);
    selected.merge = true;
    selected.kind = ElementKind::Rectangle {
        width: 100.,
        height: 80.,
        fill: [0, 0, 0, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut artwork = layer("masked-layer", vec![selected], false);
    let mut mask_element = object("selection-mask-fill", 20., 20.);
    mask_element.kind = ElementKind::Rectangle {
        width: 30.,
        height: 80.,
        fill: [0, 0, 0, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut mask = layer("selection-mask", vec![mask_element], false);
    mask.properties.role = LayerRole::Mask;
    mask.locked = true;
    artwork.properties.mask = Some(mask.id.clone());
    editor.symbol_mut().timeline.layers = vec![artwork, mask];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.tool = Tool::Select;
    editor.zoom = 1.;
    editor.set_layer_masking(&ctx, true);
    editor.select_single_id("masked-selection".into());
    let authored = editor.presented_interactive_authored_scene(true);
    assert!(
        authored
            .draws
            .iter()
            .any(|draw| draw.item.element_id == "masked-selection" && !draw.clip_ids.is_empty())
    );
    let scene: Vec<_> = authored.draws.into_iter().map(|draw| draw.item).collect();
    let origin = egui::pos2(80., 90.);
    let view = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(view),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let response = ui.allocate_rect(view, egui::Sense::click_and_drag());
                    editor.selection_stage(ui, ui.painter(), origin, view, &scene, &response);
                });
        },
    );
    let meshes = meshes_from(&ctx, out);
    hatch_vertices(&meshes);
    assert!(
        meshes.iter().any(|mesh| !mesh.indices.is_empty()),
        "masked selection must stay visible"
    );
    let visible = Rect::from_min_max(
        origin + egui::vec2(20., 20.),
        origin + egui::vec2(50., 100.),
    )
    .expand(0.01);
    for vertex in meshes.iter().flat_map(|mesh| &mesh.vertices) {
        assert!(
            visible.contains(vertex.pos),
            "selection hatch escaped its authored mask: {:?}",
            vertex.pos
        );
    }

    // Lasso contact with only half of a Merge fill selects the existing whole element.
    // This is the implemented whole-object path, not partial-fill topology selection.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_theme(&ctx, theme);
    let mut selected = object("lasso-raw-fill", 100., 100.);
    selected.merge = true;
    selected.kind = ElementKind::Rectangle {
        width: 120.,
        height: 100.,
        fill: [40, 100, 180, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    editor.symbol_mut().timeline.layers = vec![layer("lasso-layer", vec![selected], false)];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.tool = Tool::Lasso;
    editor.clear_selection();
    editor.history.clear();
    editor.project.validate().unwrap();
    let before_lasso = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 1., vec![]);
    let polygon = [
        (80., 80.),
        (160., 80.),
        (160., 220.),
        (80., 220.),
        (80., 80.),
    ]
    .map(|(x, y)| page.min + egui::vec2(x, y));
    stage_input_frame(
        &mut editor,
        &ctx,
        2.,
        vec![
            Event::PointerMoved(polygon[0]),
            primary_button(polygon[0], true),
        ],
    );
    for (i, point) in polygon.into_iter().enumerate().skip(1) {
        stage_input_frame(
            &mut editor,
            &ctx,
            2. + i as f64 * 0.1,
            vec![Event::PointerMoved(point)],
        );
    }
    stage_input_frame(
        &mut editor,
        &ctx,
        3.,
        vec![primary_button(polygon[0], false)],
    );
    assert_eq!(ids(&editor), ["lasso-raw-fill"]);
    assert_eq!(
        editor.tool,
        Tool::Lasso,
        "Lasso remains active after selection"
    );
    let out = stage_input_frame(&mut editor, &ctx, 3.1, vec![]);
    let meshes = meshes_from(&ctx, out);
    let hatch = hatch_vertices(&meshes);
    let whole = Rect::from_min_max(
        page.min + egui::vec2(100., 100.),
        page.min + egui::vec2(220., 200.),
    );
    assert!(
        hatch
            .iter()
            .all(|vertex| whole.expand(0.01).contains(vertex.pos))
    );
    assert!(
        hatch.iter().any(|vertex| vertex.pos.x > page.min.x + 190.),
        "whole-object Lasso selection hatches the contacted fill beyond the contact polygon"
    );
    assert_eq!(
        editor.project, before_lasso,
        "Lasso selection does not cut or mutate artwork"
    );
    assert!(
        !editor.history.can_undo(),
        "selection alone creates no content Undo"
    );

    // Both sides of the current frame remain ghosts; only current-frame artwork gets the
    // full-strength selection hatch, even when ghost outlines are requested.
    editor.tool = Tool::Select;
    editor.frame = 1;
    editor.symbol_mut().timeline.frame_count = 3;
    let current = editor.symbol().timeline.layers[0].keyframes[0].elements[0].clone();
    editor.symbol_mut().timeline.layers[0].keyframes = (0..3)
        .map(|frame| {
            let mut element = current.clone();
            element.transform.x += (frame as f64 - 1.) * 200.;
            Keyframe {
                frame,
                elements: vec![element],
                ..Default::default()
            }
        })
        .collect();
    // Keep both ghosts within the page and disjoint from the selected fill.
    for key in &mut editor.symbol_mut().timeline.layers[0].keyframes {
        key.elements[0].transform.x += 220.;
    }
    editor.onion_before = 1;
    editor.onion_after = 1;
    editor.onion = false;
    editor.select_single_id("lasso-raw-fill".into());
    let before_onion = editor.project.clone();
    let out = stage_input_frame(&mut editor, &ctx, 4., vec![]);
    let without_ghosts = out.shapes.len();
    let signature = hatch_signature(&meshes_from(&ctx, out));
    for (i, outlines) in [false, true].into_iter().enumerate() {
        editor.onion = true;
        editor.onion_outlines = outlines;
        let out = stage_input_frame(&mut editor, &ctx, 5. + i as f64, vec![]);
        assert!(
            out.shapes.len() > without_ghosts,
            "onion ghosts must actually be painted"
        );
        let meshes = meshes_from(&ctx, out);
        assert_eq!(
            hatch_signature(&meshes),
            signature,
            "onion skin must neither tint the selected hatch nor hatch neighbouring frames"
        );
        assert_eq!(editor.project, before_onion);
    }
    editor.project.validate().unwrap();

    // Enter through the existing in-place navigation callback; the nested Stage uses the
    // registration point as its origin, rather than the root page's top-left corner.
    editor.onion = false;
    let mut child = editor.symbol().clone();
    child.id = "hatch-child".into();
    child.name = "Hatch child".into();
    child.timeline.frame_count = 1;
    let mut nested = object("nested-raw-fill", -60., -30.);
    nested.merge = true;
    child.timeline.layers = vec![layer("nested-layer", vec![nested], false)];
    let mut placement = object("hatch-instance", 200., 240.);
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers = vec![layer("instance-layer", vec![placement], false)];
    editor.project.symbols.push(child);
    editor.project.validate().unwrap();
    let before_nested = editor.project.clone();
    editor.edit_symbol_from_popover(&ctx, "hatch-child", true);
    assert_eq!(editor.editing.len(), 2);
    assert_eq!(editor.symbol_id(), "hatch-child");
    editor.select_single_id("nested-raw-fill".into());
    let out = stage_input_frame(&mut editor, &ctx, 8., vec![]);
    let meshes = meshes_from(&ctx, out);
    let nested_bounds = Rect::from_min_max(
        page.center() + egui::vec2(-60., -30.),
        page.center() + egui::vec2(0., 10.),
    )
    .expand(0.01);
    assert!(
        hatch_vertices(&meshes)
            .iter()
            .all(|vertex| nested_bounds.contains(vertex.pos)),
        "in-place symbol hatching must use the nested registration-point origin"
    );
    assert_eq!(editor.project, before_nested);
    assert!(!editor.history.can_undo());
}
