//! Cross-path regressions reported from native Timeline authoring.

use crate::app::Editor;
use crate::tests::{primary_button, rectangle_stage, stage_input_frame};
use crate::workspace::TweenOnDrag;
use eframe::egui;
use neoflash_core::*;

fn timeline_panel_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| editor.timeline(ctx),
    )
}

fn accessible_bounds(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
                .map(|rect| {
                    egui::Rect::from_min_max(
                        egui::pos2(rect.x0 as f32, rect.y0 as f32),
                        egui::pos2(rect.x1 as f32, rect.y1 as f32),
                    )
                })
        })
}

fn click_timeline_control(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: &mut f64,
    output: &egui::FullOutput,
    label: &str,
) -> egui::FullOutput {
    let point = accessible_bounds(output, label)
        .unwrap_or_else(|| panic!("{label:?} must be visible"))
        .center();
    *time += 0.1;
    timeline_panel_frame(
        editor,
        ctx,
        *time,
        vec![
            egui::Event::PointerMoved(point),
            primary_button(point, true),
        ],
    );
    *time += 0.1;
    timeline_panel_frame(editor, ctx, *time, vec![primary_button(point, false)]);
    *time += 0.1;
    timeline_panel_frame(editor, ctx, *time, vec![]);
    *time += 0.1;
    timeline_panel_frame(editor, ctx, *time, vec![])
}

fn object(id: &str, x: f64, y: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 12.0,
            fill: [20, 90, 180, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    }
}

#[test]
fn motion_tween_tracks_several_objects_on_one_layer_by_stable_id() {
    let mut project = Project::blank();
    let layer = &mut project.symbols[0].timeline.layers[0];
    layer.keyframes[0].elements = vec![object("left", 0.0, 10.0), object("right", 40.0, 20.0)];
    project.symbols[0].timeline.frame_count = 11;
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    let before = editor.project.clone();

    editor.create_tween(TweenKind::Motion);
    assert_eq!(editor.current_tween(), Some(TweenKind::Motion));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(
        editor.project, before,
        "Motion tween creation is one Undo step"
    );
    editor.redo();

    editor.frame = 10;
    editor.make_key(false);
    let keyed = editor.symbol_mut().timeline.layers[0]
        .keyframes
        .iter_mut()
        .find(|key| key.frame == 10)
        .expect("F6 inside a Motion span creates a property key");
    assert_eq!(keyed.keyed_properties, Some(TweenProperty::ALL.to_vec()));
    keyed
        .elements
        .iter_mut()
        .find(|element| element.id == "left")
        .unwrap()
        .transform
        .x = 100.0;
    keyed
        .elements
        .iter_mut()
        .find(|element| element.id == "right")
        .unwrap()
        .transform
        .y = 80.0;

    let middle = sample_layer(&editor.symbol().timeline.layers[0], 5);
    let left = middle.iter().find(|element| element.id == "left").unwrap();
    let right = middle.iter().find(|element| element.id == "right").unwrap();
    assert_eq!((left.transform.x, left.transform.y), (50.0, 10.0));
    assert_eq!((right.transform.x, right.transform.y), (40.0, 50.0));
    editor.project.validate().unwrap();
}

#[test]
fn switching_layer_row_discards_the_old_frame_range_before_editing() {
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 8;
    project.symbols[0].timeline.layers.push(Layer {
        id: "upper-layer".into(),
        name: "Upper".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![Keyframe::default()],
    });
    let lower_id = project.symbols[0].timeline.layers[0].id.clone();
    let upper_id = project.symbols[0].timeline.layers[1].id.clone();
    let mut editor = Editor::from_project(&egui::Context::default(), project);
    editor.frame = 5;
    editor.select_frame_range(FrameRange {
        layers: vec![lower_id],
        start: 2,
        end: 3,
    });

    editor.select_layer_row(1, false, false);
    assert_eq!(editor.layer, 1);
    assert_eq!(editor.selected_frame_range(), None);
    assert_eq!(editor.frame_range(), None);

    let before = editor.project.clone();
    editor.convert_to_keyframes(false);
    let timeline = &editor.symbol().timeline;
    let lower = timeline
        .layers
        .iter()
        .find(|layer| layer.id != upper_id)
        .unwrap();
    let upper = timeline
        .layers
        .iter()
        .find(|layer| layer.id == upper_id)
        .unwrap();
    assert_eq!(
        lower.keyframes.len(),
        1,
        "the former layer must remain untouched"
    );
    assert!(upper.keyframes.iter().any(|key| key.frame == 5));
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(
        editor.project, before,
        "the edit on the new layer is one Undo step"
    );
}

