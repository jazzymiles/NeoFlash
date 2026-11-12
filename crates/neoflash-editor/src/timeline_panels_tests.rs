//! Ease, Scene, Motion editor and Properties — Motion tween panels: each action changes the document
//! as expected, keeps it valid and is one Undo step.
use crate::app::Editor;
use crate::panels::ease::{BACK_OUT, easing_bezier, preset_easing, preset_of};
use crate::workspace::{PanelId, WorkspacePrefs};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components::EasePresetKind;

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

/// Root with 20 frames: rectangle "a" at x 0 on frame 0 and x 100 on frame 10, classic tween.
fn classic_editor() -> Editor {
    let mut project = Project::blank();
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 20;
    timeline.layers[0].keyframes = vec![
        Keyframe {
            elements: vec![element("a", rectangle(), 0.)],
            tween: Some(Easing::Linear),
            ..Default::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![element("a", rectangle(), 100.)],
            ..Default::default()
        },
    ];
    Editor::from_project(&egui::Context::default(), project)
}

/// Root with 20 frames; a motion tween of instance "clip" from frame 0 to frame 10.
fn motion_editor() -> Editor {
    let mut project = Project::blank();
    let mut child = project.symbols[0].clone();
    child.id = "child".into();
    child.name = "Clip".into();
    child.timeline.layers[0].id = "child-layer".into();
    project.symbols.push(child);
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 20;
    let instance = ElementKind::Instance {
        symbol_id: "child".into(),
        playback: InstancePlayback::Independent,
    };
    timeline.layers[0].keyframes = vec![
        Keyframe {
            elements: vec![element("clip", instance, 0.)],
            ..Default::default()
        },
        Keyframe {
            frame: 10,
            ..Default::default()
        },
    ];
    project.format_version = 4;
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    editor.toggle_tween(TweenKind::Motion);
    assert_eq!(editor.current_tween(), Some(TweenKind::Motion));
    editor.select_single_id("clip".into());
    editor
}

fn undo_restores(editor: &mut Editor, before: &Project) {
    editor.undo();
    assert_eq!(
        &editor.project, before,
        "one Undo step restores the document"
    );
}

fn start_key(editor: &Editor) -> &Keyframe {
    &editor.symbol().timeline.layers[0].keyframes[0]
}

#[test]
fn ease_presets_round_trip_and_flash_curves_match_the_engine() {
    for preset in EasePresetKind::ALL {
        let easing = preset_easing(preset, Easing::Linear)
            .unwrap_or_else(|| panic!("{preset:?} should be enabled"));
        if preset != EasePresetKind::Custom {
            assert_eq!(preset_of(easing), preset, "{preset:?}");
        }
    }
    // The graph draws Flash acceleration as the exact cubic form of the engine's quadratic.
    for acceleration in [-100., -40., 0., 60., 100.] {
        let [x1, y1, x2, y2] = easing_bezier(Easing::Flash { acceleration }).map(f64::from);
        for i in 1..10 {
            let t = i as f64 / 10.;
            let drawn = easing(t, Easing::CubicBezier { x1, y1, x2, y2 });
            let real = easing(t, Easing::Flash { acceleration });
            assert!((drawn - real).abs() < 1e-3, "{acceleration} at {t}");
        }
    }
}

#[test]
fn ease_panel_applies_overshoot_and_flash_curves_with_one_undo_each() {
    let mut editor = classic_editor();
    editor.frame = 4;
    let span = editor.tween_span().expect("classic span");
    assert_eq!(
        (span.kind, span.start, span.end),
        (TweenKind::Classic, 0, 10)
    );
    let before = editor.project.clone();
    let [x1, y1, x2, y2] = BACK_OUT;
    let back = Easing::CubicBezier { x1, y1, x2, y2 };
    assert!(editor.set_tween_easing(&span, back));
    assert_eq!(start_key(&editor).tween, Some(back));
    editor.project.validate().unwrap();
    // Back out overshoots past the end pose before settling.
    let peak = (1..10)
        .map(|f| {
            sample_layer(&editor.symbol().timeline.layers[0], f)[0]
                .transform
                .x
        })
        .fold(f64::MIN, f64::max);
    assert!(peak > 100., "peak {peak}");
    let saved = serde_json::to_string(&editor.project).unwrap();
    assert_eq!(
        serde_json::from_str::<Project>(&saved).unwrap().symbols[0]
            .timeline
            .layers[0]
            .keyframes[0]
            .tween,
        Some(back)
    );
    undo_restores(&mut editor, &before);
    let flash = Easing::Flash { acceleration: -60. };
    assert!(editor.set_tween_easing(&span, flash));
    assert!(editor.project.format_version >= 5);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
    // Outside a tween there is nothing to edit.
    editor.frame = 15;
    assert!(editor.tween_span().is_none());
}

#[test]
fn ease_panel_edits_the_motion_span_start_from_a_property_key() {
    let mut editor = motion_editor();
    let span = editor.motion_tween_span().unwrap();
    editor.frame = 5;
    editor.set_motion_value(&span, TweenProperty::Y, 5, 40., None);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[1]
            .keyed_properties
            .is_some()
    );
    // The playhead sits on the ◆ key; the ease still belongs to the span start.
    let span = editor.tween_span().unwrap();
    assert_eq!(
        (span.kind, span.start, span.end),
        (TweenKind::Motion, 0, 10)
    );
    let before = editor.project.clone();
    assert!(editor.set_tween_easing(&span, Easing::EaseOut));
    assert_eq!(
        start_key(&editor).motion_easing_for("clip"),
        Easing::EaseOut
    );
    assert_eq!(start_key(&editor).tween, Some(Easing::Linear));
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[1].frame,
        5,
        "the ◆ key stays"
    );
    undo_restores(&mut editor, &before);
}

