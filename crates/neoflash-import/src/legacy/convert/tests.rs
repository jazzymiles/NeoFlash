//! Synthetic semantic families; no third-party artwork or scripts are embedded.
use super::*;
use crate::legacy::geometry::*;
use crate::{ImportFormat, Statistics};

fn report() -> ImportReport {
    ImportReport {
        source: "synthetic converter family".into(),
        format: ImportFormat::LegacyBinaryFla,
        version: None,
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
    }
}

fn record(
    id: u32,
    class: TimelineClass,
    payload: TimelinePayload,
    children: &[u32],
) -> TimelineObject {
    TimelineObject {
        object_id: id,
        class,
        schema: 1,
        introduced_by_pointer: 0,
        owner: None,
        children: children.to_vec(),
        encoded: TimelineByteSpan { start: 0, end: 0 },
        body_offset: 0,
        complete: true,
        base: None,
        payload: Some(payload),
        shape_index: None,
    }
}

fn timeline_fixture() -> BinaryTimelineStream {
    let mut objects = vec![
        record(
            1,
            TimelineClass::Page,
            TimelinePayload::Page(TimelinePage::default()),
            &[2, 3],
        ),
        record(
            2,
            TimelineClass::Layer,
            TimelinePayload::Layer(TimelineLayer::default()),
            &[10, 11, 12, 13, 14],
        ),
        record(
            3,
            TimelineClass::Layer,
            TimelinePayload::Layer(TimelineLayer::default()),
            &[15],
        ),
    ];
    for (id, duration, placement) in [
        (10, 4, Some(20)),
        (11, 2, Some(21)),
        (12, 2, None),
        (13, 4, Some(22)),
        (14, 2, Some(23)),
        (15, 3, Some(24)),
    ] {
        let mut fields = TimelineFrame {
            duration: Some(duration),
            key_mode: Some(0),
            ..Default::default()
        };
        if id == 10 {
            fields.key_mode = Some(1);
            fields.acceleration = Some((-100i16) as u16);
        }
        if id == 13 {
            fields.key_mode = Some(1);
            fields.custom_ease_present = Some(1);
            fields.single_ease_curve = Some(1);
            fields.custom_ease =
                vec![vec![[0., 0., 0., 0., 0.2, 0.], [0.8, 1., 1., 1., 1., 1.]]; 6];
        }
        objects.push(record(
            id,
            TimelineClass::Frame,
            TimelinePayload::Frame(Box::new(fields)),
            &placement.into_iter().collect::<Vec<_>>(),
        ));
    }
    for (id, x) in [(20, 0), (21, 80), (22, 20), (23, 100), (24, 50)] {
        objects.push(record(
            id,
            TimelineClass::Sprite,
            TimelinePayload::Instance(Box::new(TimelineInstance {
                matrix_words: Some([65536, 0, 0, 65536, x * 20, 0]),
                library_id: Some(100),
                ..Default::default()
            })),
            &[],
        ));
    }
    BinaryTimelineStream {
        index: 5,
        stream_kind: Some(1),
        root: Some(1),
        references: vec![],
        pointers: vec![],
        objects,
        shapes: vec![],
        original_bytes: vec![],
    }
}

fn convert_timeline(stream: &BinaryTimelineStream) -> Result<Timeline, String> {
    convert_timeline_reporting(stream).map(|(timeline, _)| timeline)
}

