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
        ..Element::default()
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
    assert!(editor.status.contains("motion tween needs at least one"));
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
    assert!(
        !editor.can_add_motion_guide_to(1),
        "a guide layer cannot be guided"
    );
    // A guided, locked layer can get a new guide; it replaces the old link.
    editor.symbol_mut().timeline.layers[0].locked = true;
    assert!(editor.can_add_motion_guide_to(0));
    editor.layer = 0;
    editor.add_motion_guide();
    let timeline = &editor.symbol().timeline;
    assert_eq!(timeline.layers.len(), 3);
    assert_eq!(
        timeline.layers[0].properties.motion_guide.as_ref(),
        Some(&timeline.layers[1].id)
    );
    editor.project.validate().unwrap();
    editor.undo();
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

    // Play all scenes is independent of looping: retain the last frame of the first
    // scene, enter the second on the next tick, then stop at the final scene's end.
    editor.set_playing(false);
    editor.switch_scene(&root);
    editor.workspace.timeline.loop_playback = false;
    editor.frame = 18;
    editor.set_playing(true);
    let before = editor.frame;
    editor.tick_authored_preview();
    editor.after_playback_tick(before);
    assert_eq!(
        (editor.current_scene(), editor.frame, editor.playing),
        (root.as_str(), 19, true)
    );
    let before = editor.frame;
    editor.tick_authored_preview();
    editor.after_playback_tick(before);
    assert_eq!(
        (editor.current_scene(), editor.frame, editor.playing),
        (second.as_str(), 0, true)
    );
    for _ in 0..30 {
        if !editor.playing {
            break;
        }
        let before = editor.frame;
        editor.tick_authored_preview();
        editor.after_playback_tick(before);
    }
    assert_eq!(
        (editor.current_scene(), editor.frame, editor.playing),
        (second.as_str(), 19, false)
    );
    editor.project.validate().unwrap();
}

#[test]
fn grouping_objects_on_other_frames_is_safe_and_actionable() {
    let mut editor = editor_with(
        vec![],
        vec![
            element("first", rectangle(), 0.),
            element("second", rectangle(), 30.),
        ],
    );
    editor.frame = 9;
    editor.timeline_edit.edit_multiple = true;
    editor.select_all();
    assert_eq!(editor.selection.targets.len(), 2);
    let before = editor.project.clone();
    editor.group_selection();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.status.contains("current keyframe"));
    editor.project.validate().unwrap();

    // Following the diagnostic works: grouping those objects on their keyframe is
    // still a single undoable operation with the multi-frame view enabled.
    editor.frame = 10;
    editor.select_all();
    editor.group_selection();
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        1
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
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

fn test_wav(samples: usize, amp: i16) -> Vec<u8> {
    let data: Vec<u8> = (0..samples)
        .flat_map(|i| (((i % 16) as i16 - 8) * amp).to_le_bytes())
        .collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    for value in [16u32, 1 | (1 << 16), 8000, 16000, 2 | (16 << 16)] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    bytes
}

#[test]
fn sound_properties_modal_cancel_rename_replace_and_undo() {
    use crate::panels::sound_properties::{
        has_sound_properties_dialog, set_sound_properties_draft_name, sound_properties_draft,
    };

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let original_wav = test_wav(2400, 512);
    project.resources.push(
        ResourceUnit::from_bytes("beep", "audio/beep.wav", "audio/wav", &original_wav).unwrap(),
    );
    project.symbols[0].timeline.layers[0].keyframes[0].sound = Some(FrameSound::new("beep"));
    project.format_version = 6;
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    // Open Sound properties and exercise full modal UI rendering.
    editor.open_sound_properties(&ctx, Some("beep".into()));
    assert!(has_sound_properties_dialog(&ctx));
    assert!(editor.has_modal_dialog_in(&ctx));
    assert_eq!(
        sound_properties_draft(&ctx).unwrap().resource_id,
        "beep".to_owned()
    );
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.sound_properties_dialog(ctx);
    });

    // Mutate draft name + bytes, then Cancel: 0 Undo steps, project untouched.
    set_sound_properties_draft_name(&ctx, "audio/discarded.wav");
    editor
        .replace_sound_properties_draft_bytes(
            &ctx,
            "audio/discarded.wav",
            "audio/wav",
            &test_wav(800, 256),
        )
        .unwrap();
    editor.cancel_sound_properties(&ctx);
    assert!(!has_sound_properties_dialog(&ctx));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());

    // Re-open (resolving via active frame sound), replace bytes + rename, then Confirm (OK):
    // preserves resource ID + FrameSound link in 1 Undo step.
    let replacement_wav = test_wav(4800, 768);
    editor.open_sound_properties(&ctx, None);
    assert!(has_sound_properties_dialog(&ctx));
    editor
        .replace_sound_properties_draft_bytes(
            &ctx,
            "audio/chime.wav",
            "audio/wav",
            &replacement_wav,
        )
        .unwrap();
    editor.confirm_sound_properties(&ctx).unwrap();
    assert!(!has_sound_properties_dialog(&ctx));
    assert_eq!(editor.project.resources.len(), 1);
    assert_eq!(editor.project.resources[0].id, "beep");
    assert_eq!(editor.project.resources[0].path, "audio/chime.wav");
    assert_eq!(
        editor.project.resources[0].decoded_bytes().unwrap(),
        replacement_wav
    );
    assert_eq!(editor.frame_sound().unwrap().1.resource, "beep");
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Direct headless replacement helper also commits in 1 Undo step and validates.
    editor
        .replace_sound_resource_bytes("beep", "audio/ping.wav", "audio/wav", &replacement_wav)
        .unwrap();
    assert_eq!(editor.project.resources[0].path, "audio/ping.wav");
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
}

#[test]
fn layer_properties_modal_cancel_roles_outline_height_and_undo() {
    use crate::panels::layer_properties::{
        LayerHeightChoice, LayerTypeChoice, edit_layer_properties_draft,
        has_layer_properties_dialog, layer_properties_draft,
    };
    use crate::workspace::FramePreview;

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.format_version = 4;
    project.symbols[0].timeline.frame_count = 20;
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        elements: vec![element("a", rectangle(), 0.)],
        ..Default::default()
    }];
    let mut guide = project.symbols[0].timeline.layers[0].clone();
    guide.id = "guide-layer".into();
    guide.name = "Path Guide".into();
    guide.keyframes = vec![Keyframe::default()];
    guide.properties.role = LayerRole::Guide;
    let mut empty = project.symbols[0].timeline.layers[0].clone();
    empty.id = "empty-layer".into();
    empty.name = "Empty".into();
    empty.keyframes = vec![Keyframe::default()];
    project.symbols[0].timeline.layers.push(guide);
    project.symbols[0].timeline.layers.push(empty);
    project.validate().unwrap();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    // Open Layer properties and render one UI pass, then Cancel -> 0 Undo steps.
    editor.open_layer_properties(&ctx, 0);
    assert!(has_layer_properties_dialog(&ctx));
    assert!(editor.has_modal_dialog_in(&ctx));
    assert_eq!(
        layer_properties_draft(&ctx).unwrap().kind,
        LayerTypeChoice::Normal
    );
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.layer_properties_dialog(ctx);
    });
    edit_layer_properties_draft(&ctx, |d| {
        d.name = "Discarded".into();
        d.visible = false;
    });
    editor.cancel_layer_properties(&ctx);
    assert!(!has_layer_properties_dialog(&ctx));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());

    // Keeping the generated default must not create a stored override or an Undo step.
    editor.open_layer_properties(&ctx, 0);
    let default_outline = layer_properties_draft(&ctx).unwrap().outline_color;
    edit_layer_properties_draft(&ctx, |d| d.outline_color = default_outline);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert!(!has_layer_properties_dialog(&ctx));
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.outline_color,
        None
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.project.validate().unwrap();

    // The default depends on the generated layer ID, so a fixed palette colour can equal it.
    // Change one channel to guarantee a distinct override regardless of test order.
    let custom_outline = [
        default_outline.r().wrapping_add(1),
        default_outline.g(),
        default_outline.b(),
    ];

    // Edit layer 0: rename, hide, lock, link as Guided, set outline colour + outline mode + 200% height.
    editor.open_layer_properties(&ctx, 0);
    edit_layer_properties_draft(&ctx, |d| {
        d.name = "Hero".into();
        d.visible = false;
        d.locked = true;
        d.kind = LayerTypeChoice::Guided;
        d.outline_color =
            egui::Color32::from_rgb(custom_outline[0], custom_outline[1], custom_outline[2]);
        d.outline = true;
        d.height = LayerHeightChoice::Preview200;
    });
    editor.confirm_layer_properties(&ctx).unwrap();
    assert!(!has_layer_properties_dialog(&ctx));
    let l0 = &editor.symbol().timeline.layers[0];
    assert_eq!(l0.name, "Hero");
    assert!(!l0.visible);
    assert!(l0.locked);
    assert_eq!(l0.properties.role, LayerRole::Normal);
    assert_eq!(l0.properties.motion_guide.as_deref(), Some("guide-layer"));
    assert_eq!(l0.properties.outline_color, Some(custom_outline));
    assert!(l0.properties.outline);
    assert!(editor.project.format_version >= 6);
    assert_ne!(editor.workspace.timeline.preview, FramePreview::Off);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Non-empty layer 0 cannot become a Folder; empty layer 2 converts to Folder and back cleanly.
    editor.open_layer_properties(&ctx, 0);
    edit_layer_properties_draft(&ctx, |d| d.kind = LayerTypeChoice::Folder);
    assert!(editor.confirm_layer_properties(&ctx).is_err());
    editor.cancel_layer_properties(&ctx);
    assert_eq!(editor.project, before);

    editor.open_layer_properties(&ctx, 2);
    edit_layer_properties_draft(&ctx, |d| d.kind = LayerTypeChoice::Folder);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[2].properties.role,
        LayerRole::Folder
    );
    assert!(editor.symbol().timeline.layers[2].keyframes.is_empty());
    editor.project.validate().unwrap();

    editor.open_layer_properties(&ctx, 2);
    edit_layer_properties_draft(&ctx, |d| d.kind = LayerTypeChoice::Normal);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[2].properties.role,
        LayerRole::Normal
    );
    assert_eq!(editor.symbol().timeline.layers[2].keyframes.len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    undo_restores(&mut editor, &before);

    // Mask + Masked role conversion in 1 Undo step each.
    editor.open_layer_properties(&ctx, 1);
    edit_layer_properties_draft(&ctx, |d| d.kind = LayerTypeChoice::Mask);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.role,
        LayerRole::Mask
    );
    editor.project.validate().unwrap();

    let after_mask = editor.project.clone();
    editor.open_layer_properties(&ctx, 0);
    edit_layer_properties_draft(&ctx, |d| d.kind = LayerTypeChoice::Masked);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[0]
            .properties
            .mask
            .as_deref(),
        Some("guide-layer")
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &after_mask);
    undo_restores(&mut editor, &before);
}