#[test]
fn scenes_duplicate_rename_and_reorder_with_one_undo_each() {
    let mut editor = motion_editor();
    let root = editor.project.root.clone();
    editor.new_scene();
    editor.switch_scene(&root);
    let property_pose = sample_layer(&editor.symbol().timeline.layers[0], 5);
    let start = &mut editor.symbol_mut().timeline.layers[0].keyframes[0];
    start.ensure_motion_track("clip").easing = Some(Easing::EaseIn);
    editor.symbol_mut().timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 5,
            elements: property_pose,
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Motion,
            keyed_properties: Some(vec![TweenProperty::X]),
            motion_keys: Some(vec![MotionPropertyKey {
                element_id: "clip".into(),
                properties: vec![TweenProperty::X],
                roving: false,
            }]),
            ..Default::default()
        });
    editor.symbol_mut().timeline.layers[0]
        .keyframes
        .sort_by_key(|key| key.frame);
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    // Duplicate: inserted after the current scene, fresh IDs, same content, valid.
    let before = editor.project.clone();
    editor.duplicate_scene();
    let scenes = editor.scene_list();
    assert_eq!(scenes.len(), 3);
    assert_eq!(scenes[1].1, "Scene 1 copy");
    assert_eq!(editor.current_scene(), scenes[1].0);
    let (original, copy) = (
        editor.project.symbol(&root).unwrap(),
        editor.project.symbol(&scenes[1].0).unwrap(),
    );
    assert_ne!(original.timeline.layers[0].id, copy.timeline.layers[0].id);
    let ids = |s: &Symbol| -> Vec<Id> {
        s.timeline.layers[0]
            .keyframes
            .iter()
            .flat_map(|k| &k.elements)
            .map(|e| e.id.clone())
            .collect()
    };
    assert!(ids(copy).iter().all(|id| !ids(original).contains(id)));
    // One element keeps one ID on every keyframe of its layer.
    let copied = ids(copy);
    assert!(copied.windows(2).all(|w| w[0] == w[1]));
    let copied_id = copied[0].clone();
    assert_eq!(
        copy.timeline.layers[0].keyframes[0].motion_tracks[0].element_id,
        copied_id
    );
    assert_eq!(
        copy.timeline.layers[0]
            .keyframes
            .iter()
            .find(|key| key.frame == 5)
            .unwrap()
            .motion_keys
            .as_ref()
            .unwrap()[0]
            .element_id,
        copied_id
    );
    assert_eq!(
        sample_layer(&copy.timeline.layers[0], 5)[0].transform,
        sample_layer(&original.timeline.layers[0], 5)[0].transform
    );
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
    editor.switch_scene(&root);
    // Rename: the scene symbol's name; duplicates and empty names are refused.
    let before = editor.project.clone();
    assert!(editor.rename_scene(&root, "Intro"));
    assert_eq!(editor.scene_list()[0].1, "Intro");
    editor.project.validate().unwrap();
    assert!(!editor.rename_scene(&root, "Scene 2"));
    assert!(!editor.rename_scene(&root, "  "));
    undo_restores(&mut editor, &before);
    // Reorder: the first scene is always the root timeline.
    let second = editor.scene_list()[1].0.clone();
    assert!(editor.move_scene(1, 0));
    assert_eq!(editor.project.scenes, vec![second.clone(), root.clone()]);
    assert_eq!(editor.project.root, second);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
    assert!(!editor.move_scene(0, 0));
    // Delete the current scene, never the last one.
    editor.switch_scene(&second);
    editor.delete_scene();
    assert_eq!(editor.scene_list().len(), 1);
    editor.delete_scene();
    assert_eq!(editor.scene_list().len(), 1);
    editor.project.validate().unwrap();
}

#[test]
fn motion_editor_adds_and_removes_one_property_key() {
    let mut editor = motion_editor();
    let span = editor.motion_tween_span().unwrap();
    editor.frame = 4;
    let before = editor.project.clone();
    editor.toggle_motion_key(&span, TweenProperty::Rotation);
    let layer = &editor.symbol().timeline.layers[0];
    let key = layer.keyframes.iter().find(|k| k.frame == 4).unwrap();
    assert_eq!(key.keyed_properties, Some(vec![TweenProperty::Rotation]));
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Rotation),
        vec![0, 4]
    );
    assert_eq!(editor.motion_key_frames(&span, TweenProperty::X), vec![0]);
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &before);
    // Edit a value at the playhead: keys only that property.
    editor.set_motion_value(&span, TweenProperty::Rotation, 4, 90., None);
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Rotation),
        vec![0, 4]
    );
    assert_eq!(
        editor.motion_value(&span, TweenProperty::Rotation, 4),
        Some(90.)
    );
    assert_eq!(editor.motion_value(&span, TweenProperty::X, 4), Some(0.));
    assert_eq!(
        editor.motion_value(&span, TweenProperty::Rotation, 2),
        Some(45.)
    );
    editor.set_motion_value(&span, TweenProperty::Alpha, 4, 50., None);
    let key = &editor.symbol().timeline.layers[0].keyframes[1];
    assert_eq!(
        key.keyed_properties,
        Some(vec![TweenProperty::Rotation, TweenProperty::Alpha])
    );
    assert_eq!(key.elements[0].transform.alpha, 0.5);
    // Removing Rotation keeps the ◆ key for Alpha; removing Alpha removes the key.
    let keyed = editor.project.clone();
    editor.toggle_motion_key(&span, TweenProperty::Rotation);
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Rotation),
        vec![0]
    );
    assert_eq!(
        editor.motion_value(&span, TweenProperty::Rotation, 4),
        Some(0.)
    );
    editor.toggle_motion_key(&span, TweenProperty::Alpha);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .all(|k| k.frame != 4)
    );
    editor.project.validate().unwrap();
    editor.undo();
    undo_restores(&mut editor, &keyed);
    // The first frame always keys every property.
    editor.frame = 0;
    let unchanged = editor.project.clone();
    editor.toggle_motion_key(&span, TweenProperty::X);
    assert_eq!(editor.project, unchanged);
}

