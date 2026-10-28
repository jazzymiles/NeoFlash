//! Headless regression checks for authoring operations. These exercise the same
//! transactions and document mutations as the UI, without creating a GPU/window.

use crate::app::Editor;
use eframe::egui;
use neoflash_core::*;

fn rectangle() -> ElementKind {
    ElementKind::Rectangle {
        width: 20.,
        height: 12.,
        fill: [20, 90, 180, 255],
        stroke: [0, 0, 0, 255],
        stroke_width: 1.,
    }
}

fn tween_project(easing: Easing) -> (Project, Id) {
    let mut project = Project::blank();
    let id = new_id("object");
    let start = Element {
        accessibility: None,
        id: id.clone(),
        name: "moving_shape".into(),
        transform: Transform::default(),
        kind: rectangle(),
        scripts: vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "this.x += 1;".into(),
            },
        }],
    };
    let mut end = start.clone();
    end.transform.x = 100.;
    end.transform.y = 40.;
    if let ElementKind::Rectangle { width, .. } = &mut end.kind {
        *width = 40.;
    }
    let root = project.root.clone();
    let timeline = &mut project.symbol_mut(&root).unwrap().timeline;
    timeline.frame_count = 11;
    timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![start],
            tween: Some(easing),
            label: "start".into(),
        },
        Keyframe {
            frame: 10,
            elements: vec![end],
            tween: None,
            label: "finish".into(),
        },
    ];
    project.validate().unwrap();
    (project, id)
}

fn editor_at_middle(easing: Easing) -> Editor {
    let (project, id) = tween_project(easing);
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    editor.frame = 5;
    editor.select_single_id(id);
    editor
}

fn accept_default_symbol(editor: &mut Editor) {
    let draft = editor.symbol_conversion.take().expect("conversion dialog");
    editor
        .commit_symbol_conversion(
            &draft,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(30., 12.)),
        )
        .unwrap();
}

#[test]
fn inserting_key_uses_eased_pose_and_undo_restores_document() {
    let mut editor = editor_at_middle(Easing::EaseIn);
    let before = editor.project.clone();
    let expected = editor.selected_sampled().unwrap();
    assert_eq!(expected.transform.x, 25.);
    assert_eq!(expected.transform.y, 10.);

    editor.make_key(false);

    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(
        layer.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>(),
        [0, 5, 10]
    );
    let inserted = layer.keyframes.iter().find(|k| k.frame == 5).unwrap();
    assert_eq!(inserted.elements, [expected]);
    assert!(
        inserted.label.is_empty(),
        "inserting a key must not duplicate a frame label"
    );
    editor.project.validate().unwrap();
    let after = editor.project.clone();

    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    editor.redo();
    assert_eq!(editor.project, after);
}

#[test]
fn duplicating_between_keys_uses_visible_pose_without_rewriting_previous_key() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before = editor.project.clone();
    let previous = editor.symbol().timeline.layers[0].keyframes[0].clone();
    let sampled = editor.selected_sampled().unwrap();
    let original_id = sampled.id.clone();

    editor.duplicate();

    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes[0], previous);
    let current = layer.keyframes.iter().find(|k| k.frame == 5).unwrap();
    assert_eq!(current.elements.len(), 2);
    assert_eq!(
        current
            .elements
            .iter()
            .find(|e| e.id == original_id)
            .unwrap(),
        &sampled
    );
    let copy = editor.selected_element().unwrap();
    assert_ne!(copy.id, original_id);
    assert_ne!(copy.name, sampled.name);
    assert_eq!(copy.transform.x, sampled.transform.x + 16.);
    assert_eq!(copy.transform.y, sampled.transform.y + 16.);
    assert_eq!(copy.kind, sampled.kind);
    assert_eq!(copy.scripts, sampled.scripts);
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);
    assert!(
        editor.selection.is_empty(),
        "undo must clear the now-absent duplicate selection"
    );
}