#[test]
fn symbols_popover_filters_large_library_navigates_scenes_and_in_place_stack() {
    use crate::panels::symbols_popover::{PopoverTarget, is_symbols_popover_open, popover_state};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let scene_1 = editor.project.root.clone();
    editor.new_scene();
    let scene_2 = editor.current_scene().to_owned();
    editor.switch_scene(&scene_1);

    let template = editor.project.symbols[0].clone();
    let folders = ["", "Characters/Hero", "UI/Buttons", "FX/Sparks"];
    for i in 0..320 {
        let mut sym = template.clone();
        sym.id = format!("sym_{i:03}");
        sym.name = format!("Asset_{i:03}");
        sym.folder = folders[i % folders.len()].into();
        sym.kind = match i % 3 {
            0 => SymbolKind::MovieClip,
            1 => SymbolKind::Graphic,
            _ => SymbolKind::Button,
        };
        sym.timeline.layers[0].id = format!("sym_layer_{i:03}");
        sym.timeline.layers[0].keyframes.truncate(1);
        editor.project.symbols.push(sym);
    }
    editor.project.validate().unwrap();

    editor.open_symbols_popover(&ctx, None);
    assert!(is_symbols_popover_open(&ctx));

    // Single-pass instant search across >300 symbols and multiple scenes.
    let started = std::time::Instant::now();
    editor.set_symbols_popover_query(&ctx, "asset_042");
    let model = editor.build_symbols_popover_model(&popover_state(&ctx));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(50),
        "filtering 320 symbols took {:?}",
        started.elapsed()
    );
    assert_eq!(
        model.flat_targets(),
        vec![PopoverTarget::Symbol("sym_042".into())]
    );

    // Scene navigation closes the popover and switches the active scene.
    editor.activate_symbols_popover_target(&ctx, PopoverTarget::Scene(scene_2.clone()), false);
    assert!(!is_symbols_popover_open(&ctx));
    assert_eq!(editor.current_scene(), scene_2);
    assert_eq!(editor.editing, vec![scene_2.clone()]);

    // Enter (`in_place = false`) opens [current_scene, symbol] and records Recent.
    editor.open_symbols_popover(&ctx, None);
    editor.activate_symbols_popover_target(&ctx, PopoverTarget::Symbol("sym_001".into()), false);
    assert_eq!(editor.editing, vec![scene_2.clone(), "sym_001".into()]);
    assert_eq!(popover_state(&ctx).recent_symbols[0], "sym_001");

    // Option+Enter (`in_place = true`) pushes onto the breadcrumb stack without cycles.
    editor.open_symbols_popover(&ctx, None);
    editor.activate_symbols_popover_target(&ctx, PopoverTarget::Symbol("sym_002".into()), true);
    assert_eq!(
        editor.editing,
        vec![scene_2.clone(), "sym_001".into(), "sym_002".into()]
    );
    editor.activate_symbols_popover_target(&ctx, PopoverTarget::Symbol("sym_001".into()), true);
    assert_eq!(
        editor.editing,
        vec![scene_2.clone(), "sym_001".into()],
        "re-entering an ancestor truncates instead of forming a breadcrumb cycle"
    );

    // Push > 4 editing levels to exercise both `breadcrumb_overflow` and `symbols_popover` rendering.
    for id in ["sym_002", "sym_003", "sym_004"] {
        editor.activate_symbols_popover_target(&ctx, PopoverTarget::Symbol(id.into()), true);
    }
    assert_eq!(editor.editing.len(), 5);
    editor.open_symbols_popover(&ctx, None);
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0));
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            editor.stage_chrome(ui, canvas);
        });
        editor.symbols_popover(ctx, canvas);
    });
}

#[test]
fn test_scene_isolates_current_scene_timeline_and_exports_clean_bundle() {
    let mut editor = editor_with(vec![element("s1_only_box", rectangle(), 10.)], vec![]);
    let scene_1 = editor.project.root.clone();
    editor.symbol_mut().timeline.layers[0].keyframes.truncate(1);
    editor.symbol_mut().timeline.frame_count = 5;
    editor.symbol_mut().timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.sceneOneSentinel = 111;".into(),
        },
    });

    // Shared library symbol instantiated in Scene 2.
    let mut shared = editor.project.symbols[0].clone();
    shared.id = "shared_mc".into();
    shared.name = "SharedClip".into();
    shared.timeline.frame_count = 3;
    shared.timeline.scripts.clear();
    shared.timeline.layers[0].id = "shared_layer".into();
    shared.timeline.layers[0].keyframes = vec![Keyframe {
        elements: vec![element("shared_shape", rectangle(), 0.)],
        ..Default::default()
    }];
    editor.project.symbols.push(shared);

    editor.new_scene();
    let scene_2 = editor.current_scene().to_owned();
    editor.symbol_mut().timeline.frame_count = 12;
    editor.symbol_mut().timeline.layers[0].keyframes = vec![Keyframe {
        elements: vec![
            element("s2_only_box", rectangle(), 40.),
            element(
                "s2_shared_inst",
                ElementKind::Instance {
                    symbol_id: "shared_mc".into(),
                    playback: InstancePlayback::Independent,
                },
                80.,
            ),
        ],
        ..Default::default()
    }];
    editor.symbol_mut().timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.sceneTwoSentinel = 222;".into(),
        },
    });
    editor.project.validate().unwrap();
    let authored_before = editor.project.clone();

    let single = editor.test_scene_project();
    assert_eq!(single.root, scene_2);
    assert!(single.scenes.is_empty());
    assert!(
        single.symbol(&scene_1).is_none(),
        "other scene symbols must be stripped so export compiles and flattens only the active scene"
    );
    assert_eq!(single.symbol(&scene_2).unwrap().timeline.frame_count, 12);
    assert!(
        single.symbol("shared_mc").is_some(),
        "library symbols remain available to the tested scene"
    );
    single.validate().unwrap();
    assert_eq!(
        editor.project, authored_before,
        "test_scene_project never mutates the authored document"
    );

    // Verify full HTML5 export succeeds on the isolated single-scene project and contains only Scene 2.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!("neoflash-test-scene-{stamp}"));
    let runtime_dir = scratch.join("pkg");
    let out_dir = scratch.join("out");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(
        runtime_dir.join("neoflash_web.js"),
        "var NeoFlashWasm = function() {};",
    )
    .unwrap();
    std::fs::write(
        runtime_dir.join("neoflash_web_bg.wasm"),
        b"\0asm\x01\x00\x00\x00",
    )
    .unwrap();

    let index_html = neoflash_export::export_project(&single, &out_dir, &runtime_dir).unwrap();
    assert!(index_html.exists());
    let bundle_js = std::fs::read_to_string(out_dir.join("bundle.js")).unwrap();
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(bundle_js.contains("s2_only_box"));
    assert!(bundle_js.contains("sceneTwoSentinel"));
    assert!(bundle_js.contains("shared_mc"));
    assert!(!bundle_js.contains("s1_only_box"));
    assert!(!bundle_js.contains("sceneOneSentinel"));
}

#[test]
fn edit_multiple_frames_selects_other_keyframe_only_objects_and_transforms_range_in_one_undo() {
    use crate::app::Tool;
    use crate::tests::{primary_button, rectangle_stage, stage_input_frame};

    let stage_drag =
        |editor: &mut Editor, ctx: &egui::Context, t: f64, from: egui::Pos2, to: egui::Pos2| {
            let mid = from.lerp(to, 0.5);
            stage_input_frame(
                editor,
                ctx,
                t,
                vec![egui::Event::PointerMoved(from), primary_button(from, true)],
            );
            stage_input_frame(editor, ctx, t + 0.02, vec![egui::Event::PointerMoved(mid)]);
            stage_input_frame(editor, ctx, t + 0.04, vec![egui::Event::PointerMoved(to)]);
            stage_input_frame(editor, ctx, t + 0.06, vec![primary_button(to, false)]);
            stage_input_frame(editor, ctx, t + 0.08, vec![]);
        };

    let rect_40x20 = || ElementKind::Rectangle {
        width: 40.,
        height: 20.,
        fill: [40, 120, 220, 255],
        stroke: [0, 0, 0, 0],
        stroke_width: 0.,
    };
    let placed = |id: &str, x: f64, y: f64| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind: rect_40x20(),
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    };

    let (mut editor, ctx, page) = rectangle_stage();
    {
        let timeline = &mut editor.symbol_mut().timeline;
        timeline.frame_count = 20;
        timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![placed("shared", 40., 40.)],
                ..Default::default()
            },
            Keyframe {
                frame: 4,
                elements: vec![placed("shared", 140., 40.)],
                ..Default::default()
            },
            Keyframe {
                frame: 8,
                elements: vec![placed("shared", 240., 40.), placed("only_on_8", 320., 120.)],
                ..Default::default()
            },
            Keyframe {
                frame: 16,
                elements: vec![placed("shared", 400., 40.)],
                ..Default::default()
            },
        ];
    }
    editor.project.validate().unwrap();
    editor.history = History::default();
    editor.frame = 0;
    editor.timeline_edit.edit_multiple = true;
    editor.onion_before = 0;
    editor.onion_after = 10; // Range 0..=10 includes keyframes 0, 4, 8 and excludes 16.
    editor.tool = Tool::Select;
    stage_input_frame(&mut editor, &ctx, 0.02, vec![]);

    let baseline = editor.project.clone();

    // 1. Click and drag "only_on_8" on stage while playhead is at frame 0 (where "only_on_8" does not exist).
    let pick_only_8 = page.min + egui::vec2(335., 130.);
    stage_drag(
        &mut editor,
        &ctx,
        0.1,
        pick_only_8,
        pick_only_8 + egui::vec2(25., -15.),
    );
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some("only_on_8")
    );
    assert_eq!(editor.selected_element().unwrap().id, "only_on_8");
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1,
        "editing an other-keyframe object must not duplicate it onto frame 0"
    );
    let moved_8 = editor.symbol().timeline.layers[0].keyframes[2]
        .elements
        .iter()
        .find(|e| e.id == "only_on_8")
        .unwrap();
    assert_eq!((moved_8.transform.x, moved_8.transform.y), (345., 105.));
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &baseline);

    // 2. Free Transform East-handle resize/scale across keyframes 0, 4, 8 in 1 Undo step (keyframe 16 untouched).
    editor.tool = Tool::Transform;
    let pick_shared = page.min + egui::vec2(55., 50.);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![
            egui::Event::PointerMoved(pick_shared),
            primary_button(pick_shared, true),
            primary_button(pick_shared, false),
        ],
    );
    stage_input_frame(&mut editor, &ctx, 0.32, vec![]);
    assert_eq!(
        editor.selection.single().map(|t| t.element.as_str()),
        Some("shared")
    );

    // East handle of (40..80, 40..60) is at (80, 50); drag to (120, 50) -> 2x horizontal size/scale.
    stage_drag(
        &mut editor,
        &ctx,
        0.4,
        page.min + egui::vec2(80., 50.),
        page.min + egui::vec2(120., 50.),
    );
    let kfs = &editor.symbol().timeline.layers[0].keyframes;
    let effective_width = |el: &Element| match &el.kind {
        ElementKind::Rectangle { width, .. } => width * el.transform.scale_x,
        other => panic!("unexpected element kind: {other:?}"),
    };
    assert!(
        (effective_width(&kfs[0].elements[0]) - 80.).abs() < 1e-4,
        "keyframe 0 width scaled to 80"
    );
    assert!(
        (effective_width(&kfs[1].elements[0]) - 80.).abs() < 1e-4,
        "keyframe 4 width scaled to 80"
    );
    assert!(
        (effective_width(&kfs[2].elements[0]) - 80.).abs() < 1e-4,
        "keyframe 8 width scaled to 80"
    );
    assert!(
        (effective_width(&kfs[3].elements[0]) - 40.).abs() < 1e-4,
        "keyframe 16 outside onion_range must stay untouched"
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &baseline);

    // 3. Free Transform outer-corner rotate across keyframes 0, 4, 8 in 1 Undo step.
    stage_input_frame(&mut editor, &ctx, 0.55, vec![]);
    stage_drag(
        &mut editor,
        &ctx,
        0.6,
        page.min + egui::vec2(88., 68.),
        page.min + egui::vec2(60., 88.),
    );
    let kfs = &editor.symbol().timeline.layers[0].keyframes;
    let rot_0 = kfs[0].elements[0].transform.rotation;
    assert!(
        rot_0.abs() > 5.,
        "expected non-trivial rotation, got {rot_0}"
    );
    assert!(
        (kfs[1].elements[0].transform.rotation - rot_0).abs() < 1e-4,
        "keyframe 4 receives the same rotation delta"
    );
    assert!(
        (kfs[2].elements[0].transform.rotation - rot_0).abs() < 1e-4,
        "keyframe 8 receives the same rotation delta"
    );
    assert_eq!(
        kfs[3].elements[0].transform.rotation, 0.,
        "keyframe 16 outside onion_range must not rotate"
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &baseline);

    // 4. Programmatic `scale_and_rotate_selected_in_edit_range` while parked on in-between frame 2
    // updates keyframes 0, 4, 8 without inserting a spurious keyframe at frame 2.
    editor.frame = 2;
    editor.onion_before = 2;
    editor.onion_after = 8; // Range 0..=10.
    editor.select_single_id("shared".into());
    editor.scale_and_rotate_selected_in_edit_range([1.5, 2.0], 30.0);
    let kfs = &editor.symbol().timeline.layers[0].keyframes;
    assert_eq!(
        kfs.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 4, 8, 16],
        "must not insert an in-between keyframe at frame 2"
    );
    for idx in [0, 1, 2] {
        let t = &kfs[idx].elements[0].transform;
        assert!((t.scale_x - 1.5).abs() < 1e-6);
        assert!((t.scale_y - 2.0).abs() < 1e-6);
        assert!((t.rotation - 30.0).abs() < 1e-6);
    }
    assert_eq!(kfs[3].elements[0].transform.scale_x, 1.0);
    assert_eq!(kfs[3].elements[0].transform.rotation, 0.0);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &baseline);
}

