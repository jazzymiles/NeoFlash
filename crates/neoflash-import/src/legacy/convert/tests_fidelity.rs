//! Unit tests for binary FLA fidelity & approximation honesty:
//! - Embedded video muxing (`VP6` without extra `0x00`, `VP6A` alpha FLV CodecID 5,
//!   AVC header inspection, negative `frame_raw` rejection, `0`-frame config header
//!   timestamp preservation, 8kHz MP3 `SoundFormat=14`, empty initial audio packet scan,
//!   and 48kHz->44.1kHz PCM resampling).
//! - Classic text metrics/flags (`fla_text_metrics` disclosure, negative-leading clamp
//!   `fla_text_approximated`, tight fixed-width Static Text `word_wrap`/`multiline`,
//!   multi-run `bitmap_rendering` check, and Dynamic `aa_mode == 1`).
//! - Component parameter `ObjectEvent::Load` synthesis strictly on AS1/AS2 `MovieClip`
//!   instances with implementation timelines/scripts (never AS3, `Graphic`, `Button`,
//!   or unimplemented clips) + strict value validation (`fla_instance_settings`).
//! - Broken `1 -> N` and non-final empty-endpoint classic tweens (`fla_tween_dropped`),
//!   `LoopFrom(f)` `fla_graphic_first_frame` when `f % len != 0`, and tweened group
//!   endpoint deduplication provenance remapping.

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
fn build_cmedia_video_stream_with_audio(
    revision: u8,
    video_type: u32,
    audio_sample_rate: f64,
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
    b.extend_from_slice(&audio_sample_rate.to_le_bytes());
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
            b.extend_from_slice(&1_u32.to_le_bytes());
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
fn cmedia_video_stream_muxes_vp6_vp6a_avc_and_audio_accurately() {
    // 1. VP6 (video_type = 4) + empty initial audio packet + MP3 frame:
    //    - No extra 0x00 byte prepended before VP6FLVVideoPacket.
    //    - Scans past empty initial audio packet to detect MP3 (SoundFormat = 2).
    let vp6_kf = [0x00_u8, 0x78, 0x56, 0x34, 0x12];
    let vp6_inter = [0x00_u8, 0xaa, 0xbb];
    let mp3_frame = [0xff_u8, 0xfb, 0x90, 0xc4, 0x00, 0x00];
    let stream = build_cmedia_video_stream_with_audio(
        3,
        4,
        44_100.0,
        320,
        240,
        2.0,
        &[(1, 1, &vp6_kf), (1, 0, &vp6_inter)],
        &[(0, 0, &[]), (1, 0, &mp3_frame)],
        None,
    );
    let decoded = decode_binary_video(&stream).expect("decode VP6 CMediaVideoStream");
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
    assert_eq!(bytes[4], 0x05);

    let mut pos = 9usize;
    let mut video_bodies = Vec::new();
    let mut audio_headers = Vec::new();
    let mut video_timestamps = Vec::new();
    while pos < bytes.len() {
        pos += 4;
        if pos == bytes.len() {
            break;
        }
        let tag_type = bytes[pos];
        let data_size = (usize::from(bytes[pos + 1]) << 16)
            | (usize::from(bytes[pos + 2]) << 8)
            | usize::from(bytes[pos + 3]);
        let ts = (u32::from(bytes[pos + 7]) << 24)
            | (u32::from(bytes[pos + 4]) << 16)
            | (u32::from(bytes[pos + 5]) << 8)
            | u32::from(bytes[pos + 6]);
        let body = &bytes[pos + 11..pos + 11 + data_size];
        if tag_type == 9 {
            video_bodies.push(body.to_vec());
            video_timestamps.push(ts);
        } else if tag_type == 8 {
            audio_headers.push(body[0]);
        }
        pos += 11 + data_size;
    }
    assert_eq!(
        video_bodies[0],
        vec![0x14, 0x00, 0x78, 0x56, 0x34, 0x12],
        "VP6 tag body must NOT have a duplicate 0x00 byte"
    );
    assert_eq!(video_bodies[1], vec![0x24, 0x00, 0xaa, 0xbb]);
    assert_eq!(video_timestamps, vec![0, 40]);
    assert_eq!(
        audio_headers[0] >> 4,
        2,
        "must detect MP3 after empty initial packet"
    );

    // 2. VP6A (video_type = 5) -> FLV CodecID 5 (`0x15` keyframe header), recognized by flv_has_vp6a_alpha.
    let vp6a_pkt = [
        0x00_u8, 0x00, 0x00, 0x04, 0x41, 0x46, 0x00, 0x20, 0x41, 0x46,
    ];
    let vp6a_stream = build_cmedia_video_stream_with_audio(
        2,
        5,
        44_100.0,
        320,
        240,
        1.0,
        &[(1, 1, &vp6a_pkt)],
        &[],
        None,
    );
    let vp6a = decode_binary_video(&vp6a_stream).expect("decode VP6A");
    let BinaryVideoPayload::Container {
        bytes: vp6a_flv, ..
    } = vp6a.payload
    else {
        panic!("expected muxed FLV for VP6A");
    };
    assert!(neoflash_core::timeline_video::flv_has_vp6a_alpha(&vp6a_flv));

    // 3. AVC (video_type = 7): 0-duration AVC sequence header + pre-wrapped B-frame CTO preserved!
    let avc_seq = [
        0x00_u8, 0x00, 0x00, 0x00, // AVCPacketType = 0, CTO = 0
        0x01, 0x42, 0x00, 0x1e, 0xff, 0xe1, 0x00, 0x02, 0x67, 0x42, 0x01, 0x00, 0x02, 0x68, 0xce,
    ];
    let avc_bframe = [
        0x01_u8, 0x00, 0x00, 0x50, // AVCPacketType = 1, CTO = +80ms
        0x00, 0x00, 0x00, 0x02, 0x41, 0x9a,
    ];
    let avc_stream = build_cmedia_video_stream_with_audio(
        3,
        7,
        8_000.0,
        320,
        240,
        1.0,
        &[(0, 1, &avc_seq), (1, 0, &avc_bframe)],
        &[(1, 0, &[0xff, 0xf3, 0x40, 0xc4, 0x00])], // MPEG-2 8kHz MP3
        None,
    );
    let avc_dec = decode_binary_video(&avc_stream).expect("decode AVC + 8kHz MP3");
    let BinaryVideoPayload::Container { bytes: avc_flv, .. } = avc_dec.payload else {
        panic!("expected muxed AVC FLV");
    };
    let mut p = 9usize;
    let mut avc_tags = Vec::new();
    let mut mp3_8k_hdr = None;
    while p < avc_flv.len() {
        p += 4;
        if p == avc_flv.len() {
            break;
        }
        let tt = avc_flv[p];
        let dsz = (usize::from(avc_flv[p + 1]) << 16)
            | (usize::from(avc_flv[p + 2]) << 8)
            | usize::from(avc_flv[p + 3]);
        let ts = (u32::from(avc_flv[p + 7]) << 24)
            | (u32::from(avc_flv[p + 4]) << 16)
            | (u32::from(avc_flv[p + 5]) << 8)
            | u32::from(avc_flv[p + 6]);
        let body = &avc_flv[p + 11..p + 11 + dsz];
        if tt == 9 {
            avc_tags.push((ts, body.to_vec()));
        } else if tt == 8 {
            mp3_8k_hdr = Some(body[0]);
        }
        p += 11 + dsz;
    }
    assert_eq!(avc_tags[0].0, 0, "0-frame config header keeps timestamp 0");
    assert_eq!(
        avc_tags[1].0, 0,
        "first coded frame after 0-frame config header also starts at 0ms"
    );
    assert_eq!(
        &avc_tags[0].1[..5],
        &[0x17, 0x00, 0x00, 0x00, 0x00],
        "AVC seq header must not be double-wrapped"
    );
    assert_eq!(
        &avc_tags[1].1[..5],
        &[0x27, 0x01, 0x00, 0x00, 0x50],
        "AVC B-frame CTO (+80ms) must be preserved"
    );
    assert_eq!(
        mp3_8k_hdr.unwrap() >> 4,
        14,
        "8kHz MP3 must use SoundFormat = 14"
    );

    // 4. Non-FLV 48kHz PCM resamples to 44.1kHz 16-bit LE (`SoundFormat = 3, SoundRate = 3`).
    let pcm_48k: Vec<u8> = (0..480_i16).flat_map(|i| (i * 32).to_le_bytes()).collect();
    let pcm_stream = build_cmedia_video_stream_with_audio(
        2,
        2,
        48_000.0,
        160,
        120,
        0.04,
        &[(1, 1, &[0x00, 0x00, 0x80, 0x02])],
        &[(1, 0, &pcm_48k)],
        None,
    );
    let pcm_dec = decode_binary_video(&pcm_stream).expect("decode 48kHz PCM video");
    let BinaryVideoPayload::Container { bytes: pcm_flv, .. } = pcm_dec.payload else {
        panic!("expected FLV");
    };
    assert!(
        !neoflash_core::inspect_video_resource("vid", "video/x-flv", &pcm_flv, None)
            .iter()
            .any(|d| d.is_error)
    );

    // 5. Negative `frame_raw < 0` is rejected with `fla_video_index`.
    let bad_stream = build_cmedia_video_stream_with_audio(
        2,
        4,
        44_100.0,
        160,
        120,
        1.0,
        &[(-1, 1, &vp6_kf)],
        &[],
        None,
    );
    let err = decode_binary_video(&bad_stream).expect_err("negative frame_raw must fail");
    assert_eq!(err.code, "fla_video_index");

    // 6. Empty non-zero `video_type` (`video_type == 4` with 0 packets) flags UnsupportedCodec.
    let zero_pkt_vp6 =
        build_cmedia_video_stream_with_audio(2, 4, 44_100.0, 160, 120, 1.0, &[], &[], None);
    let zero_dec = decode_binary_video(&zero_pkt_vp6).expect("decode 0-packet VP6 header");
    assert!(matches!(
        zero_dec.payload,
        BinaryVideoPayload::UnsupportedCodec(_)
    ));
}

#[test]
fn cpic_text_discloses_metrics_clamped_negative_leading_tight_fixed_wrap_and_multi_run_bitmap_aa() {
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
    let mut push_utf16 = |s: &str| {
        let start = stream.original_bytes.len();
        stream
            .original_bytes
            .extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        let span = TimelineByteSpan {
            start,
            end: stream.original_bytes.len(),
        };
        TimelineString {
            encoded: span,
            data: span,
            utf16: true,
        }
    };
    let font = push_utf16("Verdana");
    let dormant_var = push_utf16("legacy_unused_var");
    let body_a = push_utf16("Tight ");
    let body_b = push_utf16("wrapped text");
    let dyn_body = push_utf16("Line 1\rLine 2");
    let static_body = push_utf16("First\rSecond\r\nThird");
    let aa_readability_start = stream.original_bytes.len();
    stream
        .original_bytes
        .extend_from_slice(&[2, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let aa_readability = TimelineByteSpan {
        start: aa_readability_start,
        end: stream.original_bytes.len(),
    };

    // A. Static fixed-width (`auto_expand == 0`) box with tight height (14px < 1.8 * 10pt),
    //    negative leading (-14pt on 10pt font), dormant variable, and bitmap_rendering on run 2!
    let style_run1 = TimelineTextStyle {
        revision: 10,
        font_face: Some(font.clone()),
        font_size_twips: Some(200), // 10 pt
        color_rgba: Some(0xff112233),
        line_spacing: Some(-280), // -14 pt -> clamped with fla_text_approximated
        ..Default::default()
    };
    let style_run2 = TimelineTextStyle {
        bitmap_rendering: Some(1),
        ..style_run1.clone()
    };
    let static_text = TimelineText {
        revision: Some(10),
        matrix_words: Some([65536, 0, 0, 65536, 200, 400]),
        bounds_words: Some([0, 2400, 0, 280]), // 120 x 14 px (< 10 * 1.8)
        auto_expand: Some(0),
        flags: Some(0x0080),
        variable_name: Some(dormant_var),
        runs: vec![
            TimelineTextRun {
                characters: body_a,
                style: style_run1.clone(),
            },
            TimelineTextRun {
                characters: body_b,
                style: style_run2,
            },
        ],
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
        rep.diagnostics.iter().any(|d| d.code == "fla_text_metrics"),
        "must emit honest fla_text_metrics: {:?}",
        rep.diagnostics
    );
    let approx = rep
        .diagnostics
        .iter()
        .find(|d| d.code == "fla_text_approximated")
        .expect("clamped negative leading, variable binding & run-2 bitmap AA must emit fla_text_approximated");
    assert!(approx.message.contains("clamped"));
    assert!(approx.message.contains("variable binding"));
    assert!(approx.message.contains("bitmap-aliased"));

    let ElementKind::TextBox {
        format,
        word_wrap,
        multiline,
        ..
    } = &el.kind
    else {
        panic!("expected TextBox");
    };
    assert!(
        *word_wrap && *multiline,
        "horizontal fixed-width Static Text must wrap even when height < 1.8 * size"
    );
    assert!(format.validate().is_ok());

    // B. Dynamic text with aa_mode == 1 ("advanced") -> maps anti_alias_type = "advanced".
    let dyn_style = TimelineTextStyle {
        revision: 11,
        line_spacing: Some(40),
        anti_alias: Some(aa_readability),
        ..style_run1
    };
    let dyn_text = TimelineText {
        flags: Some(0x0001 | 0x0002 | 0x0010 | 0x0008),
        auto_expand: Some(0),
        variable_name: None,
        runs: vec![TimelineTextRun {
            characters: dyn_body,
            style: dyn_style,
        }],
        ..static_text
    };
    let mut dyn_rep = empty_report();
    let dyn_el = convert_timeline_text(&stream, &dyn_text, "txt-dyn", "", Some(0x3f), &mut dyn_rep)
        .expect("convert dynamic text");
    let dyn_state = TextFieldState::from_kind(&dyn_el.kind).expect("dynamic text state");
    assert_eq!(dyn_state.field_type, TextFieldType::Dynamic);
    assert_eq!(dyn_state.anti_alias_type, "advanced");

    // A static field has the same archived readability mode. The editable project
    // and its XFL export should contain the same line boundaries.
    let read_text = TimelineText {
        flags: Some(0),
        runs: vec![TimelineTextRun {
            characters: static_body,
            style: dyn_text.runs[0].style.clone(),
        }],
        ..dyn_text
    };
    let static_el = convert_timeline_text(
        &stream,
        &read_text,
        "static-readability",
        "",
        Some(0x3f),
        &mut empty_report(),
    )
    .unwrap();
    let ElementKind::TextBox { text, .. } = &static_el.kind else {
        panic!("expected an editable text box");
    };
    assert_eq!(text, "First\nSecond\nThird");
    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![static_el];
    let bytes = crate::export_fla_bytes(&project).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("DOMDocument.xml").unwrap(), &mut xml)
        .unwrap();
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let field = dom
        .descendants()
        .find(|n| n.has_tag_name("DOMStaticText"))
        .unwrap();
    assert_eq!(field.attribute("fontRenderingMode"), Some("advanced"));
    let chars = field
        .descendants()
        .find(|n| n.has_tag_name("characters"))
        .unwrap();
    // XFL encodes each Flash paragraph break as a carriage-return reference.
    assert_eq!(chars.text(), Some("First\rSecond\rThird"));
}

#[test]
fn component_props_only_attach_to_implemented_avm1_movieclips_and_broken_tweens_warn() {
    fn push_str(bytes: &mut Vec<u8>, s: &str) -> TimelineString {
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
    }
    let mut bytes = Vec::new();
    let prop_autoplay = push_str(&mut bytes, "autoPlay");
    let prop_val_true = push_str(&mut bytes, "true");
    let prop_volume = push_str(&mut bytes, "volume");
    let prop_val_vol = push_str(&mut bytes, "0.75");
    let prop_tint = push_str(&mut bytes, "tint");
    let prop_val_tint = push_str(&mut bytes, "#ff3366");
    let prop_items = push_str(&mut bytes, "items");
    let prop_item_a = push_str(&mut bytes, "alpha");
    let prop_item_b = push_str(&mut bytes, "beta");
    let prop_preview = push_str(&mut bytes, "preview");
    let prop_preview_blob = push_str(&mut bytes, "ignored");

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
            mk_prop(prop_autoplay, 7, vec![prop_val_true]),
            mk_prop(prop_volume, 6, vec![prop_val_vol]),
            mk_prop(prop_tint, 9, vec![prop_val_tint]),
            mk_prop(prop_items, 2, vec![prop_item_a, prop_item_b]),
            mk_prop(prop_preview, 19, vec![prop_preview_blob]),
        ],
        parameters_are_new: None,
    };

    // 1. Graphic instance (TimelineClass::Symbol) & AS3 MovieClip must NEVER get ObjectScript;
    //    both emit fla_instance_settings!
    let (_, lossy_graphic) = instance_props::translate_component_properties(
        &bytes,
        Some(&comp_props),
        Some(comp_props.entries.len() as u32),
        ScriptLanguage::ActionScript2,
        false,
    );
    assert!(lossy_graphic);
    let (as3_script, lossy_as3) = instance_props::translate_component_properties(
        &bytes,
        Some(&comp_props),
        Some(comp_props.entries.len() as u32),
        ScriptLanguage::ActionScript3,
        true,
    );
    assert!(as3_script.is_none() && lossy_as3);

    // 2. Truncated 8-digit ARGB color, negative selected_index, and non-empty cue points set lossy!
    let bad_color_str = push_str(&mut bytes, "#ff112233");
    let bad_color_prop = mk_prop(push_str(&mut bytes, "badColor"), 9, vec![bad_color_str]);
    let one_str = push_str(&mut bytes, "one");
    let mut neg_list_prop = mk_prop(push_str(&mut bytes, "choice"), 4, vec![one_str]);
    neg_list_prop.value.as_mut().unwrap().selected_index = Some((-1_i32) as u32);
    let intro_str = push_str(&mut bytes, "intro");
    let cue_prop = mk_prop(push_str(&mut bytes, "cuePoints"), 16, vec![intro_str]);
    let bad_props = TimelineComponentProperties {
        encoded: TimelineByteSpan { start: 0, end: 1 },
        revision: 1,
        entries: vec![bad_color_prop, neg_list_prop, cue_prop],
        parameters_are_new: None,
    };
    let (bad_script, bad_lossy) = instance_props::translate_component_properties(
        &bytes,
        Some(&bad_props),
        Some(3),
        ScriptLanguage::ActionScript2,
        true,
    );
    assert!(bad_script.is_none() && bad_lossy);

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

    let txt_name = push_str(&mut bytes, "label");
    let txt_font = push_str(&mut bytes, "Arial");
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

    // Frame 0: Sprite (MovieClip) instance of implemented lib 101 (gets ObjectEvent::Load)
    // Frame 4: [Graphic instance of lib 100 with LoopFrom(11), helper TextBox] (1 -> 2 broken tween!)
    // Frame 5: Blank keyframe [] (non-final empty-endpoint tween!)
    // Frame 6: Graphic instance of lib 100 with LoopFrom(8) (exact multiple 8 % 4 == 0)
    let inst_f0 = TimelineInstance {
        revision: Some(23),
        library_id: Some(101),
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
        matrix_words: Some([65536, 0, 0, 65536, 1600, 0]),
        bitmap_render_mode: Some(1),
        is_visible: Some(1),
        ..Default::default()
    };
    let inst_f6 = TimelineInstance {
        revision: Some(23),
        loop_mode: Some(0),
        first_frame: Some(8),
        library_id: Some(100),
        matrix_words: Some([65536, 0, 0, 65536, 0, 0]),
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
                &[10, 11, 12, 13],
            ),
            mk_obj(
                10,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(4),
                    key_mode: Some(1),
                    ..Default::default()
                })),
                &[20],
            ),
            mk_obj(
                11,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(1),
                    key_mode: Some(1),
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
                13,
                TimelineClass::Frame,
                TimelinePayload::Frame(Box::new(TimelineFrame {
                    duration: Some(1),
                    key_mode: Some(0),
                    ..Default::default()
                })),
                &[23],
            ),
            mk_obj(
                20,
                TimelineClass::Sprite,
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
            mk_obj(
                23,
                TimelineClass::Symbol,
                TimelinePayload::Instance(Box::new(inst_f6)),
                &[],
            ),
        ],
        shapes: vec![],
        original_bytes: bytes,
    };

    let mut rep = empty_report();
    let mut ctx = Context {
        report: &mut rep,
        library: BTreeMap::from([(100, "sym-gr".into()), (101, "sym-comp".into())]),
        library_kinds: BTreeMap::from([(100, SymbolKind::Graphic), (101, SymbolKind::MovieClip)]),
        implemented_symbols: BTreeSet::from(["sym-comp".into()]),
        scene_stream_indices: BTreeSet::from([0]),
        bitmaps: BTreeMap::new(),
        sounds: BTreeMap::new(),
        videos: BTreeMap::new(),
        compiled_clips: BTreeMap::new(),
        skipped_bitmaps: BTreeSet::new(),
        stream_stops: BTreeSet::new(),
        language: ScriptLanguage::ActionScript2,
        include_hidden: true,
        motion_pose_budget: crate::motion_xml::DOCUMENT_POSE_BUDGET,
        contents_revision: Some(0x3f),
        motion_3d: ([550.0, 400.0], None, None),
        generated: vec![],
        inline_resources: vec![],
        compiled_clip_sources: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let timeline = ctx.timeline(&stream).expect("convert timeline");
    let dropped_count = rep
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_tween_dropped")
        .count();
    assert_eq!(
        dropped_count, 2,
        "both 1->2 multi-element tween and non-final blank-endpoint tween must emit fla_tween_dropped: {:?}",
        rep.diagnostics
    );

    let layer = &timeline.layers[0];
    assert!(layer.keyframes[0].tween.is_none());
    assert!(layer.keyframes[1].tween.is_none());

    let f0_el = &layer.keyframes[0].elements[0];
    assert!(!f0_el.visible);
    assert!(!f0_el.cache_as_bitmap);
    assert_eq!(f0_el.scripts.len(), 1);
    let script_src = &f0_el.scripts[0].script.source;
    assert!(script_src.contains("this.autoPlay = true;"));
    assert!(script_src.contains("this.volume = 0.75;"));
    assert!(script_src.contains("this.tint = 0xff3366;"));
    assert!(script_src.contains("this.items = [\"alpha\", \"beta\"];"));

    let mut symbols = vec![
        Symbol {
            id: "sym-gr".into(),
            name: "GR".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
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
    symbols[1].timeline.layers[0].keyframes.push(Keyframe {
        frame: 8,
        elements: vec![
            Element {
                id: "sf-over".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-gr".into(),
                    playback: InstancePlayback::SingleFrame(11),
                },
                ..Element::default()
            },
            Element {
                id: "po-over".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-gr".into(),
                    playback: InstancePlayback::PlayOnceFrom(11),
                },
                ..Element::default()
            },
        ],
        tween: None,
        label: String::new(),
        ..Default::default()
    });
    normalize_playback(&mut symbols, &mut rep);
    let ff_warns: Vec<_> = rep
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_graphic_first_frame")
        .collect();
    assert_eq!(
        ff_warns.len(),
        2,
        "LoopFrom(11) and LoopFrom(8) normalize losslessly without fla_graphic_first_frame, while SingleFrame(11) and PlayOnceFrom(11) clamp with fla_graphic_first_frame: {:?}",
        rep.diagnostics
    );
    assert_eq!(
        symbols[1].timeline.layers[0].keyframes[1].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "sym-gr".into(),
            playback: InstancePlayback::LoopFrom(3),
        }
    );
    assert_eq!(
        symbols[1].timeline.layers[0].keyframes[3].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "sym-gr".into(),
            playback: InstancePlayback::Loop,
        }
    );
    assert_eq!(
        symbols[1].timeline.layers[0].keyframes[4].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "sym-gr".into(),
            playback: InstancePlayback::SingleFrame(3),
        }
    );
    assert_eq!(
        symbols[1].timeline.layers[0].keyframes[4].elements[1].kind,
        ElementKind::Instance {
            symbol_id: "sym-gr".into(),
            playback: InstancePlayback::PlayOnceFrom(3),
        }
    );
}