#[test]
fn deleting_between_keys_creates_blank_current_key_and_preserves_authored_endpoints() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before = editor.project.clone();
    let endpoints = editor.symbol().timeline.layers[0].keyframes.clone();

    editor.delete_selected();

    let keys = &editor.symbol().timeline.layers[0].keyframes;
    assert_eq!(keys[0], endpoints[0]);
    assert_eq!(keys[2], endpoints[1]);
    assert_eq!(keys[1].frame, 5);
    assert!(keys[1].elements.is_empty());
    assert!(evaluate(&editor.project, editor.symbol_id(), 5).is_empty());
    assert!(editor.selection.is_empty());
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn blank_key_replaces_only_current_key_and_can_be_undone() {
    let mut editor = editor_at_middle(Easing::Linear);
    editor.make_key(false);
    let before = editor.project.clone();

    editor.make_key(true);

    let current = editor.symbol().timeline.layers[0].key_at(5).unwrap();
    assert_eq!(current.frame, 5);
    assert!(current.elements.is_empty());
    assert!(current.tween.is_none());
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn valid_empty_timeline_becomes_editable_without_indexing_empty_layers() {
    let mut project = Project::blank();
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.layers.clear();
    project.validate().unwrap();
    let mut editor = Editor::from_project(&egui::Context::default(), project);

    assert_eq!(editor.layer, 0);
    assert_eq!(editor.symbol().timeline.layers.len(), 1);
    assert!(editor.layer_editable());
    editor.add_element(rectangle(), 12., 34.);
    assert_eq!(editor.selected_element().unwrap().transform.x, 12.);
    assert_eq!(editor.selected_element().unwrap().transform.y, 34.);
    editor.project.validate().unwrap();
    editor.undo();
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
}

#[test]
fn conversion_preserves_appearance_identity_and_scripts_on_the_new_instance() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before = editor.project.clone();
    let expected = editor.selected_sampled().unwrap();
    let expected_scene = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    let previous_key = editor.symbol().timeline.layers[0].keyframes[0].clone();

    editor.convert_to_symbol();

    assert_eq!(
        editor.project, before,
        "opening the dialog must not convert artwork"
    );
    assert!(!editor.history.can_undo());
    editor.symbol_conversion = None;
    assert_eq!(
        editor.project, before,
        "Cancel does not change the document"
    );
    editor.convert_to_symbol();
    accept_default_symbol(&mut editor);

    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0],
        previous_key
    );
    let instance = editor.selected_element().unwrap();
    assert_eq!(
        instance.id, expected.id,
        "instance references must survive conversion"
    );
    assert_eq!(instance.name, expected.name);
    assert_eq!(instance.transform, expected.transform);
    assert_eq!(instance.scripts, expected.scripts);
    let ElementKind::Instance {
        symbol_id,
        playback,
    } = &instance.kind
    else {
        panic!("conversion must place a symbol instance");
    };
    assert_eq!(*playback, InstancePlayback::Independent);
    let new_symbol_id = symbol_id.clone();
    let symbol = editor.project.symbol(symbol_id).unwrap();
    assert_eq!(symbol.timeline.frame_count, 1);
    let content = &symbol.timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(content.kind, expected.kind);
    assert_eq!(content.transform, Transform::default());
    assert_ne!(content.id, expected.id);
    assert!(
        content.scripts.is_empty(),
        "object handlers must not run twice after conversion"
    );
    let scene = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    assert_eq!(scene.len(), expected_scene.len());
    assert_eq!(scene[0].transform, expected_scene[0].transform);
    assert_eq!(scene[0].kind, expected_scene[0].kind);
    editor.project.validate().unwrap();

    editor.enter_symbol(new_symbol_id);
    editor.undo();
    assert_eq!(editor.project, before);
    assert_eq!(editor.symbol_id(), editor.project.root);
    assert!(editor.selection.is_empty());

    // The same conversion transaction handles a chosen registration point and
    // runtime identifier without changing a transformed object's visible pose.
    let mut editor = editor_at_middle(Easing::Linear);
    editor.ensure_selected_key();
    editor.selected_element_mut().unwrap().transform = Transform {
        x: 80.,
        y: 40.,
        scale_x: -2.,
        scale_y: 1.5,
        rotation: 30.,
        alpha: 0.7,
    };
    let before = editor.project.clone();
    let visible = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    editor.convert_to_symbol();
    let mut draft = editor.symbol_conversion.take().unwrap();
    draft.registration = 4;
    draft.name = "Card".into();
    draft.export_enabled = true;
    draft.linkage_id = "card-export".into();
    editor
        .commit_symbol_conversion(
            &draft,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(30., 12.)),
        )
        .unwrap();
    let symbol = editor
        .project
        .symbol(editor.library_selected.as_ref().unwrap())
        .unwrap();
    assert_eq!(symbol.name, "Card");
    assert_eq!(symbol.linkage_id.as_deref(), Some("card-export"));
    let converted = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    for (before, after) in visible.iter().zip(&converted) {
        assert_eq!(before.kind, after.kind);
        assert_eq!(before.alpha, after.alpha);
        for (before, after) in before.transform.iter().zip(after.transform) {
            assert!((before - after).abs() < 1e-10);
        }
    }
    editor.undo();
    assert_eq!(editor.project, before);
    // A contiguous multi-selection becomes one Graphic without changing its
    // draw order. An unselected object between selections is not flattened.
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    for x in [10., 30., 50.] {
        editor.add_element(rectangle(), x, 20.);
    }
    let targets = editor.editable_targets();
    editor.set_selection(vec![targets[0].clone(), targets[2].clone()]);
    let before = editor.project.clone();
    editor.convert_to_symbol();
    assert!(editor.symbol_conversion.is_none());
    assert_eq!(editor.project, before);
    editor.select_all();
    let visible = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    editor.convert_to_symbol();
    let mut draft = editor.symbol_conversion.take().unwrap();
    draft.kind = SymbolKind::Graphic;
    draft.registration = 8;
    editor
        .commit_symbol_conversion(
            &draft,
            egui::Rect::from_min_max(egui::pos2(10., 20.), egui::pos2(70., 32.)),
        )
        .unwrap();
    let symbol = editor
        .project
        .symbol(editor.library_selected.as_ref().unwrap())
        .unwrap();
    assert_eq!(symbol.kind, SymbolKind::Graphic);
    let converted = evaluate(&editor.project, editor.symbol_id(), editor.frame);
    assert_eq!(visible.len(), converted.len());
    for (before, after) in visible.iter().zip(&converted) {
        assert_eq!(before.kind, after.kind);
        assert_eq!(before.transform, after.transform);
    }
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn recursive_symbol_placement_is_rejected_without_changing_document_or_history() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before_conversion = editor.project.clone();
    editor.convert_to_symbol();
    accept_default_symbol(&mut editor);
    let root = editor.project.root.clone();
    let child = editor.library_selected.clone().unwrap();
    editor.enter_symbol(child.clone());
    let before_attempt = editor.project.clone();

    // Root already references child; inserting root into child would create a cycle.
    editor.add_element(
        ElementKind::Instance {
            symbol_id: root,
            playback: InstancePlayback::Independent,
        },
        0.,
        0.,
    );
    assert_eq!(editor.project, before_attempt);
    assert!(editor.status.contains("recursive"));
    editor.add_element(
        ElementKind::Instance {
            symbol_id: child,
            playback: InstancePlayback::Independent,
        },
        0.,
        0.,
    );
    assert_eq!(editor.project, before_attempt);
    editor.project.validate().unwrap();

    // Rejected operations must not consume undo entries.
    editor.undo();
    assert_eq!(editor.project, before_conversion);
}