#[test]
fn multi_layer_paste_auto_creates_layers_and_shape_hints_swatches_bitmap_paste_and_sessions_work() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1. Multi-layer frame copy & paste into a timeline with fewer destination layers auto-creates layers.
    editor.symbol_mut().timeline.layers.push(Layer {
        id: "layer-extra".into(),
        name: "Layer 2".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe::default()],
        properties: Default::default(),
    });
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    let l0 = editor.symbol().timeline.layers[0].id.clone();
    let l1 = editor.symbol().timeline.layers[1].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![l1.clone(), l0.clone()],
        start: 0,
        end: 2,
    });
    assert!(editor.copy_frame_range(false));
    // Select the bottom row (`l0`) where only 1 destination layer exists from `first..`.
    let before_paste = editor.project.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![l0.clone()],
        start: 3,
        end: 3,
    });
    editor.paste_frame_range();
    assert_eq!(
        editor.symbol().timeline.layers.len(),
        3,
        "missing destination layer should be auto-created"
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_paste);

    // 2. Shape hints (`add_shape_hint` / `remove_shape_hints`) on a shape tween span.
    let rect_el = |id: &str, x: f64, y: f64| Element {
        id: id.into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 30.0,
            height: 20.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Default::default()
    };
    editor.layer = 0;
    editor.frame = 0;
    editor.symbol_mut().timeline.frame_count = 10;
    editor.symbol_mut().timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Shape,
            elements: vec![rect_el("s1", 10.0, 10.0)],
            ..Default::default()
        },
        Keyframe {
            frame: 9,
            elements: vec![rect_el("s2", 80.0, 80.0)],
            ..Default::default()
        },
    ];
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    let before_hints = editor.project.clone();
    assert_eq!(editor.add_shape_hint(), Some('a'));
    assert_eq!(editor.add_shape_hint(), Some('b'));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .shape_hints
            .len(),
        2
    );
    editor.project.validate().unwrap();
    assert!(editor.remove_shape_hints());
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .shape_hints
            .is_empty()
    );
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, before_hints);

    // 3. Document-level custom swatches (`add_active_swatch` / `delete_custom_swatch`).
    editor.fill = [12, 34, 56, 255];
    editor.add_active_swatch(&ctx);
    assert!(
        editor
            .project
            .custom_swatches()
            .contains(&[12, 34, 56, 255])
    );
    assert!(editor.document_swatch_colors().contains(&[12, 34, 56, 255]));
    editor.project.validate().unwrap();
    assert!(editor.delete_custom_swatch([12, 34, 56, 255]));
    assert!(
        !editor
            .project
            .custom_swatches()
            .contains(&[12, 34, 56, 255])
    );

    // 4. Bitmap paste (`paste_image_bytes` and `data:image/png;base64,...`).
    let mut png_buf = std::io::Cursor::new(Vec::new());
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 128, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut png_buf, image::ImageFormat::Png)
        .unwrap();
    let png_bytes = png_buf.into_inner();
    let before_bmp = editor.project.clone();
    assert!(editor.paste_image_bytes(&ctx, &png_bytes, "image/png", true));
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_bmp);

    // 5. Variable-width pressure brush outline & timeline row virtualization range.
    let pts = vec![
        egui::pos2(0.0, 0.0),
        egui::pos2(20.0, 0.0),
        egui::pos2(40.0, 10.0),
    ];
    let outline = crate::authoring_tools::brush_outline_with_pressure(&pts, &[0.3, 0.8, 1.4], 12.0);
    assert!(outline.len() >= 6);
    let vis = crate::panels::timeline::visible_layer_row_range(100.0, 220.0, 0.0, 24.0, 50);
    assert!(vis.start >= 2 && vis.end <= 12);

    // 6. Named workspace presets and multi-document session tabs.
    assert!(editor.workspace.save_named_preset("My Studio"));
    assert!(editor.workspace.apply_named_preset("My Studio"));
    assert!(editor.workspace.delete_named_preset("My Studio"));
    assert_eq!(editor.session_count(), 1);
    let mut second = Project::blank();
    second.name = "Second Tab".into();
    let idx = editor.open_new_session_tab(&ctx, second, None);
    assert_eq!(idx, 1);
    assert_eq!(editor.session_count(), 2);
    assert_eq!(editor.project.name, "Second Tab");
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_ne!(editor.project.name, "Second Tab");
    assert!(editor.close_session_tab(&ctx, 1));
    assert_eq!(editor.session_count(), 1);

    // 7. Group/multi-selection skew, gap closing modes, stroke-segment eraser mode, and external modification reload.
    editor.select_all();
    assert!(editor.skew_selection(&ctx, 15.0, 0.0));
    editor.project.validate().unwrap();
    for mode in [
        crate::authoring_tools::GapCloseMode::None,
        crate::authoring_tools::GapCloseMode::Small,
        crate::authoring_tools::GapCloseMode::Medium,
        crate::authoring_tools::GapCloseMode::Large,
    ] {
        editor.authoring_tools.gap_close = mode;
        assert!(editor.authoring_tools.gap_close.max_gap_px() >= 0.0);
    }
    editor.authoring_tools.eraser_mode = crate::authoring_tools::EraserMode::StrokeSegments;
    editor.sync_disk_mtime();
    assert!(!editor.check_external_modification());
    assert!(!editor.reload_external_modification(&ctx));
}

