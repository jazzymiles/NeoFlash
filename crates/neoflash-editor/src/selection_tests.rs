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

#[test]
fn blank_stage_click_clears_frame_context_and_double_click_exits_nested_edit_to_scene() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Select);

    // A Timeline frame selection used to keep Properties in Frame mode even after the user
    // deliberately clicked empty Stage space. The blank press must expose Stage properties.
    let root_layer = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![root_layer],
        start: 0,
        end: 0,
    });
    assert!(editor.selected_frame_range().is_some());
    let root_blank = page.min + egui::vec2(30., 30.);
    input(
        &mut editor,
        &ctx,
        0.1,
        vec![
            Event::PointerMoved(root_blank),
            primary_button(root_blank, true),
        ],
        Modifiers::NONE,
    );
    input(
        &mut editor,
        &ctx,
        0.2,
        vec![primary_button(root_blank, false)],
        Modifiers::NONE,
    );
    assert!(editor.frame_range().is_none());
    assert!(!editor.timeline_edit.focused);

    // Build two real placement levels and enter through them. The root placement deliberately
    // remains selected in its saved view: before T-136 the release half of the exit double-click
    // saw that restored selection and immediately entered it again, even though the pointer was
    // over empty Stage space.
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .clear();
    let mut inner = editor.symbol().clone();
    inner.id = "double-click-inner".into();
    inner.name = "double-click-inner".into();
    inner.class_name = None;
    inner.linkage_id = None;
    inner.timeline.layers[0].id = "double-click-inner-layer".into();
    inner.timeline.layers[0].keyframes[0].elements = vec![object("double-click-inner-art", 0., 0.)];

    let mut inner_placement = object("double-click-inner-placement", 40., 40.);
    inner_placement.kind = ElementKind::Instance {
        symbol_id: inner.id.clone(),
        playback: InstancePlayback::Independent,
    };
    let mut outer = editor.symbol().clone();
    outer.id = "double-click-outer".into();
    outer.name = "double-click-outer".into();
    outer.class_name = None;
    outer.linkage_id = None;
    outer.timeline.layers[0].id = "double-click-outer-layer".into();
    outer.timeline.layers[0].keyframes[0].elements = vec![inner_placement];

    let mut outer_placement = object("double-click-outer-placement", 60., 60.);
    outer_placement.kind = ElementKind::Instance {
        symbol_id: outer.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![outer_placement];
    editor.project.symbols.extend([inner, outer]);
    editor.project.validate().unwrap();
    let before_navigation = editor.project.clone();
    editor.select_single_id("double-click-outer-placement".into());
    editor.enter_selected_symbol_in_place();
    editor.select_single_id("double-click-inner-placement".into());
    editor.enter_selected_symbol_in_place();
    assert_eq!(editor.edit_depth(), 3);
    editor.set_tool(Tool::PolyStar);

    let nested_blank = page.center();
    for (time, pressed) in [(2.0, true), (2.05, false)] {
        input(
            &mut editor,
            &ctx,
            time,
            if pressed {
                vec![
                    Event::PointerMoved(nested_blank),
                    primary_button(nested_blank, true),
                ]
            } else {
                vec![primary_button(nested_blank, false)]
            },
            Modifiers::NONE,
        );
    }
    assert_eq!(
        editor.edit_depth(),
        3,
        "one click keeps the nested edit open"
    );
    for (time, pressed) in [(2.15, true), (2.2, false)] {
        input(
            &mut editor,
            &ctx,
            time,
            if pressed {
                vec![
                    Event::PointerMoved(nested_blank),
                    primary_button(nested_blank, true),
                ]
            } else {
                vec![primary_button(nested_blank, false)]
            },
            Modifiers::NONE,
        );
    }
    assert!(editor.editing_scene());
    assert_eq!(editor.edit_depth(), 1);
    assert_eq!(ids(&editor), vec!["double-click-outer-placement"]);
    assert_eq!(
        editor.tool,
        Tool::PolyStar,
        "the sticky drawing tool is preserved"
    );
    assert_eq!(editor.project, before_navigation);
    assert!(!editor.history.can_undo());

    // A selected MovieClip alone is not enough to enter it: the double-click must actually hit
    // that placement. This prevents any later empty double-click from reopening a stale selection.
    editor.set_tool(Tool::Select);
    let empty_root = page.center();
    for (time, pressed) in [(3.0, true), (3.05, false), (3.15, true), (3.2, false)] {
        input(
            &mut editor,
            &ctx,
            time,
            if pressed {
                vec![
                    Event::PointerMoved(empty_root),
                    primary_button(empty_root, true),
                ]
            } else {
                vec![primary_button(empty_root, false)]
            },
            Modifiers::NONE,
        );
    }
    assert!(editor.editing_scene());

    // The ordinary affordance is preserved when the pointer really is on the instance artwork.
    let root_instance_art = page.min + egui::vec2(130., 120.);
    for (time, pressed) in [(4.0, true), (4.05, false), (4.15, true), (4.2, false)] {
        input(
            &mut editor,
            &ctx,
            time,
            if pressed {
                vec![
                    Event::PointerMoved(root_instance_art),
                    primary_button(root_instance_art, true),
                ]
            } else {
                vec![primary_button(root_instance_art, false)]
            },
            Modifiers::NONE,
        );
    }
    assert_eq!(editor.symbol_id(), "double-click-outer");
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
fn marquee_and_default_lasso_split_merge_fill_and_bezier_stroke_before_deletion() {
    let raw_path = |id: &str, commands: Vec<PathCommand>, fill, stroke, stroke_width| Element {
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
        ..Element::default()
    };
    let raw_elements = || {
        vec![
            raw_path(
                "raw-fill",
                vec![
                    PathCommand::MoveTo { x: 100., y: 100. },
                    PathCommand::LineTo { x: 220., y: 100. },
                    PathCommand::LineTo { x: 220., y: 200. },
                    PathCommand::LineTo { x: 100., y: 200. },
                    PathCommand::Close,
                ],
                [40, 100, 180, 255],
                [0; 4],
                0.,
            ),
            raw_path(
                "raw-curve",
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
                [20, 30, 40, 255],
                4.,
            ),
        ]
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

    // A Select-tool marquee cuts both raw topology classes and selects just the pieces inside.
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.layers = vec![layer("merge-layer", raw_elements(), false)];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.history.clear();
    let before = editor.project.clone();
    editor.set_tool(Tool::Select);
    drag(
        &mut editor,
        &ctx,
        1.,
        page.min + egui::vec2(135., 80.),
        page.min + egui::vec2(185., 290.),
        false,
        Modifiers::NONE,
    );
    assert!(editor.selection.targets.len() >= 2);
    assert_eq!(editor.history.undo_len(), 1);
    assert_ne!(editor.project, before);
    let split = editor.project.clone();
    editor.delete_selected();
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
    assert!(fill_at([115., 150.]) && fill_at([205., 150.]));
    assert!(
        !fill_at([160., 150.]),
        "Backspace removes only the marquee fill fragment"
    );
    assert!(elements.iter().any(is_merge_stroke));
    assert!(
        !stroke_at([160., 250.]),
        "Backspace removes only the marquee curve segment"
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, split);
    editor.undo();
    assert_eq!(editor.project, before);

    // Lasso uses the same partial-shape semantics by default, and Shift preserves the previous
    // object selection while adding the enclosed raw fragments.
    let (mut editor, ctx, page) = rectangle_stage();
    let mut elements = raw_elements();
    elements.push(object("prior", 400., 120.));
    editor.symbol_mut().timeline.layers = vec![layer("merge-layer", elements, false)];
    editor.project.format_version = editor.project.format_version.max(7);
    editor.project.validate().unwrap();
    editor.history.clear();
    editor.select_single_id("prior".into());
    let before = editor.project.clone();
    editor.set_tool(Tool::Lasso);
    assert!(editor.authoring_tools.lasso_partial_fills);
    let polygon = [
        (135., 80.),
        (185., 80.),
        (185., 290.),
        (135., 290.),
        (135., 80.),
    ]
    .map(|(x, y)| page.min + egui::vec2(x, y));
    input(
        &mut editor,
        &ctx,
        2.,
        vec![
            Event::PointerMoved(polygon[0]),
            button(polygon[0], true, Modifiers::SHIFT),
        ],
        Modifiers::SHIFT,
    );
    for (index, point) in polygon.into_iter().enumerate().skip(1) {
        input(
            &mut editor,
            &ctx,
            2. + index as f64 * 0.1,
            vec![Event::PointerMoved(point)],
            Modifiers::SHIFT,
        );
    }
    input(
        &mut editor,
        &ctx,
        3.,
        vec![button(polygon[0], false, Modifiers::SHIFT)],
        Modifiers::SHIFT,
    );
    assert!(
        editor
            .selection
            .targets
            .iter()
            .any(|target| target.element == "prior")
    );
    assert!(editor.selection.targets.len() >= 3);
    assert_eq!(editor.history.undo_len(), 1);
    assert_ne!(editor.project, before);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
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
fn frame_click_selects_one_by_default_and_span_selection_remains_explicit() {
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

    // Ordinary timeline contract: one click targets exactly one frame.
    assert!(!editor.workspace.timeline_prefs.span_selection);
    click_cell(&mut editor, 1.0, 3, Modifiers::NONE);
    assert_eq!(editor.frame, 3);
    assert_eq!(editor.selected_frame_range(), Some((3, 3, 1)));

    // Shift-clicking extends from the exact anchor frame.
    click_cell(&mut editor, 1.2, 14, Modifiers::SHIFT);
    assert_eq!(editor.selected_frame_range(), Some((3, 14, 1)));

    // A plain click inside the selected range narrows it back to one frame.
    click_cell(&mut editor, 1.4, 12, Modifiers::NONE);
    assert_eq!(editor.selected_frame_range(), Some((12, 12, 1)));

    // Drag-to-select across in-between frames selects the exact dragged sub-range (2..=6).
    let p2 = cell_pos(2);
    let p6 = cell_pos(6);
    timeline_input(
        &ctx,
        &mut editor,
        1.6,
        vec![Event::PointerMoved(p2), button(p2, true, Modifiers::NONE)],
        Modifiers::NONE,
    );
    timeline_input(
        &ctx,
        &mut editor,
        1.65,
        vec![Event::PointerMoved(p6)],
        Modifiers::NONE,
    );
    timeline_input(
        &ctx,
        &mut editor,
        1.7,
        vec![Event::PointerMoved(p6), button(p6, false, Modifiers::NONE)],
        Modifiers::NONE,
    );
    assert_eq!(editor.selected_frame_range(), Some((2, 6, 1)));

    // Span-based selection remains available only as an explicit preference.
    editor.workspace.timeline_prefs.span_selection = true;
    click_cell(&mut editor, 1.9, 3, Modifiers::NONE);
    assert_eq!(editor.selected_frame_range(), Some((0, 9, 1)));
    click_cell(&mut editor, 2.1, 14, Modifiers::SHIFT);
    assert_eq!(editor.selected_frame_range(), Some((0, 19, 1)));
    click_cell(&mut editor, 2.3, 12, Modifiers::NONE);
    assert_eq!(editor.selected_frame_range(), Some((10, 19, 1)));

    // Command-click keeps the existing single-cell override in opt-in span mode.
    click_cell(&mut editor, 2.5, 13, Modifiers::COMMAND);
    assert_eq!(editor.selected_frame_range(), Some((13, 13, 1)));
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
    let mut format = TextFormat {
        font: "Helvetica".into(),
        size: 24.0,
        color: 0x112233,
        letter_spacing: 2.0,
        ..TextFormat::default()
    };
    let mut large_green = format.clone();
    large_green.size = 32.0;
    large_green.bold = true;
    large_green.color = 0x55aa33;
    let mut small_blue = format.clone();
    small_blue.size = 18.0;
    small_blue.italic = true;
    small_blue.color = 0x3355aa;
    format.set_field_options(Some(AuthoredTextFieldOptions {
        format_runs: vec![
            TextFormatRun {
                start: 0,
                end: 2,
                format: format.clone(),
            },
            TextFormatRun {
                start: 2,
                end: 3,
                format: large_green,
            },
            TextFormatRun {
                start: 3,
                end: 4,
                format: format.clone(),
            },
            TextFormatRun {
                start: 4,
                end: 5,
                format: small_blue,
            },
        ],
        ..AuthoredTextFieldOptions::default()
    }));
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
    assert!((letters[1].transform.y - 80.0).abs() < 1e-6);
    assert!(
        letters[1].transform.x > letters[0].transform.x + 10.0,
        "B must be shifted right past A and space"
    );
    assert!(
        letters[1].transform.y < letters[0].transform.y,
        "the larger B run must retain its baseline-adjusted placement"
    );
    assert!((letters[2].transform.x - 50.0).abs() < 1e-6);
    assert!(
        letters[2].transform.y > letters[0].transform.y + 20.0,
        "C on line 2 must be shifted down by line advance"
    );
    let letter_formats: Vec<_> = letters
        .iter()
        .filter_map(|element| match &element.kind {
            ElementKind::TextBox { format, .. } => {
                Some((format.size, format.color, format.bold, format.italic))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        letter_formats,
        vec![
            (24.0, 0x112233, false, false),
            (32.0, 0x55aa33, true, false),
            (18.0, 0x3355aa, false, true),
        ],
        "Break Apart must preserve every rich-text character format"
    );
    let split_text = editor.project.clone();
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    let outlined_letters: Vec<Element> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, element)| element)
        .collect();
    assert_eq!(outlined_letters.len(), 3);
    assert!(outlined_letters.iter().all(|element| element.merge));
    let outline_fills: Vec<_> = outlined_letters
        .iter()
        .filter_map(|element| match element.kind {
            ElementKind::Path { fill, .. } => Some(fill),
            _ => None,
        })
        .collect();
    assert_eq!(
        outline_fills,
        vec![
            [0x11, 0x22, 0x33, 255],
            [0x55, 0xaa, 0x33, 255],
            [0x33, 0x55, 0xaa, 255],
        ],
        "the outline stage must keep rich-text paints separate"
    );
    assert!(outlined_letters.iter().any(|element| {
        matches!(&element.kind, ElementKind::Path { commands, .. } if commands.iter().any(|command| matches!(command, PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. })))
    }), "the second Break Apart must use real font curves, not rectangular placeholder glyphs");
    editor.undo();
    assert_eq!(editor.project, split_text);
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
    let split_point_text = editor.project.clone();
    editor.break_apart_selection();
    editor.project.validate().unwrap();
    let point_outlines: Vec<Element> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, element)| element)
        .collect();
    assert_eq!(point_outlines.len(), 3);
    assert!(point_outlines.iter().all(|element| element.merge));
    assert!(point_outlines.iter().all(|element| matches!(
        element.kind,
        ElementKind::Path {
            fill: [255, 255, 255, 255],
            stroke: [0, 0, 0, 0],
            ..
        }
    )));
    assert!(point_outlines.iter().any(|element| {
        matches!(&element.kind, ElementKind::Path { commands, .. } if commands.iter().any(|command| matches!(command, PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. })))
    }));
    editor.undo();
    assert_eq!(editor.project, split_point_text);
    editor.undo();
    assert_eq!(editor.project, before_pt);
}

