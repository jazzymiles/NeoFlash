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

fn secondary_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
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
fn blank_layers_area_opens_the_existing_active_layer_menu() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.clear_history();
    let baseline = editor.project.clone();
    let mut time = 0.1;

    timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    time += 0.1;
    let output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    let row = accessible_bounds(&output, "Layer 1 (Layer)")
        .expect("the active Timeline layer row must be accessible");
    let blank = egui::pos2(row.center().x, row.bottom() + row.height() * 2.0);

    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(blank),
            secondary_button(blank, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![secondary_button(blank, false)],
    );
    assert!(
        editor.are_layers_deselected() && !editor.is_layer_row_selected(0),
        "right-clicking empty space below layers must clear layer selection"
    );
    time += 0.1;
    let menu = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    assert!(
        accessible_bounds(&menu, "Add new layer").is_some(),
        "blank Layers space must open the shortened contextless layer menu"
    );
    assert!(
        accessible_bounds(&menu, "Insert layer").is_none(),
        "blank Layers space must not open the layer-specific menu"
    );

    let output = click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Add new layer");
    assert!(
        accessible_bounds(&output, "Add new layer").is_none(),
        "choosing a layer command closes the context menu"
    );
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    assert!(!editor.are_layers_deselected());
    assert!(editor.is_layer_row_selected(1));
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);
    editor.redo();
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    editor.project.validate().unwrap();
}