#[test]
fn layer_blend_mode_sync_timeline_first_frame_and_alt_drag_symbol_swap_with_undo() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1. Layer blend mode via timeline command and via Filters panel when no object is selected.
    let before_blend = editor.project.clone();
    editor.clear_selection();
    assert_eq!(editor.layer_blend_mode(0), BlendMode::Normal);
    assert!(editor.set_layer_blend_mode(0, BlendMode::Multiply));
    assert_eq!(editor.layer_blend_mode(0), BlendMode::Multiply);
    assert_eq!(editor.project.format_version, 8);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_blend);

    editor.clear_selection();
    editor.set_selected_blend_mode(BlendMode::Screen);
    assert_eq!(editor.layer_blend_mode(0), BlendMode::Screen);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_blend);

    // 2. Layer timeline-sync (`sync_timeline`) toggle with Undo and format-8 validation.
    let before_sync = editor.project.clone();
    assert!(!editor.layer_sync_timeline(0));
    assert!(editor.set_layer_sync_timeline(0, true));
    assert!(editor.layer_sync_timeline(0));
    assert_eq!(editor.project.format_version, 8);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_sync);

    // 3. `set_selected_instance_first_frame` on a MovieClip and Graphic instance, plus AE-style
    // `swap_layer_or_selected_symbol` when the layer is selected (even with empty object selection).
    let make_mc_symbol = |id: &str, name: &str, frames: u32| Symbol {
        id: id.into(),
        name: name.into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: frames,
            layers: vec![Layer {
                id: format!("{id}-layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: format!("{id}-rect"),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 30.0,
                            height: 30.0,
                            fill: [200, 100, 50, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                properties: Default::default(),
            }],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    editor
        .project
        .symbols
        .push(make_mc_symbol("sym-a", "HeroA", 12));
    editor
        .project
        .symbols
        .push(make_mc_symbol("sym-b", "HeroB", 8));
    editor.symbol_mut().timeline.frame_count = 10;
    editor.symbol_mut().timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            tween: Some(Easing::Linear),
            elements: vec![Element {
                id: "hero-inst".into(),
                name: "hero".into(),
                transform: Transform {
                    x: 25.0,
                    y: 40.0,
                    ..Default::default()
                },
                blend_mode: BlendMode::Overlay,
                kind: ElementKind::Instance {
                    symbol_id: "sym-a".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Default::default()
            }],
            ..Default::default()
        },
        Keyframe {
            frame: 9,
            elements: vec![Element {
                id: "hero-inst".into(),
                name: "hero".into(),
                transform: Transform {
                    x: 125.0,
                    y: 40.0,
                    ..Default::default()
                },
                blend_mode: BlendMode::Overlay,
                kind: ElementKind::Instance {
                    symbol_id: "sym-a".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Default::default()
            }],
            ..Default::default()
        },
    ];
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();

    // Setting `First frame` to 7 (1-based) on the MovieClip instance switches playback to LoopFrom(6).
    let before_first = editor.project.clone();
    editor.frame = 0;
    editor.select_single_id("hero-inst".into());
    assert!(editor.set_selected_instance_first_frame(7));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "sym-a".into(),
            playback: InstancePlayback::LoopFrom(6),
        }
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_first);

    // AE-style Alt-drag replacement on the selected layer (with no stage object selected) swaps
    // the instance symbol across all keyframes of that layer in one Undo step.
    let before_swap = editor.project.clone();
    editor.clear_selection();
    editor.layer = 0;
    editor.frame = 0;
    assert!(editor.can_swap_layer_symbol(0, "sym-b"));
    assert!(!editor.can_swap_layer_symbol(0, "sym-a"));
    assert!(editor.swap_layer_or_selected_symbol(0, "sym-b"));
    for kf in &editor.symbol().timeline.layers[0].keyframes {
        assert_eq!(
            kf.elements[0].kind,
            ElementKind::Instance {
                symbol_id: "sym-b".into(),
                playback: InstancePlayback::Independent,
            }
        );
        assert_eq!(kf.elements[0].name, "hero");
        assert_eq!(kf.elements[0].blend_mode, BlendMode::Overlay);
    }
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_swap);

    // Exporting the document or a Library symbol to Lottie JSON writes valid Bodymovin v5.7.14 JSON
    // without mutating the document or Undo history.
    let before_lottie = editor.project.clone();
    let dir = std::env::temp_dir().join(format!("neoflash-lottie-test-{}", std::process::id()));
    let movie_lottie = dir.join("scene.lottie.json");
    let sym_lottie = dir.join("sym-b.lottie.json");
    let movie_report = editor.export_lottie_to_path(&movie_lottie, None).unwrap();
    assert!(movie_report.bytes > 0);
    assert!(movie_report.layer_count >= 1);
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&movie_lottie).unwrap()).unwrap();
    assert_eq!(parsed["v"], neoflash_export::LOTTIE_VERSION);
    let sym_report = editor
        .export_lottie_to_path(&sym_lottie, Some("sym-b"))
        .unwrap();
    assert_eq!(sym_report.name, "HeroB");
    assert_eq!(sym_report.frame_count, 8);

    // Opening the R-043 `Export Lottie` modal preselects the symbol, disables background for symbol
    // exports, computes live compatibility estimates, and renders both the main dialog and the
    // compatibility notes modal without mutating the document or Undo history.
    editor.open_export_lottie(Some("sym-b"));
    let draft = editor.export_lottie.as_ref().unwrap();
    assert!(!draft.options.include_background);
    assert!(draft.preview_report.is_some());
    for theme in [neoflash_ui::Theme::Dark, neoflash_ui::Theme::Light] {
        neoflash_ui::theme::install(&ctx, theme);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            editor.export_lottie_modal(ctx);
        });
    }
    editor.export_lottie.as_mut().unwrap().show_notes_modal = true;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        editor.export_lottie_modal(ctx);
    });
    editor.export_lottie = None;

    assert_eq!(editor.project, before_lottie);
    assert!(!editor.history.can_undo());
    let _ = std::fs::remove_dir_all(&dir);

    // R-040 & R-042 in `Layer properties`: editing `blend_mode` and `sync_timeline` in the draft
    // and confirming commits both in one `"Layer properties"` Undo step; inherited folder blend
    // mode is formatted as `"Blending: Inherited (<mode>)"`.
    let before_dialog = editor.project.clone();
    editor.open_layer_properties(&ctx, 0);
    crate::panels::layer_properties::edit_layer_properties_draft(&ctx, |draft| {
        draft.blend_mode = BlendMode::Screen;
        draft.sync_timeline = true;
    });
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(editor.layer_blend_mode(0), BlendMode::Screen);
    assert!(editor.layer_sync_timeline(0));
    assert_eq!(
        crate::layer_controls::layer_blending_select_label(
            BlendMode::Normal,
            Some(BlendMode::Multiply)
        ),
        "Blending: Inherited (Multiply)"
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before_dialog);

    // R-045 Layer Solo: transient view state filters Stage evaluation to soloed layers (and
    // propagates folder solo to children) without mutating `project`, `layer.visible`, or Undo.
    editor.symbol_mut().timeline.layers.push(Layer {
        id: "bg-layer".into(),
        name: "Background".into(),
        visible: true,
        locked: false,
        properties: Default::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "bg-rect".into(),
                name: "bg".into(),
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 80.0,
                    fill: [10, 20, 30, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
    });
    editor.history = History::new(150);
    let before_solo = editor.project.clone();

    // If an object on a layer had an element-level blend mode override, setting the layer's
    // blend mode from the Timeline clears the element override so the layer mode takes effect.
    editor.select_single_id("bg-rect".into());
    assert!(editor.set_selected_blend_mode(BlendMode::Difference));
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0].elements[0].blend_mode,
        BlendMode::Difference
    );
    assert!(editor.set_layer_blend_mode(1, BlendMode::Overlay));
    assert_eq!(editor.layer_blend_mode(1), BlendMode::Overlay);
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0].elements[0].blend_mode,
        BlendMode::Normal
    );
    assert!(editor.current_authored_scene(true).has_blend_modes());
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    editor.clear_selection();
    editor.history = History::new(150);
    assert_eq!(editor.project, before_solo);

    assert!(!editor.has_any_solo_layers());
    assert_eq!(editor.current_authored_scene(true).draws.len(), 2);

    // Click solo on layer 0: only layer 0 renders on Stage; layer 1's `visible` flag stays true.
    assert!(editor.toggle_layer_solo(0, false));
    assert!(editor.has_any_solo_layers());
    assert!(editor.is_layer_effectively_soloed(0));
    assert!(!editor.is_layer_effectively_soloed(1));
    assert!(editor.symbol().timeline.layers[1].visible);
    let solo_scene = editor.current_authored_scene(true);
    assert_eq!(solo_scene.draws.len(), 1);
    assert_eq!(
        solo_scene.draws[0]
            .item
            .instance_path
            .first()
            .map(String::as_str),
        Some("hero-inst")
    );

    // Click solo on layer 1 too: both layers are now soloed.
    assert!(editor.toggle_layer_solo(1, false));
    assert!(editor.is_layer_effectively_soloed(0));
    assert!(editor.is_layer_effectively_soloed(1));
    assert_eq!(editor.current_authored_scene(true).draws.len(), 2);

    // ⌥-click solo on layer 1: exclusively solos layer 1; ⌥-clicking layer 1 again clears solo.
    assert!(editor.toggle_layer_solo(1, true));
    assert!(!editor.is_layer_effectively_soloed(0));
    assert!(editor.is_layer_effectively_soloed(1));
    assert!(editor.toggle_layer_solo(1, true));
    assert!(!editor.has_any_solo_layers());
    assert_eq!(editor.project, before_solo);
    assert!(!editor.history.can_undo());

    // Timeline UI renders the R-040 `Mode` cell and R-045 `Solo` toggle + header clear button.
    ctx.enable_accesskit();
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        },
        |ctx| editor.timeline(ctx),
    );
    let labels: Vec<String> = out
        .platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .filter_map(|(_, node)| node.label().map(str::to_owned))
        .collect();
    assert!(
        labels.iter().any(|l| l == "Clear solo on all layers"),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l == "Solo layer"), "{labels:?}");
    assert!(
        labels.iter().any(|l| l == "Blend mode: Normal"),
        "{labels:?}"
    );
}

#[test]
fn mask_context_command_links_displayed_lower_sibling_and_roundtrips_with_one_undo() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.format_version = 4;
    let original = project.symbols[0].timeline.layers[0].clone();
    let mut layers = Vec::new();
    for id in ["outside", "inside-bottom", "inside-top", "folder"] {
        let mut layer = original.clone();
        layer.id = id.into();
        layer.name = id.into();
        layer.keyframes[0].elements = vec![element(&format!("{id}-art"), rectangle(), 0.)];
        if id.starts_with("inside") {
            layer.properties.parent = Some("folder".into());
        }
        if id == "folder" {
            layer.properties.role = LayerRole::Folder;
            layer.keyframes.clear();
        }
        layers.push(layer);
    }
    project.symbols[0].timeline.layers = layers;
    project.validate().unwrap();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();
    editor.set_layer_role(2, LayerRole::Mask);
    let layers = &editor.symbol().timeline.layers;
    assert_eq!(layers[1].properties.mask.as_deref(), Some("inside-top"));
    assert!(
        layers[0].properties.mask.is_none(),
        "mask stays within its folder"
    );
    assert_eq!(
        crate::layer_controls::visible_layer_indices(&editor.symbol().timeline),
        [3, 2, 1, 0]
    );
    editor.project.validate().unwrap();
    let reopened = Project::from_json(&editor.project.to_json().unwrap()).unwrap();
    reopened.validate().unwrap();
    assert_eq!(reopened, editor.project);
    undo_restores(&mut editor, &before);
    assert!(!editor.history.can_undo());
    editor.set_layer_role(2, LayerRole::Mask);
    editor.project.format_version = 9;
    editor.symbol_mut().timeline.layers[2].properties.mask_mode = MaskMode::Alpha;
    editor.history = History::new(150);
    let alpha_before = editor.project.clone();
    editor.set_layer_role(2, LayerRole::Normal);
    assert_eq!(
        editor.symbol().timeline.layers[2].properties.mask_mode,
        MaskMode::Geometry
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &alpha_before);
    use crate::panels::layer_properties::{LayerTypeChoice, edit_layer_properties_draft};
    editor.open_layer_properties(&ctx, 2);
    edit_layer_properties_draft(&ctx, |draft| draft.kind = LayerTypeChoice::Normal);
    editor.confirm_layer_properties(&ctx).unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[2].properties.mask_mode,
        MaskMode::Geometry
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &alpha_before);
    editor.project = before.clone();
    editor.history = History::new(150);
    // The bottom child has no eligible sibling below in its folder; unrelated root artwork stays free.
    editor.set_layer_role(1, LayerRole::Mask);
    assert!(
        editor
            .symbol()
            .timeline
            .layers
            .iter()
            .all(|l| l.properties.mask.is_none())
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
}

#[test]
fn solo_hidden_layers_are_excluded_from_selection_and_edits_and_reconciled_on_toggle() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let mut l0 = project.symbols[0].timeline.layers[0].clone();
    l0.id = "l0".into();
    l0.name = "Bottom".into();
    l0.keyframes[0].elements = vec![element("bottom-box", rectangle(), 0.)];
    let mut l1 = l0.clone();
    l1.id = "l1".into();
    l1.name = "Top".into();
    l1.keyframes[0].elements = vec![element("top-box", rectangle(), 80.)];
    project.symbols[0].timeline.layers = vec![l0, l1];
    project.validate().unwrap();
    let before = project.clone();

    let mut editor = Editor::from_project(&ctx, project);
    editor.select_all();
    assert_eq!(editor.selection.targets.len(), 2);

    // Toggling Solo on the top layer immediately drops the non-soloed bottom layer from selection.
    editor.toggle_layer_solo(1, false);
    assert_eq!(editor.selection.targets.len(), 1);
    assert_eq!(editor.selection.targets[0].element, "top-box");

    // Select All while Solo is active selects only solo-visible artwork.
    editor.clear_selection();
    editor.select_all();
    assert_eq!(editor.selection.targets.len(), 1);
    assert_eq!(editor.selection.targets[0].element, "top-box");

    // Deleting selection while Solo is active removes only the solo-visible top-box; bottom-box survives.
    editor.delete_selected();
    editor.clear_all_solo_layers();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        1
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].id,
        "bottom-box"
    );
    assert!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .is_empty()
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
}