#[test]
fn classic_break_group_shortcuts_work_with_properties_text_focus_and_one_undo() {
    let (mut editor, ctx, _page) = fixture(0);
    let mut field = "Properties text focus".to_owned();
    let mut press_with_text_focus =
        |editor: &mut Editor, time: f64, key: egui::Key, modifiers: Modifiers| {
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
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut field)
                                .id(egui::Id::new("t136-properties-text-focus")),
                        )
                        .request_focus();
                    });
                    assert!(
                        ctx.wants_keyboard_input(),
                        "the regression requires a live Properties-style text focus"
                    );
                    editor.shortcuts(ctx);
                },
            );
        };

    // Newly created text focuses Properties. The classic ⌘B object command must still run from
    // that state; use a primitive here so the assertion is independent of installed system fonts.
    select(&mut editor, &["first"]);
    let before_break = editor.project.clone();
    press_with_text_focus(&mut editor, 1.0, egui::Key::B, Modifiers::COMMAND);
    assert!(matches!(
        sampled(&editor, "first").kind,
        ElementKind::Path { .. }
    ));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_break);
    assert!(!editor.history.can_undo(), "Break Apart is one Undo step");

    // Group and Ungroup use their Flash shortcuts even if the previous Properties editor kept
    // focus. Grouping changes metadata only: transforms, z-order and the selected member IDs stay.
    select(&mut editor, &["first", "untouched"]);
    let before_group = editor.project.clone();
    let before_order: Vec<_> = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .map(|element| (element.id.clone(), element.transform))
        .collect();
    press_with_text_focus(&mut editor, 2.0, egui::Key::G, Modifiers::COMMAND);
    let groups = &editor.symbol().timeline.layers[0].properties.groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].members, ["first", "untouched"]);
    let group_id = groups[0].id.clone();
    assert_eq!(ids(&editor), ["first", "untouched"]);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .map(|element| (element.id.clone(), element.transform))
            .collect::<Vec<_>>(),
        before_order
    );
    editor.project.validate().unwrap();
    let grouped = editor.project.clone();

    // Grouping one already-selected group is a no-op: it must not churn its stable group ID or
    // create a redundant history entry.
    press_with_text_focus(&mut editor, 2.5, egui::Key::G, Modifiers::COMMAND);
    assert_eq!(editor.project, grouped);
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups[0].id,
        group_id
    );

    press_with_text_focus(
        &mut editor,
        3.0,
        egui::Key::G,
        Modifiers::SHIFT | Modifiers::COMMAND,
    );
    assert!(
        editor.symbol().timeline.layers[0]
            .properties
            .groups
            .is_empty()
    );
    assert_eq!(ids(&editor), ["first", "untouched"]);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .map(|element| (element.id.clone(), element.transform))
            .collect::<Vec<_>>(),
        before_order
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, grouped, "Ungroup is one Undo step");
    editor.undo();
    assert_eq!(editor.project, before_group, "Group is one Undo step");
}