#[test]
fn selected_frame_context_remove_frames_keeps_the_range_and_is_undoable() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 8;
    project.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![object("hero", 20.0, 30.0)],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 7,
            ..Keyframe::default()
        },
    ];
    let layer_id = project.symbols[0].timeline.layers[0].id.clone();
    let mut editor = Editor::from_project(&ctx, project);
    editor.clear_history();
    let baseline = editor.project.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id],
        start: 2,
        end: 3,
    });
    let mut time = 0.1;

    timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    time += 0.1;
    let output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    let first_key = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Circle(circle)
                if (1.5..=3.5).contains(&circle.radius)
                    && circle.fill == neoflash_ui::palette(&ctx).timeline_marker
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
        .expect("the Timeline must paint its first keyframe marker");
    let selected_cell = first_key + egui::vec2(editor.timeline_cell * 2.0, 0.0);

    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(selected_cell),
            secondary_button(selected_cell, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![secondary_button(selected_cell, false)],
    );
    assert_eq!(
        editor.selected_frame_range(),
        Some((2, 3, 1)),
        "right-click inside the range must not collapse it to one frame"
    );
    time += 0.1;
    let menu = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    let remove_label = format!(
        "Remove frames ({})",
        editor.sc(crate::commands::CommandId::EditTimelineRemoveFrames)
    );
    assert!(
        accessible_bounds(&menu, &remove_label).is_some(),
        "selected-frame context must expose the existing Remove frames command"
    );

    click_timeline_control(&mut editor, &ctx, &mut time, &menu, &remove_label);
    assert_eq!(editor.symbol().timeline.frame_count, 6);
    assert_eq!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .map(|key| key.frame)
            .collect::<Vec<_>>(),
        vec![0, 5]
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    let removed = editor.project.clone();
    editor.undo();
    assert_eq!(editor.project, baseline);
    editor.redo();
    assert_eq!(editor.project, removed);
    editor.project.validate().unwrap();
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
            (row.width() - neoflash_ui::metrics::menu_width()).abs() < 0.75,
            "{label} must use the `width/menu` popup width, got {row:?}"
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
    // `paint_onion_range`: the band leaves 6 pt above and 2 pt below inside the frame ruler.
    let band_height = neoflash_ui::metrics::timeline_ruler() - 8.0;
    let mut band = None;
    let mut last_covering_background = None;
    let mut handles = Vec::new();
    for (index, clipped) in output.shapes.iter().enumerate() {
        let egui::epaint::Shape::Rect(rect) = &clipped.shape else {
            continue;
        };
        if rect.fill == palette.accent_subtle
            && (rect.rect.height() - band_height).abs() < 0.2
            && (rect.rect.width() - 7.0 * editor.timeline_cell).abs() < 0.5
            && clipped.clip_rect.contains_rect(rect.rect)
        {
            band = Some((index, rect.rect));
        }
        if rect.fill == palette.timeline_onion
            && (rect.rect.width() - 3.0).abs() < 0.2
            && (rect.rect.height() - band_height).abs() < 0.2
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

    let (band_index, band) = band.expect("NF2/TL Onion Range must paint its Range fill");
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

#[test]
fn selected_keyframe_drags_multi_clear_keyframe_and_remove_frames_preserve_spans() {
    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 19;
    project.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![object("hero", 40.0, 50.0)],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 18,
            elements: vec![],
            ..Keyframe::default()
        },
    ];
    let layer_id = project.symbols[0].timeline.layers[0].id.clone();
    let mut editor = Editor::from_project(&ctx, project);
    editor.clear_history();
    let baseline = editor.project.clone();

    timeline_panel_frame(&mut editor, &ctx, 0.1, vec![]);
    let output = timeline_panel_frame(&mut editor, &ctx, 0.2, vec![]);
    let key0_pos = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Circle(circle)
                if (1.5..=3.5).contains(&circle.radius)
                    && circle.fill == neoflash_ui::palette(&ctx).timeline_marker
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
        .expect("timeline must paint the first keyframe marker");
    let cell = editor.timeline_cell;
    let cell_center = |f: u32| key0_pos + egui::vec2(cell * f as f32, 0.0);

    // 1. Clicking the first keyframe (frame 0) selects that 1-frame cell, and dragging it to frame 5
    // (Frame 6) moves the keyframe cleanly leaving empty timeline cells 0..5 before it.
    let p0 = cell_center(0);
    let p5 = cell_center(5);
    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.3,
        vec![egui::Event::PointerMoved(p0), primary_button(p0, true)],
    );
    timeline_panel_frame(&mut editor, &ctx, 0.35, vec![primary_button(p0, false)]);
    assert_eq!(editor.selected_frame_range(), Some((0, 0, 1)));

    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.4,
        vec![egui::Event::PointerMoved(p0), primary_button(p0, true)],
    );
    timeline_panel_frame(&mut editor, &ctx, 0.45, vec![egui::Event::PointerMoved(p5)]);
    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.5,
        vec![egui::Event::PointerMoved(p5), primary_button(p5, false)],
    );
    let keys: Vec<u32> = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .map(|k| k.frame)
        .collect();
    assert_eq!(
        keys,
        vec![5, 18],
        "dragging first keyframe must move it without creating extra keyframes"
    );
    assert!(sample_layer(&editor.symbol().timeline.layers[0], 0).is_empty());
    assert_eq!(
        sample_layer(&editor.symbol().timeline.layers[0], 5).len(),
        1
    );
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 2. Clicking the trailing blank keyframe (frame 18) and dragging it right to frame 32 (Frame 33,
    // past the previous 19-frame duration) moves the blank keyframe and extends the preceding span.
    let p18 = cell_center(18);
    let p32 = cell_center(32);
    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.6,
        vec![egui::Event::PointerMoved(p18), primary_button(p18, true)],
    );
    timeline_panel_frame(&mut editor, &ctx, 0.65, vec![primary_button(p18, false)]);
    assert_eq!(editor.selected_frame_range(), Some((18, 18, 1)));

    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.7,
        vec![egui::Event::PointerMoved(p18), primary_button(p18, true)],
    );
    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.75,
        vec![egui::Event::PointerMoved(p32)],
    );
    timeline_panel_frame(
        &mut editor,
        &ctx,
        0.8,
        vec![egui::Event::PointerMoved(p32), primary_button(p32, false)],
    );
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(
        layer.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 32],
        "dragging trailing blank keyframe must move it and extend the preceding span"
    );
    assert_eq!(editor.symbol().timeline.frame_count, 33);
    assert_eq!(sample_layer(layer, 31).len(), 1);
    assert!(sample_layer(layer, 32).is_empty());
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 3. Remove frames (`TimelineEdit::Delete`) inside a static span shortens the span without
    // synthesizing a new keyframe at the cut boundary.
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id.clone()],
        start: 4,
        end: 7,
    });
    editor.run_frame_command(&ctx, crate::timeline_edit::FrameCommand::Remove);
    let layer = &editor.symbol().timeline.layers[0];
    assert_eq!(
        layer.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 14],
        "Remove frames inside a span must not insert a keyframe at the cut"
    );
    assert_eq!(editor.symbol().timeline.frame_count, 15);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 4. Clear keyframe with multiple keyframes selected clears all keyframes in the range in one Undo step.
    for f in [4, 8, 12] {
        editor.frame = f;
        editor.make_key(false);
    }
    editor.clear_history();
    let with_keys = editor.project.clone();
    assert_eq!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 4, 8, 12, 18]
    );
    editor.select_frame_range(FrameRange {
        layers: vec![layer_id],
        start: 4,
        end: 18,
    });
    editor.clear_keyframe();
    assert_eq!(
        editor.symbol().timeline.layers[0]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0],
        "Clear keyframe must remove every selected keyframe in the range"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, with_keys);
}

