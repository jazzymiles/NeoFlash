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
    editor.selected = Some(id);
    editor
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
        editor.selected.is_none(),
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
    assert!(editor.selected.is_none());
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
    assert!(editor.selected.is_none());
}

#[test]
fn recursive_symbol_placement_is_rejected_without_changing_document_or_history() {
    let mut editor = editor_at_middle(Easing::Linear);
    let before_conversion = editor.project.clone();
    editor.convert_to_symbol();
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
    editor.selected = Some(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0]
            .id
            .clone(),
    );
    assert!(editor.selected_element().is_some());
    editor.frame = 5;

    editor.normalize_selection();

    assert!(editor.selected.is_none());
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
    assert!(editor.selected.is_none());
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

fn stage_input_frame(
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
        |ctx| editor.stage(ctx),
    )
}

fn primary_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn rectangle_stage() -> (Editor, egui::Context, egui::Rect) {
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
    let ElementKind::Rectangle { width, height, .. } = shape.kind else {
        panic!("rectangle tool created a different primitive");
    };
    assert_eq!((width, height), (120., 80.));
    assert!(editor.draw_start.is_none());
    editor.project.validate().unwrap();

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
    assert!(editor.draw_start.is_none());
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
        assert!(editor.draw_start.is_none());
        assert!(!editor.history.can_undo());
    }
}
