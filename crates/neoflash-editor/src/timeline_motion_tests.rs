//! Motion path editing on the stage, Copy / Paste motion, Synchronise symbols, Orient to path and
//! the Timeline Preview frame view.
use crate::app::Editor;
use crate::selection::SelectionTarget;
use crate::tests::primary_button;
use crate::workspace::FramePreview;
use eframe::egui;
use neoflash_core::*;

fn instance(id: &str, symbol: &str, x: f64, y: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: symbol.into(),
            playback: InstancePlayback::Loop,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    }
}

fn art(symbol: &str, kind: SymbolKind, frames: u32) -> Symbol {
    Symbol {
        id: symbol.into(),
        name: symbol.into(),
        folder: String::new(),
        kind,
        timeline: Timeline {
            frame_count: frames,
            layers: vec![Layer {
                id: format!("{symbol}-art"),
                name: "Art".into(),
                visible: true,
                locked: false,
                keyframes: (0..frames)
                    .map(|frame| Keyframe {
                        frame,
                        elements: vec![Element {
                            kind: ElementKind::Rectangle {
                                width: 10.,
                                height: 10.,
                                fill: [0, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.,
                            },
                            ..instance(&format!("dot-{frame}"), symbol, frame as f64, 0.)
                        }],
                        ..Default::default()
                    })
                    .collect(),
                properties: LayerProperties::default(),
            }],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    }
}

fn row(id: &str, keyframes: Vec<Keyframe>) -> Layer {
    Layer {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes,
        properties: LayerProperties::default(),
    }
}

fn key(frame: u32, elements: Vec<Element>) -> Keyframe {
    Keyframe {
        frame,
        elements,
        ..Default::default()
    }
}

/// Root with 30 frames: layer "moving" holds a motion tween of `star` (◆ X at 10, ◆ Y at 20);
/// layer "still" holds `other` at (50, 50) for the whole timeline.
fn motion_editor() -> Editor {
    let mut project = Project::blank();
    project.symbols.push(art("clip", SymbolKind::MovieClip, 1));
    let mut start = key(0, vec![instance("star", "clip", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut x_key = key(10, vec![instance("star", "clip", 100., 0.)]);
    x_key.keyed_properties = Some(vec![TweenProperty::X]);
    let mut y_key = key(20, vec![instance("star", "clip", 100., 60.)]);
    y_key.keyed_properties = Some(vec![TweenProperty::Y]);
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 30;
    timeline.layers = vec![
        row("moving", vec![start, x_key, y_key]),
        row(
            "still",
            vec![key(0, vec![instance("other", "clip", 50., 50.)])],
        ),
    ];
    project.format_version = 6;
    project.validate().unwrap();
    Editor::from_project(&egui::Context::default(), project)
}

fn multi_object_motion_editor() -> Editor {
    let mut editor = motion_editor();
    editor.project.resources.push(
        ResourceUnit::from_bytes("motion-sound", "motion.wav", "audio/wav", b"")
            .expect("test audio resource"),
    );
    for key in &mut editor.project.symbols[0].timeline.layers[0].keyframes {
        let mut secondary = key.elements[0].clone();
        secondary.id = "secondary".into();
        secondary.name = "secondary".into();
        secondary.transform.x += 300.;
        secondary.transform.y += 50. + key.frame as f64;
        key.elements.push(secondary);
        if key.keyed_properties.is_some() {
            key.materialize_motion_keys();
        }
        if key.frame == 10 {
            key.label = "keep-label".into();
            key.sound = Some(FrameSound::new("motion-sound"));
        }
    }
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    editor
}

fn select(editor: &mut Editor, layer: &str, element: &str) {
    editor.set_selection(vec![SelectionTarget {
        layer: layer.into(),
        element: element.into(),
    }]);
    assert_eq!(editor.selection.len(), 1, "{element} is selectable");
}

fn position(editor: &Editor, layer: usize, id: &str, frame: u32) -> (f64, f64) {
    let timeline = &editor.symbol().timeline;
    let element = sample_layer_in(timeline, &timeline.layers[layer], frame)
        .into_iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("{id} at frame {frame}"));
    (element.transform.x, element.transform.y)
}

fn authored_position(editor: &Editor, layer: usize, id: &str, frame: u32) -> (f64, f64) {
    let element = editor.symbol().timeline.layers[layer]
        .keyframes
        .iter()
        .find(|key| key.frame == frame)
        .and_then(|key| key.elements.iter().find(|element| element.id == id))
        .unwrap_or_else(|| panic!("authored {id} at frame {frame}"));
    (element.transform.x, element.transform.y)
}

#[test]
fn paste_motion_reproduces_the_animation_on_another_instance() {
    let mut editor = motion_editor();
    editor.frame = 5;
    editor.layer = 0;
    select(&mut editor, "moving", "star");
    assert!(editor.can_copy_motion());
    editor.copy_motion();
    editor.clear_selection();
    assert!(
        !editor.can_paste_motion(),
        "Paste motion needs a selected object"
    );
    editor.frame = 0;
    editor.layer = 1;
    select(&mut editor, "still", "other");
    assert!(editor.can_paste_motion());
    let before = editor.project.clone();
    editor.paste_motion();
    editor.project.validate().unwrap();
    for frame in 0..30 {
        let (x, y) = position(&editor, 0, "star", frame);
        assert_eq!(
            position(&editor, 1, "other", frame),
            (x + 50., y + 50.),
            "frame {frame}"
        );
    }
    let still = &editor.symbol().timeline.layers[1];
    assert_eq!(still.keyframes[0].tween_kind, TweenKind::Motion);
    assert_eq!(still.keyframes.len(), 3, "start and two ◆ keys");
    editor.undo();
    assert_eq!(editor.project, before, "one Undo step");
}

#[test]
fn paste_motion_merges_only_the_selected_object_on_a_shared_motion_layer() {
    let mut editor = multi_object_motion_editor();
    editor.layer = 0;
    editor.frame = 0;
    select(&mut editor, "moving", "star");
    editor.copy_motion();
    select(&mut editor, "moving", "secondary");
    let before = editor.project.clone();
    editor.paste_motion();
    editor.project.validate().unwrap();

    let timeline = &editor.symbol().timeline;
    for frame in 0..30 {
        let old_star = sampled_element(&before.symbols[0].timeline, 0, "star", frame);
        let new_star = sampled_element(timeline, 0, "star", frame);
        assert_eq!(new_star, old_star, "source object at frame {frame}");
        let secondary = sampled_element(timeline, 0, "secondary", frame);
        assert!((secondary.transform.x - old_star.transform.x - 300.).abs() < 1e-7);
        assert!(
            (secondary.transform.y - old_star.transform.y - 50.).abs() < 1e-7,
            "secondary y at {frame}: {} vs {}",
            secondary.transform.y,
            old_star.transform.y + 50.
        );
    }
    let shared = timeline.layers[0]
        .keyframes
        .iter()
        .find(|key| key.frame == 10)
        .unwrap();
    assert_eq!(shared.label, "keep-label");
    assert_eq!(shared.sound, Some(FrameSound::new("motion-sound")));
    assert!(
        shared
            .motion_keys
            .as_ref()
            .is_some_and(|keys| keys.iter().any(|key| key.element_id == "star")
                && keys.iter().any(|key| key.element_id == "secondary"))
    );
    let reopened = Project::from_json(&editor.project.to_json().unwrap()).unwrap();
    reopened.validate().unwrap();
    editor.undo();
    assert_eq!(
        editor.project, before,
        "one Undo restores both tracks exactly"
    );
}

#[test]
fn paste_motion_special_on_shared_motion_layer_preserves_other_entries_and_metadata() {
    use crate::timeline_motion::PasteMotionOptions;
    let mut editor = multi_object_motion_editor();
    editor.layer = 0;
    editor.frame = 0;
    select(&mut editor, "moving", "star");
    editor.copy_motion();
    select(&mut editor, "moving", "secondary");
    let before = editor.project.clone();
    let baseline = before.symbols[0].timeline.clone();
    let mut options = PasteMotionOptions {
        properties: [false; 9],
        ..Default::default()
    };
    options.properties[0] = true;
    editor.paste_motion_special(&options);
    editor.project.validate().unwrap();
    for frame in 0..30 {
        let current_star = sampled_element(&editor.symbol().timeline, 0, "star", frame);
        let old_star = sampled_element(&baseline, 0, "star", frame);
        assert!(
            (current_star.transform.x - old_star.transform.x).abs() < 1e-9
                && (current_star.transform.y - old_star.transform.y).abs() < 1e-9
                && (current_star.transform.rotation - old_star.transform.rotation).abs() < 1e-9
                && (current_star.transform.alpha - old_star.transform.alpha).abs() < 1e-9,
            "other track at frame {frame}"
        );
        let actual = sampled_element(&editor.symbol().timeline, 0, "secondary", frame);
        let old_secondary = sampled_element(&baseline, 0, "secondary", frame);
        let source = sampled_element(&baseline, 0, "star", frame);
        assert!((actual.transform.x - source.transform.x - 300.).abs() < 1e-7);
        assert_eq!(
            actual.transform.y, old_secondary.transform.y,
            "unticked Y at frame {frame}"
        );
    }
    let shared = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .find(|key| key.frame == 10)
        .unwrap();
    assert_eq!(shared.label, "keep-label");
    assert_eq!(shared.sound, Some(FrameSound::new("motion-sound")));
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn paste_motion_copies_a_classic_tween_relative_to_the_target() {
    let mut editor = motion_editor();
    {
        let timeline = &mut editor.project.symbols[0].timeline;
        let mut start = key(0, vec![instance("star", "clip", 0., 0.)]);
        start.tween = Some(Easing::EaseIn);
        let mut end = instance("star", "clip", 40., 20.);
        end.transform.rotation = 90.;
        timeline.layers[0].keyframes = vec![start, key(8, vec![end])];
    }
    editor.frame = 2;
    select(&mut editor, "moving", "star");
    editor.copy_motion();
    editor.frame = 4;
    editor.layer = 1;
    select(&mut editor, "still", "other");
    editor.paste_motion();
    editor.project.validate().unwrap();
    for offset in 0..=8 {
        let (x, y) = position(&editor, 0, "star", offset);
        assert_eq!(
            position(&editor, 1, "other", 4 + offset),
            (x + 50., y + 50.),
            "offset {offset}"
        );
    }
    // Before the pasted span the object stays where it was; the end key holds afterwards.
    assert_eq!(position(&editor, 1, "other", 2), (50., 50.));
    assert_eq!(position(&editor, 1, "other", 20), (90., 70.));
    let still = &editor.symbol().timeline.layers[1];
    let end = still.keyframes.iter().find(|k| k.frame == 12).unwrap();
    assert_eq!(end.elements[0].transform.rotation, 90.);
}

#[test]
fn dragging_a_motion_path_point_keys_x_and_y_with_one_undo() {
    let mut editor = motion_editor();
    editor.frame = 15;
    select(&mut editor, "moving", "star");
    let path = editor
        .motion_path()
        .expect("motion path of the selected star");
    assert_eq!(path.frames.len(), 30);
    assert_eq!(
        path.keys,
        vec![(0, [0., 0.]), (10, [100., 30.]), (20, [100., 60.])],
        "key points sit on the shown path (Y is unkeyed at the ◆ X key)"
    );
    let before = editor.project.clone();
    // Drag the ◆ X key at frame 10 by (+20, +30).
    assert!(editor.begin_motion_key_drag(10, [100., 30.]));
    editor.drag_motion_key_to([110., 40.]);
    editor.drag_motion_key_to([120., 60.]);
    editor.finish_motion_key_drag();
    let layer = &editor.symbol().timeline.layers[0];
    let diamond = layer.keyframes.iter().find(|k| k.frame == 10).unwrap();
    let keyed = diamond.keyed_properties.as_ref().unwrap();
    assert!(keyed.contains(&TweenProperty::X) && keyed.contains(&TweenProperty::Y));
    assert_eq!(position(&editor, 0, "star", 10), (120., 60.));
    assert_eq!(
        editor.motion_path().unwrap().keys[1],
        (10, [120., 60.]),
        "the path follows the moved key"
    );
    editor.undo();
    assert_eq!(editor.project, before, "the drag is one Undo step");
    // A press without movement changes nothing and records no Undo step.
    assert!(editor.begin_motion_key_drag(20, [100., 60.]));
    editor.drag_motion_key_to([100., 60.]);
    editor.finish_motion_key_drag();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    // Outside the motion span (or without a selection) there is no path.
    editor.clear_selection();
    assert!(editor.motion_path().is_none());
}

#[test]
fn transformed_motion_path_drag_inverse_maps_the_pointer() {
    let mut editor = motion_editor();
    editor.zoom = 1.0;
    editor.frame = 15;
    select(&mut editor, "moving", "star");
    let mapping =
        crate::stage_mapping::StageMapping::try_new([1.2, 0.4, -0.3, 0.9, 50., 20.]).unwrap();
    let origin = egui::pos2(100., 100.);
    let screen = |point: egui::Pos2| mapping.edit_to_screen(origin, editor.zoom, point);
    let from = screen(egui::pos2(100., 30.));
    let to = screen(egui::pos2(125., 65.));
    let ctx = egui::Context::default();
    let mut frame = |time: f64, events: Vec<egui::Event>| {
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.motion_path_input(ui, ui.max_rect(), origin, mapping);
                });
            },
        );
    };
    frame(
        0.0,
        vec![egui::Event::PointerMoved(from), primary_button(from, true)],
    );
    frame(0.02, vec![egui::Event::PointerMoved(to)]);
    frame(0.04, vec![primary_button(to, false)]);

    let (x, y) = position(&editor, 0, "star", 10);
    assert!(
        (x - 125.).abs() < 1e-4 && (y - 65.).abs() < 1e-4,
        "({x}, {y})"
    );
    assert!(editor.timeline_motion.drag.is_none());
}

#[test]
fn synchronise_symbols_adjusts_graphic_first_frames() {
    let mut project = Project::blank();
    project.symbols.push(art("walk", SymbolKind::Graphic, 8));
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 12;
    timeline.layers[0].keyframes = vec![
        key(0, vec![instance("a", "walk", 0., 0.)]),
        key(3, vec![instance("b", "walk", 0., 0.)]),
    ];
    project.validate().unwrap();
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    editor.frame = 3;
    assert!(editor.can_sync_symbols());
    let before = editor.project.clone();
    editor.sync_symbols();
    let key = &editor.symbol().timeline.layers[0].keyframes[1];
    assert!(matches!(
        key.elements[0].kind,
        ElementKind::Instance {
            playback: InstancePlayback::LoopFrom(3),
            ..
        }
    ));
    editor.undo();
    assert_eq!(editor.project, before, "one Undo step");
}

#[test]
fn orient_to_path_is_set_on_the_tween_under_the_playhead() {
    let mut editor = motion_editor();
    editor.layer = 1;
    editor.frame = 0;
    select(&mut editor, "still", "other");
    if let Some(span) = editor.tween_span() {
        editor.set_orient_to_path(&span, true);
    }
    assert!(
        !editor.symbol().timeline.layers[1].keyframes[0].orient_to_path,
        "an untweened span cannot orient"
    );
    editor.project.symbols[0].timeline.layers[1].keyframes[0].tween = Some(Easing::Linear);
    editor.project.symbols[0].timeline.layers[1].keyframes[0].tween_kind = TweenKind::Motion;
    editor.project.symbols[0].timeline.layers[1]
        .keyframes
        .push(key(10, vec![instance("other", "clip", 80., 50.)]));
    let before = editor.project.clone();
    editor.frame = 4;
    if let Some(span) = editor.tween_span() {
        editor.set_orient_to_path(&span, true);
    }
    assert!(editor.symbol().timeline.layers[1].keyframes[0].motion_orient_to_path_for("other"));
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn preview_frame_view_grows_rows_and_caches_visible_thumbnails() {
    let mut editor = motion_editor();
    let view = &mut editor.workspace.timeline;
    assert_eq!(view.row_height(), neoflash_ui::metrics::LAYER_ROW);
    view.short_rows = true;
    view.preview = FramePreview::Layer;
    assert_eq!(view.row_height(), neoflash_ui::metrics::LAYER_ROW_PREVIEW);
    view.preview = FramePreview::InContext;
    assert_eq!(view.row_height(), neoflash_ui::metrics::LAYER_ROW_PREVIEW);
    // The option persists with the workspace.
    let json = serde_json::to_string(&editor.workspace.timeline).unwrap();
    let restored: crate::workspace::TimelineView = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.preview, FramePreview::InContext);
    // Thumbnails paint and are cached per keyframe until the document changes.
    editor.workspace.timeline.preview = FramePreview::Layer;
    let ctx = egui::Context::default();
    let mut painted = 0;
    let mut paint = |editor: &mut Editor| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let painter = ui.painter().clone();
                editor.begin_thumbnail_pass();
                let rect = egui::Rect::from_min_size(egui::pos2(10., 10.), egui::vec2(62., 38.));
                let layer = editor.symbol().timeline.layers[1].clone();
                editor.paint_span_thumbnail(&painter, &layer, 0, rect);
            });
        });
        painted += 1;
    };
    paint(&mut editor);
    assert_eq!(editor.timeline_thumbnails.len(), 1);
    let left = |editor: &Editor| {
        editor
            .timeline_thumbnails
            .bounds("still", 0)
            .unwrap()
            .left()
    };
    let first = left(&editor);
    // Without an edit the cached scene is reused, even when the document is changed in place.
    editor.project.symbols[0].timeline.layers[1].keyframes[0].elements[0]
        .transform
        .x = 60.;
    paint(&mut editor);
    assert_eq!(left(&editor), first, "cached");
    editor.transact(|s| {
        s.symbol_mut().timeline.layers[1].keyframes[0].elements[0]
            .transform
            .x = 70.
    });
    paint(&mut editor);
    assert!(
        (left(&editor) - first - 20.).abs() < 1e-3,
        "re-rendered after the committed edit"
    );
    assert_eq!(painted, 3);
}

