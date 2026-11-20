//! Unit tests for T-408 binary FLA fidelity: embedded video muxing, classic
//! text metrics/flags, CPicSymbol rev 23 visibility & bitmap render mode,
//! per-instance behavior & looping graphic modulo wrap, component parameter
//! script synthesis, and classic tween continuity.

use super::*;
use crate::legacy::media::{BinaryVideoPayload, decode_binary_video};
use crate::{ImportFormat, Statistics};
use std::collections::{BTreeMap, BTreeSet};

fn empty_report() -> ImportReport {
    ImportReport {
        source: "synthetic.fla".into(),
        format: ImportFormat::LegacyBinaryFla,
        version: None,
        can_import: true,
        statistics: Statistics::default(),
        diagnostics: vec![],
        binary_fla: None,
        swf: None,
        swc: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_cmedia_video_stream(
    revision: u8,
    video_type: u32,
    width: i32,
    height: i32,
    duration_seconds: f64,
    video_packets: &[(i32, i32, &[u8])],
    audio_packets: &[(i32, i32, &[u8])],
    bundled: Option<(&str, &[u8])>,
) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(revision);
    b.extend_from_slice(&video_type.to_le_bytes());
    b.extend_from_slice(&25.0_f64.to_le_bytes()); // frame_rate (40ms/frame)
    b.extend_from_slice(&44_100.0_f64.to_le_bytes()); // audio_sample_rate
    b.extend_from_slice(&16_i32.to_le_bytes()); // audio_bits_per_sample
    b.extend_from_slice(&1_i32.to_le_bytes()); // audio_channels
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&duration_seconds.to_le_bytes());
    b.extend_from_slice(&0_u32.to_le_bytes()); // is_special
    b.push(u8::from(!audio_packets.is_empty())); // import_audio
    b.push(1); // sync_video
    b.extend_from_slice(&0_u32.to_le_bytes()); // drop_ratio_index
    b.extend_from_slice(&80_i32.to_le_bytes()); // quality
    b.extend_from_slice(&1.0_f64.to_le_bytes()); // scale_ratio
    b.extend_from_slice(&12_u32.to_le_bytes()); // key_frame_interval

    for (kind, packets) in [(0_u32, video_packets), (1_u32, audio_packets)] {
        b.extend_from_slice(&kind.to_le_bytes());
        let mut offset = 0_i32;
        let mut pool = Vec::new();
        for &(frame_count, flags, payload) in packets {
            b.push(1); // marker = 1
            b.extend_from_slice(&offset.to_le_bytes());
            b.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            b.extend_from_slice(&frame_count.to_le_bytes());
            b.extend_from_slice(&flags.to_le_bytes());
            offset += payload.len() as i32;
            pool.extend_from_slice(payload);
        }
        b.push(0); // end of packet indices
        b.extend_from_slice(&(pool.len() as u32).to_le_bytes());
        b.extend_from_slice(&pool);
    }

    if revision >= 3 {
        if let Some((path, bytes)) = bundled {
            b.extend_from_slice(&1_u32.to_le_bytes()); // has_bundled = 1
            b.push(path.len() as u8);
            b.extend_from_slice(path.as_bytes());
            b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(bytes);
        } else {
            b.extend_from_slice(&0_u32.to_le_bytes());
            b.push(0);
            b.extend_from_slice(&0_u32.to_le_bytes());
        }
    }
    b
}