#[test]
fn break_apart_single_character_text_goes_directly_to_exact_font_outlines() {
    let (mut editor, _ctx, _page) = fixture(0);
    editor.layer = 1;
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .clear();

    let cases = [
        Element {
            id: "single-point-text".into(),
            transform: Transform {
                x: 30.0,
                y: 40.0,
                ..Transform::default()
            },
            kind: ElementKind::Text {
                text: "S".into(),
                font_family: "Helvetica".into(),
                font_size: 36.0,
                color: [20, 80, 160, 192],
            },
            ..Element::default()
        },
        Element {
            id: "single-area-text".into(),
            transform: Transform {
                x: 100.0,
                y: 140.0,
                alpha: 0.5,
                ..Transform::default()
            },
            kind: ElementKind::TextBox {
                text: "S".into(),
                width: 100.0,
                height: 60.0,
                format: TextFormat {
                    font: "Helvetica".into(),
                    size: 36.0,
                    color: 0xc83250,
                    ..TextFormat::default()
                },
                multiline: false,
                word_wrap: false,
            },
            ..Element::default()
        },
    ];
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .extend(cases);
    let before = editor.project.clone();
    select(&mut editor, &["single-point-text", "single-area-text"]);
    editor.break_apart_selection();
    editor.project.validate().unwrap();

    let outlines: Vec<Element> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, element)| element)
        .collect();
    assert_eq!(outlines.len(), 2);
    assert!(outlines.iter().all(|element| element.merge));
    assert!(outlines.iter().all(|element| {
        matches!(&element.kind, ElementKind::Path { commands, .. }
            if commands.len() > 8
                && commands.iter().any(|command| matches!(command, PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. })))
    }));
    assert!(matches!(
        outlines[0].kind,
        ElementKind::Path {
            fill: [20, 80, 160, 192],
            ..
        }
    ));
    assert!(matches!(
        outlines[1].kind,
        ElementKind::Path {
            fill: [200, 50, 80, 255],
            ..
        }
    ));
    assert_eq!(outlines[1].transform.alpha, 0.5);

    editor.undo();
    assert_eq!(editor.project, before);
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

    // 5. Modify ▸ Combine objects (Union, Intersect, Punch, Crop) and downstream shape workflows.
    editor.workspace.drawing.object_drawing = true;
    editor.symbol_mut().timeline.layers[1].keyframes[0]
        .elements
        .clear();
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.0,
            height: 80.0,
            fill: [100, 100, 240, 255],
            stroke: [20, 30, 40, 255],
            stroke_width: 2.0,
        },
        100.0,
        50.0,
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
        140.0,
        50.0,
    );
    let id_b = editor.symbol().timeline.layers[1].keyframes[0].elements[1]
        .id
        .clone();
    let layer_id = editor.symbol().timeline.layers[1].id.clone();
    let both_targets = vec![
        crate::selection::SelectionTarget {
            layer: layer_id.clone(),
            element: id_a.clone(),
        },
        crate::selection::SelectionTarget {
            layer: layer_id.clone(),
            element: id_b,
        },
    ];
    for (op, crop_by_top, expected_x, expected_w) in [
        (BooleanOp::Union, false, 100.0, 120.0),
        (BooleanOp::Intersection, false, 140.0, 40.0),
        (BooleanOp::Difference, false, 100.0, 40.0),
        (BooleanOp::Intersection, true, 140.0, 40.0),
    ] {
        editor.set_selection(both_targets.clone());
        assert!(editor.can_combine_objects());
        let before_combine = editor.project.clone();
        editor.combine_objects_selection(op, crop_by_top);
        editor.project.validate().unwrap();
        assert_eq!(
            editor.symbol().timeline.layers[1].keyframes[0]
                .elements
                .len(),
            1
        );
        let combined_id = {
            let combined_el = &editor.symbol().timeline.layers[1].keyframes[0].elements[0];
            assert!((combined_el.transform.x - expected_x).abs() < 1e-6);
            assert!((combined_el.transform.y - 50.0).abs() < 1e-6);
            let ElementKind::Path {
                commands,
                fill,
                stroke,
                stroke_width,
                ..
            } = &combined_el.kind
            else {
                panic!("expected combined Path");
            };
            assert_eq!(*fill, [100, 100, 240, 255]);
            assert_eq!(*stroke, [20, 30, 40, 255]);
            assert!((*stroke_width - 2.0).abs() < 1e-6);
            let b = neoflash_core::path_bounds(commands).unwrap().unwrap();
            assert!(b.x_min.abs() < 1e-6 && b.y_min.abs() < 1e-6);
            assert!((b.width() - expected_w).abs() < 1e-6);
            combined_el.id.clone()
        };

        // Downstream edits on the combined shape: Subselect point deletion and Break Apart.
        let before_point_del = editor.project.clone();
        editor.set_tool(Tool::Subselect);
        editor.vector.selected_anchors = Some((combined_id, vec![1]));
        assert!(editor.delete_selected_vector_points());
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before_point_del);

        editor.set_tool(Tool::Select);
        editor.break_apart_selection();
        editor.project.validate().unwrap();
        assert_eq!(
            editor.symbol().timeline.layers[1].keyframes[0]
                .elements
                .len(),
            2,
            "Break Apart splits combined Object Drawing shape into Merge fill + stroke"
        );
        editor.undo();
        assert_eq!(editor.project, before_point_del);

        editor.undo();
        assert_eq!(editor.project, before_combine);
    }

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

    // 6. Dense variable-pressure Brush stroke in Merge mode dissolves internal dab overlaps on
    // commit and re-merges cleanly when dragged across existing Merge fills, with single-step Undo.
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .clear();
    editor.workspace.drawing.object_drawing = false;
    editor.add_element(
        ElementKind::Rectangle {
            width: 90.0,
            height: 90.0,
            fill: [80, 170, 230, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        220.0,
        110.0,
    );
    let points: Vec<Pos2> = (0..120)
        .map(|i| {
            egui::pos2(
                60.0 + i as f32 * 0.9,
                140.0 + (i as f32 * 0.08).sin() * 14.0,
            )
        })
        .collect();
    let pressures: Vec<f32> = (0..120)
        .map(|i| 0.35 + ((i as f32 * 0.11).sin().abs()) * 0.55)
        .collect();
    let brush_cmds = crate::authoring_tools::brush_outline_with_pressure(&points, &pressures, 16.0);
    let raw_move_tos = brush_cmds
        .iter()
        .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
        .count();
    assert_eq!(raw_move_tos, 239);
    let before_brush = editor.project.clone();
    editor.add_element(
        ElementKind::Path {
            commands: brush_cmds,
            fill_rule: FillRule::NonZero,
            fill: [212, 162, 249, 255],
            stroke: [212, 162, 249, 255],
            stroke_width: 0.0,
        },
        0.0,
        0.0,
    );
    editor.project.validate().unwrap();
    let brush_el = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .last()
        .unwrap();
    let ElementKind::Path { commands, .. } = &brush_el.kind else {
        panic!("expected dissolved Merge brush Path");
    };
    let dissolved_move_tos = commands
        .iter()
        .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
        .count();
    assert_eq!(
        dissolved_move_tos, 1,
        "239 overlapping brush dabs/quads must dissolve into 1 clean boundary contour"
    );

    let before_brush_move = editor.project.clone();
    stage_input_frame(&mut editor, &ctx, 12.0, vec![]);
    drag(
        &mut editor,
        &ctx,
        13.0,
        page.min + egui::vec2(100.0, 145.0),
        page.min + egui::vec2(210.0, 145.0),
        false,
        Modifiers::NONE,
    );
    editor.project.validate().unwrap();
    assert_ne!(editor.project, before_brush_move);
    editor.undo();
    assert_eq!(editor.project, before_brush_move);
    editor.undo();
    assert_eq!(editor.project, before_brush);
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
            path_fingerprint: None,
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
            path_fingerprint: None,
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
                    editor.selection_stage(
                        ui,
                        ui.painter(),
                        crate::stage_mapping::StageMapping::identity(),
                        origin,
                        &scene,
                        &response,
                    );
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

    // Turning off partial geometry preserves the explicit whole-object Lasso option used by this
    // hatch rendering check. The default split-and-select behavior is covered above.
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
    editor.authoring_tools.lasso_partial_fills = false;
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
        "whole-object selection alone creates no content Undo"
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

    // Enter through the in-place navigation callback. The child stays under the exact affine
    // placement instead of jumping to the old isolated registration-point view.
    editor.onion = false;
    let mut child = editor.symbol().clone();
    child.id = "hatch-child".into();
    child.name = "Hatch child".into();
    child.timeline.frame_count = 1;
    let mut nested = object("nested-raw-fill", -60., -30.);
    nested.merge = true;
    child.timeline.layers = vec![layer("nested-layer", vec![nested], false)];
    let placement_transform = Transform {
        x: 200.,
        y: 240.,
        scale_x: 1.4,
        scale_y: 0.8,
        rotation: 23.,
        skew: -11.,
        ..Default::default()
    };
    let mut placement = object("hatch-instance", 0., 0.);
    placement.transform = placement_transform;
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers = vec![layer("instance-layer", vec![placement], false)];
    editor.project.symbols.push(child);
    editor.project.format_version = editor.project.format_version.max(5);
    editor.project.validate().unwrap();
    let before_nested = editor.project.clone();
    editor.edit_symbol_from_popover(&ctx, "hatch-child", true);
    assert_eq!(editor.edit_depth(), 2);
    assert!(editor.editing_in_place());
    assert_eq!(editor.symbol_id(), "hatch-child");
    editor.select_single_id("nested-raw-fill".into());
    let out = stage_input_frame(&mut editor, &ctx, 8., vec![]);
    let meshes = meshes_from(&ctx, out);
    let nested_bounds =
        crate::stage_mapping::StageMapping::try_new(transform_matrix(placement_transform))
            .unwrap()
            .map_rect_to_screen(
                page.min,
                editor.zoom,
                Rect::from_min_max(egui::pos2(-60., -30.), egui::pos2(0., 10.)),
            )
            .expand(0.01);
    assert!(
        hatch_vertices(&meshes)
            .iter()
            .all(|vertex| nested_bounds.contains(vertex.pos)),
        "in-place symbol hatching must follow the selected instance's complete affine transform"
    );
    assert_eq!(editor.project, before_nested);
    assert!(!editor.history.can_undo());
}

#[test]
fn affine_in_place_selection_moves_child_locally_and_parent_stays_noninteractive() {
    let (mut editor, ctx, page) = rectangle_stage();
    let mut child = editor.symbol().clone();
    child.id = "affine-edit-child".into();
    child.name = "Affine edit child".into();
    child.timeline.frame_count = 1;
    child.timeline.layers = vec![layer(
        "affine-child-layer",
        vec![object("affine-child-shape", 10., 20.)],
        false,
    )];

    let placement_transform = Transform {
        x: 250.,
        y: 180.,
        scale_x: 1.5,
        scale_y: 0.7,
        rotation: 31.,
        skew: -14.,
        ..Default::default()
    };
    let mut placement = object("affine-instance", 0., 0.);
    placement.transform = placement_transform;
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    let mut parent_only = object("parent-only", 650., 100.);
    if let ElementKind::Rectangle { fill, .. } = &mut parent_only.kind {
        *fill = [217, 83, 41, 255];
    }
    editor.symbol_mut().timeline.layers = vec![layer(
        "affine-parent-layer",
        vec![placement, parent_only],
        false,
    )];
    editor.project.symbols.push(child);
    editor.project.format_version = editor.project.format_version.max(5);
    editor.project.validate().unwrap();
    editor.set_selection(vec![editor.target_for_id("affine-instance").unwrap()]);
    assert!(editor.enter_selected_symbol_in_place());
    editor.tool = Tool::Select;
    editor.history.clear();

    let painted = stage_input_frame(&mut editor, &ctx, 19., vec![]);
    let painted: Vec<_> = ctx
        .tessellate(painted.shapes, painted.pixels_per_point)
        .into_iter()
        .filter_map(|primitive| match primitive.primitive {
            egui::epaint::Primitive::Mesh(mesh) => Some(mesh),
            _ => None,
        })
        .collect();
    let parent_bounds =
        Rect::from_min_size(page.min + egui::vec2(650., 100.), egui::vec2(60., 40.)).expand(0.01);
    assert!(
        painted
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .any(|vertex| parent_bounds.contains(vertex.pos)
                && (190..=192).contains(&vertex.color.a())),
        "parent-only artwork must be visibly dimmed while the active child stays separate"
    );

    let mapping =
        crate::stage_mapping::StageMapping::try_new(transform_matrix(placement_transform)).unwrap();
    // The visible parent sibling is context only: clicking it cannot select or mutate it.
    let before_parent_click = editor.project.clone();
    let parent_screen = page.min + egui::vec2(680., 120.);
    input(
        &mut editor,
        &ctx,
        20.,
        vec![
            Event::PointerMoved(parent_screen),
            primary_button(parent_screen, true),
            primary_button(parent_screen, false),
        ],
        Modifiers::NONE,
    );
    assert!(editor.selection.is_empty());
    assert_eq!(editor.project, before_parent_click);

    editor.workspace.drawing.click_accuracy = crate::workspace::Tolerance::CanBeDistant;
    let [a, b, ..] = mapping.matrix();
    let local_per_screen_point = 1. / (a.hypot(b) as f32 * editor.zoom);
    let edge = Pos2::new(70., 40.);
    let near = mapping.edit_to_screen(
        page.min,
        editor.zoom,
        edge + egui::vec2(6. * local_per_screen_point, 0.),
    );
    drag(&mut editor, &ctx, 20.2, near, near, true, Modifiers::NONE);
    assert_eq!(ids(&editor), ["affine-child-shape"]);
    editor.clear_selection();
    let far = mapping.edit_to_screen(
        page.min,
        editor.zoom,
        edge + egui::vec2(10. * local_per_screen_point, 0.),
    );
    drag(&mut editor, &ctx, 20.4, far, far, true, Modifiers::NONE);
    assert!(
        editor.selection.is_empty(),
        "click accuracy must remain an eight-screen-point tolerance after affine mapping"
    );

    let local_start = egui::pos2(40., 40.);
    let local_end = local_start + egui::vec2(15., -10.);
    let start = mapping.edit_to_screen(page.min, editor.zoom, local_start);
    let end = mapping.edit_to_screen(page.min, editor.zoom, local_end);
    let before_move = editor.project.clone();
    drag(&mut editor, &ctx, 21., start, end, false, Modifiers::NONE);
    assert_eq!(ids(&editor), ["affine-child-shape"]);
    let moved = sampled(&editor, "affine-child-shape");
    assert!(
        (moved.transform.x - 25.).abs() < 1e-4,
        "unexpected local x: {}",
        moved.transform.x
    );
    assert!(
        (moved.transform.y - 10.).abs() < 1e-4,
        "unexpected local y: {}",
        moved.transform.y
    );
    assert!(editor.history.can_undo());
    editor.undo();
    assert_eq!(editor.project, before_move);

    editor.authoring_modify.to_stage = true;
    editor.align_selection(&ctx, 0, 0);
    let aligned = sampled(&editor, "affine-child-shape");
    let local_bounds = Rect::from_min_size(
        Pos2::new(aligned.transform.x as f32, aligned.transform.y as f32),
        egui::vec2(60., 40.),
    );
    let context_bounds = mapping.map_rect(local_bounds);
    assert!(
        context_bounds.left().abs() < 1e-3,
        "Align Left to Stage must move along the context X axis: {context_bounds:?}"
    );
    assert!(
        aligned.transform.x != 10. && aligned.transform.y != 20.,
        "an oblique parent requires a two-axis local correction"
    );
    editor.undo();
    assert_eq!(editor.project, before_move);
    editor.project.validate().unwrap();
}

#[test]
fn selection_chrome_composes_affine_mapping_before_taking_axis_aligned_bounds() {
    let (mut editor, ctx, _) = rectangle_stage();
    let mut element = object("bounds-cancel-affine", 135., 90.);
    element.transform.scale_x = 1.7;
    element.transform.scale_y = 0.6;
    element.transform.rotation = 37.;
    element.transform.skew = -19.;
    editor.symbol_mut().timeline.layers = vec![layer("bounds-cancel-layer", vec![element], false)];
    select(&mut editor, &["bounds-cancel-affine"]);

    let scene = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    let painter = ctx.debug_painter();
    let mapping = crate::stage_mapping::StageMapping::try_new(
        invert_affine_matrix(scene[0].transform).unwrap(),
    )
    .unwrap();
    let exact = editor
        .selection_context_bounds(&scene, &painter, mapping)
        .unwrap();
    assert!((exact.min.x - 0.).abs() < 1e-3, "{exact:?}");
    assert!((exact.min.y - 0.).abs() < 1e-3, "{exact:?}");
    assert!((exact.width() - 60.).abs() < 1e-3, "{exact:?}");
    assert!((exact.height() - 40.).abs() < 1e-3, "{exact:?}");

    let local_aabb = crate::stage::world_bounds(&scene[0], &painter);
    let aabb_of_aabb = mapping.map_rect(local_aabb);
    assert!(
        aabb_of_aabb.width() > exact.width() + 20. || aabb_of_aabb.height() > exact.height() + 20.,
        "the fixture must distinguish transform-then-bounds from AABB-of-AABB: {aabb_of_aabb:?} vs {exact:?}"
    );
}

#[test]
fn affine_transform_box_uses_context_opposite_corner_and_context_skew_axes() {
    let (mut editor, ctx, page) = rectangle_stage();
    let mut child = editor.symbol().clone();
    child.id = "context-transform-child".into();
    child.name = "Context transform child".into();
    child.timeline.frame_count = 1;
    child.timeline.layers = vec![layer(
        "context-transform-layer",
        vec![
            object("context-transform-a", 0., 0.),
            object("context-transform-b", 100., 100.),
        ],
        false,
    )];

    let placement_transform = Transform {
        x: 500.,
        y: 200.,
        scale_x: 1.5,
        scale_y: 0.75,
        rotation: 90.,
        ..Default::default()
    };
    let mut placement = object("context-transform-instance", 0., 0.);
    placement.transform = placement_transform;
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers =
        vec![layer("context-transform-parent", vec![placement], false)];
    editor.project.symbols.push(child);
    editor.project.format_version = editor.project.format_version.max(5);
    editor.project.validate().unwrap();
    editor.set_selection(vec![
        editor.target_for_id("context-transform-instance").unwrap(),
    ]);
    assert!(editor.enter_selected_symbol_in_place());
    select(&mut editor, &["context-transform-a", "context-transform-b"]);
    editor.set_tool(Tool::Transform);
    editor.history.clear();

    let mapping =
        crate::stage_mapping::StageMapping::try_new(transform_matrix(placement_transform)).unwrap();
    let context_bounds = |editor: &Editor| {
        let scene = evaluate(&editor.project, editor.symbol_id(), editor.frame);
        editor
            .selection_context_bounds(&scene, &ctx.debug_painter(), mapping)
            .unwrap()
    };
    let before = editor.project.clone();
    let initial = context_bounds(&editor);
    stage_input_frame(&mut editor, &ctx, 23., vec![]);

    // The north-west screen handle is the inverse-mapped south-west local corner after this
    // quarter turn. Scaling must keep the screen-opposite south-east corner fixed.
    let fixed = page.min + initial.right_bottom().to_vec2() * editor.zoom;
    let start = page.min + initial.left_top().to_vec2() * editor.zoom;
    let end = fixed + (start - fixed) * 1.25;
    drag(&mut editor, &ctx, 23.2, start, end, false, Modifiers::NONE);
    let scaled = context_bounds(&editor);
    assert!(scaled.right_bottom().distance(initial.right_bottom()) < 1e-3);
    let expected_min = initial.right_bottom() + (initial.min - initial.right_bottom()) * 1.25;
    assert!(scaled.min.distance(expected_min) < 1e-3, "{scaled:?}");
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);

    // A top screen edge drag is an X shear in the parent context, not an X shear in the
    // quarter-turned child's edit axes. Verify the complete conjugated affine matrices.
    stage_input_frame(&mut editor, &ctx, 24., vec![]);
    let original_transforms: Vec<_> = ["context-transform-a", "context-transform-b"]
        .map(|id| sampled(&editor, id).transform)
        .into_iter()
        .collect();
    let start_context = Pos2::new(initial.left() + initial.width() * 0.25, initial.top());
    let start = page.min + start_context.to_vec2() * editor.zoom;
    let displacement = 30.;
    let end = start + egui::vec2(displacement * editor.zoom, 0.);
    drag(&mut editor, &ctx, 24.2, start, end, false, Modifiers::NONE);
    let edge = f64::from(initial.top());
    let anchor = f64::from(initial.bottom());
    let k = f64::from(displacement) / (edge - anchor);
    let shear = [1., 0., k, 1., -k * anchor, 0.];
    for (id, original) in ["context-transform-a", "context-transform-b"]
        .into_iter()
        .zip(original_transforms)
    {
        let expected = affine_multiply(
            shear,
            affine_multiply(mapping.matrix(), transform_matrix(original)),
        );
        let actual = affine_multiply(
            mapping.matrix(),
            transform_matrix(sampled(&editor, id).transform),
        );
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
        }
    }
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
}