#[test]
fn copy_paste_motion_preserves_per_property_eases() {
    let mut editor = motion_editor();
    editor.project.symbols[0].timeline.layers[0].keyframes[0].property_eases = vec![
        (TweenProperty::X, Easing::Bounce),
        (TweenProperty::Y, Easing::EaseIn),
    ];
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();

    editor.frame = 4;
    editor.layer = 0;
    select(&mut editor, "moving", "star");
    editor.copy_motion();

    editor.frame = 0;
    editor.layer = 1;
    select(&mut editor, "still", "other");
    let before = editor.project.clone();
    editor.paste_motion();
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[1].keyframes[0]
            .motion_track("other")
            .and_then(|track| track.property_eases.clone())
            .unwrap(),
        vec![
            (TweenProperty::X, Easing::Bounce),
            (TweenProperty::Y, Easing::EaseIn),
        ]
    );
    for frame in 0..30 {
        let (x, y) = position(&editor, 0, "star", frame);
        let (ox, oy) = position(&editor, 1, "other", frame);
        assert!(
            (ox - (x + 50.0)).abs() < 1e-6 && (oy - (y + 50.0)).abs() < 1e-6,
            "frame {frame}: ({ox}, {oy}) vs ({}, {})",
            x + 50.0,
            y + 50.0
        );
    }
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn show_motion_path_and_frame_dots_preferences_control_stage_overlay_without_history() {
    let legacy: crate::workspace::TimelineView =
        serde_json::from_str(r#"{"tinted":true,"show_seconds":true}"#).unwrap();
    assert!(legacy.show_motion_path);
    assert!(legacy.show_motion_frame_dots);

    let mut editor = motion_editor();
    editor.frame = 5;
    select(&mut editor, "moving", "star");
    assert!(editor.motion_paths_interactive());

    let ctx = egui::Context::default();
    let count_shapes = |editor: &Editor, paint: bool| -> usize {
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if paint {
                    editor.paint_motion_path(
                        ui.painter(),
                        egui::pos2(20.0, 20.0),
                        crate::stage_mapping::StageMapping::identity(),
                    );
                }
            });
        })
        .shapes
        .len()
    };
    let baseline = count_shapes(&editor, false);
    let with_dots = count_shapes(&editor, true).saturating_sub(baseline);
    editor.workspace.timeline.show_motion_frame_dots = false;
    let without_dots = count_shapes(&editor, true).saturating_sub(baseline);
    assert!(
        with_dots > without_dots && without_dots > 0,
        "hiding frame dots keeps path line and key diamonds ({without_dots} vs {with_dots})"
    );
    editor.workspace.timeline.show_motion_path = false;
    assert!(!editor.motion_paths_interactive());
    assert!(!editor.begin_motion_key_drag(10, [100.0, 30.0]));
    assert_eq!(count_shapes(&editor, true).saturating_sub(baseline), 0);
    assert!(
        !editor.history.can_undo(),
        "view toggles never record Undo steps"
    );
}

