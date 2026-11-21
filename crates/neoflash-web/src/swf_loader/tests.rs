//! Unit tests for `swf_loader` timeline OOM budgets, JS literal escaping, sandbox-hardened codegen, and actionable warnings.

use super::vm::js_string_literal;
use super::{MAX_SWF_KEYFRAMES, decode_swf_bundle};
use std::time::Instant;

fn make_tag(code: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len());
    if payload.len() < 0x3F {
        let hdr = (code << 6) | (payload.len() as u16);
        out.extend_from_slice(&hdr.to_le_bytes());
    } else {
        let hdr = (code << 6) | 0x3F;
        out.extend_from_slice(&hdr.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn wrap_fws(version: u8, frame_count: u16, tags: &[Vec<u8>]) -> Vec<u8> {
    // RECT(0, 2000, 0, 2000) with nbits = 13:
    // 5 + 4*13 = 57 bits -> 8 bytes; simpler: nbits = 0 -> 1 byte `0x00` (clamped to 1x1 px in decoder)
    let mut body = vec![0x00];
    body.extend_from_slice(&24u16.wrapping_shl(8).to_le_bytes());
    body.extend_from_slice(&frame_count.to_le_bytes());
    for t in tags {
        body.extend_from_slice(t);
    }
    body.extend_from_slice(&[0x00, 0x00]); // End tag
    let total_len = (8 + body.len()) as u32;
    let mut swf = Vec::with_capacity(total_len as usize);
    swf.extend_from_slice(b"FWS");
    swf.push(version);
    swf.extend_from_slice(&total_len.to_le_bytes());
    swf.extend_from_slice(&body);
    swf
}

fn minimal_define_shape_tag(char_id: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&char_id.to_le_bytes());
    p.push(0x00); // empty RECT (nbits = 0)
    p.push(0x00); // 0 fill styles
    p.push(0x00); // 0 line styles
    p.push(0x00); // numFillBits = 0, numLineBits = 0
    p.push(0x00); // EndShapeRecord
    make_tag(2, &p)
}

fn minimal_place_object2_tag(depth: u16, char_id: u16) -> Vec<u8> {
    let mut p = Vec::with_capacity(6);
    p.push(0x02); // HasCharacter only (no matrix)
    p.extend_from_slice(&depth.to_le_bytes());
    p.extend_from_slice(&char_id.to_le_bytes());
    make_tag(26, &p)
}

#[test]
fn swf_loader_bounds_keyframes_without_oom_on_thousands_of_placements_and_4096_frames() {
    let mut tags = Vec::with_capacity(8_010);
    tags.push(minimal_define_shape_tag(1));
    for depth in 1..=8_000u16 {
        tags.push(minimal_place_object2_tag(depth, 1));
    }
    tags.push(make_tag(1, &[])); // 1 ShowFrame, header declares 4096 frames!
    // Add 4095 empty ShowFrame tags as well to verify unchanged frames never clone BTreeMap
    for _ in 1..4096 {
        tags.push(make_tag(1, &[]));
    }

    let swf = wrap_fws(8, 4096, &tags);
    let started = Instant::now();
    let bundle = decode_swf_bundle(&swf).expect("decode 8000-depth x 4096-frame SWF cleanly");
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 2.0,
        "expected fast O(changes) timeline decode, took {elapsed:?}"
    );
    assert_eq!(bundle.swf_info.frame_count, 4096);
    let root_sym = bundle
        .project
        .symbols
        .iter()
        .find(|s| s.id == bundle.project.root)
        .expect("root symbol");
    assert_eq!(root_sym.timeline.frame_count, 4096);
    assert_eq!(root_sym.timeline.layers.len(), 8_000);
    let total_kfs: usize = bundle
        .project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .map(|l| l.keyframes.len())
        .sum();
    assert!(
        total_kfs <= MAX_SWF_KEYFRAMES + 16,
        "expected bounded keyframes <= {MAX_SWF_KEYFRAMES}, got {total_kfs}"
    );
    // Each unchanged depth must hold a single keyframe at frame 0 across all 4096 frames
    assert!(
        root_sym
            .timeline
            .layers
            .iter()
            .all(|l| l.keyframes.len() == 1 && l.keyframes[0].frame == 0)
    );
}

#[test]
fn js_string_literal_escapes_nul_digits_and_line_separators_for_strict_mode() {
    let s1 = js_string_literal("\x001");
    assert_eq!(s1, "\"\\u00001\"");
    assert!(!s1.contains("\\0"));

    let s8 = js_string_literal("\x008");
    assert_eq!(s8, "\"\\u00008\"");

    let sep = js_string_literal("a\u{2028}b\u{2029}c");
    assert_eq!(sep, "\"a\\u2028b\\u2029c\"");
}

#[test]
fn swf_loader_emits_deduplicated_warnings_and_never_emits_unguarded_js_member_access() {
    // DoAction with GetMember (0x4E), SetMember (0x4F), GetURL (0x83), Try (0x8F), and unknown 0x7F
    let mut do_action = Vec::new();
    // Push "obj", "prop" -> GetMember
    do_action.extend_from_slice(&[0x96, 10, 0, 0, b'o', b'b', b'j', 0, 0, b'p', b'r', b'o', 0]);
    do_action.push(0x4E);
    // GetURL("https://example.com", "_blank") -> 0x83
    do_action.extend_from_slice(&[0x83, 4, 0, b'a', 0, b'b', 0]);
    // Duplicate GetURL to verify warning deduplication
    do_action.extend_from_slice(&[0x83, 4, 0, b'a', 0, b'b', 0]);
    // Unknown opcode 0x7F
    do_action.push(0x7F);
    do_action.push(0x00);

    // PlaceObject3 with blendMode = 3 (multiply)
    let place3 = make_tag(70, &[0x02, 0x02, 1, 0, 1, 0, 3]);
    let sym_class = make_tag(76, &[1, 0, 1, 0, b'C', b'h', b'i', b'l', b'd', 0]);

    let swf = wrap_fws(
        9,
        1,
        &[
            minimal_define_shape_tag(1),
            place3,
            make_tag(12, &do_action),
            sym_class,
            make_tag(1, &[]),
        ],
    );
    let bundle = decode_swf_bundle(&swf).expect("decode SWF with unsupported opcodes/features");
    assert!(
        bundle.warnings.iter().any(|w| w.contains("GetURL (0x83)")),
        "expected GetURL warning, got {:?}",
        bundle.warnings
    );
    assert_eq!(
        bundle
            .warnings
            .iter()
            .filter(|w| w.contains("GetURL (0x83)"))
            .count(),
        1,
        "warnings must be deduplicated"
    );
    assert!(bundle.warnings.iter().any(|w| w.contains("0x7F")));
    assert!(
        bundle
            .warnings
            .iter()
            .any(|w| w.contains("PlaceObject3 blendMode"))
    );
    assert!(
        bundle
            .warnings
            .iter()
            .any(|w| w.contains("SymbolClass binding 'Child'"))
    );

    for unit in &bundle.scripts {
        assert!(!unit.code.contains("__o[__m]"));
        assert!(!unit.code.contains(".apply("));
        assert!(!unit.code.contains("function(){}"));
        assert!(unit.code.contains("__nfSwfGet"));
    }
}