fn count_selected_layer_row_fills(
    output: &egui::FullOutput,
    ctx: &egui::Context,
    max_x: f32,
) -> usize {
    let selected_fill = neoflash_ui::palette(ctx).timeline_row_selected;
    output
        .shapes
        .iter()
        .filter(|clipped| {
            let egui::epaint::Shape::Rect(rect) = &clipped.shape else {
                return false;
            };
            rect.fill == selected_fill
                && rect.rect.left() < max_x
                && clipped.clip_rect.intersects(rect.rect)
        })
        .count()
}

#[test]
fn t385_blank_layers_left_and_right_click_deselect_all_rows_and_reselect_on_row_or_frame_click() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 8;
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![object("a", 10.0, 10.0)];
    project.symbols[0].timeline.layers.push(Layer {
        id: "layer-2".into(),
        name: "Layer 2".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![object("b", 40.0, 40.0)],
            ..Keyframe::default()
        }],
    });
    let layer1_id = project.symbols[0].timeline.layers[0].id.clone();
    let mut editor = Editor::from_project(&ctx, project);
    editor.select_layer_row(0, false, false);
    editor.select_layer_row(1, false, true);
    editor.select_frame_range(FrameRange {
        layers: vec![layer1_id],
        start: 1,
        end: 3,
    });
    assert_eq!(editor.selected_layer_indices(), vec![0, 1]);
    assert!(editor.is_layer_row_selected(0));
    assert!(editor.is_layer_row_selected(1));
    assert!(editor.timeline_edit.focused);

    let mut time = 0.1;
    timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    time += 0.1;
    let output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    let row_top = accessible_bounds(&output, "Layer 2 (Layer)").expect("Layer 2 row");
    let row_bottom = accessible_bounds(&output, "Layer 1 (Layer)").expect("Layer 1 row");
    assert_eq!(
        count_selected_layer_row_fills(&output, &ctx, row_bottom.right() + 4.0),
        2,
        "both selected rows must paint blue selection fills initially"
    );

    // 1. Left-click in blank space below layers deselects all layers and clears frame selection.
    let blank = egui::pos2(
        row_bottom.center().x,
        row_bottom.bottom() + row_bottom.height() * 2.0,
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(blank),
            primary_button(blank, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(&mut editor, &ctx, time, vec![primary_button(blank, false)]);
    time += 0.1;
    let deselected_output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    assert!(editor.are_layers_deselected());
    assert!(!editor.is_layer_row_selected(0));
    assert!(!editor.is_layer_row_selected(1));
    assert!(editor.selected_layer_indices().is_empty());
    assert_eq!(editor.selected_frame_range(), None);
    assert!(!editor.timeline_edit.focused);
    assert_eq!(
        count_selected_layer_row_fills(&deselected_output, &ctx, row_bottom.right() + 4.0),
        0,
        "zero LayerRows may stay highlighted blue after clicking empty space below layers"
    );

    // 2. Clicking a layer row re-selects ONLY that layer row.
    let output_after_row = click_timeline_control(
        &mut editor,
        &ctx,
        &mut time,
        &deselected_output,
        "Layer 2 (Layer)",
    );
    assert!(!editor.are_layers_deselected());
    assert!(!editor.is_layer_row_selected(0));
    assert!(editor.is_layer_row_selected(1));
    assert_eq!(editor.selected_layer_indices(), vec![1]);
    assert_eq!(
        count_selected_layer_row_fills(&output_after_row, &ctx, row_bottom.right() + 4.0),
        1
    );

    // 3. Right-clicking blank space below layers also deselects all layers and opens the menu.
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(blank),
            secondary_button(blank, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![secondary_button(blank, false)],
    );
    time += 0.1;
    let right_click_output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    assert!(editor.are_layers_deselected());
    assert!(!editor.is_layer_row_selected(0));
    assert!(!editor.is_layer_row_selected(1));
    assert!(editor.selected_layer_indices().is_empty());
    assert_eq!(
        count_selected_layer_row_fills(&right_click_output, &ctx, row_bottom.right() + 4.0),
        0
    );
    assert!(accessible_bounds(&right_click_output, "Add new layer").is_some());

    // Close menu by clicking empty Timeline header space, verify layers stay deselected, then click a frame cell in row_top!
    let dismiss_pt = egui::pos2(row_top.left() + 12.0, row_top.top() - 16.0);
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(dismiss_pt),
            primary_button(dismiss_pt, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![primary_button(dismiss_pt, false)],
    );
    time += 0.1;
    let closed_output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    assert!(accessible_bounds(&closed_output, "Add new layer").is_none());
    assert!(editor.are_layers_deselected());
    let marker_color = neoflash_ui::palette(&ctx).timeline_marker;
    let first_key_on_top_row = closed_output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Circle(circle)
                if (1.5..=3.5).contains(&circle.radius)
                    && circle.fill == marker_color
                    && circle.center.x > row_top.right()
                    && (circle.center.y - row_top.center().y).abs() < row_top.height() * 0.5
                    && shape.clip_rect.contains(circle.center) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .expect("top row keyframe marker in the frame grid");
    let frame_2_cell = first_key_on_top_row + egui::vec2(editor.timeline_cell * 2.0, 0.0);
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![
            egui::Event::PointerMoved(frame_2_cell),
            primary_button(frame_2_cell, true),
        ],
    );
    time += 0.1;
    timeline_panel_frame(
        &mut editor,
        &ctx,
        time,
        vec![primary_button(frame_2_cell, false)],
    );
    time += 0.1;
    let frame_selected_output = timeline_panel_frame(&mut editor, &ctx, time, vec![]);
    assert!(!editor.are_layers_deselected());
    assert!(editor.is_layer_row_selected(1));
    assert!(!editor.is_layer_row_selected(0));
    assert!(editor.timeline_edit.focused);
    assert_eq!(editor.selected_frame_range(), Some((2, 2, 1)));
    assert_eq!(
        count_selected_layer_row_fills(&frame_selected_output, &ctx, row_bottom.right() + 4.0),
        1
    );
}