#[test]
fn playhead_normalization_drops_selection_when_object_disappears() {
    let mut editor = editor_at_middle(Easing::Linear);
    editor.make_key(true);
    editor.frame = 0;
    editor.select_single_id(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0]
            .id
            .clone(),
    );
    assert!(editor.selected_element().is_some());
    editor.frame = 5;

    editor.normalize_selection();

    assert!(editor.selection.is_empty());
    assert!(editor.selected_element().is_none());
}

#[test]
fn locked_layer_blocks_document_mutations() {
    let mut editor = editor_at_middle(Easing::Linear);
    editor.symbol_mut().timeline.layers[0].locked = true;
    let before = editor.project.clone();

    editor.make_key(false);
    editor.duplicate();
    editor.delete_selected();
    editor.convert_to_symbol();
    editor.add_element(rectangle(), 0., 0.);

    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn moving_key_preserves_instance_identity_tween_and_label_and_is_one_undo_step() {
    let mut editor = editor_at_middle(Easing::EaseInOut);
    let before = editor.project.clone();
    let original_key = editor.symbol().timeline.layers[0].keyframes[0].clone();

    assert!(editor.move_key(0, 0, 3, false));

    let keys = &editor.symbol().timeline.layers[0].keyframes;
    assert_eq!(keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [3, 10]);
    assert_eq!(keys[0].elements, original_key.elements);
    assert_eq!(keys[0].tween, original_key.tween);
    assert_eq!(keys[0].label, original_key.label);
    assert_eq!(editor.frame, 3);
    assert!(editor.selection.is_empty());
    editor.project.validate().unwrap();
    let moved = editor.project.clone();

    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.redo();
    assert_eq!(editor.project, moved);
}

#[test]
fn key_copy_retains_source_and_rejected_destinations_do_not_change_history() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before = editor.project.clone();
    let source_key = editor.symbol().timeline.layers[0].keyframes[0].clone();

    assert!(
        !editor.move_key(0, 0, 10, false),
        "destination collision must not overwrite a key"
    );
    assert!(
        !editor.move_key(0, 0, 11, true),
        "copy must stay within the timeline"
    );
    assert!(
        !editor.move_key(0, 4, 7, false),
        "missing source must not synthesize a key"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.move_key(0, 0, 4, true));

    let keys = &editor.symbol().timeline.layers[0].keyframes;
    assert_eq!(keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 4, 10]);
    assert_eq!(keys[0], source_key);
    assert_eq!(keys[1].elements, source_key.elements);
    assert_eq!(keys[1].tween, source_key.tween);
    assert!(
        keys[1].label.is_empty(),
        "copy must not duplicate a named frame target"
    );
    editor.project.validate().unwrap();

    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.symbol_mut().timeline.layers[0].locked = true;
    let locked = editor.project.clone();
    assert!(!editor.move_key(0, 0, 4, false));
    assert_eq!(editor.project, locked);
}