#[test]
fn cmedia_video_stream_muxes_vp6_flv_handles_empty_viewport_bundled_mp4_and_vp6_alpha() {
    // 1. VP6 (video_type = 4) + MP3 audio packets -> valid FLV v1 container.
    let vp6_kf = [0x00_u8, 0x78, 0x56, 0x34, 0x12];
    let vp6_inter = [0x00_u8, 0xaa, 0xbb];
    let mp3_frame = [0xff_u8, 0xfb, 0x90, 0xc4, 0x00, 0x00];
    let stream = build_cmedia_video_stream(
        3,
        4,
        320,
        240,
        2.0,
        &[(1, 1, &vp6_kf), (1, 0, &vp6_inter)],
        &[(1, 0, &mp3_frame)],
        None,
    );
    let decoded = decode_binary_video(&stream).expect("decode VP6 CMediaVideoStream");
    assert_eq!(decoded.width, 320.0);
    assert_eq!(decoded.height, 240.0);
    assert!(decoded.sync_video && decoded.import_audio);
    let BinaryVideoPayload::Container {
        media_type,
        extension,
        bytes,
    } = decoded.payload
    else {
        panic!("expected muxed FLV container");
    };
    assert_eq!(media_type, "video/x-flv");
    assert_eq!(extension, "flv");
    assert_eq!(&bytes[..4], b"FLV\x01");
    assert_eq!(bytes[4], 0x05); // video (0x01) | audio (0x04)
    assert_eq!(u32::from_be_bytes(bytes[5..9].try_into().unwrap()), 9);
    // Verify FLV tag chain integrity (ScriptData + Video + Audio + Video).
    let mut pos = 9usize;
    let mut tag_types = Vec::new();
    while pos < bytes.len() {
        let prev_size = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        if pos == 9 {
            assert_eq!(prev_size, 0);
        }
        pos += 4;
        if pos == bytes.len() {
            break;
        }
        let tag_type = bytes[pos];
        let data_size = (usize::from(bytes[pos + 1]) << 16)
            | (usize::from(bytes[pos + 2]) << 8)
            | usize::from(bytes[pos + 3]);
        tag_types.push(tag_type);
        pos += 11 + data_size;
        let recorded_prev = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        assert_eq!(recorded_prev, 11 + data_size);
    }
    assert!(tag_types.contains(&18) && tag_types.contains(&9) && tag_types.contains(&8));
    assert_eq!(tag_types.len(), 4);

    // 2. ActionScript-controlled empty Video library item (video_type = 0).
    let empty_stream = build_cmedia_video_stream(2, 0, 160, 120, 0.0, &[], &[], None);
    let empty = decode_binary_video(&empty_stream).expect("decode empty video viewport");
    assert!(matches!(empty.payload, BinaryVideoPayload::EmptyViewport));

    // 3. Raw VP6 with alpha (video_type = 5) -> honest UnsupportedCodec.
    let vp6a_stream =
        build_cmedia_video_stream(2, 5, 320, 240, 1.0, &[(1, 1, &[0x15, 0x00])], &[], None);
    let vp6a = decode_binary_video(&vp6a_stream).expect("decode VP6-alpha header");
    assert!(matches!(
        vp6a.payload,
        BinaryVideoPayload::UnsupportedCodec(_)
    ));

    // 4. Bundled MP4 device video in rev 3 -> video/mp4 passthrough.
    let fake_mp4 = b"\x00\x00\x00\x14ftypisom\x00\x00\x02\x00isom";
    let bundled_stream =
        build_cmedia_video_stream(3, 0, 640, 360, 5.0, &[], &[], Some(("clip.mp4", fake_mp4)));
    let mp4 = decode_binary_video(&bundled_stream).expect("decode bundled MP4");
    assert!(matches!(
        mp4.payload,
        BinaryVideoPayload::Container {
            media_type: "video/mp4",
            extension: "mp4",
            ..
        }
    ));
}