#[test]
fn motion_sync_symbols_and_snap_to_guide_options_commit_with_one_undo() {
    // 1. Sync graphic symbols on a motion tween.
    let mut project = Project::blank();
    project.symbols.push(art("walk", SymbolKind::Graphic, 8));
    let mut k0 = key(0, vec![instance("a", "walk", 0., 0.)]);
    k0.tween = Some(Easing::Linear);
    k0.tween_kind = TweenKind::Motion;
    let mut b_inst = instance("b", "walk", 40., 0.);
    b_inst.id = "a".into();
    b_inst.kind = ElementKind::Instance {
        symbol_id: "walk".into(),
        playback: InstancePlayback::LoopFrom(6),
    };
    let k3 = key(3, vec![b_inst]);
    project.symbols[0].timeline.frame_count = 10;
    project.symbols[0].timeline.layers[0].keyframes = vec![k0, k3];
    project.format_version = 6;
    project.validate().unwrap();
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    assert_eq!(
        evaluate(&editor.project, &editor.project.root, 3)[0].element_id,
        "dot-1"
    );
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    select(&mut editor, &layer_id, "a");
    let span = editor.motion_tween_span().unwrap();
    let before = editor.project.clone();
    editor.set_motion_sync_symbols(&span, true);
    editor.project.validate().unwrap();
    assert_eq!(editor.project.format_version, 10);
    assert!(editor.symbol().timeline.layers[0].keyframes[0].motion_sync_symbols_for("a"));
    assert!(matches!(
        editor.symbol().timeline.layers[0].keyframes[1].elements[0].kind,
        ElementKind::Instance {
            playback: InstancePlayback::Loop,
            ..
        }
    ));
    assert_eq!(
        evaluate(&editor.project, &editor.project.root, 3)[0].element_id,
        "dot-3"
    );
    editor.undo();
    assert_eq!(editor.project, before);

    // 2. Snap to motion guide snaps X/Y keys and stage drags onto the guide stroke.
    let mut editor = motion_editor();
    editor.add_motion_guide();
    // "moving" is layer 0; Guide is inserted above it at layer 1.
    editor.project.symbols[0].timeline.layers[1].keyframes = vec![key(
        0,
        vec![Element {
            id: "guide-line".into(),
            name: "guide-line".into(),
            transform: Transform::default(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 20.0 },
                    PathCommand::LineTo { x: 200.0, y: 20.0 },
                ],
                fill_rule: Default::default(),
                fill: [0, 0, 0, 0],
                stroke: [0, 0, 0, 255],
                stroke_width: 1.0,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        }],
    )];
    editor.layer = 0;
    editor.frame = 5;
    select(&mut editor, "moving", "star");
    let span = editor.motion_tween_span().unwrap();
    let before_snap = editor.project.clone();
    editor.set_motion_snap_to_guide(&span, true);
    editor.project.validate().unwrap();
    assert!(editor.symbol().timeline.layers[0].keyframes[0].motion_snap_to_guide_for("star"));
    assert_eq!(position(&editor, 0, "star", 0), (0.0, 20.0));
    assert_eq!(position(&editor, 0, "star", 10), (100.0, 20.0));

    // Dragging key 10 to (130, 26) (within 12 px of y=20) snaps onto (130, 20).
    select(&mut editor, "moving", "star");
    assert!(editor.begin_motion_key_drag(10, [100.0, 20.0]));
    editor.drag_motion_key_to([130.0, 26.0]);
    editor.finish_motion_key_drag();
    assert_eq!(position(&editor, 0, "star", 10), (130.0, 20.0));
    editor.undo();
    editor.undo();
    assert_eq!(editor.project, before_snap);
}