#[test]
fn deduplicate_tweened_group_endpoint_removes_non_tail_symbol_and_remaps_provenance() {
    let mk_group = |id: &str| {
        group_symbol(
            id,
            id.into(),
            vec![Element {
                id: format!("{}-member-0", id.strip_suffix("-group").unwrap()),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 20.0,
                    fill: [255, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Default::default()
            }],
        )
    };
    let mut generated = vec![
        mk_group("fla-0-2-10-group"),
        mk_group("fla-0-2-20-group"),
        group_symbol("fla-unsupported-object", "Placeholder".into(), vec![]),
    ];
    let mut provenance = vec![
        serde_json::json!({"stream": 0, "object": 21, "target": "fla-0-2-20-member-0"}),
        serde_json::json!({"stream": 0, "object": 20, "target": "fla-0-2-20-paint"}),
    ];
    let prev_el = Element {
        id: "fla-0-2-10".into(),
        kind: ElementKind::Instance {
            symbol_id: "fla-0-2-10-group".into(),
            playback: InstancePlayback::SingleFrame(0),
        },
        ..Default::default()
    };
    let mut curr_el = Element {
        id: "fla-0-2-20".into(),
        kind: ElementKind::Instance {
            symbol_id: "fla-0-2-20-group".into(),
            playback: InstancePlayback::SingleFrame(0),
        },
        ..Default::default()
    };
    deduplicate_tweened_group_endpoint(&mut generated, &mut provenance, &prev_el, &mut curr_el);
    assert!(
        !generated.iter().any(|s| s.id == "fla-0-2-20-group"),
        "old_b must be removed by ID even when not last in generated"
    );
    assert_eq!(provenance[0]["target"], "fla-0-2-10-member-0");
    assert_eq!(provenance[1]["target"], "fla-0-2-10-paint");
}

#[test]
fn stroke_outline_expansion_caps_at_max_path_commands_and_dynamic_scale_mode_warns() {
    use crate::legacy::geometry::{
        BinaryContourStyles, BinaryEdge, BinaryFill, BinaryFillEncoding, BinaryFillPayload,
        BinaryShapeRecord, BinaryStroke, BinaryStrokeExtension,
    };

    let solid_paint = BinaryFill {
        packed_value: 0xff224488,
        encoding: BinaryFillEncoding::Extended {
            flags: 0,
            secondary_flags: 0,
            payload: BinaryFillPayload::Plain,
        },
    };
    // Butt cap (`cap_style = 1`) + non-normal `scale_mode = 1` (`None`) with 4,000 curved edges.
    let stroke = BinaryStroke {
        color_word: 0xff224488,
        width_word: 80, // 4 px
        style_words: [0, 0],
        extension: Some(BinaryStrokeExtension {
            pixel_hinting: 0,
            scale_mode: 1, // StrokeScaleMode::None
            cap_style: 0,  // Butt cap -> triggers expand_stroke_to_outline
            join_style: 0,
            miter_limit_word: 0x0300,
            paint: solid_paint,
        }),
    };
    let mut edges = Vec::with_capacity(4_000);
    for i in 0..4_000_i32 {
        let x0 = i * 40;
        let x1 = (i + 1) * 40;
        let cy = if i % 2 == 0 { 200 } else { -200 };
        edges.push(BinaryEdge {
            encoding_flags: 0,
            styles: BinaryContourStyles {
                stroke: 1,
                fill_0: 0,
                fill_1: 0,
            },
            start_delta: [0, 0],
            control_delta: [20, cy],
            end_delta: [20, -cy],
            start: [x0, 0],
            control: Some([x0 + 20, cy]),
            end: [x1, 0],
            line_flags: None,
        });
    }
    let rec = BinaryShapeRecord {
        shape_revision: 5,
        matrix_words: [65536, 0, 0, 65536, 0, 0],
        data_revision: 5,
        metadata: 0,
        fills: vec![],
        strokes: vec![stroke],
        edges,
        auxiliary: vec![],
        original_bytes: vec![],
    };

    let mut rep = empty_report();
    let converted = shape::convert_shape_with_effects_opts(
        &rec,
        "big-stroke",
        &BTreeMap::new(),
        &mut rep,
        "stream #1 frame 0",
        true,
        true,
    )
    .expect("convert curved butt-cap stroke exceeding MAX_PATH_COMMANDS");
    assert!(!converted.elements.is_empty());
    for el in &converted.elements {
        match &el.kind {
            ElementKind::PaintedPath { commands, .. } | ElementKind::Path { commands, .. } => {
                assert!(
                    commands.len() <= MAX_PATH_COMMANDS,
                    "element exceeded MAX_PATH_COMMANDS: {}",
                    commands.len()
                );
            }
            other => panic!("unexpected element kind {other:?}"),
        }
    }
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_stroke_rendering"),
        "must emit fla_stroke_rendering when outline cap falls back or scale_mode != Normal"
    );

    // Also verify a small round-cap non-Normal `scale_mode` stroke inside a symbol (`dynamic_scale_possible = true`)
    // emits `fla_stroke_rendering`, while a static scene stroke (`dynamic_scale_possible = false`) does not.
    let mut round_rec = rec;
    round_rec.edges.truncate(2);
    round_rec.strokes[0].extension.as_mut().unwrap().cap_style = 1; // Round
    let mut rep_sym = empty_report();
    shape::convert_shape_with_effects_opts(
        &round_rec,
        "sym-stroke",
        &BTreeMap::new(),
        &mut rep_sym,
        "stream #2 frame 0",
        true,
        true,
    )
    .unwrap();
    assert!(
        rep_sym
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_stroke_rendering")
    );

    let mut rep_scene = empty_report();
    shape::convert_shape_with_effects_opts(
        &round_rec,
        "scene-stroke",
        &BTreeMap::new(),
        &mut rep_scene,
        "stream #0 frame 0",
        true,
        false,
    )
    .unwrap();
    assert!(
        !rep_scene
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_stroke_rendering")
    );
}