#[test]
fn mask_luminance_and_invert_modes_and_compact_row_height_roundtrip_with_single_undo() {
    use crate::panels::layer_properties::{
        LayerHeightChoice, LayerTypeChoice, edit_layer_properties_draft,
    };
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let mut content = project.symbols[0].timeline.layers[0].clone();
    content.id = "content".into();
    content.name = "Content".into();
    content.keyframes[0].elements = vec![element("box", rectangle(), 0.)];
    let mut mask = content.clone();
    mask.id = "mask".into();
    mask.name = "Mask".into();
    mask.keyframes[0].elements = vec![element("mask-shape", rectangle(), 0.)];
    project.symbols[0].timeline.layers = vec![content, mask];
    project.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, project);
    editor.set_layer_role(1, LayerRole::Mask);
    let after_role = editor.project.clone();

    editor.set_layer_mask_mode(1, MaskMode::Luminance);
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.mask_mode,
        MaskMode::Luminance
    );
    assert_eq!(editor.project.format_version, 9);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &after_role);

    editor.set_layer_mask_invert(1, true);
    assert!(editor.symbol().timeline.layers[1].properties.mask_invert);
    assert_eq!(editor.project.format_version, 9);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &after_role);

    // Layer properties modal edits mask_mode, mask_invert and Compact layer height in one Undo step.
    editor.open_layer_properties(&ctx, 1);
    edit_layer_properties_draft(&ctx, |d| {
        d.kind = LayerTypeChoice::Mask;
        d.mask_mode = MaskMode::Luminance;
        d.mask_invert = true;
        d.height = LayerHeightChoice::Compact;
    });
    editor.confirm_layer_properties(&ctx).unwrap();
    let l1 = &editor.symbol().timeline.layers[1];
    assert_eq!(l1.properties.mask_mode, MaskMode::Luminance);
    assert!(l1.properties.mask_invert);
    assert!(editor.workspace.timeline.compact_rows);
    assert_eq!(
        editor.workspace.timeline.row_height(),
        crate::workspace::TimelineView::LAYER_ROW_COMPACT
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &after_role);

    // Inline mask row icons fit strictly inside the 64 pt Mode column (even on 16 pt Compact rows)
    // without overlapping the Visibility eye toggle, and respond to real pointer clicks.
    use crate::tests::primary_button;
    ctx.enable_accesskit();
    editor.workspace.timeline.compact_rows = true;
    editor.workspace.timeline.preview = crate::workspace::FramePreview::Off;
    assert_eq!(editor.workspace.timeline.row_height(), 16.0);
    let mut time = 0.0;
    let mut render_tl = |editor: &mut Editor, events: Vec<egui::Event>| {
        time += 0.1;
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 480.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| editor.timeline(ctx),
        )
    };
    let find_rect = |out: &egui::FullOutput, label: &str| -> egui::Rect {
        let (_, node) = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .unwrap_or_else(|| panic!("expected node {label:?}"));
        let b = node.bounds().expect("bounds");
        egui::Rect::from_min_max(
            egui::pos2(b.x0 as f32, b.y0 as f32),
            egui::pos2(b.x1 as f32, b.y1 as f32),
        )
    };
    let out = render_tl(&mut editor, vec![]);
    let mode_rect = find_rect(&out, "Mask mode: Geometry");
    let chan_rect = find_rect(&out, "Mask channel");
    let inv_rect = find_rect(&out, "Invert mask");
    assert!(
        chan_rect.height() <= 16.0 && inv_rect.height() <= 16.0,
        "Compact 16 pt row must clamp inline mask icon height: chan={chan_rect:?}, inv={inv_rect:?}"
    );
    assert!(
        inv_rect.right() - mode_rect.left() <= 64.0 + 1e-3,
        "Mode button + 2 inline mask icons must fit strictly within the 64 pt Mode cell: mode={mode_rect:?}, inv={inv_rect:?}"
    );

    let click_pos =
        |editor: &mut Editor,
         render_tl: &mut dyn FnMut(&mut Editor, Vec<egui::Event>) -> egui::FullOutput,
         at: egui::Pos2| {
            render_tl(
                editor,
                vec![egui::Event::PointerMoved(at), primary_button(at, true)],
            );
            render_tl(editor, vec![primary_button(at, false)]);
            render_tl(editor, vec![])
        };
    // Click channel icon: Geometry -> Alpha -> Luminance -> Geometry.
    let chan_center = chan_rect.center();
    click_pos(&mut editor, &mut render_tl, chan_center);
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.mask_mode,
        MaskMode::Alpha
    );
    click_pos(&mut editor, &mut render_tl, chan_center);
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.mask_mode,
        MaskMode::Luminance
    );
    click_pos(&mut editor, &mut render_tl, chan_center);
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.mask_mode,
        MaskMode::Geometry
    );
    // Click invert icon: false -> true.
    let inv_center = inv_rect.center();
    click_pos(&mut editor, &mut render_tl, inv_center);
    assert!(editor.symbol().timeline.layers[1].properties.mask_invert);
    editor.project.validate().unwrap();
}

#[test]
fn multi_row_layer_selection_and_drag_reparent_into_folders_commit_in_one_undo_step() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    let mut l0 = project.symbols[0].timeline.layers[0].clone();
    l0.id = "l0".into();
    l0.name = "L0".into();
    l0.keyframes[0].elements = vec![element("e0", rectangle(), 0.)];
    let mut l1 = l0.clone();
    l1.id = "l1".into();
    l1.name = "L1".into();
    l1.keyframes[0].elements = vec![element("e1", rectangle(), 20.)];
    let mut folder = l0.clone();
    folder.id = "folder".into();
    folder.name = "Folder".into();
    folder.properties.role = LayerRole::Folder;
    folder.properties.collapsed = true;
    folder.keyframes.clear();
    project.symbols[0].timeline.layers = vec![l0, l1, folder];
    project.format_version = 4;
    project.validate().unwrap();
    let before = project.clone();

    let mut editor = Editor::from_project(&ctx, project);
    editor.select_layer_row(0, false, false);
    editor.select_layer_row(1, false, true);
    assert_eq!(editor.selected_layer_indices(), vec![0, 1]);
    assert!(editor.is_layer_row_selected(0));
    assert!(editor.is_layer_row_selected(1));

    // Batch visibility toggle on selected rows -> 1 Undo step.
    editor.toggle_selected_layers_visibility(0);
    assert!(!editor.symbol().timeline.layers[0].visible);
    assert!(!editor.symbol().timeline.layers[1].visible);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Batch blend mode change on selected rows -> 1 Undo step.
    assert!(editor.set_layer_blend_mode(1, BlendMode::Multiply));
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.blend_mode,
        BlendMode::Multiply
    );
    assert_eq!(
        editor.symbol().timeline.layers[1].properties.blend_mode,
        BlendMode::Multiply
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Drag-reparent selected layers into `folder` -> uncollapses folder and reparents both in 1 Undo step.
    editor.reparent_timeline_layer_to_folder("l1", Some("folder"));
    let layers = &editor.symbol().timeline.layers;
    assert_eq!(layers[0].properties.parent.as_deref(), Some("folder"));
    assert_eq!(layers[1].properties.parent.as_deref(), Some("folder"));
    assert!(!layers[2].properties.collapsed);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Drop single layer into folder via visual drop -> 1 Undo step.
    editor.select_layer_row(0, false, false);
    editor.drop_timeline_layer_visual("l0", Some("l1"), Some(Some("folder")));
    let l0_after = editor
        .symbol()
        .timeline
        .layers
        .iter()
        .find(|l| l.id == "l0")
        .unwrap();
    assert_eq!(l0_after.properties.parent.as_deref(), Some("folder"));
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Multi-selected boundary drop moves AND reparents all selected rows together in 1 Undo step.
    editor.select_layer_row(0, false, false);
    editor.select_layer_row(1, false, true);
    editor.drop_timeline_layer_visual("l0", None, Some(Some("folder")));
    for id in ["l0", "l1"] {
        let layer = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == id)
            .unwrap();
        assert_eq!(
            layer.properties.parent.as_deref(),
            Some("folder"),
            "{id} should be reparented into folder"
        );
    }
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
}

#[test]
fn t100_library_lifecycle_movie_explorer_and_smart_rename() {
    use crate::panels::find_replace::{
        CaseTransform, CollisionPolicy, RenameNamespace, RenameScope, SmartRenameOptions,
    };

    let mut project = Project::blank();
    project.symbols.push(Symbol {
        id: "unused-sym".into(),
        name: "UnusedHero".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "ul".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            }],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    });
    project.symbols.push(Symbol {
        id: "used-sym".into(),
        name: "UsedBadge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "bl".into(),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            }],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    });
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "inst-1".into(),
            name: "hero_btn_old".into(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: "used-sym".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    let (unused_syms, _) = editor.select_unused_library_items();
    assert_eq!(unused_syms, vec!["unused-sym".to_owned()]);
    let (removed_syms, _) = editor.delete_unused_library_items();
    assert_eq!(removed_syms, 1);
    assert!(editor.project.symbol("unused-sym").is_none());
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);

    // Movie Explorer actions: select instances on current frame & copy all text.
    assert_eq!(
        editor.select_symbol_instances_on_current_frame("used-sym"),
        1
    );
    assert!(!editor.copy_all_movie_explorer_text().is_empty());

    // Bulk Library folder move & symbol kind change.
    assert_eq!(
        editor.bulk_move_library_symbols(&["used-sym".into()], "Actors"),
        Ok(1)
    );
    assert_eq!(
        editor.bulk_set_symbol_kind(&["used-sym".into()], SymbolKind::Graphic),
        Ok(1)
    );
    editor.undo();
    undo_restores(&mut editor, &before);

    // Smart Rename 2.0 with regex capture groups and case transform.
    let opts = SmartRenameOptions {
        scope: RenameScope::ActiveSymbol,
        namespace: RenameNamespace::InstanceName,
        find: "^([a-z]+)_([a-z]+)_old$".into(),
        replace_template: "$1_$2_{n:02}".into(),
        regex: true,
        match_case: true,
        case_transform: CaseTransform::CamelCase,
        collision_policy: CollisionPolicy::AutoNumber,
        start_number: 1,
        step_number: 1,
    };
    let preview = editor.preview_smart_rename(&opts);
    assert_eq!(preview.renamed_count, 1);
    assert_eq!(preview.preview[0].after, "heroBtn01");

    let report = editor.apply_smart_rename(&opts);
    assert_eq!(report.renamed_count, 1);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].name,
        "heroBtn01"
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
}