#[test]
fn motion_guide_drag_snap_threshold_is_measured_in_affine_screen_space() {
    let mut editor = motion_editor();
    editor.add_motion_guide();
    editor.project.symbols[0].timeline.layers[1].keyframes = vec![key(
        0,
        vec![Element {
            id: "guide-line-screen-distance".into(),
            name: "guide-line-screen-distance".into(),
            transform: Transform::default(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 20.0 },
                    PathCommand::LineTo { x: 200.0, y: 20.0 },
                ],
                fill_rule: Default::default(),
                fill: [0, 0, 0, 0],
                stroke: [0, 0, 0, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        }],
    )];
    editor.layer = 0;
    editor.frame = 5;
    select(&mut editor, "moving", "star");
    let span = editor.motion_tween_span().unwrap();
    editor.set_motion_snap_to_guide(&span, true);

    // Only 0.2 local units from the guide, but 20 screen points after the captured Y stretch:
    // the key must remain unsnapped.
    let stretched = crate::stage_mapping::StageMapping::try_new([1., 0., 0., 10., 0., 0.]).unwrap();
    editor.zoom = 10.;
    assert!(
        editor.begin_motion_key_drag_in_stage_context(10, [100., 20.], stretched, editor.zoom,)
    );
    editor.drag_motion_key_to([130., 20.2]);
    editor.finish_motion_key_drag();
    let unsnapped = authored_position(&editor, 0, "star", 10);
    assert!((unsnapped.0 - 130.).abs() < 1e-9, "{unsnapped:?}");
    assert!((unsnapped.1 - 20.2).abs() < 1e-9, "{unsnapped:?}");
    editor.undo();

    // Twenty local units become ten screen points after the captured Y compression, so the same
    // 12-point threshold must attract the key to the guide.
    let compressed = crate::stage_mapping::StageMapping::try_new([1., 0., 0., 2., 0., 0.]).unwrap();
    editor.zoom = 0.25;
    assert!(editor.begin_motion_key_drag_in_stage_context(
        10,
        [100., 20.],
        compressed,
        editor.zoom,
    ));
    editor.drag_motion_key_to([130., 40.]);
    editor.finish_motion_key_drag();
    assert_eq!(authored_position(&editor, 0, "star", 10), (130., 20.));
    editor.project.validate().unwrap();
}