#[test]
fn motion_editor_follows_one_selected_object_and_keeps_sibling_tracks_independent() {
    let mut editor = motion_editor();
    let instance_kind = editor.symbol().timeline.layers[0].keyframes[0].elements[0]
        .kind
        .clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(element("satellite", instance_kind, 200.0));

    editor.select_single_id("clip".into());
    let clip_span = editor.motion_tween_span().expect("selected clip span");
    assert_eq!(clip_span.motion_element.as_deref(), Some("clip"));
    editor.set_motion_value(&clip_span, TweenProperty::X, 4, 80.0, None);
    assert!(editor.set_motion_property_ease(&clip_span, TweenProperty::X, Some(Easing::EaseOut),));
    editor.set_orient_to_path(&clip_span, true);

    editor.select_single_id("satellite".into());
    let satellite_span = editor.motion_tween_span().expect("selected satellite span");
    assert_eq!(satellite_span.motion_element.as_deref(), Some("satellite"));
    assert_eq!(
        editor.motion_key_frames(&satellite_span, TweenProperty::X),
        vec![0],
        "the clip's X key must not appear on its sibling track"
    );
    editor.set_motion_value(&satellite_span, TweenProperty::Rotation, 6, 45.0, None);
    assert!(editor.set_motion_property_ease(
        &satellite_span,
        TweenProperty::Rotation,
        Some(Easing::EaseIn),
    ));
    editor.set_orient_to_path(&satellite_span, false);

    let layer = &editor.symbol().timeline.layers[0];
    let clip_key = layer.keyframes.iter().find(|key| key.frame == 4).unwrap();
    let satellite_key = layer.keyframes.iter().find(|key| key.frame == 6).unwrap();
    assert_eq!(
        clip_key.motion_key_properties_for("clip"),
        &[TweenProperty::X]
    );
    assert!(clip_key.motion_key_properties_for("satellite").is_empty());
    assert_eq!(
        satellite_key.motion_key_properties_for("satellite"),
        &[TweenProperty::Rotation]
    );
    assert!(satellite_key.motion_key_properties_for("clip").is_empty());
    let start = &layer.keyframes[0];
    assert_eq!(
        start.motion_property_ease_for("clip", TweenProperty::X),
        Easing::EaseOut
    );
    assert_eq!(
        start.motion_property_ease_for("satellite", TweenProperty::Rotation),
        Easing::EaseIn
    );
    assert!(start.motion_orient_to_path_for("clip"));
    assert!(!start.motion_orient_to_path_for("satellite"));
    editor.project.validate().unwrap();

    editor.selection.targets.clear();
    assert!(
        editor.motion_tween_span().is_none(),
        "no Stage object selection must never fall back to the first element"
    );
    editor.select_single_id("clip".into());
    let clip_target = editor.selection.targets[0].clone();
    editor.select_single_id("satellite".into());
    let satellite_target = editor.selection.targets[0].clone();
    editor.selection.targets = vec![clip_target, satellite_target];
    assert!(
        editor.motion_tween_span().is_none(),
        "an ambiguous Stage selection must show no object track"
    );

    let json = editor.project.to_json().unwrap();
    let restored = Project::from_json(&json).unwrap();
    restored.validate().unwrap();
    let start = &restored.symbols[0].timeline.layers[0].keyframes[0];
    assert_eq!(
        start.motion_property_ease_for("clip", TweenProperty::X),
        Easing::EaseOut
    );
    assert_eq!(
        start.motion_property_ease_for("satellite", TweenProperty::Rotation),
        Easing::EaseIn
    );
}

#[test]
fn motion_tween_properties_bind_rotation_orient_and_path() {
    let mut editor = motion_editor();
    let span = editor.motion_tween_span().unwrap();
    assert_eq!(editor.motion_rotation(&span), (0, true));
    let before = editor.project.clone();
    editor.set_motion_rotation(&span, 2, true, None);
    assert_eq!(editor.motion_rotation(&span), (2, true));
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Rotation),
        vec![0, 9]
    );
    assert_eq!(
        editor.motion_value(&span, TweenProperty::Rotation, 9),
        Some(720.)
    );
    editor.set_motion_rotation(&span, 1, false, None);
    assert_eq!(editor.motion_rotation(&span), (1, false));
    assert_eq!(
        editor.motion_value(&span, TweenProperty::Rotation, 9),
        Some(-360.)
    );
    editor.project.validate().unwrap();
    editor.undo();
    undo_restores(&mut editor, &before);
    // Orient to path lives on the selected object's span track (format 10).
    editor.set_orient_to_path(&span, true);
    assert!(start_key(&editor).motion_orient_to_path_for("clip"));
    assert!(editor.project.format_version >= 10);
    editor.project.validate().unwrap();
    let saved = serde_json::to_string(&editor.project).unwrap();
    assert!(saved.contains("\"orient_to_path\":true"));
    undo_restores(&mut editor, &before);
    // Path box: bounds of the X / Y keys; moving it moves every key.
    editor.set_motion_value(&span, TweenProperty::X, 6, 200., None);
    editor.set_motion_value(&span, TweenProperty::Y, 8, 50., None);
    assert_eq!(editor.motion_path_bounds(&span), Some([0., 0., 200., 50.]));
    let keyed = editor.project.clone();
    editor.set_motion_path_bounds(&span, [10., 20., 100., 50.], None);
    assert_eq!(
        editor.motion_path_bounds(&span),
        Some([10., 20., 100., 50.])
    );
    assert_eq!(editor.motion_value(&span, TweenProperty::X, 6), Some(110.));
    editor.project.validate().unwrap();
    undo_restores(&mut editor, &keyed);
}