pub(crate) fn stage_input_frame(
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
        |ctx| {
            editor.bitmap_textures.begin_frame(&editor.project);
            editor.stage(ctx);
        },
    )
}

pub(crate) fn primary_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(crate) fn rectangle_stage() -> (Editor, egui::Context, egui::Rect) {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.tool = crate::app::Tool::Rectangle;
    editor.zoom = 1.;
    let layout = stage_input_frame(&mut editor, &ctx, 0., vec![]);
    let page = layout
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == egui::Color32::WHITE
                    && rect.rect.size() == egui::vec2(960., 600.) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("stage paint should expose the white page bounds");
    (editor, ctx, page)
}

#[test]
fn rectangle_tool_uses_pointer_press_origin_and_release_to_create_undoable_shape() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.fill = [120, 80, 40, 96];
    editor.stroke = [10, 20, 30, 176];
    editor.stroke_width = 2.5;
    let before = editor.project.clone();
    let start = page.min + egui::vec2(50., 60.);
    let end = page.min + egui::vec2(170., 140.);

    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
        ],
    );
    stage_input_frame(&mut editor, &ctx, 0.2, vec![egui::Event::PointerMoved(end)]);
    stage_input_frame(&mut editor, &ctx, 0.3, vec![primary_button(end, false)]);
    stage_input_frame(&mut editor, &ctx, 0.4, vec![]);

    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(
        elements.len(),
        1,
        "one pointer drag must create one rectangle"
    );
    let shape = &elements[0];
    assert_eq!(shape.transform.x, 50.);
    assert_eq!(shape.transform.y, 60.);
    let ElementKind::Rectangle {
        width,
        height,
        fill,
        stroke,
        stroke_width,
    } = shape.kind
    else {
        panic!("rectangle tool created a different primitive");
    };
    assert_eq!((width, height), (120., 80.));
    assert_eq!(
        (fill, stroke, stroke_width),
        (editor.fill, editor.stroke, 2.5)
    );
    assert!(editor.shape_draft.is_none());
    editor.project.validate().unwrap();

    // A continuous property interaction retains its initial snapshot.
    let drawn = editor.project.clone();
    let scope = egui::Id::new("shape-properties");
    let control = egui::Id::new("opacity-control");
    editor.begin_property_edit(scope, control);
    editor.selected_element_mut().unwrap().transform.alpha = 0.75;
    editor.begin_property_edit(scope, control);
    editor.selected_element_mut().unwrap().transform.alpha = 0.25;
    editor.finish_property_edit();
    editor.undo();
    assert_eq!(
        editor.project, drawn,
        "one Undo restores the whole property interaction"
    );
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(
        !editor.history.can_undo(),
        "one drawing gesture is one undo entry"
    );
}