fn sampled_element(timeline: &Timeline, layer: usize, id: &str, frame: u32) -> Element {
    sample_layer_in(timeline, &timeline.layers[layer], frame)
        .into_iter()
        .find(|element| element.id == id)
        .unwrap()
}

#[test]
fn paste_motion_special_preserves_unticked_animation_and_other_objects_across_both_boundaries() {
    use crate::timeline_motion::PasteMotionOptions;
    let mut editor = motion_editor();
    {
        let timeline = &mut editor.project.symbols[0].timeline;
        timeline.frame_count = 60;
        timeline.layers[0]
            .keyframes
            .push(key(30, vec![instance("star", "clip", 100., 60.)]));
        let mut start = key(
            0,
            vec![
                instance("other", "clip", 50., 20.),
                instance("bystander", "clip", 10., 30.),
            ],
        );
        start.tween = Some(Easing::EaseInOut);
        let mut end = start.clone();
        end.frame = 50;
        end.tween = None;
        end.label = "Destination label".into();
        end.elements[0].transform = Transform {
            x: 150.,
            y: 300.,
            scale_x: 2.,
            scale_y: 3.,
            rotation: 120.,
            skew: 10.,
            alpha: 0.3,
            ..Transform::default()
        };
        end.elements[1].transform.x = 160.;
        timeline.layers[1].keyframes = vec![start, end];
    }
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    editor.layer = 0;
    editor.frame = 0;
    select(&mut editor, "moving", "star");
    editor.copy_motion();
    editor.layer = 1;
    editor.frame = 5;
    select(&mut editor, "still", "other");
    let before = editor.project.clone();
    let baseline = &before.symbols[0].timeline;
    let target_x = sampled_element(baseline, 1, "other", 5).transform.x;
    let mut options = PasteMotionOptions {
        properties: [false; 9],
        ..Default::default()
    };
    options.properties[0] = true;
    options.properties[8] = true;
    editor.paste_motion_special(&options);
    editor.project.validate().unwrap();
    for frame in 0..60 {
        let mut wanted = sampled_element(baseline, 1, "other", frame);
        if (5..35).contains(&frame) {
            wanted.transform.x =
                target_x + sampled_element(baseline, 0, "star", frame - 5).transform.x;
        }
        assert_eq!(
            sampled_element(&editor.symbol().timeline, 1, "other", frame),
            wanted,
            "chosen X only, frame {frame}"
        );
        assert_eq!(
            sampled_element(&editor.symbol().timeline, 1, "bystander", frame),
            sampled_element(baseline, 1, "bystander", frame),
            "other object, frame {frame}"
        );
    }
    assert_eq!(
        editor.symbol().timeline.layers[1].key_at(50).unwrap().label,
        "Destination label"
    );
    editor.undo();
    assert_eq!(
        editor.project, before,
        "one Undo restores editable target spans"
    );
}