#[test]
fn dragging_one_of_several_objects_can_create_a_motion_tween() {
    let (mut editor, ctx, page) = rectangle_stage();
    editor.symbol_mut().timeline.frame_count = 20;
    editor.symbol_mut().timeline.layers[0].keyframes[0].elements =
        vec![object("left", 100.0, 100.0), object("right", 300.0, 100.0)];
    editor.workspace.timeline_prefs.tween_on_drag = TweenOnDrag::Motion;
    editor.history.clear();
    let before = editor.project.clone();

    editor.frame = 10;
    editor.set_tool(crate::app::Tool::Select);
    editor.select_single_id("left".into());
    stage_input_frame(&mut editor, &ctx, 0.1, vec![]);
    let start = page.min + egui::vec2(110.0, 105.0);
    let end = start + egui::vec2(100.0, 60.0);
    stage_input_frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
            egui::Event::PointerMoved(end),
            primary_button(end, false),
        ],
    );

    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(layer.keyframes[0].tween, Some(Easing::Linear));
    assert_eq!(layer.keyframes[0].tween_kind, TweenKind::Motion);
    let property_key = layer
        .keyframes
        .iter()
        .find(|key| key.frame == 10)
        .expect("drag frame becomes a Motion property key");
    assert_eq!(
        property_key.keyed_properties,
        Some(vec![TweenProperty::X, TweenProperty::Y])
    );
    let moved = property_key
        .elements
        .iter()
        .find(|element| element.id == "left")
        .unwrap();
    let stationary = property_key
        .elements
        .iter()
        .find(|element| element.id == "right")
        .unwrap();
    assert_eq!((moved.transform.x, moved.transform.y), (200.0, 160.0));
    assert_eq!(
        (stationary.transform.x, stationary.transform.y),
        (300.0, 100.0)
    );
    let middle = sample_layer(layer, 5);
    let moved = middle.iter().find(|element| element.id == "left").unwrap();
    let stationary = middle.iter().find(|element| element.id == "right").unwrap();
    assert_eq!((moved.transform.x, moved.transform.y), (150.0, 130.0));
    assert_eq!(
        (stationary.transform.x, stationary.transform.y),
        (300.0, 100.0)
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[test]
fn timeline_blend_mode_popup_is_compact_and_changes_one_layer_with_one_undo() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    assert!(editor.set_layer_blend_mode(0, BlendMode::Multiply));
    editor.clear_history();
    let before = editor.project.clone();
    let mut time = 0.1;
    timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    time += 0.1;
    let mut output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);

    output = click_timeline_control(
        &mut editor,
        &ctx,
        &mut time,
        &output,
        "Blend mode: Multiply",
    );
    let normal = accessible_bounds(&output, "Normal").expect("Normal menu row");
    let layer = accessible_bounds(&output, "Layer").expect("Layer menu row");
    let hard_light = accessible_bounds(&output, "Hard Light").expect("Hard Light menu row");
    for (label, row) in [
        ("Normal", normal),
        ("Layer", layer),
        ("Hard Light", hard_light),
    ] {
        assert!(
            (row.width() - 180.0).abs() < 0.75,
            "{label} must use the compact 180 pt popup width, got {row:?}"
        );
    }
    assert!(
        (layer.top() - normal.bottom()).abs() < 0.75,
        "NF2 menu rows in one group must not have extra spacing: {normal:?} / {layer:?}"
    );

    click_timeline_control(&mut editor, &ctx, &mut time, &output, "Hard Light");
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.blend_mode,
        BlendMode::Hardlight
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn onion_range_fill_is_painted_after_the_ruler_background() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::demo());
    editor.frame = 5;
    editor.onion_before = 3;
    editor.onion_after = 3;
    editor.onion = true;

    let output = timeline_panel_frame(&mut editor, &ctx, 0.1, vec![]);
    let palette = neoflash_ui::palette(&ctx);
    let mut band = None;
    let mut last_covering_background = None;
    let mut handles = Vec::new();
    for (index, clipped) in output.shapes.iter().enumerate() {
        let egui::epaint::Shape::Rect(rect) = &clipped.shape else {
            continue;
        };
        if rect.fill == palette.accent_subtle
            && (rect.rect.height() - 24.0).abs() < 0.2
            && (rect.rect.width() - 7.0 * editor.timeline_cell).abs() < 0.5
            && clipped.clip_rect.contains_rect(rect.rect)
        {
            band = Some((index, rect.rect));
        }
        if rect.fill == palette.timeline_onion
            && (rect.rect.width() - 3.0).abs() < 0.2
            && (rect.rect.height() - 24.0).abs() < 0.2
            && clipped.clip_rect.contains_rect(rect.rect)
        {
            handles.push(rect.rect);
        }
        if let Some((_, range)) = band
            && rect.fill == palette.bg_panel
            && clipped.clip_rect.intersects(range)
            && rect.rect.contains_rect(range)
        {
            last_covering_background = Some(index);
        }
    }

    let (band_index, band) = band.expect("NF2/TL Onion Range must paint its 24 pt Range fill");
    assert!(
        last_covering_background.is_none_or(|index| index < band_index),
        "the ruler background must not erase the onion Range fill"
    );
    handles.sort_by(|left, right| left.left().total_cmp(&right.left()));
    handles.dedup_by(|left, right| (left.left() - right.left()).abs() < 0.5);
    assert_eq!(handles.len(), 2, "the Range fill must retain both handles");
    assert!((band.left() - handles[0].left()).abs() < 0.2);
    assert!((band.right() - handles[1].right()).abs() < 0.2);
    assert!(
        (band.width() - 7.0 * editor.timeline_cell).abs() < 0.5,
        "frames 2..=8 must fill exactly seven frame cells"
    );
}