#[test]
fn affine_in_place_shape_creation_uses_screen_size_not_tiny_local_bounds() {
    let (mut editor, ctx, page) = rectangle_stage();
    let mut child = editor.symbol().clone();
    child.id = "scaled-drawing-child".into();
    child.name = "Scaled drawing child".into();
    child.timeline.frame_count = 1;
    child.timeline.layers = vec![layer("scaled-drawing-layer", vec![], false)];

    let placement_transform = Transform {
        x: 50.,
        y: 50.,
        scale_x: 100.,
        scale_y: 1.,
        ..Default::default()
    };
    let mut placement = object("scaled-drawing-instance", 0., 0.);
    placement.transform = placement_transform;
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers =
        vec![layer("scaled-drawing-parent", vec![placement], false)];
    editor.project.symbols.push(child);
    editor.project.validate().unwrap();
    editor.set_selection(vec![
        editor.target_for_id("scaled-drawing-instance").unwrap(),
    ]);
    assert!(editor.enter_selected_symbol_in_place());
    editor.set_tool(Tool::Rectangle);

    let mapping =
        crate::stage_mapping::StageMapping::try_new(transform_matrix(placement_transform)).unwrap();
    let start = mapping.edit_to_screen(page.min, editor.zoom, Pos2::new(1., 1.));
    let end = mapping.edit_to_screen(page.min, editor.zoom, Pos2::new(2., 21.));
    drag(&mut editor, &ctx, 22., start, end, false, Modifiers::NONE);

    let created = editor
        .selected_element()
        .expect("a 100 by 20 screen-point drag creates artwork");
    let ElementKind::Rectangle { width, height, .. } = created.kind else {
        panic!("expected a rectangle")
    };
    assert!((width - 1.).abs() < 1e-6, "unexpected local width: {width}");
    assert!(
        (height - 20.).abs() < 1e-6,
        "unexpected local height: {height}"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
}

#[test]
fn oblique_shape_and_text_drags_require_both_mapped_local_edges_to_exceed_minimum() {
    let (mut editor, ctx, page) = rectangle_stage();
    let mut child = editor.symbol().clone();
    child.id = "oblique-minimum-child".into();
    child.name = "Oblique minimum child".into();
    child.timeline.frame_count = 1;
    child.timeline.layers = vec![layer("oblique-minimum-layer", vec![], false)];

    let placement_transform = Transform {
        x: 320.,
        y: 100.,
        scale_x: 1.25,
        scale_y: 0.8,
        rotation: 45.,
        skew: 18.,
        ..Default::default()
    };
    let mut placement = object("oblique-minimum-instance", 0., 0.);
    placement.transform = placement_transform;
    placement.kind = ElementKind::Instance {
        symbol_id: child.id.clone(),
        playback: InstancePlayback::Independent,
    };
    editor.symbol_mut().timeline.layers =
        vec![layer("oblique-minimum-parent", vec![placement], false)];
    editor.project.symbols.push(child);
    editor.project.format_version = editor.project.format_version.max(5);
    editor.project.validate().unwrap();
    editor.set_selection(vec![
        editor.target_for_id("oblique-minimum-instance").unwrap(),
    ]);
    assert!(editor.enter_selected_symbol_in_place());
    let mapping =
        crate::stage_mapping::StageMapping::try_new(transform_matrix(placement_transform)).unwrap();

    editor.set_tool(Tool::Rectangle);
    let zero_width_start = Pos2::new(20., 20.);
    let zero_width_end = Pos2::new(20., 80.);
    let zoom = editor.zoom;
    let zero_width_start = mapping.edit_to_screen(page.min, zoom, zero_width_start);
    let zero_width_end = mapping.edit_to_screen(page.min, zoom, zero_width_end);
    drag(
        &mut editor,
        &ctx,
        25.,
        zero_width_start,
        zero_width_end,
        false,
        Modifiers::NONE,
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty(),
        "an oblique zero-width local drag must not pass via its non-zero screen AABB"
    );

    editor.set_tool(Tool::Text);
    let [a, b, ..] = mapping.matrix();
    let narrow_width = 1.5 / (a.hypot(b) * f64::from(editor.zoom));
    let narrow_start = Pos2::new(40., 20.);
    let narrow_end = Pos2::new(40. + narrow_width as f32, 80.);
    let narrow_start = mapping.edit_to_screen(page.min, zoom, narrow_start);
    let narrow_end = mapping.edit_to_screen(page.min, zoom, narrow_end);
    drag(
        &mut editor,
        &ctx,
        25.4,
        narrow_start,
        narrow_end,
        false,
        Modifiers::NONE,
    );
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty(),
        "a long oblique text drag with a 1.5-point local edge must not create a text box"
    );
    assert_eq!(editor.history.undo_len(), 0);
    editor.project.validate().unwrap();
}

