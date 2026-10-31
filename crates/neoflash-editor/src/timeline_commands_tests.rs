//! Timeline commands: tweens, motion guides, keyframe conversion, scenes and playback options.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::*;

fn element(id: &str, kind: ElementKind, x: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            ..Transform::default()
        },
        kind,
        scripts: vec![],
        accessibility: None,
    }
}

fn rectangle() -> ElementKind {
    ElementKind::Rectangle {
        width: 20.,
        height: 10.,
        fill: [0, 0, 0, 255],
        stroke: [0, 0, 0, 0],
        stroke_width: 0.,
    }
}

/// Root with 20 frames; layer 0 holds `first` at frame 0 and `second` at frame 10.
fn editor_with(first: Vec<Element>, second: Vec<Element>) -> Editor {
    let mut project = Project::blank();
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 20;
    timeline.layers[0].keyframes = vec![
        Keyframe {
            elements: first,
            ..Default::default()
        },
        Keyframe {
            frame: 10,
            elements: second,
            ..Default::default()
        },
    ];
    Editor::from_project(&egui::Context::default(), project)
}

fn clip_editor() -> Editor {
    let mut editor = editor_with(vec![], vec![]);
    let mut child = editor.project.symbols[0].clone();
    child.id = "child".into();
    child.name = "Clip".into();
    child.timeline.layers[0].id = "child-layer".into();
    child.timeline.layers[0].keyframes.truncate(1);
    editor.project.symbols.push(child);
    let instance = ElementKind::Instance {
        symbol_id: "child".into(),
        playback: InstancePlayback::Independent,
    };
    editor.project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![element("clip", instance, 0.)];
    editor.project.format_version = 4;
    editor.project.validate().unwrap();
    editor
}

fn undo_restores(editor: &mut Editor, before: &Project) {
    editor.undo();
    assert_eq!(
        &editor.project, before,
        "one Undo step restores the document"
    );
}

#[test]
fn tween_kinds_switch_and_remove_with_one_undo_each() {
    let mut editor = editor_with(
        vec![element("a", rectangle(), 0.)],
        vec![element("b", rectangle(), 100.)],
    );
    let before = editor.project.clone();
    editor.toggle_tween(TweenKind::Shape);
    assert_eq!(editor.current_tween(), Some(TweenKind::Shape));
    assert!(editor.project.format_version >= 6);
    editor.project.validate().unwrap();
    // A shape tween morphs shapes with different identities.
    let mid = sample_layer(&editor.symbol().timeline.layers[0], 5);
    assert_eq!(mid.len(), 1);
    assert!(matches!(mid[0].kind, ElementKind::Path { .. }));
    undo_restores(&mut editor, &before);
    editor.toggle_tween(TweenKind::Classic);
    assert_eq!(editor.current_tween(), Some(TweenKind::Classic));
    editor.toggle_tween(TweenKind::Classic);
    assert_eq!(editor.current_tween(), None);
}

#[test]
fn shape_and_motion_tweens_explain_unsuitable_content() {
    let mut editor = editor_with(vec![], vec![]);
    let before = editor.project.clone();
    editor.toggle_tween(TweenKind::Shape);
    assert_eq!(editor.project, before);
    assert!(editor.status.contains("Shape tweens need shapes"));
    editor.toggle_tween(TweenKind::Motion);
    assert_eq!(editor.project, before);
    assert!(editor.status.contains("motion tween needs exactly one"));
}

#[test]
fn motion_tween_keys_only_the_properties_an_edit_changes() {
    let mut editor = clip_editor();
    editor.toggle_tween(TweenKind::Motion);
    assert_eq!(editor.current_tween(), Some(TweenKind::Motion));
    // Moving the clip at frame 5 creates a ◆ key that keys X only.
    editor.frame = 5;
    editor.select_single_id("clip".into());
    editor.transact(|s| {
        s.ensure_selected_key();
        s.selected_element_mut().unwrap().transform.x = 50.;
    });
    let layer = &editor.symbol().timeline.layers[0];
    let key = layer.keyframes.iter().find(|k| k.frame == 5).unwrap();
    assert_eq!(key.keyed_properties, Some(vec![TweenProperty::X]));
    assert_eq!(sample_layer(layer, 2)[0].transform.x, 20.);
    assert_eq!(sample_layer(layer, 9)[0].transform.x, 50.);
    // F6 inside the span keys every property.
    editor.frame = 7;
    editor.make_key(false);
    let key = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 7)
        .unwrap();
    assert_eq!(
        key.keyed_properties.as_ref().unwrap().len(),
        TweenProperty::ALL.len()
    );
    editor.project.validate().unwrap();
    // Removing the motion tween drops its ◆ keys.
    editor.frame = 3;
    editor.toggle_tween(TweenKind::Motion);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .all(|k| k.keyed_properties.is_none())
    );
}

#[test]
fn add_motion_guide_creates_a_guide_above_the_layer() {
    let mut editor = editor_with(vec![element("a", rectangle(), 0.)], vec![]);
    let before = editor.project.clone();
    assert!(editor.can_add_motion_guide_to(0));
    editor.add_motion_guide();
    let timeline = &editor.symbol().timeline;
    assert_eq!(timeline.layers.len(), 2);
    assert_eq!(timeline.layers[1].properties.role, LayerRole::Guide);
    assert_eq!(timeline.layers[1].name, "Guide: Layer 1");
    assert_eq!(
        timeline.layers[0].properties.motion_guide.as_ref(),
        Some(&timeline.layers[1].id)
    );
    assert_eq!(
        editor.layer, 1,
        "the guide is selected for drawing the path"
    );
    assert_eq!(
        crate::layer_controls::layer_kind(timeline, &timeline.layers[1]),
        neoflash_ui::components::timeline::LayerKind::MotionGuide
    );
    editor.project.validate().unwrap();
    assert!(!editor.can_add_motion_guide_to(0), "one guide per layer");
    undo_restores(&mut editor, &before);
}