fn convert_timeline_reporting(
    stream: &BinaryTimelineStream,
) -> Result<(Timeline, ImportReport), String> {
    let mut report = report();
    let mut context = Context {
        report: &mut report,
        library: BTreeMap::from([(100, "graphic".into())]),
        bitmaps: BTreeMap::new(),
        sounds: BTreeMap::new(),
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let timeline = context.timeline(stream)?;
    Ok((timeline, report))
}

#[test]
fn synthetic_timeline_preserves_tween_identity_and_ends_lifetimes_at_blanks() {
    let source = timeline_fixture();
    let timeline = convert_timeline(&source).unwrap();
    assert_eq!(timeline.frame_count, 14);
    let layer = &timeline.layers[0];
    let first = &layer.keyframes[0].elements[0];
    let next = &layer.keyframes[1].elements[0];
    let resumed = &layer.keyframes[3].elements[0];
    assert_eq!(
        first.id, next.id,
        "different wire objects form one continuous placement"
    );
    assert_ne!(first.id, resumed.id, "a blank ends the prior lifetime");
    assert_eq!(resumed.id, layer.keyframes[4].elements[0].id);
    assert_eq!(
        layer.keyframes[0].tween,
        Some(Easing::Flash {
            acceleration: -100.
        })
    );
    assert_eq!(sample_layer(layer, 2)[0].transform.x, 20.);
    assert!(sample_layer(layer, 6).is_empty());
    assert!(sample_layer(layer, 7).is_empty());
    assert_eq!(sample_layer(layer, 8)[0].transform.x, 20.);
    assert_eq!(
        layer.keyframes[3].tween,
        Some(Easing::CubicBezier {
            x1: 0.2,
            y1: 0.,
            x2: 0.8,
            y2: 1.
        })
    );
    let curved = sample_layer(layer, 9)[0].transform.x;
    assert!(
        curved > 20. && curved < 40.,
        "custom curve must differ from linear quarter-point"
    );
    let short = &timeline.layers[1];
    assert_eq!(short.keyframes.len(), 2);
    assert_eq!(short.keyframes[1].frame, 3);
    assert!(short.keyframes[1].elements.is_empty());
    assert_eq!(sample_layer(short, 2).len(), 1);
    assert!(sample_layer(short, 3).is_empty());
    assert!(sample_layer(short, 13).is_empty());

    // A tween whose endpoint is a different placement cannot interpolate; the
    // span holds its poses and the report says so. A final tween without an
    // endpoint holds too, as in Flash.
    let mut mismatch = source.clone();
    if let Some(TimelinePayload::Instance(instance)) = &mut mismatch
        .objects
        .iter_mut()
        .find(|object| object.object_id == 21)
        .unwrap()
        .payload
    {
        instance.instance_name = Some(TimelineString {
            encoded: TimelineByteSpan { start: 0, end: 1 },
            data: TimelineByteSpan { start: 0, end: 1 },
            utf16: false,
        });
    }
    mismatch.original_bytes.push(b'x');
    let (held, report) = convert_timeline_reporting(&mismatch).unwrap();
    assert_eq!(held.layers[0].keyframes[0].tween, None);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_tween_dropped")
    );
    let mut unfinished = source;
    if let Some(TimelinePayload::Frame(frame)) = &mut unfinished
        .objects
        .iter_mut()
        .find(|object| object.object_id == 14)
        .unwrap()
        .payload
    {
        frame.key_mode = Some(1);
    }
    let held = convert_timeline(&unfinished).unwrap();
    assert!(held.layers[0].keyframes.last().unwrap().tween.is_none());
}

fn edge(
    start: [i32; 2],
    end: [i32; 2],
    fill_0: u16,
    fill_1: u16,
    stroke: u16,
    control: Option<[i32; 2]>,
) -> BinaryEdge {
    BinaryEdge {
        encoding_flags: 0,
        styles: BinaryContourStyles {
            stroke,
            fill_0,
            fill_1,
        },
        start_delta: [0; 2],
        control_delta: [0; 2],
        end_delta: [0; 2],
        start: start.map(|value| value * 5120),
        end: end.map(|value| value * 5120),
        control: control.map(|point| point.map(|value| value * 5120)),
        line_flags: None,
    }
}