#[test]
fn pasteboard_objects_render_full_artwork_and_support_authoring_tools_outside_page() {
    let (mut editor, ctx, page) = rectangle_stage();
    let fill = [220, 60, 40, 255];
    let fill_color = egui::Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]);
    let mut offstage = object("pasteboard-rect", -110., 120.);
    offstage.kind = ElementKind::Rectangle {
        width: 80.,
        height: 50.,
        fill,
        stroke: [15, 25, 35, 255],
        stroke_width: 3.,
    };
    editor.symbol_mut().timeline.layers = vec![layer("pasteboard-layer", vec![offstage], false)];
    editor.clear_selection();
    editor.history.clear();
    editor.project.validate().unwrap();

    let out = stage_input_frame(&mut editor, &ctx, 1., vec![]);
    let canvas = editor.stage_view.canvas;
    let expected_center = page.min + egui::vec2(-70., 145.);
    assert!(
        canvas.contains(expected_center) && !page.contains(expected_center),
        "test point must lie on the visible pasteboard outside the stage page"
    );
    let painted_offstage_fill = out.shapes.iter().any(|clipped| {
        clipped.clip_rect.contains(expected_center)
            && match &clipped.shape {
                egui::Shape::Mesh(mesh) => mesh
                    .vertices
                    .iter()
                    .any(|v| v.color == fill_color && !page.contains(v.pos)),
                egui::Shape::Rect(rect) => {
                    rect.fill == fill_color && rect.rect.contains(expected_center)
                }
                _ => false,
            }
    });
    assert!(
        painted_offstage_fill,
        "pasteboard object must render its filled artwork outside page, not only a selection contour"
    );

    // Paint Bucket works on pasteboard objects outside `page` and records one Undo step.
    let before_bucket = editor.project.clone();
    editor.fill = [40, 190, 110, 255];
    editor.set_tool(Tool::Bucket);
    crate::tests::pen_click(&mut editor, &ctx, 2., expected_center, expected_center);
    assert!(matches!(
        editor.selected_element().unwrap().kind,
        ElementKind::Rectangle {
            fill: [40, 190, 110, 255],
            ..
        }
    ));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_bucket);
    assert!(!editor.history.can_undo());

    // Drawing a new rectangle directly on the right pasteboard (outside `page`, inside `canvas`) works.
    editor.set_tool(Tool::Rectangle);
    let drag_start = egui::pos2(page.max.x + 25., page.min.y + 80.);
    let drag_end = egui::pos2(page.max.x + 95., page.min.y + 130.);
    assert!(canvas.contains(drag_start) && canvas.contains(drag_end));
    assert!(!page.contains(drag_start) && !page.contains(drag_end));
    crate::tests::pen_click(&mut editor, &ctx, 3., drag_start, drag_end);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        2
    );
    let created = editor.selected_element().unwrap();
    assert_eq!(
        (created.transform.x, created.transform.y),
        (editor.project.width as f64 + 25., 80.)
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before_bucket);
}