#[test]
fn timeline_marks_unexposed_grid_cells_and_extends_frames_on_f5_f6_f7_and_stage_draw() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 1;
    let mut editor = Editor::from_project(&ctx, project);
    editor.clear_history();
    let before = editor.project.clone();
    assert_eq!(editor.symbol().timeline.frame_count, 1);

    let output = timeline_panel_frame(&mut editor, &ctx, 0.1, vec![]);
    let palette = neoflash_ui::palette(&ctx);
    let mut ruler_numbers = Vec::new();
    let mut fifth_cells = 0usize;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::epaint::Shape::Text(text) => {
                ruler_numbers.push(text.galley.job.text.clone());
            }
            egui::epaint::Shape::Rect(rect)
                if rect.fill == palette.timeline_empty_5th
                    && (rect.rect.width() - editor.timeline_cell).abs() < 0.5 =>
            {
                fifth_cells += 1;
            }
            _ => {}
        }
    }
    for expected in ["1", "5", "10", "15", "20"] {
        assert!(
            ruler_numbers.iter().any(|text| text == expected),
            "ruler must render frame label {expected} even on a 1-frame timeline, got {ruler_numbers:?}"
        );
    }
    assert!(
        fifth_cells >= 10,
        "layer row must render shaded 5th empty frame cells across the timeline width, got {fifth_cells}"
    );

    // Selecting an unexposed frame cell (frame 10, index 9) keeps the playhead there across UI frames
    // and extends exposure to frame 10 on F5 (insert_frame).
    let layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.frame = 9;
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id.clone()],
        start: 9,
        end: 9,
    });
    editor.normalize_selection();
    assert_eq!(editor.frame, 9);
    editor.insert_frame();
    assert_eq!(editor.symbol().timeline.frame_count, 10);
    assert_eq!(editor.frame, 9);
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 1);
    editor.undo();
    assert_eq!(editor.project, before);

    // F7 (Insert blank keyframe) on unexposed frame 15 (index 14) extends the timeline and creates keyframe 14.
    editor.frame = 14;
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id.clone()],
        start: 14,
        end: 14,
    });
    editor.make_key(true);
    assert_eq!(editor.symbol().timeline.frame_count, 15);
    assert!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .any(|keyframe| keyframe.frame == 14 && keyframe.elements.is_empty())
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);

    // Drawing on the Stage while an unexposed frame cell (frame 8, index 7) is selected extends the timeline
    // and places the new element on a keyframe at that frame.
    editor.frame = 7;
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id],
        start: 7,
        end: 7,
    });
    editor.normalize_selection();
    assert_eq!(editor.frame, 7);
    assert!(editor.current_authored_scene(false).draws.is_empty());
    editor.add_element(object("drawn-at-8", 60.0, 40.0).kind, 60.0, 40.0);
    assert_eq!(editor.symbol().timeline.frame_count, 8);
    assert!(sample_layer(&editor.symbol().timeline.layers[0], 0).is_empty());
    assert_eq!(
        sample_layer(&editor.symbol().timeline.layers[0], 7).len(),
        1
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);
}