#[test]
fn synthetic_shape_keeps_touching_contours_holes_reversed_fills_and_stroke_segments() {
    let mut edges = vec![];
    for (points, reverse) in [
        ([[0, 0], [20, 0], [20, 20], [0, 20]], false),
        ([[5, 5], [15, 5], [15, 15], [5, 15]], true),
        ([[20, 20], [30, 20], [30, 30], [20, 30]], false),
    ] {
        for i in 0..4 {
            edges.push(edge(
                points[i],
                points[(i + 1) % 4],
                if reverse { 0x8001 } else { 0 },
                if reverse { 0 } else { 0x8001 },
                0,
                None,
            ));
        }
    }
    edges.push(edge([0, 0], [20, 20], 1, 1, 0, None)); // interior edge is not a fill boundary
    edges.extend([
        edge([40, 0], [45, 5], 0, 0, 0x8001, Some([42, 10])),
        edge([50, 0], [45, 5], 0, 0, 1, None),
        edge([50, 0], [55, 0], 0, 0, 1, None),
    ]);
    let shape = BinaryShapeRecord {
        shape_revision: 6,
        data_revision: 5,
        matrix_words: [65536, 0, 0, 65536, 40, 60],
        metadata: 0,
        fills: vec![BinaryFill {
            packed_value: 0xff332211,
            encoding: BinaryFillEncoding::Extended {
                flags: 0,
                secondary_flags: 0,
                payload: BinaryFillPayload::Plain,
            },
        }],
        strokes: vec![BinaryStroke {
            color_word: 0xff665544,
            width_word: 40,
            style_words: [0, 0],
            extension: None,
        }],
        edges,
        auxiliary: vec![],
        original_bytes: vec![],
    };
    let converted = shape::convert_shape(&shape, "shape", &mut report(), "synthetic").unwrap();
    assert_eq!(converted.len(), 2);
    assert_eq!(
        (converted[0].transform.x, converted[0].transform.y),
        (2., 3.)
    );
    let ElementKind::PaintedPath {
        commands,
        fill_rule,
        fill,
        ..
    } = &converted[0].kind
    else {
        panic!()
    };
    assert_eq!(*fill, [0x11, 0x22, 0x33, 255]);
    for (point, expected) in [
        ([2., 2.], true),
        ([10., 10.], false),
        ([25., 25.], true),
        ([25., 10.], false),
    ] {
        assert_eq!(
            hit_test_path(
                commands,
                *fill_rule,
                point,
                true,
                0.,
                FlattenOptions::default()
            )
            .unwrap(),
            expected
        );
    }
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, PathCommand::LineTo { .. }))
            .count(),
        12
    );
    let ElementKind::PaintedPath {
        commands,
        stroke,
        stroke_width,
        ..
    } = &converted[1].kind
    else {
        panic!()
    };
    assert_eq!(*stroke, [0x44, 0x55, 0x66, 255]);
    assert_eq!(*stroke_width, 2.);
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(
                command,
                PathCommand::LineTo { .. } | PathCommand::QuadraticTo { .. }
            ))
            .count(),
        3
    );
    assert!(commands.contains(&PathCommand::QuadraticTo {
        cx: 42.,
        cy: 10.,
        x: 45.,
        y: 5.
    }));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, PathCommand::Close))
    );
    for point in [[47.5, 2.5], [52.5, 0.], [42.25, 6.25]] {
        assert!(
            hit_test_path(
                commands,
                FillRule::NonZero,
                point,
                false,
                *stroke_width,
                FlattenOptions::default()
            )
            .unwrap()
        );
    }
    let mut unclosed = shape;
    unclosed.edges.remove(0);
    let mut diagnostics = report();
    let partial = shape::convert_shape(&unclosed, "broken", &mut diagnostics, "synthetic").unwrap();
    assert!(partial.iter().all(|e| !e.name.starts_with("Fill")));
    assert!(
        diagnostics.diagnostics.iter().any(
            |d| d.code == "fla_shape_approximated" && d.message.contains("unmatched endpoints")
        )
    );
}

