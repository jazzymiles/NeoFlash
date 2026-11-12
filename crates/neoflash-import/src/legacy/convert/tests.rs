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
        swc: None,
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
        contents_revision: None,
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
        contents_revision: None,
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
        contents_revision: None,
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

    // Non-preset custom envelope and in/out trim points convert directly without warnings:
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
    frame.sound_out_point = Some(22_050);
    let custom = context
        .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
        .unwrap();
    assert_eq!(custom.effect, SoundEffect::Custom);
    let env_pts = custom.envelope.as_ref().unwrap();
    assert_eq!(env_pts.len(), 3);
    assert_eq!(env_pts[0], [0.0, 0.5, 0.5]);
    assert!((env_pts[1][0] - (20_000.0 / 44_100.0)).abs() < 1e-12);
    assert_eq!((env_pts[1][1], env_pts[1][2]), (1.0, 0.25));
    assert_eq!(env_pts[2], [1.0, 0.0, 0.0]);
    assert!((custom.in_point.unwrap() - (1_000.0 / 44_100.0)).abs() < 1e-12);
    assert!((custom.out_point.unwrap() - 0.5).abs() < 1e-12);
    assert!(context.report.diagnostics.is_empty());

    // Missing/unimported sound media ID emits fla_frame_sound warning and returns None:
    frame.sound_media_id = Some(99);
    assert!(
        context
            .frame_sound(&stream, &frame, "Page 1#layer:Audio#frame:0")
            .is_none()
    );
    assert_eq!(context.report.diagnostics.len(), 1);
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
        ..Element::default()
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
                                ..Element::default()
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
                                ..Element::default()
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
                                ..Element::default()
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

#[test]
fn converts_legacy_filters_and_blend_modes_with_approximations_reported() {
    let mut rep = report();
    assert_eq!(
        convert_blend_mode(Some(3), "inst-1", &mut rep),
        BlendMode::Multiply
    );
    assert_eq!(
        convert_blend_mode(Some(14), "inst-1", &mut rep),
        BlendMode::Hardlight
    );
    assert!(rep.diagnostics.is_empty());

    // Subtract is preserved in the model and reports the Canvas 2D compositing approximation.
    assert_eq!(
        convert_blend_mode(Some(9), "inst-sub", &mut rep),
        BlendMode::Subtract
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_blend_mode" && d.location == "inst-sub")
    );

    let span = TimelineByteSpan { start: 0, end: 8 };
    let filters = vec![
        TimelineFilter {
            encoded: span,
            type_code: 0,
            kind: TimelineFilterKind::DropShadow,
            revision: 3,
            enabled: Some(true),
            params: TimelineFilterParams {
                color_rgba: Some(0x8033_2211),
                distance: Some(6.0),
                blur_x: Some(8.0),
                blur_y: Some(4.0),
                angle_radians: Some(std::f32::consts::FRAC_PI_4),
                strength_percent: Some(150),
                quality: Some(2),
                inner: Some(1),
                knockout: Some(0),
                hide_object: Some(0),
                ..TimelineFilterParams::default()
            },
        },
        TimelineFilter {
            encoded: span,
            type_code: 6,
            kind: TimelineFilterKind::AdjustColor,
            revision: 1,
            enabled: Some(true),
            params: TimelineFilterParams {
                brightness: Some(10.0),
                contrast: Some(-20.0),
                saturation: Some(-100.0),
                hue: Some(45.0),
                ..TimelineFilterParams::default()
            },
        },
        TimelineFilter {
            encoded: span,
            type_code: 1,
            kind: TimelineFilterKind::Blur,
            revision: 2,
            enabled: Some(false),
            params: TimelineFilterParams {
                blur_x: Some(12.0),
                blur_y: Some(12.0),
                quality: Some(1),
                ..TimelineFilterParams::default()
            },
        },
    ];
    let converted = convert_filters(&filters, "inst-filters", &mut rep);
    assert_eq!(converted.len(), 3);
    validate_filters(&converted).unwrap();
    assert!(matches!(
        &converted[0],
        FilterSpec::DropShadow {
            color: 0x112233,
            distance,
            blur_x,
            blur_y,
            strength,
            quality: 2,
            inner: true,
            ..
        } if (*distance - 6.0).abs() < 1e-5
            && (*blur_x - 8.0).abs() < 1e-5
            && (*blur_y - 4.0).abs() < 1e-5
            && (*strength - 1.5).abs() < 1e-5
    ));
    assert!(matches!(&converted[1], FilterSpec::ColorMatrix { .. }));
    assert!(matches!(
        &converted[2],
        FilterSpec::Blur {
            blur_x,
            blur_y,
            quality: 0x81,
        } if (*blur_x - 12.0).abs() < 1e-5 && (*blur_y - 12.0).abs() < 1e-5
    ));
    assert!(!converted[2].is_enabled());
    assert_eq!(converted[2].quality_level(), 1);
    assert!(
        !rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_filter_approximated" && d.location == "inst-filters"),
        "disabled filters are retained exactly and must not be reported as omitted"
    );
}