#[test]
fn new_panels_load_from_old_preferences_hidden_and_open_beside_their_group() {
    let mut legacy = serde_json::to_value(WorkspacePrefs::default()).unwrap();
    for key in ["ease", "scene", "motion_editor"] {
        legacy["panels"].as_object_mut().unwrap().remove(key);
        legacy["detached"].as_object_mut().unwrap().remove(key);
    }
    for edge in ["top", "bottom", "left", "right"] {
        for group in legacy["dock"][edge]["groups"].as_array_mut().unwrap() {
            group["tabs"]
                .as_array_mut()
                .unwrap()
                .retain(|tab| !matches!(tab.as_str(), Some("ease" | "scene" | "motion_editor")));
        }
    }
    let mut prefs = WorkspacePrefs::from_json(&legacy.to_string());
    for panel in [PanelId::Ease, PanelId::Scene, PanelId::MotionEditor] {
        assert!(!prefs.is_panel_visible(panel));
        assert!(prefs.is_panel_docked(panel));
    }
    let group = |prefs: &WorkspacePrefs, panel| prefs.dock.location(panel).unwrap().group;
    assert_eq!(
        group(&prefs, PanelId::Ease),
        group(&prefs, PanelId::Properties)
    );
    assert_eq!(
        group(&prefs, PanelId::Scene),
        group(&prefs, PanelId::Properties)
    );
    assert_eq!(
        group(&prefs, PanelId::MotionEditor),
        group(&prefs, PanelId::Timeline)
    );
    // The default workspace keeps them hidden; showing one selects its tab.
    let default = WorkspacePrefs::default();
    assert!(!default.is_panel_visible(PanelId::Ease));
    prefs.show_panel(PanelId::Ease);
    assert!(prefs.is_panel_visible(PanelId::Ease));
}

#[test]
fn ease_scene_motion_panels_paint_without_editing() {
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    let mut editor = motion_editor();
    editor.frame = 3;
    for panel in [PanelId::Ease, PanelId::Scene, PanelId::MotionEditor] {
        editor.show_panel(panel);
    }
    let before = editor.project.clone();
    for _ in 0..2 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600., 1000.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.ease_panel_ui(ui);
                    editor.scene_panel_ui(ui);
                    editor.motion_editor_panel_ui(ui);
                    editor.properties(ui, ctx);
                });
            },
        );
    }
    assert_eq!(editor.project, before, "drawing the panels never edits");
}

#[test]
fn per_property_eases_and_move_motion_key_with_single_undo() {
    let ctx = egui::Context::default();
    let mut editor = motion_editor();
    let span = editor.motion_tween_span().unwrap();
    editor.set_motion_value(&span, TweenProperty::X, 9, 100., None);
    editor.set_motion_value(&span, TweenProperty::Y, 9, 200., None);

    let span = editor.motion_tween_span().unwrap();
    let before_ease = editor.project.clone();
    assert!(editor.set_motion_property_ease(&span, TweenProperty::X, Some(Easing::Bounce)));
    assert!(editor.project.format_version >= 10);
    editor.project.validate().unwrap();
    let span = editor.motion_tween_span().unwrap();
    assert_eq!(
        span.property_override(TweenProperty::X),
        Some(Easing::Bounce)
    );
    assert_eq!(span.property_override(TweenProperty::Y), None);
    let x5 = editor.motion_value(&span, TweenProperty::X, 5).unwrap();
    let y5 = editor.motion_value(&span, TweenProperty::Y, 5).unwrap();
    let expected_x5 = (easing(5.0 / 10.0, Easing::Bounce) * 10.0 / 9.0).min(1.0) * 100.0;
    assert!(
        (x5 - expected_x5).abs() < 1e-6,
        "X uses Bounce: {x5} vs {expected_x5}"
    );
    assert!(
        (y5 - (5.0 / 9.0) * 200.0).abs() < 1e-6,
        "Y inherits Linear span ease: {y5}"
    );
    editor.open_ease_for_property(&ctx, TweenProperty::X);
    assert!(editor.workspace.is_panel_visible(PanelId::Ease));
    undo_restores(&mut editor, &before_ease);

    // Move ◆ key in time: place X at frame 4 (value 80) and Y at frame 4 (value 30).
    let span = editor.motion_tween_span().unwrap();
    editor.set_motion_value(&span, TweenProperty::X, 4, 80., None);
    editor.set_motion_value(&span, TweenProperty::Y, 4, 30., None);
    let span = editor.motion_tween_span().unwrap();
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::X),
        vec![0, 4, 9]
    );
    // Start key at frame 0 is never movable.
    assert_eq!(
        editor.move_motion_key(&span, TweenProperty::X, 0, 3, None),
        None
    );

    // Moving X from 4 -> 6 -> 7 inside one drag control commits as ONE Undo step and leaves Y at 4.
    let before_move = editor.project.clone();
    let drag_id = egui::Id::new("test-lane-drag");
    assert_eq!(
        editor.move_motion_key(&span, TweenProperty::X, 4, 6, Some(drag_id)),
        Some(6)
    );
    assert_eq!(
        editor.move_motion_key(&span, TweenProperty::X, 6, 7, Some(drag_id)),
        Some(7)
    );
    editor.finish_edits();
    editor.project.validate().unwrap();
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::X),
        vec![0, 7, 9]
    );
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Y),
        vec![0, 4, 9]
    );
    assert_eq!(
        editor.motion_key_value(&span, TweenProperty::X, 7),
        Some(80.)
    );
    undo_restores(&mut editor, &before_move);
}

#[test]
fn motion_editor_reveals_and_edits_skew_property_track() {
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    let mut editor = motion_editor();
    editor.frame = 4;
    let span = editor.motion_tween_span().expect("motion span");
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Skew),
        vec![0]
    );

    let before = editor.project.clone();
    editor.set_motion_value(&span, TweenProperty::Skew, 4, 24.0, None);
    editor.finish_edits();
    editor.project.validate().unwrap();

    let span = editor.motion_tween_span().expect("motion span");
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Skew),
        vec![0, 4]
    );
    let mid = editor
        .motion_value(&span, TweenProperty::Skew, 2)
        .expect("interpolated skew");
    assert!((mid - 12.0).abs() < 1e-6);

    let after_edit = editor.project.clone();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600., 1000.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.motion_editor_panel_ui(ui);
            });
        },
    );
    assert_eq!(editor.project, after_edit);
    undo_restores(&mut editor, &before);
}