#[test]
fn t103_history_command_steps_and_saved_command_replay() {
    use neoflash_core::AuthoringCommandStep;

    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "box-1".into(),
            name: "Box".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 30.0,
                fill: [200, 100, 50, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        });
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();
    editor.select_single_id("box-1".into());

    let applied = editor
        .execute_command_steps(
            "Move and tint",
            &[
                AuthoringCommandStep::Translate { dx: 15.0, dy: -5.0 },
                AuthoringCommandStep::SetAlpha { alpha: 0.5 },
            ],
        )
        .expect("execute steps");
    assert_eq!(applied, 2);
    let el = &editor.symbol().timeline.layers[0].keyframes[0].elements[0];
    assert!((el.transform.x - 15.0).abs() < 1e-6);
    assert!((el.transform.y - -5.0).abs() < 1e-6);
    assert!((el.transform.alpha - 0.5).abs() < 1e-6);

    let json = editor.copy_history_steps_json(&[0]).expect("step json");
    let saved = neoflash_core::SavedCommand::from_json(&json).expect("round-trip saved command");
    editor.run_saved_command(&saved).expect("run saved command");
    editor.repeat_last_command().expect("repeat last command");
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn history_selection_mutations_apply_to_every_selected_layer() {
    use crate::selection::SelectionTarget;
    use neoflash_core::AuthoringCommandStep;

    let mut project = Project::blank();
    let first_layer = project.symbols[0].timeline.layers[0].id.clone();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "first".into(),
            name: "First".into(),
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let mut second = project.symbols[0].timeline.layers[0].clone();
    second.id = "second-layer".into();
    second.name = "Second".into();
    second.keyframes[0].elements[0].id = "second".into();
    second.keyframes[0].elements[0].name = "Second".into();
    project.symbols[0].timeline.layers.push(second);
    let before = project.clone();

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let original_targets = vec![
        SelectionTarget {
            layer: first_layer,
            element: "first".into(),
        },
        SelectionTarget {
            layer: "second-layer".into(),
            element: "second".into(),
        },
    ];
    editor.set_selection(original_targets.clone());
    let applied = editor
        .execute_command_steps(
            "Duplicate both layers",
            &[AuthoringCommandStep::DuplicateSelection {
                offset_x: 4.0,
                offset_y: 6.0,
            }],
        )
        .expect("duplicate both layers");
    assert_eq!(applied, 2);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        2
    );
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .len(),
        2
    );
    assert_eq!(editor.selection.targets.len(), 2);
    assert!(
        editor
            .selection
            .targets
            .iter()
            .all(|target| target.element != "first" && target.element != "second")
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);

    editor.set_selection(original_targets);
    let applied = editor
        .execute_command_steps(
            "Delete both layers",
            &[AuthoringCommandStep::DeleteSelection],
        )
        .expect("delete both layers");
    assert_eq!(applied, 2);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .elements
            .is_empty()
    );
    assert!(editor.selection.is_empty());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn history_replay_rejects_cross_layer_duplicate_ids_atomically() {
    use crate::selection::SelectionTarget;
    use neoflash_core::AuthoringCommandStep;

    let mut project = Project::blank();
    let first_layer = project.symbols[0].timeline.layers[0].id.clone();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "duplicate-import-id".into(),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255; 4],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let mut unselected = project.symbols[0].timeline.layers[0].clone();
    unselected.id = "unselected-layer".into();
    unselected.name = "Imported duplicate ID".into();
    project.symbols[0].timeline.layers.push(unselected);
    assert!(
        project.validate().is_err(),
        "fixture must model corrupt import data"
    );

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project.clone());
    editor.set_selection(vec![SelectionTarget {
        layer: first_layer,
        element: "duplicate-import-id".into(),
    }]);
    assert!(
        editor
            .execute_command_steps(
                "Move selected layer only",
                &[AuthoringCommandStep::Translate { dx: 5.0, dy: 0.0 }],
            )
            .is_err()
    );
    assert_eq!(editor.project, project, "failed replay must be atomic");
    assert!(!editor.history.can_undo());
}

#[test]
fn t104_t105_curve_split_planar_bucket_brush_modes_and_variable_width() {
    use neoflash_core::{
        BrushPaintMode, StrokeProfilePreset, StrokeWidthProfile, apply_brush_stroke_with_mode,
        expand_variable_width_stroke, paint_bucket_planar_face, split_bezier_path_with_segments,
    };

    let curve = vec![
        PathCommand::MoveTo { x: 0.0, y: 0.0 },
        PathCommand::CubicTo {
            cx1: 30.0,
            cy1: 60.0,
            cx2: 70.0,
            cy2: 60.0,
            x: 100.0,
            y: 0.0,
        },
    ];
    let pieces = split_bezier_path_with_segments(&curve, &[([50.0, -20.0], [50.0, 80.0])]);
    assert_eq!(pieces.len(), 2);
    assert!(
        pieces[0]
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. }))
    );

    let tapered = expand_variable_width_stroke(
        &curve,
        6.0,
        &StrokeWidthProfile::from_preset(StrokeProfilePreset::TaperBoth),
    );
    assert!(tapered.len() >= 4);

    // Planar Paint Bucket enclosed by 4 separate stroke lines.
    let mk_line = |id: &str, x0: f64, y0: f64, x1: f64, y1: f64| Element {
        id: id.into(),
        name: String::new(),
        transform: Transform::default(),
        merge: true,
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: x0, y: y0 },
                PathCommand::LineTo { x: x1, y: y1 },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 0],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    };
    let lines = vec![
        mk_line("s1", 0.0, 0.0, 50.0, 0.0),
        mk_line("s2", 50.0, 0.0, 50.0, 50.0),
        mk_line("s3", 50.0, 50.0, 0.0, 50.0),
        mk_line("s4", 0.0, 50.0, 0.0, 0.0),
    ];
    let face = paint_bucket_planar_face(&lines, [25.0, 25.0], 2.0)
        .expect("bounded planar bucket")
        .expect("planar bucket face");
    assert!(face.len() >= 4);

    // Brush PaintBehind inserts behind existing elements.
    let mut els = vec![lines[0].clone()];
    let brush_el = Element {
        id: "b1".into(),
        name: String::new(),
        transform: Transform::default(),
        merge: true,
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    assert!(
        !apply_brush_stroke_with_mode(&mut els, brush_el, BrushPaintMode::PaintBehind, &[], None)
            .is_empty()
    );
    assert_eq!(els[0].id, "b1");
}