#[test]
fn cpic_text_maps_negative_leading_dormant_static_variable_fixed_wrap_and_aa_modes() {
    let mut stream = BinaryTimelineStream {
        index: 0,
        stream_kind: Some(1),
        root: None,
        references: vec![],
        pointers: vec![],
        objects: vec![],
        shapes: vec![],
        original_bytes: vec![],
    };
    let mut push_ascii = |s: &str| {
        let start = stream.original_bytes.len();
        stream.original_bytes.extend_from_slice(s.as_bytes());
        let span = TimelineByteSpan {
            start,
            end: stream.original_bytes.len(),
        };
        TimelineString {
            encoded: span,
            data: span,
            utf16: false,
        }
    };
    let font = push_ascii("Verdana");
    let dormant_var = push_ascii("legacy_unused_var");
    let body = push_ascii("Wrapped static paragraph text inside a fixed-width box");
    let dyn_body = push_ascii("Line 1\rLine 2");
    let aa_readability_start = stream.original_bytes.len();
    stream
        .original_bytes
        .extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let aa_readability = TimelineByteSpan {
        start: aa_readability_start,
        end: stream.original_bytes.len(),
    };

    // A. Static fixed-width multi-line box with negative leading (-14 pt on 10 pt font),
    // flag 0x0080, and dormant variable_name -> stays Static, wraps, zero warnings!
    let static_style = TimelineTextStyle {
        revision: 10,
        font_face: Some(font.clone()),
        font_size_twips: Some(200), // 10 pt
        color_rgba: Some(0xff112233),
        line_spacing: Some(-280), // -14 pt (10 * 1.2 - 14 = -2 <= 0)
        ..Default::default()
    };
    let static_text = TimelineText {
        revision: Some(10),
        matrix_words: Some([65536, 0, 0, 65536, 200, 400]),
        bounds_words: Some([0, 2400, 0, 960]), // 120 x 48 px (>= 10 * 1.8)
        auto_expand: Some(0),
        flags: Some(0x0080), // historical bit on Static Text must NOT turn into Dynamic HTML
        variable_name: Some(dormant_var),
        runs: vec![TimelineTextRun {
            characters: body,
            style: static_style.clone(),
        }],
        ..Default::default()
    };
    let mut rep = empty_report();
    let el = convert_timeline_text(
        &stream,
        &static_text,
        "txt-static",
        "",
        Some(0x3f),
        &mut rep,
    )
    .expect("convert static text");
    assert!(
        rep.diagnostics.is_empty(),
        "unexpected static text warnings: {:?}",
        rep.diagnostics
    );
    let ElementKind::TextBox {
        format,
        word_wrap,
        multiline,
        ..
    } = &el.kind
    else {
        panic!("expected TextBox");
    };
    assert!(format.field_options().is_none(), "must remain Static Text");
    assert!(*word_wrap && *multiline);
    assert!(format.validate().is_ok());

    // B. Dynamic text with aa_mode == 1 ("advanced") -> maps anti_alias_type = "advanced".
    let dyn_style = TimelineTextStyle {
        anti_alias: Some(aa_readability),
        ..static_style.clone()
    };
    let dyn_text = TimelineText {
        flags: Some(0x0001 | 0x0002 | 0x0010 | 0x0008),
        auto_expand: Some(0),
        runs: vec![TimelineTextRun {
            characters: dyn_body,
            style: dyn_style,
        }],
        ..static_text.clone()
    };
    let dyn_el = convert_timeline_text(&stream, &dyn_text, "txt-dyn", "", Some(0x3f), &mut rep)
        .expect("convert dynamic text");
    assert!(rep.diagnostics.is_empty(), "{:?}", rep.diagnostics);
    let dyn_state = TextFieldState::from_kind(&dyn_el.kind).expect("dynamic text state");
    assert_eq!(dyn_state.field_type, TextFieldType::Dynamic);
    assert_eq!(dyn_state.anti_alias_type, "advanced");
    assert_eq!(dyn_state.sharpness, 0.0);
    assert_eq!(dyn_state.thickness, 0.0);

    // C. Bitmap-aliased text (bitmap_rendering = 1) -> emits honest fla_text_metrics.
    let mut bitmap_text = static_text;
    bitmap_text.runs[0].style.bitmap_rendering = Some(1);
    let _ = convert_timeline_text(&stream, &bitmap_text, "txt-bmp", "", Some(0x3f), &mut rep)
        .expect("convert bitmap-aliased text");
    assert!(rep.diagnostics.iter().any(|d| d.code == "fla_text_metrics"));
}

