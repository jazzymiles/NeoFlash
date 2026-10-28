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
    let mut report = report();
    let mut context = Context {
        report: &mut report,
        library: BTreeMap::from([(100, "graphic".into())]),
        bitmaps: BTreeMap::new(),
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    context.timeline(stream)
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

    // A mismatched library at a tween endpoint must not silently turn motion
    // into a hold. A final tween without its endpoint is equally incomplete.
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
    assert!(
        convert_timeline(&mismatch)
            .unwrap_err()
            .contains("stable symbol placement")
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
    assert!(
        convert_timeline(&unfinished)
            .unwrap_err()
            .contains("unresolved tween")
    );
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
    assert!(
        shape::convert_shape(&unclosed, "broken", &mut report(), "synthetic")
            .unwrap_err()
            .contains("unmatched endpoints")
    );
}