#[test]
fn converts_binary_morph_shape_hints_into_keyframe_shape_hints() {
    let make_rect_shape = |x0: i32, y0: i32, x1: i32, y1: i32| -> LocatedBinaryShape {
        let pts = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let edges = (0..4)
            .map(|i| edge(pts[i], pts[(i + 1) % 4], 0, 0x8001, 0, None))
            .collect();
        LocatedBinaryShape {
            offset: 0,
            record: BinaryShapeRecord {
                shape_revision: 6,
                data_revision: 5,
                matrix_words: [65536, 0, 0, 65536, 0, 0],
                metadata: 0,
                fills: vec![BinaryFill {
                    packed_value: 0xffff0000,
                    encoding: BinaryFillEncoding::Extended {
                        flags: 0,
                        secondary_flags: 0,
                        payload: BinaryFillPayload::Plain,
                    },
                }],
                strokes: vec![],
                edges,
                auxiliary: vec![],
                original_bytes: vec![],
            },
        }
    };

    let mut f0 = record(
        10,
        TimelineClass::Frame,
        TimelinePayload::Frame(Box::new(TimelineFrame {
            duration: Some(10),
            key_mode: Some(2),
            morph_shape: Some(30),
            ..Default::default()
        })),
        &[],
    );
    f0.shape_index = Some(0);

    let mut f1 = record(
        11,
        TimelineClass::Frame,
        TimelinePayload::Frame(Box::new(TimelineFrame {
            duration: Some(1),
            key_mode: Some(0),
            ..Default::default()
        })),
        &[],
    );
    f1.shape_index = Some(1);

    let morph = record(
        30,
        TimelineClass::Morph,
        TimelinePayload::Morph(Box::new(TimelineMorphShape {
            revision: Some(2),
            matrix_words_start: Some([65536, 0, 0, 65536, 0, 0]),
            matrix_words_end: Some([65536, 0, 0, 65536, 0, 0]),
            hints: vec![Some(31), None, Some(32)],
            ..Default::default()
        })),
        &[],
    );
    // Shape 0 is [0..100, 0..100] px (0..2000 twips); Shape 1 is [0..200, 0..100] px (0..4000, 0..2000 twips).
    // Hint 'a' (index 0): top-left (0, 0) twips -> top-right (4000, 0) twips => start_uv [0, 0], end_uv [1, 0].
    // Hint 'b' (index 1): top-right (2000, 0) twips -> bottom-right (4000, 2000) twips => start_uv [1, 0], end_uv [1, 1].
    let hint_a = record(
        31,
        TimelineClass::MorphHint,
        TimelinePayload::MorphHint(TimelineMorphHint {
            point_a: Some([0, 0]),
            point_b: Some([4000, 0]),
            index: Some(0),
        }),
        &[],
    );
    let hint_b = record(
        32,
        TimelineClass::MorphHint,
        TimelinePayload::MorphHint(TimelineMorphHint {
            point_a: Some([2000, 0]),
            point_b: Some([4000, 2000]),
            index: Some(1),
        }),
        &[],
    );

    let stream = BinaryTimelineStream {
        index: 1,
        stream_kind: Some(1),
        root: Some(1),
        references: vec![],
        pointers: vec![],
        objects: vec![
            record(
                1,
                TimelineClass::Page,
                TimelinePayload::Page(TimelinePage::default()),
                &[2],
            ),
            record(
                2,
                TimelineClass::Layer,
                TimelinePayload::Layer(TimelineLayer::default()),
                &[10, 11],
            ),
            f0,
            f1,
            morph,
            hint_a,
            hint_b,
        ],
        shapes: vec![
            make_rect_shape(0, 0, 100, 100),
            make_rect_shape(0, 0, 200, 100),
        ],
        original_bytes: vec![],
    };

    let (timeline, report) = convert_timeline_reporting(&stream).unwrap();
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "fla_morph_hints_approximated"
                && diagnostic.location == "stream #1 layer <unnamed> frame 0"
        }),
        "{:?}",
        report.diagnostics
    );
    let k0 = &timeline.layers[0].keyframes[0];
    assert_eq!(k0.tween_kind, TweenKind::Shape);
    assert_eq!(k0.shape_hints.len(), 2);
    assert_eq!(k0.shape_hints[0].label, 'a');
    assert!((k0.shape_hints[0].start_uv[0] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].start_uv[1] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].end_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[0].end_uv[1] - 0.0).abs() < 1e-4);

    assert_eq!(k0.shape_hints[1].label, 'b');
    assert!((k0.shape_hints[1].start_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].start_uv[1] - 0.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].end_uv[0] - 1.0).abs() < 1e-4);
    assert!((k0.shape_hints[1].end_uv[1] - 1.0).abs() < 1e-4);
}