#[test]
fn rectangle_tool_handles_entire_reverse_drag_batched_into_one_input_frame() {
    let (mut editor, ctx, page) = rectangle_stage();
    let before = editor.project.clone();
    let start = page.min + egui::vec2(210., 180.);
    let end = page.min + egui::vec2(80., 70.);

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
    stage_input_frame(&mut editor, &ctx, 0.2, vec![]);

    let elements = &editor.symbol().timeline.layers[0].keyframes[0].elements;
    assert_eq!(
        elements.len(),
        1,
        "batched drag must create exactly one shape"
    );
    let shape = &elements[0];
    assert_eq!((shape.transform.x, shape.transform.y), (80., 70.));
    let ElementKind::Rectangle { width, height, .. } = shape.kind else {
        panic!("rectangle tool created a different primitive");
    };
    assert_eq!((width, height), (130., 110.));
    assert!(editor.shape_draft.is_none());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn rectangle_click_without_movement_creates_neither_shape_nor_undo_entry() {
    for batched in [false, true] {
        let (mut editor, ctx, page) = rectangle_stage();
        let before = editor.project.clone();
        let pos = page.min + egui::vec2(50., 60.);
        let mut events = vec![egui::Event::PointerMoved(pos), primary_button(pos, true)];
        if batched {
            events.push(primary_button(pos, false));
        }
        stage_input_frame(&mut editor, &ctx, 0.1, events);
        if !batched {
            stage_input_frame(&mut editor, &ctx, 0.2, vec![primary_button(pos, false)]);
        }
        stage_input_frame(&mut editor, &ctx, 0.3, vec![]);
        assert_eq!(
            editor.project, before,
            "click without movement must not alter the document"
        );
        assert!(editor.shape_draft.is_none());
        assert!(!editor.history.can_undo());
    }
}

pub(crate) fn pen_click(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    from: egui::Pos2,
    to: egui::Pos2,
) {
    stage_input_frame(
        editor,
        ctx,
        time,
        vec![
            egui::Event::PointerMoved(from),
            primary_button(from, true),
            egui::Event::PointerMoved(to),
            primary_button(to, false),
        ],
    );
}

#[test]
fn pen_drag_creates_cubic_closed_path_as_one_undo_step() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = crate::app::Tool::Pen;
    let before = editor.project.clone();
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(250., 200.);
    let c = page.min + egui::vec2(100., 300.);
    pen_click(&mut editor, &ctx, 0.1, a, a + egui::vec2(50., 0.));
    pen_click(&mut editor, &ctx, 0.2, b, b);
    pen_click(&mut editor, &ctx, 0.3, c, c);
    assert_eq!(
        editor.project, before,
        "unfinished path must remain a draft"
    );
    pen_click(&mut editor, &ctx, 0.4, a, a);
    assert!(editor.vector.draft.is_none());
    let ElementKind::Path { commands, fill, .. } = &editor.selected_element().unwrap().kind else {
        panic!("expected path")
    };
    assert!(matches!(commands[1], PathCommand::CubicTo { .. }));
    assert!(matches!(commands.last(), Some(PathCommand::Close)));
    assert_eq!(*fill, editor.fill);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    editor.redo();
    assert!(matches!(
        editor.symbol().timeline.layers[0]
            .key_at(0)
            .unwrap()
            .elements[0]
            .kind,
        ElementKind::Path { .. }
    ));
}