#[test]
fn clear_and_convert_keyframes() {
    let mut editor = editor_with(
        vec![element("a", rectangle(), 0.)],
        vec![element("a", rectangle(), 100.)],
    );
    editor.frame = 10;
    editor.clear_keyframe();
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes.len(), 1);
    assert_eq!(
        sample_layer(layer, 12)[0].transform.x,
        0.,
        "frame holds the previous key"
    );
    editor.frame = 4;
    editor.convert_to_keyframes(true);
    let layer = &editor.symbol().timeline.layers[0];
    assert!(sample_layer(layer, 4).is_empty());
    assert_eq!(sample_layer(layer, 5)[0].transform.x, 0.);
}

#[test]
fn scenes_are_created_switched_and_hidden_from_the_library() {
    let mut editor = editor_with(vec![], vec![]);
    let root = editor.project.root.clone();
    editor.new_scene();
    let scenes = editor.scene_list();
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[1].1, "Scene 2");
    assert_eq!(editor.current_scene(), scenes[1].0);
    assert_eq!(editor.symbol_id(), scenes[1].0);
    editor.project.validate().unwrap();
    assert!(editor.project.is_scene(&scenes[1].0));
    editor.switch_scene(&root);
    assert_eq!(editor.symbol_id(), root);
    // Deleting the first scene makes the next one the root timeline.
    editor.delete_scene();
    assert_eq!(editor.project.root, scenes[1].0);
    assert!(editor.project.scenes.is_empty());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.scene_list().len(), 2);
}

#[test]
fn loop_off_stops_on_the_last_frame_and_play_all_scenes_continues() {
    let mut editor = editor_with(vec![], vec![]);
    editor.workspace.timeline.loop_playback = false;
    editor.frame = 18;
    editor.set_playing(true);
    for _ in 0..3 {
        let before = editor.frame;
        editor.tick_authored_preview();
        editor.after_playback_tick(before);
    }
    assert!(!editor.playing);
    assert_eq!(editor.frame, 19);
    // Play again from the last frame restarts at the first.
    editor.set_playing(true);
    assert_eq!(editor.frame, 0);
    editor.set_playing(false);

    editor.new_scene();
    let second = editor.current_scene().to_owned();
    let root = editor.project.root.clone();
    editor.switch_scene(&root);
    editor.timeline_edit.play_all_scenes = true;
    editor.workspace.timeline.loop_playback = true;
    editor.frame = 19;
    editor.set_playing(true);
    let before = editor.frame;
    editor.tick_authored_preview();
    editor.after_playback_tick(before);
    assert_eq!(editor.current_scene(), second);
    assert!(editor.playing);
}

#[test]
fn snap_playhead_and_short_rows() {
    let mut editor = editor_with(vec![], vec![]);
    // Key at 10; 8 pt cells: x = 10 * 8 = 80 .. 88, snapping within 4 pt.
    assert_eq!(editor.snap_scrub_frame(9, 77., 0., 8.), 10);
    assert_eq!(editor.snap_scrub_frame(8, 70., 0., 8.), 8);
    assert_eq!(
        editor.workspace.timeline.row_height(),
        neoflash_ui::metrics::LAYER_ROW
    );
    editor.workspace.timeline.short_rows = true;
    assert_eq!(
        editor.workspace.timeline.row_height(),
        neoflash_ui::metrics::LAYER_ROW_SHORT
    );
    // Saved workspace preferences without the timeline block still load.
    let prefs: crate::workspace::WorkspacePrefs = serde_json::from_str("{}").unwrap();
    assert!(prefs.timeline.loop_playback);
}

#[test]
fn edit_multiple_frames_moves_objects_on_every_keyframe_in_range() {
    let mut editor = editor_with(
        vec![element("a", rectangle(), 0.)],
        vec![element("a", rectangle(), 100.)],
    );
    editor.timeline_edit.edit_multiple = true;
    editor.onion_before = 0;
    editor.onion_after = 15;
    assert_eq!(editor.edit_multiple_key_frames(), [0, 10]);
    editor.history.begin(&editor.project);
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements[0]
        .transform
        .x = 5.;
    editor.move_on_other_keyframes(&[("a".into(), [5., 0.])]);
    editor.commit_history();
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes[1].elements[0].transform.x, 105.);
    editor.undo();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[1].elements[0]
            .transform
            .x,
        100.
    );
}

#[test]
fn reverse_frames_from_the_editor() {
    let mut editor = editor_with(
        vec![element("a", rectangle(), 0.)],
        vec![element("a", rectangle(), 100.)],
    );
    let layer = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![layer],
        start: 0,
        end: 10,
    });
    assert!(editor.can_reverse_frames());
    editor.reverse_frames();
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(sample_layer(layer, 0)[0].transform.x, 100.);
    assert_eq!(sample_layer(layer, 10)[0].transform.x, 0.);
}