#[test]
fn t106_t107_rich_text_html_outlines_bitmap_properties_and_scale9() {
    use neoflash_core::{AuthoredTextFieldOptions, BitmapCompression, TextFormatPatch};

    let mut project = Project::blank();
    project.format_version = 3;
    let png_bytes: [u8; 68] = [
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0B, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x60,
        0x00, 0x02, 0x00, 0x00, 0x05, 0x00, 0x01, 0xE2, 0x26, 0x05, 0x9B, 0x00, 0x00, 0x00, 0x00,
        b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];
    project.resources.push(
        ResourceUnit::from_bytes("bmp-1", "hero.png", "image/png", &png_bytes).expect("png unit"),
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "tb-1".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::TextBox {
                text: "Hello World".into(),
                width: 160.0,
                height: 40.0,
                format: TextFormat::default(),
                multiline: true,
                word_wrap: true,
            },
            ..Element::default()
        });

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();
    editor.select_single_id("tb-1".into());

    editor
        .set_selected_text_html("<b>Hello</b> <i>World</i>")
        .expect("set html");
    let html_out = editor.selected_text_to_html().expect("html out");
    assert!(html_out.contains("<b>Hello</b>"));

    editor
        .set_selected_text_field_options(AuthoredTextFieldOptions {
            max_chars: 32,
            border: true,
            ..AuthoredTextFieldOptions::default()
        })
        .expect("field options");

    editor
        .apply_selected_text_format_range(
            0,
            5,
            TextFormatPatch {
                size: Some(32.0),
                color: Some(0xff0000),
                underline: Some(true),
                ..TextFormatPatch::default()
            },
        )
        .expect("format range");

    let cmds = editor
        .convert_selected_text_to_outlines()
        .expect("convert to outlines");
    assert!(cmds > 0);
    let outlined: Vec<_> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, element)| element)
        .collect();
    assert_eq!(
        outlined
            .iter()
            .filter_map(|element| match &element.kind {
                ElementKind::Path { commands, .. } => Some(commands.len()),
                _ => None,
            })
            .sum::<usize>(),
        cmds
    );
    assert!(
        outlined.iter().all(|element| matches!(
            &element.kind,
            ElementKind::Path { commands, .. }
                if commands.iter().any(|command| matches!(
                    command,
                    PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
                ))
        )),
        "outline conversion must retain curves from every selected font instead of synthetic boxes"
    );
    let fills: Vec<_> = outlined
        .iter()
        .filter_map(|element| match element.kind {
            ElementKind::Path { fill, .. } => Some(fill),
            _ => None,
        })
        .collect();
    assert!(
        fills.contains(&[255, 0, 0, 255]) && fills.contains(&[0, 0, 0, 255]),
        "mixed rich-text colors must remain separate editable paths"
    );
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);

    // Bitmap properties dialog, replace bytes, bitmap fill & 9-slice grid.
    editor.open_bitmap_properties(&ctx, Some("bmp-1".into()));
    assert!(crate::bitmaps::has_bitmap_properties_dialog(&ctx));
    crate::bitmaps::update_bitmap_properties_draft(&ctx, |draft| {
        draft.export.compression = BitmapCompression::PhotoJpeg;
        draft.export.use_imported_jpeg_data = false;
        draft.export.jpeg_quality = 75;
        draft.export_modified = true;
    });
    editor
        .confirm_bitmap_properties(&ctx)
        .expect("confirm bitmap properties");
    assert_eq!(
        editor.project.resources[0]
            .bitmap_export
            .unwrap()
            .jpeg_quality,
        75
    );
    editor
        .replace_bitmap_resource_bytes("bmp-1", "hero2.png", "image/png", &png_bytes, None)
        .expect("replace bitmap bytes");
    editor.select_single_id("tb-1".into());
    editor
        .set_selected_bitmap_fill("bmp-1".into(), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], true, true)
        .expect("set bitmap fill");
    let root_id = editor.project.root.clone();
    editor
        .set_symbol_scale9_grid(&root_id, Some([10.0, 10.0, 80.0, 80.0]))
        .expect("set scale9 grid");
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t108_t109_t111_classic_tween_rotate_roving_and_graphic_frame_picker() {
    use neoflash_core::InstancePropertyOverride;

    let mut project = Project::blank();
    project.symbols.push(Symbol {
        id: "mouth-sym".into(),
        name: "Mouth".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "ml".into(),
                name: "Visemes".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "Neutral".into(),
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 1,
                        label: "Ah".into(),
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            }],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    });
    project.symbols[0].timeline.frame_count = 11;
    let inst = Element {
        id: "mouth-inst".into(),
        name: String::new(),
        transform: Transform::default(),
        kind: ElementKind::Instance {
            symbol_id: "mouth-sym".into(),
            playback: InstancePlayback::Loop,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![inst.clone()],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Classic,
            ..Keyframe::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![Element {
                transform: Transform {
                    x: 90.0,
                    ..Transform::default()
                },
                ..inst
            }],
            ..Keyframe::default()
        },
    ];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    assert!(editor.set_classic_tween_rotate(2, ClassicRotateDirection::Cw));
    assert!(editor.set_classic_tween_scale(false));
    assert!(editor.set_classic_tween_sync_symbols(true));
    let sampled = sample_layer(&editor.symbol().timeline.layers[0], 5);
    assert!((sampled[0].transform.rotation - 360.0).abs() < 1e-4);
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);

    // Graphic Frame Picker, Instance Overrides, Span Rename, Audio Scrubbing & Lip Sync.
    editor.select_single_id("mouth-inst".into());
    let entries = editor.graphic_frame_picker_entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[1].1, "Ah");
    assert!(editor.pick_graphic_frame(1));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "mouth-sym".into(),
            playback: InstancePlayback::SingleFrame(1),
        }
    );
    assert!(
        editor.set_instance_property_override(InstancePropertyOverride {
            instance_id: "mouth-inst".into(),
            target_name: "label_txt".into(),
            text: Some("Hi".into()),
            color: None,
            visible: Some(true),
            frame: None,
        })
    );
    assert_eq!(editor.rename_instance_in_span("mouth_mc", true), 2);
    assert!(editor.scrub_timeline_audio_at_frame(0).is_empty());
    let _ = editor.toggle_roving_keyframe();
    let _ = editor.auto_lip_sync_selected_graphic(&[0, 1, 1, 1, 1, 1]);
    editor.project.validate().unwrap();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t112_asset_warp_pins_tween_reset_and_bake() {
    let mut project = Project::blank();
    project.format_version = 9;
    project.symbols[0].timeline.frame_count = 11;
    let rect = Element {
        id: "warp-rect".into(),
        name: "warp_rect".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 100.0,
            height: 40.0,
            fill: [40, 120, 220, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![rect.clone()],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Classic,
            ..Keyframe::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![rect],
            ..Keyframe::default()
        },
    ];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    let rig_start = AssetWarpRig {
        element_id: "warp-rect".into(),
        pins: vec![
            WarpPin {
                id: "pin-base".into(),
                kind: WarpPinKind::Fixed,
                rest_pos: [0.0, 20.0],
                pos: [0.0, 20.0],
                rotation: 0.0,
                stiffness: 1.0,
            },
            WarpPin {
                id: "pin-tip".into(),
                kind: WarpPinKind::Open,
                rest_pos: [100.0, 20.0],
                pos: [100.0, 60.0],
                rotation: 15.0,
                stiffness: 0.8,
            },
        ],
        envelop_subdivisions: 4,
    };
    assert!(editor.set_element_asset_warp(rig_start));
    editor.project.validate().unwrap();

    // Sampled element at frame 0 is deformed into a Path with displaced tip.
    let sampled0 = sample_layer(&editor.symbol().timeline.layers[0], 0);
    assert!(matches!(sampled0[0].kind, ElementKind::Path { .. }));

    // Reset returns pins to rest_pos; Undo restores the displaced warp rig.
    assert!(editor.reset_element_asset_warp("warp-rect"));
    let reset_sampled = sample_layer(&editor.symbol().timeline.layers[0], 0);
    assert!(matches!(
        reset_sampled[0].kind,
        ElementKind::Rectangle { .. }
    ));
    editor.undo();

    // Bake permanently expands the deformed contour into the element and removes the warp rig.
    assert!(editor.bake_element_asset_warp("warp-rect"));
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .asset_warp("warp-rect")
            .is_none()
    );
    assert!(matches!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind,
        ElementKind::Path { .. }
    ));
    editor.project.validate().unwrap();

    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t113_camera_depth_parallax_parenting_controllers_and_ik_bake() {
    let mut project = Project::blank();
    project.format_version = 9;
    project.symbols[0].timeline.frame_count = 11;
    let parent_el = Element {
        id: "ctrl-root".into(),
        name: "ctrl_root".into(),
        transform: Transform {
            x: 100.0,
            y: 50.0,
            rotation: 90.0,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let child_el = Element {
        id: "child-arm".into(),
        name: "child_arm".into(),
        transform: Transform {
            x: 150.0,
            y: 50.0,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 40.0,
            height: 10.0,
            fill: [0, 200, 100, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![parent_el, child_el],
        ..Keyframe::default()
    }];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    // World-transform-preserving reparenting + cycle check.
    assert!(editor.reparent_element_preserving_world("child-arm", Some("ctrl-root")));
    assert!(
        !editor.reparent_element_preserving_world("ctrl-root", Some("child-arm")),
        "cycle must be rejected"
    );
    let sampled = sample_layer(&editor.symbol().timeline.layers[0], 0);
    let sampled_child = sampled.iter().find(|e| e.id == "child-arm").unwrap();
    assert!((sampled_child.transform.x - 150.0).abs() < 1e-4);
    assert!((sampled_child.transform.y - 50.0).abs() < 1e-4);

    // Controller flag hides element from published runtime scene while keeping parenting active.
    assert!(editor.set_element_controller_flag("ctrl-root", true));
    let scene = neoflash_core::evaluate_scene(&editor.project, "root", 0, false, false);
    assert!(!scene.draws.iter().any(|d| d.item.element_id == "ctrl-root"));

    // 2D camera pose + layer Z-depth parallax.
    assert!(editor.set_layer_z_depth(500.0));
    assert!(editor.set_keyframe_camera_pose(Some(CameraPose {
        x: 60.0,
        y: 0.0,
        zoom: 2.0,
        rotation: 0.0,
        focal_length: 500.0,
        color_transform: ColorTransform::default(),
    })));
    let cam = sample_timeline_camera(&editor.symbol().timeline, 0).unwrap();
    assert!((cam.x - 60.0).abs() < 1e-6);

    // Bounded IK armature + bake to normal keyframes.
    assert!(editor.set_keyframe_ik_armature(IkArmature {
        id: "arm-1".into(),
        name: "Arm".into(),
        bones: vec![
            IkBone {
                id: "b1".into(),
                element_id: "ctrl-root".into(),
                parent_bone_id: None,
                length: 50.0,
                min_angle: Some(-90.0),
                max_angle: Some(90.0),
                stiffness: 0.2,
            },
            IkBone {
                id: "b2".into(),
                element_id: "child-arm".into(),
                parent_bone_id: Some("b1".into()),
                length: 40.0,
                min_angle: Some(-120.0),
                max_angle: Some(120.0),
                stiffness: 0.1,
            },
        ],
        effector_bone_id: "b2".into(),
        target_pos: [140.0, 90.0],
        pole_pos: None,
    }));
    editor.project.validate().unwrap();
    assert!(editor.set_layer_attach_to_camera(true));
    assert!(
        editor.symbol().timeline.layers[0]
            .properties
            .attach_to_camera
    );
    assert_eq!(editor.bake_layer_ik_span(0, 2), 3);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .ik_armatures
            .is_empty()
    );
    editor.project.validate().unwrap();

    for _ in 0..6 {
        editor.undo();
    }
    undo_restores(&mut editor, &before);
}

#[test]
fn t114_appearance_stack_modifiers_swatches_styles_and_text_bindings() {
    let mut project = Project::blank();
    project.format_version = 9;
    let shape = Element {
        id: "star-shape".into(),
        name: "star".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 60.0,
            height: 60.0,
            fill: [200, 50, 50, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![shape],
        ..Keyframe::default()
    }];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    let app = ElementAppearance {
        element_id: "star-shape".into(),
        fill_swatch_id: Some("swatch-brand".into()),
        stroke_swatch_id: None,
        style_id: Some("style-badge".into()),
        entries: vec![
            AppearancePaintEntry::Fill {
                color: [200, 50, 50, 255],
                gradient: None,
                swatch_id: Some("swatch-brand".into()),
                offset: [0.0, 0.0],
                opacity: 1.0,
            },
            AppearancePaintEntry::Stroke {
                color: [10, 10, 10, 255],
                gradient: None,
                width: 3.0,
                swatch_id: None,
                offset_distance: 4.0,
                pattern_brush: Some(PatternBrushKind::DashPattern {
                    dash: 8.0,
                    gap: 4.0,
                }),
                opacity: 0.9,
            },
        ],
        modifiers: vec![
            VectorModifier::LiveCorners { radius: 6.0 },
            VectorModifier::OffsetContour { delta: 2.0 },
            VectorModifier::Mirror {
                axis_x: Some(80.0),
                axis_y: None,
            },
            VectorModifier::Repeater {
                copies: 2,
                offset_x: 30.0,
                offset_y: 0.0,
                scale_step: 1.0,
                rotation_step: 15.0,
            },
        ],
        text_binding: None,
    };
    assert!(editor.set_element_appearance(app));
    editor.project.validate().unwrap();
    assert_eq!(editor.apply_vector_style("style-badge"), 0);

    // Linked global swatch update propagates across both element and appearance entries.
    assert_eq!(
        editor.update_global_swatch("swatch-brand", [20, 180, 90, 255]),
        2
    );

    // Sampled layer expands the 2 appearance paint entries into 2 concrete elements.
    let sampled = sample_layer(&editor.symbol().timeline.layers[0], 0);
    assert_eq!(sampled.len(), 2);

    // Explicit Expand/Bake replaces the source element with the concrete expanded elements.
    let expanded_ids = editor.expand_element_appearance_command("star-shape");
    assert_eq!(expanded_ids.len(), 2);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        2
    );
    editor.project.validate().unwrap();

    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t118_timeline_video_sync_cue_points_and_codec_diagnostics() {
    let mut project = Project::blank();
    project.format_version = 9;
    project.resources.push(
        ResourceUnit::from_bytes(
            "vid-1",
            "video/intro.mp4",
            "video/mp4",
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41",
        )
        .unwrap(),
    );
    project.validate().unwrap();
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    let span = TimelineVideoSpan {
        resource: "vid-1".into(),
        sync: VideoSyncMode::Stream,
        start_offset_ms: 100,
        loop_video: true,
        muted: false,
        cue_points: vec![VideoCuePoint {
            name: "intro_end".into(),
            time_ms: 500,
            kind: VideoCuePointKind::Navigation,
            parameters: vec![("target".into(), "main".into())],
        }],
    };
    assert!(editor.set_keyframe_video_span(Some(span.clone())));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0].video,
        Some(span.clone())
    );
    let t_ms = neoflash_core::compute_frame_video_time_ms(0, 12, 24.0, 100, Some(2000), true);
    assert_eq!(t_ms, 600);
    let crossed = neoflash_core::collect_crossed_cue_points(&span.cue_points, 100, 600);
    assert_eq!(crossed.len(), 1);
    assert_eq!(crossed[0].name, "intro_end");
    editor.project.validate().unwrap();

    let valid = editor.project.clone();
    let mut missing = span.clone();
    missing.resource = "missing-video".into();
    assert!(!editor.set_keyframe_video_span(Some(missing)));
    assert_eq!(editor.project, valid, "invalid video edit must be atomic");

    undo_restores(&mut editor, &before);
}