#[test]
fn pencil_captures_a_batched_gesture_and_simplifies_collinear_samples() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = crate::app::Tool::Pencil;
    editor.stroke = [70, 110, 150, 80];
    editor.stroke_width = 3.5;
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(180., 200.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.1,
        vec![
            egui::Event::PointerMoved(a),
            primary_button(a, true),
            egui::Event::PointerMoved(a + egui::vec2(40., 0.)),
            egui::Event::PointerMoved(a + egui::vec2(80., 0.)),
            egui::Event::PointerMoved(b),
            primary_button(b, false),
        ],
    );
    let ElementKind::Path {
        commands,
        fill,
        stroke,
        stroke_width,
        ..
    } = &editor.selected_element().unwrap().kind
    else {
        panic!("expected pencil path")
    };
    assert_eq!(
        commands.len(),
        3,
        "collinear samples should be simplified, corner retained"
    );
    assert_eq!(fill[3], 0);
    assert_eq!(*stroke_width, 3.5);
    assert_eq!(*stroke, editor.stroke);
    assert!(matches!(
        commands[2],
        PathCommand::LineTo { x: 180., y: 200. }
    ));
}

#[test]
fn subselection_moves_transformed_control_in_local_space_and_undo_restores() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::CubicTo {
                    cx1: 40.,
                    cy1: -40.,
                    cx2: 80.,
                    cy2: -40.,
                    x: 120.,
                    y: 0.,
                },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 0],
            stroke: [10, 20, 30, 255],
            stroke_width: 3.,
        },
        300.,
        300.,
    );
    editor.selected_element_mut().unwrap().transform.scale_x = 2.;
    editor.selected_element_mut().unwrap().transform.rotation = 90.;
    editor.tool = crate::app::Tool::Subselect;
    let before = editor.project.clone();
    let matrix = transform_matrix(editor.selected_element().unwrap().transform);
    let start = page.min + crate::stage::affine(matrix, egui::pos2(40., -40.)).to_vec2();
    let end = page.min + crate::stage::affine(matrix, egui::pos2(55., -20.)).to_vec2();
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
        vec![egui::Event::PointerMoved(end), primary_button(end, false)],
    );
    let ElementKind::Path { commands, .. } = &editor.selected_element().unwrap().kind else {
        panic!("expected path")
    };
    assert!(matches!(
        commands[1],
        PathCommand::CubicTo {
            cx1: 55.,
            cy1: -20.,
            x: 120.,
            ..
        }
    ));
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn path_tessellation_keeps_holes_and_concave_areas() {
    let commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: 100., y: 0. },
        PathCommand::LineTo { x: 100., y: 100. },
        PathCommand::LineTo { x: 0., y: 100. },
        PathCommand::Close,
        PathCommand::MoveTo { x: 25., y: 25. },
        PathCommand::LineTo { x: 75., y: 25. },
        PathCommand::LineTo { x: 75., y: 75. },
        PathCommand::LineTo { x: 25., y: 75. },
        PathCommand::Close,
    ];
    let mesh = crate::vector::tessellate(&commands, FillRule::EvenOdd, None, 0.1).unwrap();
    let area: f32 = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|i| {
            let a = mesh.vertices[i[0] as usize];
            let b = mesh.vertices[i[1] as usize];
            let c = mesh.vertices[i[2] as usize];
            ((b - a).cross(c - a)).abs() / 2.
        })
        .sum();
    assert!((area - 7500.).abs() < 0.1);
    let mesh = crate::vector::tessellate(&commands, FillRule::NonZero, None, 0.1).unwrap();
    let area: f32 = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|i| {
            let a = mesh.vertices[i[0] as usize];
            let b = mesh.vertices[i[1] as usize];
            let c = mesh.vertices[i[2] as usize];
            ((b - a).cross(c - a)).abs() / 2.
        })
        .sum();
    assert!((area - 10000.).abs() < 0.1);
}