#[test]
fn translated_legacy_actions_become_as1_scripts_and_flag_untranslated_parts() {
    let stream = timeline_fixture();
    let mut report = report();
    let mut context = Context {
        report: &mut report,
        library: BTreeMap::new(),
        bitmaps: BTreeMap::new(),
        sounds: BTreeMap::new(),
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let mut actions = TimelineActions {
        encoded: TimelineByteSpan { start: 7, end: 9 },
        complete: true,
        presence: TimelineActionPresence::Compiled,
        revision: Some(0),
        mode: Some(0),
        identifier: None,
        breakpoint_count: None,
        breakpoints: None,
        collapse_word_count: None,
        collapse_words: None,
        source: None,
        compiled_revision: Some(0),
        command_count: Some(1),
        decoded: Some(TimelineDecodedActions {
            form: TimelineActionForm::Compiled,
            list_revision: Some(0),
            commands: vec![],
            source: "stop();\n".into(),
            untranslated: vec![],
            needs_review: false,
        }),
    };
    let script = context.actions(&stream, Some(&actions)).unwrap().unwrap();
    assert_eq!(script.language, ScriptLanguage::ActionScript1);
    assert_eq!(script.source, "stop();\n");
    assert!(context.report.diagnostics.is_empty());
    let decoded = actions.decoded.as_mut().unwrap();
    decoded.needs_review = true;
    decoded.untranslated = vec![0];
    context.actions(&stream, Some(&actions)).unwrap();
    assert_eq!(context.report.diagnostics[0].code, "fla_actions_translated");
    actions.complete = false;
    assert!(context.actions(&stream, Some(&actions)).is_err());
}

#[test]
fn frame_sound_maps_sync_loops_presets_and_warns_on_custom_envelopes_or_missing_media() {
    use crate::legacy::contents::ContentsByteSpan;
    use neoflash_core::{SoundEffect, SoundSync};

    let mut stream = timeline_fixture();
    let mut rep = report();
    let mut sounds = BTreeMap::new();
    sounds.insert(
        7_u16,
        assets::SoundAsset {
            resource_id: "legacy-sound-7".into(),
            duration_44: 44_100,
        },
    );
    let mut context = Context {
        report: &mut rep,
        library: BTreeMap::new(),
        bitmaps: BTreeMap::new(),
        sounds,
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };

    let mut frame = TimelineFrame {
        sound_media_id: Some(7),
        sound_sync: Some(1), // 1 = Start
        sound_loop_count: Some(0x7fff),
        sound_effect: Some(5), // 5 = FadeIn
        sound_in_point: Some(0),
        sound_out_point: Some(44_100),
        ..TimelineFrame::default()
    };
    let mapped = context
        .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
        .unwrap();
    assert_eq!(mapped.resource, "legacy-sound-7");
    assert_eq!(mapped.sync, SoundSync::Start);
    assert!(mapped.loop_forever);
    assert_eq!(mapped.loops, 1);
    assert_eq!(mapped.effect, SoundEffect::FadeIn);
    assert!(context.report.diagnostics.is_empty());

    // Pre-rev-21 legacy 2-point envelope matching LeftChannel preset imports without warning:
    let mut env = Vec::new();
    env.extend_from_slice(&0_u32.to_le_bytes());
    env.extend_from_slice(&32768_u16.to_le_bytes());
    env.extend_from_slice(&0_u16.to_le_bytes());
    env.extend_from_slice(&44_100_u32.to_le_bytes());
    env.extend_from_slice(&32768_u16.to_le_bytes());
    env.extend_from_slice(&0_u16.to_le_bytes());
    let env_start = stream.original_bytes.len();
    stream.original_bytes.extend_from_slice(&env);
    let env_span = ContentsByteSpan {
        start: env_start,
        end: stream.original_bytes.len(),
    };
    frame.sound_effect = None;
    frame.sound_loop_count = Some(3);
    frame.sound_sync = Some(3); // 3 = Stream
    frame.sound_envelope_count = Some(2);
    frame.sound_envelope = Some(env_span);
    let legacy_preset = context
        .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
        .unwrap();
    assert_eq!(legacy_preset.sync, SoundSync::Stream);
    assert!(!legacy_preset.loop_forever);
    assert_eq!(legacy_preset.loops, 3);
    assert_eq!(legacy_preset.effect, SoundEffect::LeftChannel);
    assert!(context.report.diagnostics.is_empty());

    // Non-preset custom envelope + trim points keep sound and emit fla_frame_sound warning:
    frame.sound_effect = Some(7); // 7 = Custom
    let mut custom_env = Vec::new();
    for (p, l, r) in [
        (0_u32, 16384_u16, 16384_u16),
        (20_000, 32768, 8192),
        (44_100, 0, 0),
    ] {
        custom_env.extend_from_slice(&p.to_le_bytes());
        custom_env.extend_from_slice(&l.to_le_bytes());
        custom_env.extend_from_slice(&r.to_le_bytes());
    }
    let c_start = stream.original_bytes.len();
    stream.original_bytes.extend_from_slice(&custom_env);
    frame.sound_envelope_count = Some(3);
    frame.sound_envelope = Some(ContentsByteSpan {
        start: c_start,
        end: stream.original_bytes.len(),
    });
    frame.sound_in_point = Some(1_000);
    let custom = context
        .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
        .unwrap();
    assert_eq!(custom.effect, SoundEffect::None);
    assert_eq!(context.report.diagnostics.len(), 2);
    assert!(
        context
            .report
            .diagnostics
            .iter()
            .all(|d| d.code == "fla_frame_sound")
    );

    // Missing/unimported sound media ID emits fla_frame_sound warning and returns None:
    frame.sound_media_id = Some(99);
    assert!(
        context
            .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
            .is_none()
    );
    assert_eq!(context.report.diagnostics.len(), 3);
}

#[test]
fn budget_helpers_shed_bitmaps_compact_provenance_and_degrade_later_vector_shapes() {
    // 1x1 PNG resource for bitmap shedding:
    let mut png_bytes = Vec::new();
    {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    let bmp1 = ResourceUnit::from_bytes(
        "legacy-bitmap-1",
        "legacy/bitmaps/1/a.png",
        "image/png",
        &png_bytes,
    )
    .unwrap();
    let mut larger_png = png_bytes.clone();
    // Make a second valid PNG slightly larger (2x2) so largest-first picks legacy-bitmap-2 first:
    larger_png.clear();
    {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 50, 60, 255]));
        img.write_to(
            &mut std::io::Cursor::new(&mut larger_png),
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    let bmp2 = ResourceUnit::from_bytes(
        "legacy-bitmap-2",
        "legacy/bitmaps/2/b.png",
        "image/png",
        &larger_png,
    )
    .unwrap();

    let dense_commands: Vec<PathCommand> = std::iter::once(PathCommand::MoveTo { x: 0.0, y: 0.0 })
        .chain((1..=80).map(|i| PathCommand::QuadraticTo {
            cx: i as f64 * 1.25,
            cy: i as f64 * -0.75,
            x: i as f64 * 2.5,
            y: i as f64 * 1.5,
        }))
        .collect();
    let make_path = |id: &str| Element {
        id: id.into(),
        name: "Stroke 1".into(),
        transform: Transform::default(),
        kind: ElementKind::PaintedPath {
            commands: dense_commands.clone(),
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [255, 0, 0, 255],
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: 2.0,
        },
        scripts: vec![],
        accessibility: None,
    };

    let root_symbol = Symbol {
        id: "fla-symbol-1".into(),
        name: "Scene 1".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 2,
            guides: vec![],
            scripts: vec![],
            layers: vec![Layer {
                id: "fla-layer-1-1".into(),
                name: "Main".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![
                            Element {
                                id: "bmp-el-1".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Bitmap {
                                    resource_id: "legacy-bitmap-1".into(),
                                    width: 1,
                                    height: 1,
                                    smoothing: false,
                                },
                                scripts: vec![],
                                accessibility: None,
                            },
                            Element {
                                id: "bmp-el-2".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Bitmap {
                                    resource_id: "legacy-bitmap-2".into(),
                                    width: 2,
                                    height: 2,
                                    smoothing: true,
                                },
                                scripts: vec![],
                                accessibility: None,
                            },
                            make_path("root-path-f0"),
                        ],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 1,
                        elements: vec![
                            Element {
                                id: "grp-inst".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Instance {
                                    symbol_id: "fla-1-1-9-group".into(),
                                    playback: InstancePlayback::SingleFrame(0),
                                },
                                scripts: vec![],
                                accessibility: None,
                            },
                            make_path("root-path-f1"),
                        ],
                        ..Default::default()
                    },
                ],
            }],
        },
        class_name: None,
        linkage_id: None,
    };
    let group_sym = group_symbol(
        "fla-1-1-9-group",
        "Source group 9".into(),
        vec![make_path("grp-paint-stroke-0")],
    );

    let mut project = Project {
        format_version: 5,
        name: "budget".into(),
        width: 550,
        height: 400,
        fps: 24,
        background: [255; 4],
        root: "fla-symbol-1".into(),
        symbols: vec![root_symbol, group_sym],
        library_folders: vec![],
        sources: vec![],
        resources: vec![bmp1, bmp2],
        document_class: None,
        scenes: vec![],
    };
    let mut rep = report();

    // Shed largest bitmap first (excess = 1 byte):
    assert!(shed_bitmap_resources(&mut project, &mut rep, 1));
    assert!(project.resources.iter().all(|r| r.id != "legacy-bitmap-2"));
    assert!(project.resources.iter().any(|r| r.id == "legacy-bitmap-1"));
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_bitmap_skipped" && d.location == "media 2")
    );
    project.validate().unwrap();

    // Degrade later group vector shape first while keeping root frame 0 shape intact:
    assert!(degrade_excess_vector_shapes(&mut project, &mut rep, 1_000));
    assert!(
        project.symbol("fla-1-1-9-group").unwrap().timeline.layers[0].keyframes[0]
            .elements
            .is_empty()
    );
    assert!(
        project.symbol("fla-symbol-1").unwrap().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .any(|e| e.id == "root-path-f0" && is_vector_path(e))
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_shape_approximated")
    );
    project.validate().unwrap();

    // Degrade authored frame 1 vector shape to PLACEHOLDER instance:
    assert!(degrade_excess_vector_shapes(&mut project, &mut rep, 1_000));
    let f1_el =
        &project.symbol("fla-symbol-1").unwrap().timeline.layers[0].keyframes[1].elements[1];
    assert_eq!(f1_el.id, "root-path-f1");
    assert!(matches!(
        &f1_el.kind,
        ElementKind::Instance { symbol_id, .. } if symbol_id == PLACEHOLDER
    ));
    project.validate().unwrap();
}