#[test]
fn pressure_compaction_warns_on_jpeg_reencode_preserves_linked_sounds_and_sheds_video_budget() {
    use image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};

    // 1. Opaque noisy PNG (> 128 KiB even after Best PNG) re-encodes to JPEG with `fla_bitmap_reencoded`
    //    and deduplicates `{stem}-2.jpg` when `{stem}.jpg` is already taken!
    let w = 256_u32;
    let h = 256_u32;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let mut state = 0x1234_5678_u32;
    for _ in 0..(w * h) {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        rgba.push((state & 0xff) as u8);
        rgba.push(((state >> 8) & 0xff) as u8);
        rgba.push(((state >> 16) & 0xff) as u8);
        rgba.push(255);
    }
    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, w, h, ExtendedColorType::Rgba8)
        .unwrap();
    let res = ResourceUnit::from_bytes(
        "legacy-bitmap-4",
        "legacy/bitmaps/4/photo.png",
        "image/png",
        &png_bytes,
    )
    .unwrap();
    let mut resources = vec![res];
    let mut used_paths = BTreeSet::from([
        "legacy/bitmaps/4/photo.png".to_owned(),
        "legacy/bitmaps/4/photo.jpg".to_owned(), // force collision -> photo-2.jpg
    ]);
    let mut total = MAX_RESOURCE_TOTAL_BYTES;
    let mut rep = empty_report();
    assets::compact_bitmaps_under_pressure(
        &mut Default::default(),
        &mut resources,
        &mut used_paths,
        &mut total,
        1024 * 1024,
        &mut rep,
    );
    assert_eq!(resources[0].media_type, "image/jpeg");
    assert_eq!(
        resources[0].path, "legacy/bitmaps/4/photo-2.jpg",
        "JPEG re-encode must deduplicate .jpg path instead of keeping .png"
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_bitmap_reencoded"),
        "expected fla_bitmap_reencoded warning: {:?}",
        rep.diagnostics
    );

    // 2. Distinct linked sounds (`ClickA`, `ClickB`) and unlinked sound (`legacy-sound-1`) with identical bytes:
    //    `ClickA` upgrades `legacy-sound-1`, and `ClickB` stays distinct!
    let wav = [
        b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0,
        0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a',
        8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut snd_resources = vec![
        ResourceUnit::from_bytes("legacy-sound-1", "legacy/sounds/1/a.wav", "audio/wav", &wav)
            .unwrap(),
        ResourceUnit::from_bytes("ClickA", "legacy/sounds/2/b.wav", "audio/wav", &wav).unwrap(),
        ResourceUnit::from_bytes("ClickB", "legacy/sounds/3/c.wav", "audio/wav", &wav).unwrap(),
    ];
    let mut sounds = BTreeMap::from([
        (
            1_u16,
            assets::SoundAsset {
                resource_id: "legacy-sound-1".into(),
                duration_44: 4,
            },
        ),
        (
            2_u16,
            assets::SoundAsset {
                resource_id: "ClickA".into(),
                duration_44: 4,
            },
        ),
        (
            3_u16,
            assets::SoundAsset {
                resource_id: "ClickB".into(),
                duration_44: 4,
            },
        ),
    ]);
    let mut s_paths = BTreeSet::from([
        "legacy/sounds/1/a.wav".to_owned(),
        "legacy/sounds/2/b.wav".to_owned(),
        "legacy/sounds/3/c.wav".to_owned(),
    ]);
    let mut s_total = wav.len() * 3;
    assets::compact_duplicate_media_under_pressure(
        &mut snd_resources,
        &mut BTreeMap::new(),
        &mut sounds,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
        &mut s_paths,
        &mut s_total,
    );
    assert!(snd_resources.iter().any(|r| r.id == "ClickA"));
    assert!(snd_resources.iter().any(|r| r.id == "ClickB"));
    assert_eq!(sounds[&2].resource_id, "ClickA");
    assert_eq!(sounds[&3].resource_id, "ClickB");

    // 3. `shed_video_resources` drops `legacy-video-*`, clears `Keyframe.video`, and warns `fla_media_not_imported`.
    let mut proj = Project::blank();
    proj.resources.push(
        ResourceUnit::from_bytes(
            "legacy-video-9",
            "legacy/videos/9/big.flv",
            "video/x-flv",
            b"FLV\x01\x01\x00\x00\x00\x09\x00\x00\x00\x00",
        )
        .unwrap(),
    );
    proj.symbols[0].timeline.layers[0].keyframes[0].video = Some(TimelineVideoSpan {
        resource: "legacy-video-9".into(),
        ..Default::default()
    });
    let mut v_rep = empty_report();
    assert!(shed_video_resources(&mut proj, &mut v_rep, 16));
    assert!(proj.resource("legacy-video-9").is_none());
    assert!(
        proj.symbols[0].timeline.layers[0].keyframes[0]
            .video
            .is_none()
    );
    assert!(
        v_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_media_not_imported" && d.location == "media 9")
    );
    // Actual compact-JSON exhaustion still removes the largest audio first and
    // clears only keys that use it, with separate located diagnostics.
    for (id, size) in [(1, 32), (4, 256)] {
        proj.resources.push(
            ResourceUnit::from_bytes(
                format!("legacy-sound-{id}"),
                format!("legacy/sounds/{id}/clip.wav"),
                "audio/wav",
                &vec![0; size],
            )
            .unwrap(),
        );
    }
    proj.symbols[0].timeline.layers[0].keyframes[0].sound = Some(FrameSound::new("legacy-sound-4"));
    assert!(shed_audio_resources(&mut proj, &mut v_rep, 100));
    assert!(proj.resource("legacy-sound-4").is_none() && proj.resource("legacy-sound-1").is_some());
    assert!(
        proj.symbols[0].timeline.layers[0].keyframes[0]
            .sound
            .is_none()
    );
    assert!(
        v_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_media_not_imported" && d.location == "media 4")
    );
    assert!(
        v_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_frame_sound" && d.message.contains("media #4"))
    );
}