#[test]
fn t115_t116_sequence_vector_import_and_publish_profiles_network_sim() {
    // T-115: High-fidelity SVG vector import with group-to-layer mapping.
    let svg = r##"<svg width="200" height="100" xmlns="http://www.w3.org/2000/svg">
        <g id="Background"><rect x="0" y="0" width="200" height="100" fill="#224466"/></g>
        <g id="Foreground"><circle cx="100" cy="50" r="25" fill="#ffcc00"/></g>
    </svg>"##;
    let (imported, report) = neoflash_import::import_svg_with_options(
        svg,
        "banner.svg",
        &neoflash_import::VectorImportOptions::default(),
    )
    .expect("import svg");
    imported.validate().unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| !d.code.starts_with("error"))
    );

    // T-116: Publish profiles + custom HTML wrapper + network startup simulation reusing size_report.
    let mut profile = neoflash_export::PublishProfile::new(
        "Fast 3G Web",
        neoflash_export::ExportOptions::default(),
    );
    profile.network_profile.preset = neoflash_export::NetworkPreset::Fast3G;
    let dup = neoflash_export::duplicate_profile(&profile, Some("Fast 3G Copy"));
    let json = neoflash_export::export_profile_json(&dup).expect("export profile json");
    let roundtrip = neoflash_export::import_profile_json(&json).expect("import profile json");
    assert_eq!(roundtrip.name, "Fast 3G Copy");

    let sim = neoflash_export::simulate_network_startup(&imported, &roundtrip)
        .expect("simulate network startup");
    assert!(sim.initial_html_js_wasm_ms >= 0.0);
    assert!(!sim.frames.is_empty());
}

#[test]
fn t117_extended_accessibility_tree_auto_labeling_and_diagnostics() {
    let mut project = Project::blank();
    project.format_version = 9;
    let button_el = Element {
        id: "cta-btn".into(),
        name: String::new(),
        transform: Transform {
            x: 120.0,
            y: 80.0,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 140.0,
            height: 44.0,
            fill: [40, 120, 220, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        accessibility: None,
        ..Element::default()
    };
    let text_el = Element {
        id: "cta-label".into(),
        name: "label".into(),
        transform: Transform {
            x: 135.0,
            y: 92.0,
            ..Transform::default()
        },
        kind: ElementKind::Text {
            text: "Start Adventure".into(),
            font_family: "sans-serif".into(),
            font_size: 16.0,
            color: [255, 255, 255, 255],
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![button_el, text_el],
        ..Keyframe::default()
    }];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    editor.set_document_accessibility_settings(DocumentAccessibility {
        auto_label: true,
        accessible: true,
        title: "Adventure Menu".into(),
        description: "Interactive title screen".into(),
        language: "en".into(),
        reading_order_top_down: true,
        focus_ring_visible: true,
    });
    assert!(
        editor.set_element_extended_accessibility(ExtendedAccessibility {
            element_id: "cta-btn".into(),
            role: ContentAccessibleRole::Button,
            shortcut: "Enter".into(),
            live: AccessibilityLivePoliteness::Polite,
            reading_order: Some(1),
            focus_group: Some("main-nav".into()),
            ..ExtendedAccessibility::default()
        })
    );
    editor.project.validate().unwrap();

    let tree = editor.resolve_current_accessibility_tree();
    let btn_node = tree.iter().find(|n| n.element_id == "cta-btn").unwrap();
    assert_eq!(btn_node.name, "Start Adventure");
    assert_eq!(btn_node.shortcut, "Enter");
    assert_eq!(btn_node.live, AccessibilityLivePoliteness::Polite);
    assert!(editor.validate_current_accessibility_tree().is_empty());

    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t120_vector_effects_envelope_warp_mesh_perspective_and_expand() {
    let mut project = Project::blank();
    project.format_version = 9;
    let shape = Element {
        id: "warp-rect".into(),
        name: "Badge".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 100.0,
            height: 50.0,
            fill: [220, 60, 40, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.0,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![shape],
        ..Keyframe::default()
    }];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    editor.set_timeline_perspective_grid(Some(PerspectiveGridSpec {
        enabled: true,
        active_plane: PerspectivePlane::RightGrid,
        ..PerspectiveGridSpec::default()
    }));
    assert_eq!(
        editor
            .symbol()
            .timeline
            .perspective_grid()
            .unwrap()
            .active_plane,
        PerspectivePlane::RightGrid
    );

    let veff = ElementVectorEffects {
        element_id: "warp-rect".into(),
        envelope_warp: Some(EnvelopeWarp {
            preset: EnvelopeWarpPreset::Arc,
            bend: 0.4,
            ..EnvelopeWarp::default()
        }),
        procedural_effects: vec![ProceduralVectorEffect::ZigZag {
            size: 3.0,
            ridges_per_segment: 2,
            smooth: true,
        }],
        vector_blend: None,
        freeform_gradient: None,
        gradient_mesh: Some(GradientMesh::new_grid(
            [0.0, 0.0, 100.0, 50.0],
            2,
            2,
            [80, 160, 240, 255],
        )),
    };
    assert!(editor.set_element_vector_effects(veff));
    editor.project.validate().unwrap();

    let sampled = sample_layer(&editor.symbol().timeline.layers[0], 0);
    assert!(matches!(sampled[0].kind, ElementKind::Path { .. }));

    assert!(editor.expand_element_vector_effects_command("warp-rect"));
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .vector_effects("warp-rect")
            .is_none()
    );
    assert!(matches!(
        editor.symbol().timeline.layers[0].keyframes[0].elements[0].kind,
        ElementKind::Path { .. }
    ));
    editor.project.validate().unwrap();

    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn t121_motion_expressions_sketch_wiggler_work_area_and_markers() {
    let mut project = Project::blank();
    project.format_version = 9;
    let actor = Element {
        id: "actor-1".into(),
        name: "Actor".into(),
        transform: Transform {
            x: 50.0,
            y: 50.0,
            ..Transform::default()
        },
        kind: ElementKind::Ellipse {
            width: 32.0,
            height: 32.0,
            fill: [50, 200, 100, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![actor],
        ..Keyframe::default()
    }];

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    assert!(editor.set_keyframe_property_expression(PropertyExpression {
        property: TweenProperty::X,
        expression: "value + 25".into(),
        enabled: true,
        ..PropertyExpression::default()
    }));
    let sampled_f0 = sample_layer(&editor.symbol().timeline.layers[0], 0);
    assert!((sampled_f0[0].transform.x - 75.0).abs() < 1e-6);

    let sketch = vec![
        MotionSketchSample {
            time_sec: 0.0,
            position: [50.0, 50.0],
        },
        MotionSketchSample {
            time_sec: 0.05,
            position: [80.0, 70.0],
        },
        MotionSketchSample {
            time_sec: 0.10,
            position: [120.0, 90.0],
        },
    ];
    assert!(editor.apply_motion_sketch_to_element("actor-1", &sketch, 0.5, 0.5) >= 2);
    assert_eq!(
        editor.apply_wiggler_to_element("actor-1", 0, 2, &WigglerConfig::default()),
        3
    );

    editor.set_timeline_work_area(Some(PreviewWorkArea {
        enabled: true,
        in_frame: 0,
        out_frame: 2,
        skip_nth_frame: 0,
    }));
    editor.set_timeline_markers(vec![TimelineMarker {
        id: "mk-1".into(),
        frame: 1,
        duration_frames: 2,
        name: "Beat".into(),
        comment: "Accent hit".into(),
        color_tag: Some("amber".into()),
        cue_point_parameters: std::collections::BTreeMap::new(),
    }]);
    let snap = editor.capture_stage_snapshot_ab("A");
    assert_eq!(snap.label, "A");
    assert!(!snap.elements.is_empty());
    editor.project.validate().unwrap();

    editor.undo();
    editor.undo();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}

#[test]
fn editor_sampling_uses_the_document_frame_rate() {
    let mut project = Project::blank();
    project.format_version = 9;
    project.fps = 30;
    project.symbols[0].timeline.frame_count = 61;
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "fps-actor".into(),
            name: "FPS actor".into(),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255; 4],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    project.symbols[0].timeline.layers[0].keyframes[0]
        .property_expressions
        .push(PropertyExpression {
            property: TweenProperty::X,
            expression: "time * 120".into(),
            enabled: true,
            ..PropertyExpression::default()
        });
    project.validate().expect("30 fps project validates");

    let ctx = egui::Context::default();
    let mut at_30 = Editor::from_project(&ctx, project.clone());
    at_30.frame = 30;
    at_30.select_single_id("fps-actor".into());
    assert!((at_30.selected_sampled().unwrap().transform.x - 120.0).abs() < 1e-6);
    assert!(
        (at_30.capture_stage_snapshot_ab("30 fps").elements[0]
            .transform
            .x
            - 120.0)
            .abs()
            < 1e-6
    );

    project.fps = 60;
    project.validate().expect("60 fps project validates");
    let mut at_60 = Editor::from_project(&ctx, project);
    at_60.frame = 30;
    at_60.select_single_id("fps-actor".into());
    assert!((at_60.selected_sampled().unwrap().transform.x - 60.0).abs() < 1e-6);
    assert!(
        (at_60.capture_stage_snapshot_ab("60 fps").elements[0]
            .transform
            .x
            - 60.0)
            .abs()
            < 1e-6
    );
}

#[test]
fn t122_palette_interchange_fxg_and_screens_migration_commands() {
    let project = Project::blank();
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, project);
    let before = editor.project.clone();

    let act_bytes = neoflash_import::export_act_palette(&[GlobalSwatch {
        id: "act-1".into(),
        name: "Coral".into(),
        color: [250, 90, 70, 255],
    }]);
    assert_eq!(
        editor
            .import_act_palette_into_swatches(&act_bytes, "act")
            .unwrap(),
        1
    );

    let clr_bytes = neoflash_import::export_clr_palette(&[GlobalSwatch {
        id: "sw-1".into(),
        name: "Neon Mint".into(),
        color: [32, 240, 160, 255],
    }]);
    assert_eq!(
        editor
            .import_clr_palette_into_swatches(&clr_bytes, "clr")
            .unwrap(),
        1
    );

    let mut ase_bytes = Vec::new();
    ase_bytes.extend_from_slice(b"ASEF\x00\x01\x00\x00\x00\x00\x00\x01");
    let mut blk = Vec::new();
    blk.extend_from_slice(&2u16.to_be_bytes());
    blk.extend_from_slice(&(b'A' as u16).to_be_bytes());
    blk.extend_from_slice(&0u16.to_be_bytes());
    blk.extend_from_slice(b"GRAY");
    blk.extend_from_slice(&0.5f32.to_be_bytes());
    blk.extend_from_slice(&0u16.to_be_bytes());
    ase_bytes.extend_from_slice(&0x0001u16.to_be_bytes());
    ase_bytes.extend_from_slice(&(blk.len() as u32).to_be_bytes());
    ase_bytes.extend_from_slice(&blk);
    assert_eq!(
        editor.import_ase_palette_into_swatches(&ase_bytes).unwrap(),
        1
    );
    assert_eq!(editor.project.global_swatches().len(), 3);

    let fxg = r##"<Graphic version="2.0" xmlns="http://ns.adobe.com/fxg/2008">
        <Group id="FxgGroup">
            <Rect x="10" y="10" width="80" height="40">
                <fill><SolidColor color="#FF4422"/></fill>
            </Rect>
        </Group>
    </Graphic>"##;
    editor.import_fxg_xml_command(fxg, "card.fxg").unwrap();
    editor.project.validate().unwrap();

    let screens = r#"<screens><Slide name="SlideA"/><Form name="FormB"/></screens>"#;
    editor
        .import_flash_screens_xml_command(screens, "deck.xml")
        .unwrap();
    editor.project.validate().unwrap();

    editor.undo();
    editor.undo();
    editor.undo();
    editor.undo();
    undo_restores(&mut editor, &before);
}