#[test]
fn flash_mx_dynamic_input_and_mixed_utf16_runs_survive_conversion_save_reopen_and_fla_round_trip() {
    let mut original_bytes = Vec::new();
    let mut append_utf16 = |text: &str| {
        let start = original_bytes.len();
        original_bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let span = TimelineByteSpan {
            start,
            end: original_bytes.len(),
        };
        TimelineString {
            encoded: span,
            data: span,
            utf16: true,
        }
    };

    let font_arial = append_utf16("Arial");
    let font_courier = append_utf16("Courier New");
    let run_prefix = append_utf16("Score: 🎯 ");
    let run_number = append_utf16("100");
    let run_suffix = append_utf16(" pts");
    let var_score = append_utf16("_root.scoreText");
    let inst_score = append_utf16("score_txt");

    let run_pin = append_utf16("1234");
    let var_pin = append_utf16("userPin");
    let inst_pin = append_utf16("pin_txt");

    let run_status = append_utf16("Ready");
    let inst_status = append_utf16("status_txt");

    let style_blue_bold = TimelineTextStyle {
        revision: 10,
        font_family: Some(font_arial),
        font_size_twips: Some(360),   // 18pt
        color_rgba: Some(0xffcc6600), // R=0x00, G=0x66, B=0xcc, A=0xff
        bold: Some(1),
        italic: Some(0),
        alignment: Some(2), // Center
        line_spacing: Some(40),
        auto_kern: Some(0),
        ..TimelineTextStyle::default()
    };
    let style_red_italic = TimelineTextStyle {
        revision: 10,
        font_family: Some(font_courier),
        font_size_twips: Some(480),   // 24pt
        color_rgba: Some(0xff1122ee), // R=0xee, G=0x22, B=0x11, A=0xff
        bold: Some(0),
        italic: Some(1),
        alignment: Some(2), // Center
        line_spacing: Some(40),
        auto_kern: Some(1),
        ..TimelineTextStyle::default()
    };

    // 1. Dynamic text with variable binding, HTML, wordWrap, multiline, border/background,
    //    auto_expand -> auto_size="center", and 3 mixed UTF-16 style runs (including non-BMP '🎯').
    let dynamic_text = TimelineText {
        revision: Some(9),
        matrix_words: Some([65536, 0, 0, 65536, 400, 600]),
        bounds_words: Some([0, 4000, 0, 1200]),
        auto_expand: Some(1),
        flags: Some(0x0001 | 0x0002 | 0x0008 | 0x0010 | 0x0040 | 0x8000),
        max_characters: Some(0),
        variable_name: Some(var_score),
        instance_name: Some(inst_score),
        scrollable: Some(1),
        runs: vec![
            TimelineTextRun {
                characters: run_prefix,
                style: style_blue_bold.clone(),
            },
            TimelineTextRun {
                characters: run_number,
                style: style_red_italic,
            },
            TimelineTextRun {
                characters: run_suffix,
                style: style_blue_bold.clone(),
            },
        ],
        ..TimelineText::default()
    };

    // 2. Input password text field with max_characters=16, variable binding, and border/background.
    let input_text = TimelineText {
        revision: Some(9),
        matrix_words: Some([65536, 0, 0, 65536, 400, 2200]),
        bounds_words: Some([0, 3200, 0, 600]),
        auto_expand: Some(0),
        flags: Some(0x0001 | 0x0004 | 0x0040),
        max_characters: Some(16),
        variable_name: Some(var_pin),
        instance_name: Some(inst_pin),
        scrollable: Some(1),
        runs: vec![TimelineTextRun {
            characters: run_pin,
            style: style_blue_bold.clone(),
        }],
        ..TimelineText::default()
    };

    // 3. Dynamic non-selectable text with an unknown high archive flag bit (0x8000_0000).
    let status_text = TimelineText {
        revision: Some(9),
        matrix_words: Some([65536, 0, 0, 65536, 400, 3200]),
        bounds_words: Some([0, 2400, 0, 500]),
        auto_expand: Some(0),
        flags: Some(0x0001 | 0x0002 | 0x0080 | 0x8000_0000),
        max_characters: Some(0),
        instance_name: Some(inst_status),
        scrollable: Some(0),
        runs: vec![TimelineTextRun {
            characters: run_status,
            style: style_blue_bold,
        }],
        ..TimelineText::default()
    };

    let stream = BinaryTimelineStream {
        index: 1,
        stream_kind: Some(1),
        root: Some(1),
        references: vec![],
        pointers: vec![],
        objects: vec![
            record(
                1,
                TimelineClass::Page,
                TimelinePayload::Page(TimelinePage::default()),
                &[2],
            ),
            record(
                2,
                TimelineClass::Layer,
                TimelinePayload::Layer(TimelineLayer::default()),
                &[10],
            ),
            record(
                10,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(1),
                    key_mode: Some(0),
                    ..Default::default()
                })),
                &[20, 21, 22],
            ),
            record(
                20,
                TimelineClass::Text,
                TimelinePayload::Text(Box::new(dynamic_text)),
                &[],
            ),
            record(
                21,
                TimelineClass::Text,
                TimelinePayload::Text(Box::new(input_text)),
                &[],
            ),
            record(
                22,
                TimelineClass::Text,
                TimelinePayload::Text(Box::new(status_text)),
                &[],
            ),
        ],
        shapes: vec![],
        original_bytes,
    };

    let (timeline, report) = convert_timeline_reporting(&stream).unwrap();
    let elements = &timeline.layers[0].keyframes[0].elements;
    assert_eq!(elements.len(), 3);

    // Only `status_txt` (with the unknown 0x8000_0000 archive bit) should emit `fla_text_approximated`.
    let approx: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_text_approximated")
        .collect();
    assert_eq!(approx.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(approx[0].location, "status_txt");
    assert!(approx[0].message.contains("unrecognized text flag bits"));

    let check_elements = |els: &[Element]| {
        let score_state = TextFieldState::from_kind(&els[0].kind).expect("score_txt state");
        assert_eq!(els[0].name, "score_txt");
        assert_eq!(score_state.text, "Score: 🎯 100 pts");
        assert_eq!(score_state.field_type, TextFieldType::Dynamic);
        assert!(score_state.selectable);
        assert!(!score_state.display_as_password);
        assert!(score_state.multiline);
        assert!(score_state.word_wrap);
        assert!(score_state.html);
        assert!(score_state.as2_viewport.border);
        assert!(score_state.as2_viewport.background);
        assert_eq!(score_state.as2_viewport.auto_size, "center");
        assert_eq!(score_state.variable.as_deref(), Some("_root.scoreText"));
        assert_eq!(score_state.format_runs.len(), 3);
        // "Score: 🎯 " is 10 UTF-16 code units ('🎯' is a surrogate pair).
        assert_eq!(score_state.format_runs[0].start, 0);
        assert_eq!(score_state.format_runs[0].end, 10);
        assert_eq!(score_state.format_runs[0].format.font, "Arial");
        assert_eq!(score_state.format_runs[0].format.size, 18.0);
        assert!(score_state.format_runs[0].format.bold);
        assert_eq!(score_state.format_runs[0].format.color, 0x0066cc);

        assert_eq!(score_state.format_runs[1].start, 10);
        assert_eq!(score_state.format_runs[1].end, 13);
        assert_eq!(score_state.format_runs[1].format.font, "Courier New");
        assert_eq!(score_state.format_runs[1].format.size, 24.0);
        assert!(score_state.format_runs[1].format.italic);
        assert_eq!(score_state.format_runs[1].format.color, 0xee2211);

        assert_eq!(score_state.format_runs[2].start, 13);
        assert_eq!(score_state.format_runs[2].end, 17);
        assert_eq!(score_state.format_runs[2].format.font, "Arial");

        let pin_state = TextFieldState::from_kind(&els[1].kind).expect("pin_txt state");
        assert_eq!(els[1].name, "pin_txt");
        assert_eq!(pin_state.text, "1234");
        assert_eq!(pin_state.field_type, TextFieldType::Input);
        assert!(pin_state.selectable);
        assert!(pin_state.display_as_password);
        assert!(!pin_state.multiline);
        assert!(!pin_state.word_wrap);
        assert_eq!(pin_state.max_chars, 16);
        assert!(pin_state.as2_viewport.border);
        assert!(pin_state.as2_viewport.background);
        assert_eq!(pin_state.variable.as_deref(), Some("userPin"));
        assert!(pin_state.format_runs.is_empty());

        let status_state = TextFieldState::from_kind(&els[2].kind).expect("status_txt state");
        assert_eq!(els[2].name, "status_txt");
        assert_eq!(status_state.field_type, TextFieldType::Dynamic);
        assert!(!status_state.selectable);
        assert!(
            status_state.html,
            "historical 0x0080 flag retains HTML mode"
        );
    };

    check_elements(elements);

    // Verify native `.nflash` save/reopen preserves Dynamic/Input options, variable bindings, and UTF-16 runs.
    let mut project = Project::blank();
    project.name = "MX Text Test".into();
    project.symbols[0].timeline = timeline;
    project.upgrade_format_for_features();
    let saved_json = project.to_json().expect("serialize .nflash");
    let reopened = Project::from_json(&saved_json).expect("reopen .nflash");
    check_elements(&reopened.symbols[0].timeline.layers[0].keyframes[0].elements);

    // Verify `.fla` archive export and re-import round-trip.
    let tmp_dir = std::env::temp_dir().join(format!("neoflash-t179-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let fla_path = tmp_dir.join("mx_text_roundtrip.fla");
    crate::export_fla(&reopened, &fla_path).expect("export .fla");
    let fla_reimported = crate::import_project(&fla_path).expect("re-import .fla");
    fla_reimported
        .validate()
        .expect("valid re-imported .fla project");
    let re_els = &fla_reimported.symbols[0].timeline.layers[0].keyframes[0].elements;
    assert_eq!(re_els.len(), 3);
    assert_eq!(re_els[0].name, "score_txt");
    assert_eq!(
        TextFieldState::from_kind(&re_els[0].kind).unwrap().text,
        "Score: 🎯 100 pts"
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn flash8_single_run_fields_and_multibyte_metadata_keep_authored_semantics() {
    let mut original_bytes = Vec::new();
    let mut append_utf16 = |text: &str| {
        let start = original_bytes.len();
        original_bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let span = TimelineByteSpan {
            start,
            end: original_bytes.len(),
        };
        TimelineString {
            encoded: span,
            data: span,
            utf16: true,
        }
    };

    let font = append_utf16("Arial");
    let dynamic_chars = append_utf16("Dynamic");
    let input_chars = append_utf16("secret");
    let static_chars = append_utf16("Static");
    let dynamic_variable = append_utf16("_root.dynamicText");
    let input_variable = append_utf16("_root.inputText");
    let dynamic_name = append_utf16("dynamic_txt");
    let input_name = append_utf16("input_txt");
    let long_url = append_utf16(&"é".repeat(3_000));
    let long_target = append_utf16(&"界".repeat(100));

    let style = TimelineTextStyle {
        revision: 10,
        font_size_twips: Some(240),
        font_family: Some(font),
        color_rgba: Some(0xff00_0000),
        alignment: Some(0),
        url: Some(long_url),
        target: Some(long_target),
        ..TimelineTextStyle::default()
    };
    let text = |characters: TimelineString,
                instance_name: Option<TimelineString>,
                variable_name: Option<TimelineString>,
                flags: u32,
                max_characters: u16| TimelineText {
        revision: Some(9),
        matrix_words: Some([65_536, 0, 0, 65_536, 0, 0]),
        bounds_words: Some([0, 2_000, 0, 400]),
        flags: Some(flags),
        max_characters: Some(max_characters),
        variable_name,
        instance_name,
        runs: vec![TimelineTextRun {
            characters,
            style: style.clone(),
        }],
        ..TimelineText::default()
    };
    let dynamic = text(
        dynamic_chars,
        Some(dynamic_name),
        Some(dynamic_variable),
        0x0001 | 0x0002 | 0x0080,
        0,
    );
    let input = text(
        input_chars,
        Some(input_name),
        Some(input_variable),
        0x0001 | 0x0004,
        12,
    );
    let static_text = text(static_chars, None, None, 0, 0);
    let stream = BinaryTimelineStream {
        index: 1,
        stream_kind: Some(1),
        root: None,
        references: vec![],
        pointers: vec![],
        objects: vec![],
        shapes: vec![],
        original_bytes,
    };
    let mut import_report = report();

    let dynamic_element = convert_timeline_text(
        &stream,
        &dynamic,
        "dynamic",
        "",
        Some(0x3f),
        &mut import_report,
    )
    .expect("Flash 8 dynamic text");
    let dynamic_state =
        TextFieldState::from_kind(&dynamic_element.kind).expect("dynamic field state");
    assert_eq!(dynamic_element.name, "dynamic_txt");
    assert_eq!(dynamic_state.field_type, TextFieldType::Dynamic);
    assert!(!dynamic_state.selectable);
    assert!(dynamic_state.html);
    assert_eq!(dynamic_state.variable.as_deref(), Some("_root.dynamicText"));
    assert_eq!(dynamic_state.format.url.len(), 4_096);
    assert_eq!(dynamic_state.format.target_str().len(), 255);
    dynamic_state
        .format
        .validate()
        .expect("UTF-8 metadata is truncated to byte limits");

    let input_element =
        convert_timeline_text(&stream, &input, "input", "", Some(0x3f), &mut import_report)
            .expect("Flash 8 input text");
    let input_state = TextFieldState::from_kind(&input_element.kind).expect("input field state");
    assert_eq!(input_element.name, "input_txt");
    assert_eq!(input_state.field_type, TextFieldType::Input);
    assert!(input_state.display_as_password);
    assert_eq!(input_state.max_chars, 12);
    assert_eq!(input_state.variable.as_deref(), Some("_root.inputText"));

    let static_element = convert_timeline_text(
        &stream,
        &static_text,
        "static",
        "",
        Some(0x3f),
        &mut import_report,
    )
    .expect("Flash 8 static text keeps the established converter path");
    let ElementKind::TextBox { format, .. } = &static_element.kind else {
        panic!("static text must remain a text box");
    };
    assert!(format.field_options().is_none());
    assert_eq!(format.url.len(), 4_096);
    assert_eq!(format.target_str().len(), 255);
    format
        .validate()
        .expect("static text metadata is also byte-bounded");
}

#[test]
fn gradient_extensions_bitmap_fills_and_procedural_strokes_convert_without_approximation_warnings()
{
    let mut edges = vec![];
    for (points, fill_idx) in [
        ([[0, 0], [20, 0], [20, 20], [0, 20]], 1),
        ([[30, 0], [50, 0], [50, 20], [30, 20]], 2),
    ] {
        for i in 0..4 {
            edges.push(edge(points[i], points[(i + 1) % 4], 0, fill_idx, 0, None));
        }
    }
    edges.extend([
        edge([0, 30], [20, 30], 0, 0, 1, None),
        edge([0, 40], [20, 40], 0, 0, 2, None),
        edge([0, 50], [20, 50], 0, 0, 3, None),
        edge([0, 60], [20, 60], 0, 0, 4, None),
        edge([0, 70], [20, 70], 0, 0, 5, None),
    ]);
    let shape = BinaryShapeRecord {
        shape_revision: 6,
        data_revision: 5,
        matrix_words: [65536, 0, 0, 65536, 0, 0],
        metadata: 0,
        fills: vec![
            BinaryFill {
                packed_value: 0,
                encoding: BinaryFillEncoding::Extended {
                    flags: 0x12,
                    secondary_flags: 0,
                    payload: BinaryFillPayload::Gradient {
                        matrix_words: [65536, 0, 0, 65536, 0, 0],
                        extension: Some(BinaryGradientExtension {
                            flags: 1 | 4, // linearRGB + Reflect spread
                            focal_point: 128,
                        }),
                        stops: vec![
                            BinaryGradientStop {
                                ratio: 0,
                                color_word: 0xffff0000,
                            },
                            BinaryGradientStop {
                                ratio: 255,
                                color_word: 0xff0000ff,
                            },
                        ],
                    },
                },
            },
            BinaryFill {
                packed_value: 0,
                encoding: BinaryFillEncoding::Extended {
                    flags: 0x40, // repeating + smoothed bitmap fill
                    secondary_flags: 0,
                    payload: BinaryFillPayload::Bitmap {
                        matrix_words: [65536 * 20, 0, 0, 65536 * 20, 40, 80],
                        media_id: 7,
                    },
                },
            },
        ],
        strokes: vec![
            // 1 = Dashed
            BinaryStroke {
                color_word: 0xff112233,
                width_word: 40,
                style_words: [120, 60],
                extension: None,
            },
            // 2 = Dotted
            BinaryStroke {
                color_word: 0xff223344,
                width_word: 20,
                style_words: [0, 2 | (80 << 3)],
                extension: None,
            },
            // 3 = Ragged
            BinaryStroke {
                color_word: 0xff334455,
                width_word: 20,
                style_words: [0, 3 | (1 << 3) | (2 << 6) | (1 << 8)],
                extension: None,
            },
            // 4 = Stipple
            BinaryStroke {
                color_word: 0xff445566,
                width_word: 20,
                style_words: [0, 4 | (1 << 3) | (2 << 5) | (1 << 7)],
                extension: None,
            },
            // 5 = Hatched
            BinaryStroke {
                color_word: 0xff556677,
                width_word: 20,
                style_words: [0, 5 | (1 << 3) | (2 << 5) | (1 << 13)],
                extension: None,
            },
        ],
        edges,
        auxiliary: vec![],
        original_bytes: vec![],
    };
    let mut bitmaps = BTreeMap::new();
    bitmaps.insert(
        7,
        assets::BitmapAsset {
            resource_id: "legacy-bitmap-7".into(),
            width: 16.0,
            height: 16.0,
            smoothing: true,
        },
    );
    let mut rep = report();
    let converted =
        shape::convert_shape_with_effects(&shape, "shape", &bitmaps, &mut rep, "synthetic")
            .unwrap();
    assert!(
        !rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_shape_approximated"),
        "gradient spread/linearRGB, resolved bitmap fills, and procedural strokes should not emit fla_shape_approximated"
    );
    let ElementKind::PaintedPath { fill_gradient, .. } = &converted.elements[0].kind else {
        panic!("expected gradient path");
    };
    let grad = fill_gradient.as_ref().unwrap();
    assert_eq!(grad.spread, GradientSpread::Reflect);
    assert!(grad.linear_rgb);
    assert!((grad.focal_point_ratio - (128.0 / 255.0)).abs() < 1e-9);
    assert_eq!(converted.bitmap_fills.len(), 1);
    assert_eq!(converted.bitmap_fills[0].element_id, "shape-fill-1");
    assert_eq!(converted.bitmap_fills[0].resource_id, "legacy-bitmap-7");
    assert!(converted.bitmap_fills[0].repeat);
    assert!(converted.bitmap_fills[0].smoothing);
    assert_eq!(converted.appearances.len(), 2);
    assert_eq!(converted.vector_effects.len(), 3);
}

#[test]
fn classic_tween_rotation_direction_turns_and_flags_convert_cleanly() {
    let mut source = timeline_fixture();
    if let Some(TimelinePayload::Frame(frame)) = &mut source
        .objects
        .iter_mut()
        .find(|o| o.object_id == 10)
        .unwrap()
        .payload
    {
        frame.key_mode = Some(1 | 0x100 | 0x800 | 0x1000);
        frame.rotation_mode = Some(2); // CCW
        frame.rotation_count = Some(3);
    }
    let (timeline, rep) = convert_timeline_reporting(&source).unwrap();
    assert!(
        !rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_tween_rotation")
    );
    let key = &timeline.layers[0].keyframes[0];
    assert_eq!(key.classic_rotate_dir, ClassicRotateDirection::Ccw);
    assert_eq!(key.classic_rotate_turns, 3);
    assert!(key.orient_to_path);
    assert!(key.sync_symbols);
    assert!(key.snap_to_guide);
}

#[test]
fn primitive_video_and_compiled_clip_records_convert_without_placeholders() {
    let mut source = timeline_fixture();
    let mut rect_edges = Vec::new();
    let pts = [[0, 0], [40, 0], [40, 20], [0, 20]];
    for i in 0..4 {
        rect_edges.push(edge(pts[i], pts[(i + 1) % 4], 0, 1, 1, None));
    }
    source.shapes.push(LocatedBinaryShape {
        offset: 0,
        record: BinaryShapeRecord {
            shape_revision: 6,
            data_revision: 5,
            matrix_words: [65536, 0, 0, 65536, 200, 400],
            metadata: 0,
            fills: vec![BinaryFill {
                packed_value: 0xff112233,
                encoding: BinaryFillEncoding::Legacy { word: 0 },
            }],
            strokes: vec![BinaryStroke {
                color_word: 0xff445566,
                width_word: 40,
                style_words: [0, 0],
                extension: None,
            }],
            edges: rect_edges,
            auxiliary: vec![],
            original_bytes: vec![],
        },
    });
    source.objects.push(TimelineObject {
        object_id: 90,
        class: TimelineClass::Rect,
        schema: 1,
        introduced_by_pointer: 0,
        owner: Some(10),
        children: vec![],
        encoded: TimelineByteSpan { start: 0, end: 1 },
        body_offset: 0,
        complete: true,
        base: Some(TimelineBase::default()),
        payload: Some(TimelinePayload::Primitive(Box::new(TimelinePrimitive {
            revision: Some(1),
            flags: Some(1),
            corner_radii: Some([100, 100, 100, 100]),
            ..Default::default()
        }))),
        shape_index: Some(0),
    });
    source.objects.push(TimelineObject {
        object_id: 91,
        class: TimelineClass::VideoStream,
        schema: 1,
        introduced_by_pointer: 0,
        owner: Some(10),
        children: vec![],
        encoded: TimelineByteSpan { start: 0, end: 1 },
        body_offset: 0,
        complete: true,
        base: Some(TimelineBase::default()),
        payload: Some(TimelinePayload::Video(Box::new(TimelineVideo {
            revision: Some(4),
            matrix_words: Some([65536, 0, 0, 65536, 0, 0]),
            bounds_words: Some([0, 6400, 0, 4800]),
            media_present: Some(1),
            media_id: Some(3),
            ..Default::default()
        }))),
        shape_index: None,
    });
    source.objects.push(TimelineObject {
        object_id: 92,
        class: TimelineClass::CompiledClip,
        schema: 1,
        introduced_by_pointer: 0,
        owner: Some(10),
        children: vec![],
        encoded: TimelineByteSpan { start: 0, end: 1 },
        body_offset: 0,
        complete: true,
        base: Some(TimelineBase::default()),
        payload: Some(TimelinePayload::CompiledClip(Box::new(
            TimelineCompiledClip {
                revision: Some(7),
                matrix_words: Some([65536, 0, 0, 65536, 100, 200]),
                media_present: Some(1),
                media_id: Some(5),
                ..Default::default()
            },
        ))),
        shape_index: None,
    });
    if let Some(frame_obj) = source.objects.iter_mut().find(|o| o.object_id == 10) {
        frame_obj.children.extend([90, 91, 92]);
    }
    let (timeline, rep) = convert_timeline_reporting(&source).unwrap();
    assert!(
        !rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_object_placeholder"),
        "Rect, VideoStream, and CompiledClip should not emit fla_object_placeholder"
    );
    let elems = &timeline.layers[0].keyframes[0].elements;
    assert!(matches!(
        elems[1].kind,
        ElementKind::Rectangle {
            width: 40.0,
            height: 20.0,
            ..
        }
    ));
    assert_eq!(
        elems[1].transform.primitive,
        Some(PrimitiveOptions::Rectangle(RectangleOptions {
            corner_radii: [5.0; 4],
            corners_locked: true,
        }))
    );
    assert_eq!(
        elems[2].kind,
        ElementKind::Video {
            width: 320.0,
            height: 240.0,
        }
    );
    assert_eq!(
        elems[3].kind,
        ElementKind::Instance {
            symbol_id: "fla-compiled-clip-5".into(),
            playback: InstancePlayback::Independent,
        }
    );
}