#[test]
fn classic_orient_and_snap_controls_follow_guide_state_and_commit_one_undo() {
    use crate::tests::primary_button;
    for theme in [neoflash_ui::Theme::Dark, neoflash_ui::Theme::Light] {
        for guided in [false, true] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            ctx.set_fonts(neoflash_ui::typography::font_definitions());
            neoflash_ui::theme::install(&ctx, theme);
            let mut project = classic_editor().project;
            if guided {
                let mut guide = project.symbols[0].timeline.layers[0].clone();
                guide.id = "guide".into();
                guide.properties.role = LayerRole::Guide;
                guide.keyframes = vec![Keyframe {
                    elements: vec![element(
                        "guide-path",
                        ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 0., y: 20. },
                                PathCommand::LineTo { x: 100., y: 120. },
                            ],
                            fill_rule: Default::default(),
                            fill: [0, 0, 0, 0],
                            stroke: [0, 0, 0, 255],
                            stroke_width: 1.,
                        },
                        0.,
                    )],
                    ..Default::default()
                }];
                project.symbols[0].timeline.layers[0]
                    .properties
                    .motion_guide = Some(guide.id.clone());
                project.symbols[0].timeline.layers.push(guide);
            }
            project.upgrade_format_for_features();
            project.validate().unwrap();
            let mut editor = Editor::from_project(&ctx, project);
            editor.frame = 3;
            editor.select_frame_range(FrameRange {
                layers: vec![editor.symbol().timeline.layers[0].id.clone()],
                start: 3,
                end: 3,
            });
            let before = editor.project.clone();
            let mut time = 0.;
            let mut render = |editor: &mut Editor, events| {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000., 1000.),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(380.);
                            editor.properties(ui, ctx);
                        });
                    },
                )
            };
            for label in ["Orient to path", "Snap to motion guide"] {
                let output = render(&mut editor, vec![]);
                let node = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .unwrap()
                    .1;
                assert_eq!(node.is_disabled(), !guided, "{label}, guided={guided}");
                let bounds = node.bounds().unwrap();
                let at = egui::pos2(
                    ((bounds.x0 + bounds.x1) / 2.) as f32,
                    ((bounds.y0 + bounds.y1) / 2.) as f32,
                );
                render(
                    &mut editor,
                    vec![egui::Event::PointerMoved(at), primary_button(at, true)],
                );
                render(&mut editor, vec![primary_button(at, false)]);
                render(&mut editor, vec![]);
                editor.finish_edits();
                let key = start_key(&editor);
                assert_eq!(
                    if label == "Orient to path" {
                        key.motion_orient_to_path_for("clip")
                    } else {
                        key.motion_snap_to_guide_for("clip")
                    },
                    guided,
                    "{label} bound to document"
                );
                editor.project.validate().unwrap();
                if guided {
                    editor.undo();
                }
                assert_eq!(
                    editor.project, before,
                    "{label}: disabled is pure / enabled is one Undo"
                );
            }
        }
    }
}

#[test]
fn motion_editor_add_property_track_filter_and_resize_span_frames_with_single_undo() {
    let mut editor = motion_editor();
    editor.frame = 4;
    let span = editor
        .motion_tween_span()
        .expect("motion tween span active");

    // 1. Add property track on an interior frame creates a ◆ keyframe with that property.
    let before = editor.project.clone();
    assert!(editor.ensure_motion_property_track(&span, TweenProperty::Rotation));
    editor.project.validate().unwrap();
    let mid_key = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|kf| kf.frame == 4)
        .expect("interior property keyframe created at frame 4");
    assert_eq!(
        mid_key.keyed_properties.as_deref(),
        Some(&[TweenProperty::Rotation][..])
    );
    undo_restores(&mut editor, &before);

    // 2. Add filter in Motion editor adds the filter across the motion span in one Undo step.
    let before = editor.project.clone();
    let blur = FilterSpec::Blur {
        blur_x: 4.0,
        blur_y: 4.0,
        quality: 1,
    };
    assert!(editor.add_motion_span_filter(&span, blur.clone()));
    editor.project.validate().unwrap();
    assert_eq!(start_key(&editor).elements[0].filters, vec![blur]);
    undo_restores(&mut editor, &before);

    // 3. Resize motion span frames in the Motion editor footer in one Undo step.
    let before = editor.project.clone();
    assert!(editor.set_motion_span_frames(&span, 15, None));
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[1].frame,
        15,
        "span of 15 frames from frame 0 ends at frame 15"
    );
    undo_restores(&mut editor, &before);
}

