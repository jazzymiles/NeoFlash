//! Motion path editing on the stage, Copy / Paste motion, Synchronise symbols, Orient to path and
//! the Timeline Preview frame view.
use crate::app::Editor;
use crate::selection::SelectionTarget;
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

#[test]
fn paste_motion_reproduces_the_animation_on_another_instance() {
    let mut editor = motion_editor();
    editor.frame = 5;
    editor.layer = 0;
    assert!(editor.can_copy_motion());
    editor.copy_motion();
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
    if let Some(span) = editor.tween_span() {
        editor.set_orient_to_path(&span, true);
    }
    assert!(
        !editor.symbol().timeline.layers[1].keyframes[0].orient_to_path,
        "an untweened span cannot orient"
    );
    editor.project.symbols[0].timeline.layers[1].keyframes[0].tween = Some(Easing::Linear);
    editor.project.symbols[0].timeline.layers[1]
        .keyframes
        .push(key(10, vec![instance("other", "clip", 80., 50.)]));
    let before = editor.project.clone();
    editor.frame = 4;
    if let Some(span) = editor.tween_span() {
        editor.set_orient_to_path(&span, true);
    }
    assert!(editor.symbol().timeline.layers[1].keyframes[0].orient_to_path);
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