#[test]
fn moving_a_closed_path_seam_keeps_both_endpoints_and_adjacent_controls_together() {
    let mut commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::CubicTo {
            cx1: 20.,
            cy1: 0.,
            cx2: 100.,
            cy2: 80.,
            x: 100.,
            y: 100.,
        },
        PathCommand::CubicTo {
            cx1: 80.,
            cy1: 100.,
            cx2: -20.,
            cy2: 0.,
            x: 0.,
            y: 0.,
        },
        PathCommand::Close,
    ];
    let handle = crate::vector::handles(&commands)[0];
    crate::vector::move_handle(&mut commands, handle, egui::pos2(10., 20.));
    assert!(matches!(
        commands[0],
        PathCommand::MoveTo { x: 10., y: 20. }
    ));
    assert!(matches!(
        commands[1],
        PathCommand::CubicTo {
            cx1: 30.,
            cy1: 20.,
            x: 100.,
            y: 100.,
            ..
        }
    ));
    assert!(matches!(
        commands[2],
        PathCommand::CubicTo {
            cx2: -10.,
            cy2: 20.,
            x: 10.,
            y: 20.,
            ..
        }
    ));
}

#[test]
fn cancelling_a_point_drag_restores_geometry_without_an_undo_entry() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = crate::app::Tool::Pencil;
    pen_click(
        &mut editor,
        &ctx,
        0.1,
        page.min + egui::vec2(100., 100.),
        page.min + egui::vec2(200., 200.),
    );
    editor.tool = crate::app::Tool::Subselect;
    editor.history.clear();
    let before = editor.project.clone();
    let start = page.min + egui::vec2(100., 100.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
        ],
    );
    stage_input_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![egui::Event::PointerMoved(start + egui::vec2(20., 40.))],
    );
    assert_ne!(editor.project, before);
    editor.cancel_vector();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn switching_frames_during_point_drag_cancels_instead_of_overwriting_another_key() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = crate::app::Tool::Pencil;
    pen_click(
        &mut editor,
        &ctx,
        0.1,
        page.min + egui::vec2(100., 100.),
        page.min + egui::vec2(200., 200.),
    );
    editor.frame = 10;
    editor.make_key(false);
    editor.frame = 0;
    editor.tool = crate::app::Tool::Subselect;
    let before = editor.project.clone();
    let start = page.min + egui::vec2(100., 100.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
        ],
    );
    stage_input_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![egui::Event::PointerMoved(start + egui::vec2(20., 40.))],
    );
    editor.frame = 10;
    stage_input_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![
            egui::Event::PointerMoved(start + egui::vec2(50., 60.)),
            primary_button(start + egui::vec2(50., 60.), false),
        ],
    );
    assert!(editor.vector.point_drag.is_none());
    assert_eq!(editor.project, before);
}

#[test]
fn quadratic_shared_control_moves_once_with_closed_seam() {
    let mut commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 50.,
            cy: 50.,
            x: 0.,
            y: 0.,
        },
        PathCommand::Close,
    ];
    let handle = crate::vector::handles(&commands)[0];
    crate::vector::move_handle(&mut commands, handle, egui::pos2(10., 20.));
    assert!(matches!(
        commands[0],
        PathCommand::MoveTo { x: 10., y: 20. }
    ));
    assert!(matches!(
        commands[1],
        PathCommand::QuadraticTo {
            cx: 60.,
            cy: 70.,
            x: 10.,
            y: 20.
        }
    ));
}

#[test]
fn selecting_a_highly_scaled_tiny_curve_matches_visible_geometry() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.add_element(
        ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::QuadraticTo {
                    cx: 0.05,
                    cy: 0.1,
                    x: 0.1,
                    y: 0.,
                },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 0],
            stroke: [0, 0, 0, 255],
            stroke_width: 0.005,
        },
        100.,
        100.,
    );
    let id = editor.selection.clone();
    editor.selected_element_mut().unwrap().transform.scale_x = 1000.;
    editor.selected_element_mut().unwrap().transform.scale_y = 1000.;
    editor.clear_selection();
    let point = page.min + egui::vec2(150., 150.);
    pen_click(&mut editor, &ctx, 0.1, point, point);
    assert_eq!(editor.selection, id);
}