#[test]
fn motion_editor_ui_add_property_popups_have_distinct_ids_and_add_skew_color_and_filter_tracks() {
    use crate::tests::primary_button;
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    let mut editor = motion_editor();
    // Playhead at frame 0 (span start): Add property must still create a track at span.end - 1.
    editor.frame = 0;

    let mut time = 0.0;
    let mut render = |editor: &mut Editor, events: Vec<egui::Event>| {
        time += 0.1;
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.motion_editor_panel_ui(ui);
                });
            },
        )
    };
    let click_at = |editor: &mut Editor,
                    render: &mut dyn FnMut(&mut Editor, Vec<egui::Event>) -> egui::FullOutput,
                    at: egui::Pos2| {
        render(
            editor,
            vec![egui::Event::PointerMoved(at), primary_button(at, true)],
        );
        render(editor, vec![primary_button(at, false)]);
        render(editor, vec![])
    };
    let find_add_buttons = |out: &egui::FullOutput| -> Vec<egui::Pos2> {
        let mut pts: Vec<egui::Pos2> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Add property"))
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                Some(egui::pos2(
                    ((b.x0 + b.x1) * 0.5) as f32,
                    ((b.y0 + b.y1) * 0.5) as f32,
                ))
            })
            .collect();
        pts.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        pts
    };
    let find_label_pos = |out: &egui::FullOutput, label: &str| -> (egui::Pos2, bool) {
        let (_, node) = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .unwrap_or_else(|| panic!("expected node labeled {label:?}"));
        let b = node.bounds().expect("bounds");
        (
            egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32),
            node.is_disabled(),
        )
    };

    let out = render(&mut editor, vec![]);
    let add_pts = find_add_buttons(&out);
    assert_eq!(
        add_pts.len(),
        4,
        "all 4 section Add property (+) buttons must have distinct widget IDs"
    );
    for pair in add_pts.windows(2) {
        assert!(
            pair[1].y > pair[0].y + 10.0,
            "section (+) buttons must have distinct vertical positions: {add_pts:?}"
        );
    }

    // 1. Click Transformation (+) -> click Skew: creates Skew key at frame 9 even when playhead is on frame 0.
    let out = click_at(&mut editor, &mut render, add_pts[1]);
    let (skew_pos, skew_disabled) = find_label_pos(&out, "Skew");
    assert!(!skew_disabled, "Skew menu item should be enabled");
    let _ = click_at(&mut editor, &mut render, skew_pos);
    let span = editor.motion_tween_span().unwrap();
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Skew),
        vec![0, 9]
    );
    editor.project.validate().unwrap();

    // Re-opening Transformation (+) keeps Skew enabled (not permanently disabled).
    let out = render(&mut editor, vec![]);
    let add_pts = find_add_buttons(&out);
    let out = click_at(&mut editor, &mut render, add_pts[1]);
    let (skew_pos_again, skew_disabled_again) = find_label_pos(&out, "Skew");
    assert!(
        !skew_disabled_again,
        "Skew menu item must remain enabled after adding Skew track"
    );
    let _ = click_at(&mut editor, &mut render, skew_pos_again);

    // 2. Click Colour effect (+) -> click Tint / Colour: creates Color key at frame 9 and supports brightness % edits.
    let out = render(&mut editor, vec![]);
    let add_pts = find_add_buttons(&out);
    let out = click_at(&mut editor, &mut render, add_pts[2]);
    let (tint_pos, tint_disabled) = find_label_pos(&out, "Tint / Colour");
    assert!(!tint_disabled);
    let _ = click_at(&mut editor, &mut render, tint_pos);
    let span = editor.motion_tween_span().unwrap();
    assert_eq!(
        editor.motion_key_frames(&span, TweenProperty::Color),
        vec![0, 9]
    );
    editor.set_motion_value(&span, TweenProperty::Color, 9, 50.0, None);
    editor.finish_edits();
    editor.project.validate().unwrap();
    let span = editor.motion_tween_span().unwrap();
    assert!(
        (editor.motion_value(&span, TweenProperty::Color, 9).unwrap() - 50.0).abs() < 1e-6,
        "Color brightness at frame 9 should be 50%"
    );
    let mid_color = editor.motion_value(&span, TweenProperty::Color, 5).unwrap();
    assert!(
        mid_color > 20.0 && mid_color < 35.0,
        "Color brightness interpolates across the span: {mid_color}"
    );

    // 3. Click Filters (+) -> click Blur: adds Blur filter to the span and opens Filters panel.
    let out = render(&mut editor, vec![]);
    let add_pts = find_add_buttons(&out);
    let out = click_at(&mut editor, &mut render, add_pts[3]);
    let (blur_pos, _) = find_label_pos(&out, "Blur");
    let _ = click_at(&mut editor, &mut render, blur_pos);
    editor.project.validate().unwrap();
    assert_eq!(start_key(&editor).elements[0].filters.len(), 1);
    assert!(editor.workspace.is_panel_visible(PanelId::Filters));
}

fn test_virtualization_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    ctx
}