#[test]
fn cpic_symbol_rev23_instance_behavior_component_props_and_classic_tweens_convert_cleanly() {
    let mut bytes = Vec::new();
    let mut push_str = |s: &str| {
        let start = bytes.len();
        bytes.extend_from_slice(s.as_bytes());
        let span = TimelineByteSpan {
            start,
            end: bytes.len(),
        };
        TimelineString {
            encoded: span,
            data: span,
            utf16: false,
        }
    };
    let prop_autoplay = push_str("autoPlay");
    let prop_val_true = push_str("true");
    let prop_volume = push_str("volume");
    let prop_val_vol = push_str("0.75");
    let prop_tint = push_str("tint");
    let prop_val_tint = push_str("#ff3366");
    let prop_items = push_str("items");
    let prop_item_a = push_str("alpha");
    let prop_item_b = push_str("beta");
    let prop_preview = push_str("preview");
    let prop_preview_blob = push_str("ignored");

    let mk_prop = |name: TimelineString, value_type: u16, vals: Vec<TimelineString>| {
        TimelineComponentProperty {
            revision: Some(1),
            name: Some(name),
            value: Some(TimelinePropertyValue {
                revision: 1,
                value_type,
                entries: vals
                    .into_iter()
                    .map(|value| TimelinePropertyValueEntry {
                        name: None,
                        value,
                        value_type: Some(1),
                    })
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        }
    };
    let comp_props = TimelineComponentProperties {
        encoded: TimelineByteSpan { start: 0, end: 1 },
        revision: 1,
        entries: vec![
            mk_prop(prop_autoplay, 7, vec![prop_val_true]), // Boolean
            mk_prop(prop_volume, 6, vec![prop_val_vol]),    // Number
            mk_prop(prop_tint, 9, vec![prop_val_tint]),     // Color
            mk_prop(prop_items, 2, vec![prop_item_a, prop_item_b]), // Array
            mk_prop(prop_preview, 19, vec![prop_preview_blob]), // Authoring preview (ignored)
        ],
        parameters_are_new: None,
    };

    let mk_obj = |id: u32, class: TimelineClass, payload: TimelinePayload, children: &[u32]| {
        TimelineObject {
            object_id: id,
            class,
            schema: 1,
            introduced_by_pointer: 0,
            owner: None,
            children: children.to_vec(),
            encoded: TimelineByteSpan { start: 0, end: 4 },
            body_offset: 2,
            complete: true,
            base: Some(TimelineBase {
                revision: Some(1),
                flags: Some(0),
                children_complete: true,
                cache_as_bitmap: Some(1),
                ..Default::default()
            }),
            payload: Some(payload),
            shape_index: None,
        }
    };

    let txt_name = push_str("label");
    let txt_font = push_str("Arial");
    let helper_text = TimelineText {
        revision: Some(10),
        matrix_words: Some([65536, 0, 0, 65536, 0, 0]),
        bounds_words: Some([0, 800, 0, 300]),
        auto_expand: Some(1),
        flags: Some(0),
        runs: vec![TimelineTextRun {
            characters: txt_name,
            style: TimelineTextStyle {
                revision: 10,
                font_face: Some(txt_font),
                font_size_twips: Some(240),
                color_rgba: Some(0xff000000),
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    // Layer has:
    // - Frame 0..4 (classic tween): 1 instance of lib 100 (MovieClip overridden as Graphic,
    //   LoopFrom(11), rev 23 is_visible = 0, bitmap_render_mode = 0 overriding base cache_as_bitmap = 1,
    //   and component properties).
    // - Frame 4..5 (dormant classic tween before Blank Keyframe): [instance of lib 100 at x=80, helper TextBox].
    // - Frame 5..6 (Blank Keyframe): [].
    let inst_f0 = TimelineInstance {
        revision: Some(23),
        loop_mode: Some(0),
        first_frame: Some(11),
        library_id: Some(100),
        matrix_words: Some([65536, 0, 0, 65536, 0, 0]),
        bitmap_render_mode: Some(0),
        is_visible: Some(0),
        property_count: Some(comp_props.entries.len() as u32),
        properties: Some(comp_props),
        ..Default::default()
    };
    let inst_f4 = TimelineInstance {
        revision: Some(23),
        loop_mode: Some(0),
        first_frame: Some(11),
        library_id: Some(100),
        matrix_words: Some([65536, 0, 0, 65536, 1600, 0]), // x = 80 px
        bitmap_render_mode: Some(1),
        is_visible: Some(1),
        ..Default::default()
    };

    let stream = BinaryTimelineStream {
        index: 0,
        stream_kind: Some(1),
        root: Some(1),
        references: vec![],
        pointers: vec![],
        objects: vec![
            mk_obj(
                1,
                TimelineClass::Page,
                TimelinePayload::Page(TimelinePage::default()),
                &[2],
            ),
            mk_obj(
                2,
                TimelineClass::Layer,
                TimelinePayload::Layer(TimelineLayer::default()),
                &[10, 11, 12],
            ),
            mk_obj(
                10,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(4),
                    key_mode: Some(1), // classic tween
                    acceleration: Some(0),
                    ..Default::default()
                })),
                &[20],
            ),
            mk_obj(
                11,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(1),
                    key_mode: Some(1), // dormant tween before Blank Keyframe 12
                    acceleration: Some(0),
                    ..Default::default()
                })),
                &[21, 22],
            ),
            mk_obj(
                12,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(1),
                    key_mode: Some(0),
                    ..Default::default()
                })),
                &[],
            ),
            mk_obj(
                20,
                TimelineClass::Symbol,
                TimelinePayload::Instance(Box::new(inst_f0)),
                &[],
            ),
            mk_obj(
                21,
                TimelineClass::Symbol,
                TimelinePayload::Instance(Box::new(inst_f4)),
                &[],
            ),
            mk_obj(
                22,
                TimelineClass::Text,
                TimelinePayload::Text(Box::new(helper_text)),
                &[],
            ),
        ],
        shapes: vec![],
        original_bytes: bytes,
    };

    let mut rep = empty_report();
    let mut ctx = Context {
        report: &mut rep,
        library: BTreeMap::from([(100, "sym-mc".into())]),
        library_kinds: BTreeMap::from([(100, SymbolKind::MovieClip)]),
        bitmaps: BTreeMap::new(),
        sounds: BTreeMap::new(),
        videos: BTreeMap::new(),
        compiled_clips: BTreeMap::new(),
        skipped_bitmaps: BTreeSet::new(),
        stream_stops: BTreeSet::new(),
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        contents_revision: Some(0x3f),
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let timeline = ctx.timeline(&stream).expect("convert timeline");
    assert!(
        rep.diagnostics.is_empty(),
        "expected zero warnings, got {:?}",
        rep.diagnostics
    );

    let layer = &timeline.layers[0];
    assert!(
        layer.keyframes[0].tween.is_some(),
        "1->2 unambiguous tween kept"
    );
    assert!(
        layer.keyframes[1].tween.is_none(),
        "dormant tween before Blank Keyframe cleared cleanly"
    );

    let f0_el = &layer.keyframes[0].elements[0];
    assert!(!f0_el.visible, "CPicSymbol rev 23 is_visible == 0 mapped");
    assert!(
        !f0_el.cache_as_bitmap,
        "CPicSymbol rev 23 bitmap_render_mode == 0 overrides base.cache_as_bitmap"
    );
    assert_eq!(f0_el.instance_behavior, Some(SymbolKind::Graphic));
    assert_eq!(f0_el.scripts.len(), 1);
    let script_src = &f0_el.scripts[0].script.source;
    assert!(script_src.contains("this.autoPlay = true;"));
    assert!(script_src.contains("this.volume = 0.75;"));
    assert!(script_src.contains("this.tint = 0xff3366;"));
    assert!(script_src.contains("this.items = [\"alpha\", \"beta\"];"));
    assert!(!script_src.contains("preview"));

    let f4_inst = &layer.keyframes[1].elements[0];
    assert_eq!(f0_el.id, f4_inst.id, "1->2 unambiguous symbol continuity");
    assert!(f4_inst.cache_as_bitmap);
    let mid = sample_layer(layer, 2);
    assert_eq!(mid.len(), 1);
    assert!((mid[0].transform.x - 40.0).abs() < 1e-6);

    // Verify LoopFrom(11) wraps modulo 4 -> LoopFrom(3) without fla_graphic_first_frame.
    let mut symbols = vec![
        Symbol {
            id: "sym-mc".into(),
            name: "MC".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: None,
        },
        Symbol {
            id: "root".into(),
            name: "Scene 1".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline,
            class_name: None,
            linkage_id: None,
        },
    ];
    normalize_playback(&mut symbols, &mut rep);
    assert!(
        !rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_graphic_first_frame")
    );
    assert_eq!(
        symbols[1].timeline.layers[0].keyframes[0].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "sym-mc".into(),
            playback: InstancePlayback::LoopFrom(3),
        }
    );
}