#[test]
fn saving_finishes_an_open_pen_draft_and_preserves_it_in_json() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.tool = crate::app::Tool::Pen;
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(200., 200.);
    pen_click(&mut editor, &ctx, 0.1, a, a);
    pen_click(&mut editor, &ctx, 0.2, b, b);
    assert!(editor.dirty());
    let path = std::env::temp_dir().join(format!("neoflash-{}.nflash", new_id("pen-save-test")));
    editor.path = Some(path.clone());
    assert!(editor.save(false));
    assert!(editor.vector.draft.is_none());
    assert!(!editor.dirty());
    let loaded = Project::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded, editor.project);
    assert!(matches!(
        loaded.symbols[0].timeline.layers[0]
            .key_at(0)
            .unwrap()
            .elements[0]
            .kind,
        ElementKind::Path { .. }
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn navigating_away_finishes_a_pen_draft_in_its_original_frame() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.frame = 5;
    editor.tool = crate::app::Tool::Pen;
    let a = page.min + egui::vec2(100., 100.);
    let b = page.min + egui::vec2(200., 200.);
    pen_click(&mut editor, &ctx, 0.1, a, a);
    pen_click(&mut editor, &ctx, 0.2, b, b);
    editor.frame = 20;
    stage_input_frame(&mut editor, &ctx, 0.3, vec![]);
    assert_eq!(editor.frame, 20);
    assert!(editor.vector.draft.is_none());
    let keys = &editor.symbol().timeline.layers[0].keyframes;
    assert!(
        keys.iter()
            .any(|k| k.frame == 5 && matches!(k.elements[0].kind, ElementKind::Path { .. }))
    );
    assert!(!keys.iter().any(|k| k.frame == 20));
    editor.undo();
    assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 1);
}

fn button_metadata() -> Accessibility {
    Accessibility {
        name: "Open scene".into(),
        description: "Opens the next scene".into(),
        role: AccessibleRole::Button,
        tab_order: 2,
        disabled: false,
    }
}

#[test]
fn accessibility_edits_between_keys_preserve_the_visible_pose_and_undo() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before = editor.project.clone();
    let sampled = editor.selected_sampled().unwrap();
    editor.playing = true;
    editor.set_accessibility(Some(button_metadata()));
    assert!(
        !editor.playing,
        "editing semantics must keep the playhead stable"
    );
    let edited = editor.selected_element().unwrap();
    assert_eq!(edited.transform, sampled.transform);
    assert_eq!(edited.accessibility, Some(button_metadata()));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0],
        before.symbols[0].timeline.layers[0].keyframes[0]
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn conversion_preserves_one_semantic_target_instead_of_duplicating_it_inside_the_symbol() {
    let mut editor = editor_at_middle(Easing::Linear);
    editor.set_accessibility(Some(button_metadata()));
    let id = editor.selected_element().unwrap().id.clone();
    editor.convert_to_symbol();
    accept_default_symbol(&mut editor);
    let instance = editor.selected_element().unwrap();
    assert_eq!(instance.accessibility, Some(button_metadata()));
    let ElementKind::Instance { symbol_id, .. } = &instance.kind else {
        panic!("instance expected")
    };
    assert!(
        editor.project.symbol(symbol_id).unwrap().timeline.layers[0].keyframes[0].elements[0]
            .accessibility
            .is_none()
    );
    let mut player = Player::new(&editor.project);
    player.control(&editor.project, &[], PlaybackCommand::GotoAndStop(5));
    let nodes = player.accessibility_nodes(&editor.project);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].instance_path, vec![id]);
}

#[test]
fn locked_layer_blocks_accessibility_changes() {
    let mut editor = editor_at_middle(Easing::Linear);
    editor.symbol_mut().timeline.layers[0].locked = true;
    let before = editor.project.clone();
    editor.set_accessibility(Some(button_metadata()));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}