fn build_large_virtualization_fixture(
    layer_count: usize,
    frame_count: u32,
    library_symbol_count: usize,
    output_line_count: usize,
) -> Editor {
    let mut project = Project::blank();
    project.name = "Large Virtualized Project".into();
    let root_id = project.root.clone();
    {
        let root_sym = project.symbol_mut(&root_id).expect("root symbol");
        root_sym.timeline.frame_count = frame_count;
        root_sym.timeline.layers.clear();
        for li in 0..layer_count {
            let mut keyframes = vec![
                Keyframe {
                    frame: 0,
                    elements: vec![element(
                        &format!("el_{li}_0"),
                        rectangle(),
                        10.0 + (li % 20) as f64 * 4.0,
                    )],
                    tween: (li % 3 == 0).then_some(Easing::Linear),
                    tween_kind: if li % 6 == 0 {
                        TweenKind::Motion
                    } else {
                        TweenKind::Classic
                    },
                    label: if li % 10 == 0 {
                        format!("start_{li}")
                    } else {
                        String::new()
                    },
                    ..Default::default()
                },
                Keyframe {
                    frame: frame_count / 2,
                    elements: vec![element(&format!("el_{li}_mid"), rectangle(), 120.0)],
                    tween: (li % 2 == 0).then_some(Easing::Linear),
                    tween_kind: TweenKind::Classic,
                    label: String::new(),
                    ..Default::default()
                },
                Keyframe {
                    frame: frame_count.saturating_sub(1),
                    elements: vec![element(&format!("el_{li}_end"), rectangle(), 240.0)],
                    tween: None,
                    tween_kind: TweenKind::Classic,
                    label: String::new(),
                    ..Default::default()
                },
            ];
            if li % 6 == 0 && frame_count > 200 {
                keyframes.insert(
                    1,
                    Keyframe {
                        frame: 50,
                        elements: vec![element(&format!("el_{li}_0"), rectangle(), 60.0)],
                        keyed_properties: Some(vec![TweenProperty::X, TweenProperty::Y]),
                        ..Default::default()
                    },
                );
            }
            root_sym.timeline.layers.push(Layer {
                id: format!("layer_{li}"),
                name: format!("Layer {}", li + 1),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
        }
    }

    for folder_idx in 0..10 {
        let _ = project.add_library_folder(&format!("Folder_{folder_idx:02}"));
    }
    for si in 0..library_symbol_count {
        let kind = match si % 3 {
            0 => SymbolKind::MovieClip,
            1 => SymbolKind::Graphic,
            _ => SymbolKind::Button,
        };
        let frame_total = if kind == SymbolKind::Button { 4 } else { 12 };
        project.symbols.push(Symbol {
            id: format!("lib_sym_{si:04}"),
            name: format!("Item_{si:04}"),
            kind,
            folder: format!("Folder_{:02}", si % 10),
            linkage_id: (kind != SymbolKind::Graphic && si % 5 == 0)
                .then(|| format!("Export_{si:04}")),
            class_name: None,
            timeline: Timeline {
                frame_count: frame_total,
                layers: vec![Layer {
                    id: format!("sym_layer_{si}"),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![element(&format!("sym_el_{si}"), rectangle(), 0.0)],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
        });
    }

    project.upgrade_format_for_features();
    let ctx = test_virtualization_ctx();
    let mut editor = Editor::from_project(&ctx, project);
    editor.output = (0..output_line_count)
        .map(|i| {
            if i % 25 == 0 {
                format!("[Frame {}] Error: synthetic diagnostic #{i}", (i % 200) + 1)
            } else if i % 10 == 0 {
                format!("Warning: timeline check at line {i}")
            } else {
                format!("trace(\"tick {i}\");")
            }
        })
        .collect();

    editor.layer = 0;
    editor.frame = 0;
    editor
}

#[test]
fn timeline_virtual_window_bounds_and_long_span_intersection() {
    use crate::panels::timeline::{
        DEFAULT_TIMELINE_OVERSCAN_FRAMES, DEFAULT_TIMELINE_OVERSCAN_ROWS, TimelineVirtualWindow,
    };

    let total_rows = 200usize;
    let total_frames = 2_000u32;
    let row_h = 28.0_f32;
    let cell_w = 10.0_f32;

    // Scrolled to row 50 (y = -50 * 28 = -1400) and frame 800 (x = -800 * 10 = -8000)
    // inside a 600x224 viewport (60 frames wide, 8 rows tall).
    let content_origin = egui::pos2(-8_000.0, -1_400.0);
    let clip_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 224.0));
    let win = TimelineVirtualWindow::with_default_overscan(
        clip_rect,
        content_origin,
        row_h,
        cell_w,
        total_rows,
        total_frames,
    );

    assert_eq!(win.overscan_rows, DEFAULT_TIMELINE_OVERSCAN_ROWS);
    assert_eq!(win.overscan_frames, DEFAULT_TIMELINE_OVERSCAN_FRAMES);
    assert_eq!(win.first_row, 50 - DEFAULT_TIMELINE_OVERSCAN_ROWS);
    assert_eq!(win.last_row, 57 + DEFAULT_TIMELINE_OVERSCAN_ROWS);
    assert_eq!(win.first_frame, 800 - DEFAULT_TIMELINE_OVERSCAN_FRAMES);
    assert_eq!(win.last_frame, 859 + DEFAULT_TIMELINE_OVERSCAN_FRAMES);

    let row_range = win.row_range(total_rows);
    assert_eq!(row_range.len(), 8 + 2 * DEFAULT_TIMELINE_OVERSCAN_ROWS);
    let total_h = win.top_spacer_height(row_h, total_rows)
        + (row_range.len() as f32 * row_h)
        + win.bottom_spacer_height(row_h, total_rows);
    assert!((total_h - total_rows as f32 * row_h).abs() < 1e-3);

    let frame_range = win.frame_range(total_frames);
    assert_eq!(
        frame_range.end - frame_range.start,
        60 + 2 * DEFAULT_TIMELINE_OVERSCAN_FRAMES
    );
    let total_w = win.left_spacer_width(cell_w, total_frames)
        + ((frame_range.end - frame_range.start) as f32 * cell_w)
        + win.right_spacer_width(cell_w, total_frames);
    assert!((total_w - total_frames as f32 * cell_w).abs() < 1e-3);

    // Long tween/span banner [0..2000] starting before `first_frame` and ending after `last_frame`
    // must still intersect the virtual window.
    assert!(win.span_intersects(0, 2_000));
    assert!(win.span_intersects(100, 820));
    assert!(win.span_intersects(850, 1_500));
    assert!(!win.span_intersects(0, 500));
    assert!(!win.span_intersects(1_200, 1_600));
}

#[test]
fn timeline_200_layers_by_2000_frames_virtualization_selection_and_scrub() {
    let mut editor = build_large_virtualization_fixture(200, 2_000, 10, 10);
    editor.project.validate().unwrap();

    // Select an off-screen frame range on layer 180 (frames 1500..=1520) and off-screen layer rows.
    editor.select_layer_row(180, false, false);
    editor.select_layer_row(185, false, true);
    let offscreen_range = FrameRange {
        layers: vec!["layer_180".to_owned(), "layer_185".to_owned()],
        start: 1_500,
        end: 1_520,
    };
    editor.select_frame_range(offscreen_range.clone());

    let ctx = test_virtualization_ctx();
    for _ in 0..2 {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_280.0, 320.0),
            )),
            ..Default::default()
        };
        raw.focused = true;
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.timeline_contents(ui);
            });
        });
    }

    let win = editor
        .last_timeline_virtual_window(&ctx)
        .expect("timeline virtual window recorded");
    assert_eq!(win.first_row, 0);
    assert!(win.last_row < 32);
    assert_eq!(win.first_frame, 0);
    assert!(win.last_frame < 200);

    let stats = editor
        .last_timeline_virtual_stats(&ctx)
        .expect("timeline virtualization stats recorded");
    assert_eq!(stats.total_rows, 200);
    assert_eq!(stats.total_frames, 2_000);
    assert!(
        stats.drawn_layer_rows > 0 && stats.drawn_layer_rows <= 28,
        "drawn layer rows must be O(visible), got {}",
        stats.drawn_layer_rows
    );
    assert!(
        stats.drawn_grid_rows > 0 && stats.drawn_grid_rows <= 28,
        "drawn grid rows must be O(visible), got {}",
        stats.drawn_grid_rows
    );
    assert!(
        stats.drawn_frame_columns > 0 && stats.drawn_frame_columns <= 180,
        "drawn frame columns must be O(visible), got {}",
        stats.drawn_frame_columns
    );
    assert!(
        stats.drawn_spans > 0,
        "visible spans intersecting the viewport must be rendered"
    );

    // Off-screen frame selection and layer-row multi-selection must be preserved untouched.
    assert_eq!(editor.frame_range(), Some(offscreen_range));
    assert!(editor.is_layer_row_selected(180));
    assert!(editor.is_layer_row_selected(185));

    // Scrub playhead across the timeline and verify zero document mutation.
    editor.frame = 1_250;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_280.0, 320.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.timeline_contents(ui);
            });
        },
    );
    assert_eq!(editor.frame, 1_250);
    assert!(
        !editor.history.can_undo(),
        "scrubbing must not mutate history"
    );
    editor.project.validate().unwrap();
}