fn styled_paste_editor() -> Editor {
    let mut editor = motion_editor();
    let timeline = &mut editor.project.symbols[0].timeline;
    let mut start = key(0, vec![instance("star", "clip", 0., 0.)]);
    start.tween = Some(Easing::EaseIn);
    start.elements[0].transform = Transform {
        scale_x: 2.,
        scale_y: 3.,
        rotation: 20.,
        skew: 5.,
        alpha: 0.6,
        color_transform: ColorTransform {
            multipliers: [0.6, 1., 1., 1.],
            offsets: [0.; 4],
        },
        ..Transform::default()
    };
    start.elements[0].filters = vec![FilterSpec::Blur {
        blur_x: 6.,
        blur_y: 4.,
        quality: 1,
    }];
    start.elements[0].blend_mode = BlendMode::Multiply;
    let mut end = start.clone();
    end.frame = 8;
    end.tween = None;
    end.elements[0].transform = Transform {
        x: 80.,
        y: 40.,
        scale_x: 4.,
        scale_y: 6.,
        rotation: 80.,
        skew: 15.,
        alpha: 0.2,
        color_transform: ColorTransform {
            multipliers: [0.2, 1., 1., 1.],
            offsets: [0.; 4],
        },
        ..Transform::default()
    };
    timeline.layers[0].keyframes = vec![start, end];
    let target = &mut timeline.layers[1].keyframes[0].elements[0];
    target.transform = Transform {
        x: 50.,
        y: 50.,
        scale_x: 10.,
        scale_y: 20.,
        rotation: 100.,
        skew: 25.,
        alpha: 0.9,
        ..Transform::default()
    };
    editor.project.upgrade_format_for_features();
    editor.project.validate().unwrap();
    editor.layer = 0;
    editor.frame = 0;
    select(&mut editor, "moving", "star");
    editor.copy_motion();
    editor.layer = 1;
    editor.frame = 4;
    select(&mut editor, "still", "other");
    editor
}