#[test]
fn t385_blank_layers_context_menu_exact_10_items_separators_and_1_undo_commands() {
    use crate::layer_controls::visible_layer_indices;

    let open_blank_menu =
        |editor: &mut Editor, ctx: &egui::Context, time: &mut f64| -> egui::FullOutput {
            *time += 0.1;
            timeline_panel_frame(editor, ctx, *time, vec![]);
            *time += 0.1;
            let out = timeline_panel_frame(editor, ctx, *time, vec![]);
            let bottom_row = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let label = node.label()?;
                    (label.ends_with("(Layer)")
                        || label.ends_with("(Folder)")
                        || label.ends_with("(Guide)")
                        || label.ends_with("(Motion guide)"))
                    .then(|| node.bounds())
                    .flatten()
                    .map(|r| {
                        egui::Rect::from_min_max(
                            egui::pos2(r.x0 as f32, r.y0 as f32),
                            egui::pos2(r.x1 as f32, r.y1 as f32),
                        )
                    })
                })
                .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
                .expect("at least one Timeline layer row");
            let blank = egui::pos2(bottom_row.center().x, bottom_row.bottom() + 36.0);
            *time += 0.1;
            timeline_panel_frame(
                editor,
                ctx,
                *time,
                vec![
                    egui::Event::PointerMoved(blank),
                    secondary_button(blank, true),
                ],
            );
            *time += 0.1;
            timeline_panel_frame(editor, ctx, *time, vec![secondary_button(blank, false)]);
            *time += 0.1;
            timeline_panel_frame(editor, ctx, *time, vec![])
        };

    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.clear_history();
    let baseline = editor.project.clone();
    let mut time = 0.1;

    let menu_item_bounds = |output: &egui::FullOutput, label: &str| -> Option<egui::Rect> {
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
                    .filter(|r| r.width() >= 180.0)
            })
    };
    let click_menu_item = |editor: &mut Editor,
                           ctx: &egui::Context,
                           time: &mut f64,
                           output: &egui::FullOutput,
                           label: &str| {
        let point = menu_item_bounds(output, label)
            .unwrap_or_else(|| panic!("menu item {label:?} must be visible"))
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
    };

    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    let expected_items = [
        "Add new layer",
        "Add new folder",
        "Add motion guide",
        "Paste layers",
        "Show all",
        "Clear all solo",
        "Unlock all",
        "Show all as solid",
        "Expand all folders",
        "Collapse all folders",
    ];
    let rects: Vec<egui::Rect> = expected_items
        .iter()
        .map(|label| {
            menu_item_bounds(&menu, label)
                .unwrap_or_else(|| panic!("blank-layers menu must include {label:?}"))
        })
        .collect();
    for pair in rects.windows(2) {
        assert!(
            pair[1].top() >= pair[0].bottom() - 0.5,
            "menu items must appear in exact top-to-bottom order: {rects:?}"
        );
    }
    for rect in &rects {
        assert!(
            rect.width() <= 222.0,
            "blank-layers menu must use compact 220 pt width, got {rect:?}"
        );
    }
    // Verify the exact 3 separator positions: after index 2, after index 3, and after index 7.
    let gap = |i: usize| rects[i + 1].top() - rects[i].bottom();
    for sep_idx in [2usize, 3, 7] {
        assert!(
            gap(sep_idx) > 4.0,
            "expected separator after {:?}, got gap {}",
            expected_items[sep_idx],
            gap(sep_idx)
        );
    }
    for adj_idx in [0usize, 1, 4, 5, 6, 8] {
        assert!(
            gap(adj_idx).abs() < 1.0,
            "expected no separator after {:?}, got gap {}",
            expected_items[adj_idx],
            gap(adj_idx)
        );
    }
    for forbidden in [
        "Insert layer",
        "Delete layer",
        "Lock others",
        "Hide others",
        "Solo",
        "Properties…",
        "Rename layer",
        "Move to folder",
        "Convert layers to symbol…",
        "Link video & audio",
    ] {
        assert!(
            accessible_bounds(&menu, forbidden).is_none(),
            "contextless blank-layers menu must omit {forbidden:?}"
        );
    }

    // 1. Add new layer -> top of timeline, selected, 1 Undo.
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Add new layer");
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    let top_idx = visible_layer_indices(&editor.symbol().timeline)[0];
    assert_eq!(
        editor.symbol().timeline.layers[top_idx].properties.role,
        LayerRole::Normal
    );
    assert!(editor.is_layer_row_selected(top_idx));
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 2. Add new folder -> top of timeline, selected, 1 Undo.
    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Add new folder");
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    let top_idx = visible_layer_indices(&editor.symbol().timeline)[0];
    assert_eq!(
        editor.symbol().timeline.layers[top_idx].properties.role,
        LayerRole::Folder
    );
    assert!(editor.is_layer_row_selected(top_idx));
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 3. Add motion guide -> top of timeline in contextless mode, selected, 1 Undo.
    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_menu_item(&mut editor, &ctx, &mut time, &menu, "Add motion guide");
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    let top_idx = visible_layer_indices(&editor.symbol().timeline)[0];
    assert_eq!(
        editor.symbol().timeline.layers[top_idx].properties.role,
        LayerRole::Guide
    );
    assert!(editor.is_layer_row_selected(top_idx));
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 4. Paste layers: disabled when LayerClipboard is empty, enabled after copying a layer -> pastes at top, selected, 1 Undo.
    let menu_no_clip = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu_no_clip, "Paste layers");
    assert_eq!(
        editor.symbol().timeline.layers.len(),
        1,
        "Paste layers must be disabled when no LayerClipboard exists"
    );
    // Dismiss the still-open menu if disabled click kept it open, copy layer 0, then paste from blank menu.
    editor.select_layer_row(0, false, false);
    editor.copy_timeline_layers(&ctx, 0);
    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Paste layers");
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    let top_idx = visible_layer_indices(&editor.symbol().timeline)[0];
    assert_eq!(
        editor.symbol().timeline.layers[top_idx].name,
        "Layer 1 copy"
    );
    assert!(editor.is_layer_row_selected(top_idx));
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, baseline);

    // 5..8. Show all, Clear all solo, Unlock all, Show all as solid.
    editor.symbol_mut().timeline.layers[0].visible = false;
    editor.symbol_mut().timeline.layers[0].locked = true;
    editor.symbol_mut().timeline.layers[0].properties.outline = true;
    editor.project.format_version = 4;
    editor.clear_history();
    let hidden_locked_outlined = editor.project.clone();

    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Show all");
    assert!(editor.symbol().timeline.layers[0].visible);
    assert!(editor.are_layers_deselected());
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, hidden_locked_outlined);

    editor.toggle_layer_solo(0, false);
    assert!(editor.has_any_solo_layers());
    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Clear all solo");
    assert!(!editor.has_any_solo_layers());
    assert_eq!(editor.history.undo_len(), 0);

    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Unlock all");
    assert!(!editor.symbol().timeline.layers[0].locked);
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, hidden_locked_outlined);

    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Show all as solid");
    assert!(!editor.symbol().timeline.layers[0].properties.outline);
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, hidden_locked_outlined);

    // 9..10. Expand all folders & Collapse all folders.
    editor.symbol_mut().timeline.layers[0].visible = true;
    editor.symbol_mut().timeline.layers[0].locked = false;
    editor.symbol_mut().timeline.layers[0].properties.outline = false;
    editor.symbol_mut().timeline.layers[0].properties.parent = Some("folder-top".into());
    editor.symbol_mut().timeline.layers.push(Layer {
        id: "folder-top".into(),
        name: "Folder 1".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: LayerRole::Folder,
            collapsed: true,
            ..LayerProperties::default()
        },
        keyframes: vec![],
    });
    editor.project.format_version = 4;
    editor.project.validate().unwrap();
    editor.clear_history();
    let collapsed_state = editor.project.clone();

    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Expand all folders");
    assert!(!editor.symbol().timeline.layers[1].properties.collapsed);
    assert!(editor.are_layers_deselected());
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    let expanded_state = editor.project.clone();

    editor.clear_history();
    editor.select_layer_row(0, false, false);
    let menu = open_blank_menu(&mut editor, &ctx, &mut time);
    click_timeline_control(&mut editor, &ctx, &mut time, &menu, "Collapse all folders");
    assert!(editor.symbol().timeline.layers[1].properties.collapsed);
    assert!(
        editor.are_layers_deselected(),
        "collapsing all folders from blank space must keep layers deselected"
    );
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, expanded_state);
    assert_ne!(expanded_state, collapsed_state);
}