#[test]
fn library_movie_explorer_output_virtualization_and_thumbnail_budget() {
    use crate::timeline_preview::MAX_THUMBNAIL_RENDERS_PER_FRAME;

    let mut editor = build_large_virtualization_fixture(200, 2_000, 1_000, 5_000);
    editor.project.validate().unwrap();

    let ctx = test_virtualization_ctx();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_440.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::SidePanel::left("lib_test")
                .exact_width(320.0)
                .show(ctx, |ui| {
                    editor.library_body(ui);
                });
            egui::SidePanel::right("explorer_test")
                .exact_width(340.0)
                .show(ctx, |ui| {
                    editor.movie_explorer_panel_ui(ui);
                });
            egui::TopBottomPanel::bottom("output_test")
                .exact_height(220.0)
                .show(ctx, |ui| {
                    editor.output_panel_virtualized(ui);
                });
        },
    );

    let lib_stats = editor
        .last_library_virtual_stats(&ctx)
        .expect("library virtual stats");
    assert!(
        lib_stats.total_rows >= 1_000,
        "expected >= 1,000 library rows, got {}",
        lib_stats.total_rows
    );
    assert!(
        lib_stats.drawn_rows > 0 && lib_stats.drawn_rows <= 45,
        "library drawn rows must stay O(visible), got {}",
        lib_stats.drawn_rows
    );

    let me_stats = editor
        .last_movie_explorer_virtual_stats(&ctx)
        .expect("movie explorer virtual stats");
    assert!(
        me_stats.total_rows >= 200,
        "expected >= 200 movie explorer rows, got {}",
        me_stats.total_rows
    );
    assert!(
        me_stats.drawn_rows > 0 && me_stats.drawn_rows <= 45,
        "movie explorer drawn rows must stay O(visible), got {}",
        me_stats.drawn_rows
    );

    let out_stats = editor
        .last_output_virtual_stats(&ctx)
        .expect("output virtual stats");
    assert_eq!(out_stats.total_lines, 5_000);
    assert!(
        out_stats.drawn_rows > 0 && out_stats.drawn_rows <= 45,
        "output drawn rows must stay O(visible), got {}",
        out_stats.drawn_rows
    );

    // Enable timeline preview thumbnails and verify per-frame render budget cap & stale eviction.
    editor.workspace.timeline.preview = crate::workspace::FramePreview::Layer;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_280.0, 640.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.timeline_contents(ui);
            });
        },
    );
    assert!(
        editor.timeline_thumbnails.rendered_in_last_pass() <= MAX_THUMBNAIL_RENDERS_PER_FRAME,
        "per-frame thumbnail rasterization must be capped at {MAX_THUMBNAIL_RENDERS_PER_FRAME}, got {}",
        editor.timeline_thumbnails.rendered_in_last_pass()
    );
    assert!(
        editor.timeline_thumbnails.cached_frame_count() > 0,
        "rendered thumbnails must populate the bounded frame cache"
    );
    assert!(
        editor.timeline_thumbnails.coalesced_skip_count() > 0,
        "excess thumbnail requests in a single pass must be coalesced/deferred"
    );

    // Jumping to a distant frame window evicts stale off-screen cached frames.
    editor.begin_thumbnail_pass_for_window(1_200, 1_280, &["layer_0".to_owned()]);
    assert!(
        editor.timeline_thumbnails.evicted_stale_count() > 0,
        "scrolling far away must evict stale off-screen thumbnail cache entries"
    );
    editor.project.validate().unwrap();
}

#[test]
fn large_project_ui_frame_execution_budget() {
    let mut editor = build_large_virtualization_fixture(200, 2_000, 1_000, 5_000);
    let ctx = test_virtualization_ctx();

    let run_pass = |editor: &mut Editor, frame_num: u32| {
        editor.frame = frame_num;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1_600.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::left("budget_lib")
                    .exact_width(300.0)
                    .show(ctx, |ui| {
                        editor.library_body(ui);
                    });
                egui::SidePanel::right("budget_explorer")
                    .exact_width(320.0)
                    .show(ctx, |ui| {
                        editor.movie_explorer_panel_ui(ui);
                    });
                egui::TopBottomPanel::bottom("budget_output")
                    .exact_height(180.0)
                    .show(ctx, |ui| {
                        editor.output_panel_virtualized(ui);
                    });
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.timeline_contents(ui);
                });
            },
        );
    };

    // Warm-up layout pass.
    run_pass(&mut editor, 0);

    let iterations = 5u32;
    let start = std::time::Instant::now();
    for i in 1..=iterations {
        run_pass(&mut editor, i * 10);
    }
    let avg_ms = start.elapsed().as_secs_f64() * 1_000.0 / f64::from(iterations);
    assert!(
        avg_ms < 250.0,
        "virtualized large-project frame pass exceeded budget: avg {avg_ms:.2} ms/frame"
    );
    editor.project.validate().unwrap();
}