#[test]
fn paste_motion_special_scale_rotation_overrides_colour_filters_blend_and_ease() {
    use crate::timeline_motion::PasteMotionOptions;
    for preserve in [false, true] {
        let mut editor = styled_paste_editor();
        let before = editor.project.clone();
        let options = PasteMotionOptions {
            properties: [false, false, true, true, true, false, true, true, true],
            preserve_scale: preserve,
            preserve_rotation: preserve,
        };
        editor.paste_motion_special(&options);
        editor.project.validate().unwrap();
        for offset in 0..=8 {
            let copied = sampled_element(&before.symbols[0].timeline, 0, "star", offset);
            let actual = sampled_element(&editor.symbol().timeline, 1, "other", 4 + offset);
            assert_eq!(
                (
                    actual.transform.x,
                    actual.transform.y,
                    actual.transform.alpha
                ),
                (50., 50., 0.9)
            );
            assert_eq!(actual.transform.color_transform, ColorTransform::default());
            assert_eq!(actual.filters, copied.filters);
            assert_eq!(actual.blend_mode, copied.blend_mode);
            assert_eq!(
                actual.transform.scale_x,
                if preserve {
                    10. * copied.transform.scale_x / 2.
                } else {
                    copied.transform.scale_x
                }
            );
            assert_eq!(
                actual.transform.scale_y,
                if preserve {
                    20. * copied.transform.scale_y / 3.
                } else {
                    copied.transform.scale_y
                }
            );
            assert_eq!(
                actual.transform.rotation,
                if preserve {
                    100. + copied.transform.rotation - 20.
                } else {
                    copied.transform.rotation
                }
            );
            assert_eq!(
                actual.transform.skew,
                if preserve {
                    25. + copied.transform.skew - 5.
                } else {
                    copied.transform.skew
                }
            );
        }
        editor.undo();
        assert_eq!(editor.project, before);
        // Colour alone includes opacity and channel transform; Ease off defaults the still target to Linear.
        let mut colour = PasteMotionOptions {
            properties: [false; 9],
            ..Default::default()
        };
        colour.properties[5] = true;
        editor.paste_motion_special(&colour);
        editor.project.validate().unwrap();
        let mid = sampled_element(&editor.symbol().timeline, 1, "other", 8);
        assert!((mid.transform.alpha - 0.4).abs() < 1e-9);
        assert!((mid.transform.color_transform.multipliers[0] - 0.4).abs() < 1e-9);
        assert_eq!(mid.filters, Vec::<FilterSpec>::new());
        assert_eq!(mid.blend_mode, BlendMode::Normal);
        editor.undo();
        assert_eq!(editor.project, before);
    }

    let mut editor = styled_paste_editor();
    let mut end = editor.project.symbols[0].timeline.layers[1].keyframes[0].clone();
    end.frame = 20;
    end.elements[0].transform.x = 200.;
    editor.project.symbols[0].timeline.layers[1].keyframes[0].tween = Some(Easing::Linear);
    editor.project.symbols[0].timeline.layers[1]
        .keyframes
        .push(end);
    let before = editor.project.clone();
    let mut ease = PasteMotionOptions {
        properties: [false; 9],
        ..Default::default()
    };
    ease.properties[8] = true;
    editor.paste_motion_special(&ease);
    editor.project.validate().unwrap();
    let actual = &editor.symbol().timeline.layers[1];
    assert_eq!(actual.keyframes.len(), 2, "Ease only retains authored keys");
    assert_eq!(actual.keyframes[0].tween, Some(Easing::EaseIn));
    assert_eq!(
        actual.keyframes[0].elements,
        before.symbols[0].timeline.layers[1].keyframes[0].elements
    );
    assert_eq!(
        actual.keyframes[1],
        before.symbols[0].timeline.layers[1].keyframes[1]
    );
    assert!(position(&editor, 1, "other", 10).0 < 125.);
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn paste_motion_special_dialog_none_cancel_enter_and_session_choices() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_fonts(neoflash_ui::typography::font_definitions());
    neoflash_ui::theme::install(&ctx, neoflash_ui::Theme::Dark);
    let mut editor = styled_paste_editor();
    let before = editor.project.clone();
    let mut time = 0.;
    let mut render = |editor: &mut Editor, events| {
        time += 0.1;
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 800.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| editor.paste_motion_special_dialog(ctx),
        )
    };
    let key = |key| egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    editor.open_paste_motion_special();
    // The first modal pass measures its body; centering uses those dimensions on the next pass.
    render(&mut editor, vec![]);
    let output = render(&mut editor, vec![]);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some("Select none"))
        .and_then(|(_, n)| n.bounds())
        .unwrap();
    let at = egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.) as f32,
        ((bounds.y0 + bounds.y1) / 2.) as f32,
    );
    render(
        &mut editor,
        vec![
            egui::Event::PointerMoved(at),
            crate::tests::primary_button(at, true),
        ],
    );
    render(&mut editor, vec![crate::tests::primary_button(at, false)]);
    let output = render(&mut editor, vec![]);
    assert!(!editor.timeline_motion.paste_options.any());
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Paste"))
            .unwrap()
            .1
            .is_disabled()
    );
    render(&mut editor, vec![key(egui::Key::Enter)]);
    assert!(
        editor.timeline_motion.paste_special_open,
        "Enter cannot paste an empty choice"
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    render(&mut editor, vec![key(egui::Key::Escape)]);
    assert!(!editor.timeline_motion.paste_special_open);
    editor.open_paste_motion_special();
    assert!(
        !editor.timeline_motion.paste_options.any(),
        "choices persist within the session"
    );
    editor.timeline_motion.paste_options.properties[0] = true;
    render(&mut editor, vec![]);
    render(&mut editor, vec![key(egui::Key::Enter)]);
    assert!(!editor.timeline_motion.paste_special_open);
    editor.project.validate().unwrap();
    assert_ne!(editor.project, before);
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn paste_curved_motion_between_group_members_preserves_other_objects_and_undo() {
    for (source_group, target_group) in [(false, true), (true, false), (true, true)] {
        let mut editor = motion_editor();
        let keys = &mut editor.project.symbols[0].timeline.layers[0].keyframes;
        if source_group {
            for key in keys.iter_mut() {
                let (x, y) = match key.frame {
                    0 => (-20., 10.),
                    10 => (180., 10.),
                    _ => (180., 110.),
                };
                key.elements.insert(0, instance("reference", "clip", x, y));
            }
        }
        keys[0].motion_path = Some(if source_group {
            vec![
                PathCommand::MoveTo { x: -20., y: 10. },
                PathCommand::CubicTo {
                    cx1: 0.,
                    cy1: 150.,
                    cx2: 140.,
                    cy2: 150.,
                    x: 180.,
                    y: 60.,
                },
                PathCommand::LineTo { x: 180., y: 110. },
            ]
        } else {
            vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::CubicTo {
                    cx1: 0.,
                    cy1: 100.,
                    cx2: 80.,
                    cy2: 100.,
                    x: 100.,
                    y: 30.,
                },
                PathCommand::LineTo { x: 100., y: 60. },
            ]
        });
        keys[0].orient_to_path = true;
        keys[0].tween = Some(Easing::EaseIn);
        if target_group {
            editor.project.symbols[0].timeline.layers[1].keyframes[0]
                .elements
                .insert(0, instance("stationary", "clip", -40., 5.));
        }
        editor.project.upgrade_format_for_features();
        editor.project.validate().unwrap();
        editor.layer = 0;
        editor.frame = 0;
        select(&mut editor, "moving", "star");
        editor.copy_motion();
        editor.layer = 1;
        select(&mut editor, "still", "other");
        let before = editor.project.clone();
        let source_origin = position(&editor, 0, "star", 0);
        editor.paste_motion();
        editor.project.validate().unwrap();
        assert!(
            editor.symbol().timeline.layers[1].keyframes[0]
                .motion_path
                .is_none()
        );
        for frame in 0..30 {
            let source = position(&editor, 0, "star", frame);
            let target = position(&editor, 1, "other", frame);
            assert!((target.0 - (50. + source.0 - source_origin.0)).abs() < 1e-7);
            assert!((target.1 - (50. + source.1 - source_origin.1)).abs() < 1e-7);
            if target_group {
                assert_eq!(position(&editor, 1, "stationary", frame), (-40., 5.));
            }
        }
        editor.undo();
        assert_eq!(editor.project, before);
    }
}
