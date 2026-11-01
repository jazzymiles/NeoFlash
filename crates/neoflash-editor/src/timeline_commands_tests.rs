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

    // Edit layer 0: rename, hide, lock, link as Guided, set outline colour + outline mode + 200% height.
    editor.open_layer_properties(&ctx, 0);
    edit_layer_properties_draft(&ctx, |d| {
        d.name = "Hero".into();
        d.visible = false;
        d.locked = true;
        d.kind = LayerTypeChoice::Guided;
        d.outline_color = egui::Color32::from_rgb(212, 95, 193);
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
    assert_eq!(
        l0.properties.motion_guide.as_deref(),
        Some("guide-layer")
    );
    assert_eq!(l0.properties.outline_color, Some([212, 95, 193]));
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
    use crate::panels::symbols_popover::{
        PopoverTarget, is_symbols_popover_open, popover_state,
    };

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
            let _ = editor.stage_chrome(ui, canvas);
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
                elements: vec![
                    placed("shared", 240., 40.),
                    placed("only_on_8", 320., 120.),
                ],
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
    assert!(rot_0.abs() > 5., "expected non-trivial rotation, got {rot_0}");
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