#[test]
#[ignore]
fn t093_extreme_multi_brush_and_move_benchmark() {
    use crate::selection::SelectionTarget;
    use neoflash_core::merge_drawing::{
        apply_merge_shape_to_keyframe, remerge_moved_elements_in_keyframe,
    };
    use neoflash_ui::components::BrushShapeKind;

    let strokes_per_pass: usize = std::env::var("T093_STROKES_PER_PASS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let log_every: usize = std::env::var("T093_LOG_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or((strokes_per_pass / 4).max(1));

    let mut rng_state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next_f32 = || -> f32 {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        ((rng_state >> 40) as u32 as f32) / ((1u32 << 24) as f32)
    };

    // Spread strokes across local hatching patches over a stage canvas so strokes overlap locally
    // (like real shading/cross-hatching) as well as across passes.
    let grid_side = ((strokes_per_pass as f32 / 18.0).sqrt().ceil() as usize).max(8);
    let cell_step = 140.0_f32;

    let passes: [(&str, BrushShapeKind, f32, [u8; 4]); 3] = [
        (
            "Pass 1 (Round 6px · Blue)",
            BrushShapeKind::ALL[0],
            6.0,
            [34, 116, 165, 255],
        ),
        (
            "Pass 2 (Tip #2 8px · Orange)",
            BrushShapeKind::ALL[1],
            8.0,
            [247, 92, 3, 255],
        ),
        (
            "Pass 3 (Round 4.5px · Green)",
            BrushShapeKind::ALL[0],
            4.5,
            [0, 204, 102, 255],
        ),
    ];

    let mut elements: Vec<Element> = Vec::new();
    eprintln!(
        "=== T-093 Benchmark: 3 passes x {} strokes = {} total strokes (grid {}x{}) ===",
        strokes_per_pass,
        strokes_per_pass * 3,
        grid_side,
        grid_side
    );

    for (pass_idx, (label, shape, width, color)) in passes.iter().enumerate() {
        let pass_start = std::time::Instant::now();
        let mut worst_stroke = std::time::Duration::ZERO;
        let mut durations = Vec::with_capacity(strokes_per_pass);

        for i in 0..strokes_per_pass {
            let patch = (i * 2654435761) % (grid_side * grid_side);
            let px = (patch % grid_side) as f32 * cell_step + pass_idx as f32 * 18.0;
            let py = (patch / grid_side) as f32 * cell_step + pass_idx as f32 * 14.0;
            let x0 = px + next_f32() * 95.0;
            let y0 = py + next_f32() * 95.0;
            let angle = next_f32() * std::f32::consts::TAU;
            let dx = angle.cos();
            let dy = angle.sin();
            let len = 24.0 + next_f32() * 42.0;
            let n_samples = ((len / 2.0) as usize).max(6);
            let mut pts = Vec::with_capacity(n_samples);
            let mut prs = Vec::with_capacity(n_samples);
            let wobble_phase = next_f32() * std::f32::consts::TAU;
            for s in 0..n_samples {
                let t = s as f32 / (n_samples - 1).max(1) as f32;
                let dist = t * len;
                let wob = (t * std::f32::consts::PI + wobble_phase).sin() * 2.2;
                pts.push(egui::Pos2::new(
                    x0 + dx * dist - dy * wob,
                    y0 + dy * dist + dx * wob,
                ));
                prs.push(0.35 + 0.6 * (t * std::f32::consts::PI).sin());
            }
            let commands = crate::authoring_tools::shaped_brush_outline(&pts, &prs, *width, *shape);
            let el = Element {
                id: format!("p{pass_idx}-s{i}"),
                name: format!("p{pass_idx}-s{i}"),
                transform: Transform::default(),
                kind: ElementKind::Path {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill: *color,
                    stroke: *color,
                    stroke_width: 0.0,
                },
                merge: true,
                ..Element::default()
            };
            let t0 = std::time::Instant::now();
            apply_merge_shape_to_keyframe(&mut elements, el);
            let dt = t0.elapsed();
            worst_stroke = worst_stroke.max(dt);
            durations.push(dt);

            if (i + 1) % log_every == 0 || i + 1 == strokes_per_pass {
                let total_cmds: usize = elements
                    .iter()
                    .map(|e| match &e.kind {
                        ElementKind::Path { commands, .. } => commands.len(),
                        ElementKind::PaintedPath { commands, .. } => commands.len(),
                        _ => 0,
                    })
                    .sum();
                eprintln!(
                    "  [{label}] stroke {:>6}/{}: last {:>8.2?}, worst {:>8.2?}, elapsed {:>8.2?} | elements: {}, total_cmds: {}",
                    i + 1,
                    strokes_per_pass,
                    dt,
                    worst_stroke,
                    pass_start.elapsed(),
                    elements.len(),
                    total_cmds
                );
            }
        }
        durations.sort_unstable();
        let p50 = durations[durations.len() / 2];
        let p99 = durations[(durations.len() * 99) / 100];
        eprintln!(
            "==> {label} DONE in {:.2?}: avg {:.2?}/stroke, p50 {:.2?}, p99 {:.2?}, max {:.2?}",
            pass_start.elapsed(),
            pass_start.elapsed() / strokes_per_pass as u32,
            p50,
            p99,
            worst_stroke
        );
    }

    // Put the resulting artwork into an Editor stage and benchmark selecting a colour and moving it:
    // 1) Color 1: move +200px right, then move random (dx, dy)
    // 2) Color 2: move +200px down, then move random (dx, dy)
    let (mut editor, ctx, _page) = fixture(1);
    editor.symbol_mut().timeline.layers = vec![layer("bench-layer", elements, false)];
    let layer_id = editor.symbol().timeline.layers[0].id.clone();

    let t_cold_stage = std::time::Instant::now();
    stage_input_frame(&mut editor, &ctx, 1.0, vec![]);
    let cold_stage_dt = t_cold_stage.elapsed();

    let t_warm_stage = std::time::Instant::now();
    stage_input_frame(&mut editor, &ctx, 2.0, vec![]);
    let warm_stage_dt = t_warm_stage.elapsed();
    eprintln!(
        "==> Stage render (unselected): cold {:.2?}, warm (retained cache) {:.2?}",
        cold_stage_dt, warm_stage_dt
    );

    type MoveScenario<'a> = (&'a str, [u8; 4], (f64, f64), (f64, f64));
    let move_scenarios: [MoveScenario<'_>; 2] = [
        (
            "Color 1 (Blue)",
            passes[0].3,
            (200.0, 0.0),
            (
                -180.0 + (next_f32() * 140.0) as f64,
                -120.0 + (next_f32() * 240.0) as f64,
            ),
        ),
        (
            "Color 2 (Orange)",
            passes[1].3,
            (0.0, 200.0),
            (
                -150.0 + (next_f32() * 300.0) as f64,
                -160.0 + (next_f32() * 130.0) as f64,
            ),
        ),
    ];

    let mut frame_time = 3.0;
    for (color_label, target_color, step1, step2) in move_scenarios {
        for (move_idx, (dx, dy)) in [step1, step2].into_iter().enumerate() {
            let selected_ids: Vec<Id> = editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .filter(|e| {
                    matches!(
                        &e.kind,
                        ElementKind::Path { fill, .. } | ElementKind::PaintedPath { fill, .. }
                        if *fill == target_color
                    )
                })
                .map(|e| e.id.clone())
                .collect();
            let targets: Vec<SelectionTarget> = selected_ids
                .iter()
                .map(|id| SelectionTarget {
                    layer: layer_id.clone(),
                    element: id.clone(),
                })
                .collect();
            editor.set_selection(targets);

            // Measure live drag frame latency (transforming selected elements before release)
            let selected_set: std::collections::HashSet<&str> =
                selected_ids.iter().map(String::as_str).collect();
            for el in &mut editor.symbol_mut().timeline.layers[0].keyframes[0].elements {
                if selected_set.contains(el.id.as_str()) {
                    el.transform.x += dx;
                    el.transform.y += dy;
                }
            }
            frame_time += 1.0;
            let t_drag_frame = std::time::Instant::now();
            stage_input_frame(&mut editor, &ctx, frame_time, vec![]);
            let drag_frame_dt = t_drag_frame.elapsed();

            // Measure mouse release remerge + responsive frame latency
            let t_remerge = std::time::Instant::now();
            let remerged_ids = remerge_moved_elements_in_keyframe(
                &mut editor.symbol_mut().timeline.layers[0].keyframes[0].elements,
                &selected_ids,
            );
            let remerge_dt = t_remerge.elapsed();

            editor.set_selection(
                remerged_ids
                    .iter()
                    .map(|id| SelectionTarget {
                        layer: layer_id.clone(),
                        element: id.clone(),
                    })
                    .collect(),
            );
            frame_time += 1.0;
            let t_post_cold = std::time::Instant::now();
            stage_input_frame(&mut editor, &ctx, frame_time, vec![]);
            let post_cold_dt = t_post_cold.elapsed();

            frame_time += 1.0;
            let t_post_warm = std::time::Instant::now();
            stage_input_frame(&mut editor, &ctx, frame_time, vec![]);
            let post_warm_dt = t_post_warm.elapsed();

            eprintln!(
                "==> [{color_label} · Move #{} ({:+.1}, {:+.1})] targets: {} -> {} | drag_frame: {:.2?}, release_remerge: {:.2?}, post_frame(cold): {:.2?}, post_frame(warm): {:.2?}",
                move_idx + 1,
                dx,
                dy,
                selected_ids.len(),
                remerged_ids.len(),
                drag_frame_dt,
                remerge_dt,
                post_cold_dt,
                post_warm_dt
            );
        }
    }
}
