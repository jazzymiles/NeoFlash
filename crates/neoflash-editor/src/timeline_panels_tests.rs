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
        match preset_easing(preset, Easing::Linear) {
            Some(easing) if preset != EasePresetKind::Custom => {
                assert_eq!(preset_of(easing), preset, "{preset:?}");
            }
            Some(_) => {}
            None => assert!(matches!(
                preset,
                EasePresetKind::Bounce | EasePresetKind::Elastic
            )),
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
    assert_eq!(start_key(&editor).tween, Some(Easing::EaseOut));
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
    // Orient to path lives on the span start key and needs format 6.
    editor.set_orient_to_path(&span, true);
    assert!(start_key(&editor).orient_to_path);
    assert!(editor.project.format_version >= 6);
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
