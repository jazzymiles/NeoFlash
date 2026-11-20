use super::*;

#[test]
fn fte_paragraph_layout_justification_tabs_and_boundaries() {
    let req = FteLayoutRequest {
        runs: vec![
            FteInputElementRun {
                element_id: 1,
                is_graphic: false,
                text: "Hello\tworld from FTE!".into(),
                graphic_width: 0.0,
                graphic_height: 0.0,
                font_name: "_sans".into(),
                font_size: 16.0,
                font_weight: "normal".into(),
                font_posture: "normal".into(),
                color: 0x112233,
                alpha: 1.0,
                tracking_left: 0.0,
                tracking_right: 0.0,
                baseline_shift: 0.0,
                text_rotation: "auto".into(),
                break_opportunity: "auto".into(),
                digit_width: "default".into(),
                typographic_case: "default".into(),
                has_mirror: true,
            },
            FteInputElementRun {
                element_id: 2,
                is_graphic: true,
                text: FTE_GRAPHIC_CHAR.to_string(),
                graphic_width: 24.0,
                graphic_height: 18.0,
                font_name: "_sans".into(),
                font_size: 12.0,
                font_weight: "normal".into(),
                font_posture: "normal".into(),
                color: 0,
                alpha: 1.0,
                tracking_left: 0.0,
                tracking_right: 0.0,
                baseline_shift: 0.0,
                text_rotation: "auto".into(),
                break_opportunity: "auto".into(),
                digit_width: "default".into(),
                typographic_case: "default".into(),
                has_mirror: false,
            },
        ],
        start_char: 0,
        max_width: 140.0,
        line_offset: 0.0,
        fit_something: false,
        tab_stops: vec![FteTabStopSpec {
            alignment: "start".into(),
            position: 72.0,
            decimal_alignment_token: String::new(),
        }],
        justifier: FteJustifierSpec {
            kind: "space".into(),
            locale: "en".into(),
            line_justification: "allIncludingLast".into(),
            letter_spacing: true,
            minimum_spacing: 0.5,
            optimum_spacing: 1.0,
            maximum_spacing: 2.0,
            justification_style: "pushInKinsoku".into(),
            compose_trailing_ideographic_spaces: false,
        },
        baseline_font_size: 12.0,
        baseline_zero: "roman".into(),
        bidi_level: 0,
        line_rotation: "rotate0".into(),
        apply_non_linear_font_scaling: true,
    };
    let resp1 = layout_text_line(&req).expect("first line layout");
    assert_eq!(resp1.creation_result, "success");
    let line1 = resp1.line.expect("line1 some");
    assert!(line1.atoms.len() > 5);
    assert_eq!(line1.specified_width, 140.0);
    assert!(!line1.mirror_regions.is_empty());
    assert!(line1.dump.starts_with("<TextLine"));

    let next_start = line1.text_block_begin_index + line1.raw_text_length;
    let mut req2 = req.clone();
    req2.start_char = next_start;
    let resp2 = layout_text_line(&req2).expect("second line layout");
    let line2 = resp2.line.expect("line2 some");
    assert!(line2.has_graphic_element || line2.raw_text_length > 0);

    let full_text = format!("Hello\tworld from FTE!{}", FTE_GRAPHIC_CHAR);
    let next_word = find_next_word_boundary(&full_text, 0).unwrap();
    assert_eq!(next_word, 5);
    let prev_word = find_previous_word_boundary(&full_text, 11).unwrap();
    assert_eq!(prev_word, 6);
}

#[test]
fn stage3d_agal_triangle_rasterizes_and_reads_backbuffer() {
    let mut arena = Stage3dArena::default();
    let ctx = arena.create_context("software", "baseline").unwrap();
    arena
        .configure_back_buffer(ctx, 64, 64, 0, true)
        .expect("configureBackBuffer");
    arena
        .clear(ctx, 0.0, 0.0, 0.0, 1.0, 1.0, 0, 7)
        .expect("clear");

    // Build AGAL v1 vertex shader: mov op, va0; mov v0, va1
    let mut vs = vec![0xA0, 1, 0, 0, 0, 0xA1, 0x00];
    vs.extend_from_slice(&[
        0x00, 0, 0, 0, 0, 0, 0x0F, 3, // op=mov, dst=op
        0, 0, 0, 0xE4, 0, 0, 0, 0, // src1=va0
        0, 0, 0, 0, 0, 0, 0, 0, // src2=unused
    ]);
    vs.extend_from_slice(&[
        0x00, 0, 0, 0, 0, 0, 0x0F, 4, // dst=v0
        1, 0, 0, 0xE4, 0, 0, 0, 0, // src1=va1
        0, 0, 0, 0, 0, 0, 0, 0,
    ]);

    // Build AGAL v1 fragment shader: mov oc, v0
    let mut fs = vec![0xA0, 1, 0, 0, 0, 0xA1, 0x01];
    fs.extend_from_slice(&[
        0x00, 0, 0, 0, 0, 0, 0x0F, 3, // dst=oc
        0, 0, 0, 0xE4, 4, 0, 0, 0, // src1=v0
        0, 0, 0, 0, 0, 0, 0, 0,
    ]);

    let prog = arena.create_program(ctx).unwrap();
    arena.upload_program(prog, &vs, &fs).unwrap();
    arena
        .set_state_cmd(ctx, "setProgram", &serde_json::json!({ "programId": prog }))
        .unwrap();

    let vb = arena.create_vertex_buffer(ctx, 3, 7, "staticDraw").unwrap();
    #[rustfmt::skip]
    let verts: Vec<f32> = vec![
        -0.8, -0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
         0.8, -0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
         0.0,  0.8, 0.5,   1.0, 0.0, 0.0, 1.0,
    ];
    arena.upload_vertex_buffer(vb, &verts, 0, 3).unwrap();
    arena
        .set_state_cmd(
            ctx,
            "setVertexBufferAt",
            &serde_json::json!({ "index": 0, "bufferId": vb, "bufferOffset": 0, "format": "float3" }),
        )
        .unwrap();
    arena
        .set_state_cmd(
            ctx,
            "setVertexBufferAt",
            &serde_json::json!({ "index": 1, "bufferId": vb, "bufferOffset": 3, "format": "float4" }),
        )
        .unwrap();

    let ib = arena.create_index_buffer(ctx, 3, "staticDraw").unwrap();
    arena.upload_index_buffer(ib, &[0, 1, 2], 0, 3).unwrap();

    arena.draw_triangles(ctx, ib, 0, 1).unwrap();
    arena.present(ctx).unwrap();
    let (w, h, rgba) = arena.backbuffer_rgba(ctx).unwrap();
    assert_eq!((w, h), (64, 64));
    let center = ((32 * 64 + 32) * 4) as usize;
    assert_eq!(&rgba[center..center + 4], &[255, 0, 0, 255]);
}

#[test]
fn pixel_bender_pbj_and_json_shaders_evaluate() {
    // Construct a real binary .pbj shader:
    // 0xA5 version 1, 0xA4 name "TintPbj", 0xA3 input 0 ch=4 "src",
    // 0xA1 outCoord reg=0 mask=0xC "_OutCoord",
    // 0xA1 output dst reg=1 mask=0xF "dst",
    // 0xA1 param float4 reg=2 mask=0xF "color",
    // 0xA2 defaultValue [0.5, 1.0, 0.25, 1.0],
    // 0x30 sampleNearest dst=1 mask=0xF coord=0 swz=(x,y) tex=0,
    // 0x03 mul dst=1 mask=0xF len=4 src=2 swz=(x,y,z,w)
    let mut pbj = Vec::new();
    pbj.push(0xA5);
    pbj.extend_from_slice(&1i32.to_le_bytes());
    pbj.push(0xA4);
    pbj.extend_from_slice(&(7u16).to_le_bytes());
    pbj.extend_from_slice(b"TintPbj");
    pbj.push(0xA3);
    pbj.extend_from_slice(&[0, 4]);
    pbj.extend_from_slice(b"src\0");
    pbj.push(0xA1);
    pbj.extend_from_slice(&[1, 0x02]);
    pbj.extend_from_slice(&0u16.to_le_bytes());
    pbj.push(0x0C);
    pbj.extend_from_slice(b"_OutCoord\0");
    pbj.push(0xA1);
    pbj.extend_from_slice(&[2, 0x04]);
    pbj.extend_from_slice(&1u16.to_le_bytes());
    pbj.push(0x0F);
    pbj.extend_from_slice(b"dst\0");
    pbj.push(0xA1);
    pbj.extend_from_slice(&[1, 0x04]);
    pbj.extend_from_slice(&2u16.to_le_bytes());
    pbj.push(0x0F);
    pbj.extend_from_slice(b"color\0");
    pbj.push(0xA2);
    pbj.push(0x04);
    pbj.extend_from_slice(b"defaultValue\0");
    for f in [0.5f32, 1.0, 0.25, 1.0] {
        pbj.extend_from_slice(&f.to_bits().to_be_bytes());
    }
    // sampleNearest (0x30): dst=1, mask=0xF0, coord=0, swz=0x10 (x=0,y=1 -> 0b00_01_0000 = 0x10), tex=0
    pbj.push(0x30);
    pbj.extend_from_slice(&1u16.to_le_bytes());
    pbj.push(0xF0);
    pbj.extend_from_slice(&0u16.to_le_bytes());
    pbj.push(0x10);
    pbj.push(0);
    // mul (0x03): dst=1, mask_len = (0xF << 4) | (3 << 2) = 0xFC, src=2, swz = 0b00_01_10_11 = 0x1B, pad=0
    pbj.push(0x03);
    pbj.extend_from_slice(&1u16.to_le_bytes());
    pbj.push(0xFC);
    pbj.extend_from_slice(&2u16.to_le_bytes());
    pbj.push(0x1B);
    pbj.push(0);

    let meta = parse_shader_bytecode(&pbj).expect("parse pbj");
    assert_eq!(meta.name, "TintPbj");
    assert_eq!(meta.inputs.len(), 1);
    assert_eq!(meta.parameters.len(), 1);
    assert_eq!(meta.parameters[0].param_type, "float4");

    let req = ShaderEvalRequest {
        bytecode_base64: BASE64.encode(&pbj),
        precision_hint: "full".into(),
        width: 2,
        height: 1,
        inputs: vec![ShaderRuntimeInput {
            name: "src".into(),
            width: 2,
            height: 1,
            channels: 4,
            floats: vec![1.0, 0.5, 0.8, 1.0, 0.4, 0.2, 1.0, 1.0],
            rgba_base64: None,
        }],
        parameters: BTreeMap::new(),
    };
    let res = evaluate_shader(&req).expect("eval pbj");
    assert_eq!(res.floats.len(), 8);
    assert!((res.floats[0] - 0.5).abs() < 1e-5);
    assert!((res.floats[1] - 0.5).abs() < 1e-5);
    assert!((res.floats[2] - 0.2).abs() < 1e-5);
}

#[test]
fn display_3d_projection_roundtrip() {
    let pose = Display3dPose {
        x: 120.0,
        y: 80.0,
        z: 100.0,
        scale_x: 1.5,
        scale_y: 1.25,
        scale_z: 1.0,
        rotation_x: 20.0,
        rotation_y: -15.0,
        rotation_z: 30.0,
    };
    let m = compose_display_matrix3d(&pose);
    let dec = decompose_display_matrix3d(&m);
    assert!((dec.x - 120.0).abs() < 1e-3);
    assert!((dec.z - 100.0).abs() < 1e-3);
    let chain = [m];
    let screen = local3d_to_global(&chain, [25.0, -10.0, 0.0], 480.0, [250.0, 200.0]);
    let local = global_to_local3d(&chain, screen, 480.0, [250.0, 200.0]);
    assert!((local[0] - 25.0).abs() < 1e-3);
    assert!((local[1] - (-10.0)).abs() < 1e-3);
    assert!(local[2].abs() < 1e-6);
}

#[test]
fn inspect_swf_bytes_handles_nbits_31_and_rejects_cws_decompression_bomb() {
    use std::io::Write;

    // 1. nbits == 31 with extreme signed values (xmin = -(1<<30), xmax = (1<<30)-1)
    let mut fws = vec![b'F', b'W', b'S', 10, 0, 0, 0, 0];
    let nbits = 31usize;
    let total_rect_bits = 5 + nbits * 4;
    let rect_bytes = total_rect_bits.div_ceil(8);
    let mut body = vec![0u8; rect_bytes + 6];
    let mut write_bits = |start_bit: usize, count: usize, val: u64| {
        for i in 0..count {
            let bit_idx = start_bit + i;
            let bit = ((val >> (count - 1 - i)) & 1) as u8;
            if bit != 0 {
                body[bit_idx / 8] |= 1 << (7 - (bit_idx % 8));
            }
        }
    };
    write_bits(0, 5, 31);
    write_bits(5, 31, 1u64 << 30); // xmin = -1073741824
    write_bits(5 + 31, 31, (1u64 << 30) - 1); // xmax = 1073741823
    write_bits(5 + 62, 31, 1u64 << 30); // ymin = -1073741824
    write_bits(5 + 93, 31, (1u64 << 30) - 1); // ymax = 1073741823
    body[rect_bytes..rect_bytes + 2].copy_from_slice(&(24u16 << 8).to_le_bytes());
    body[rect_bytes + 2..rect_bytes + 4].copy_from_slice(&1u16.to_le_bytes());
    fws.extend_from_slice(&body);
    let len = fws.len() as u32;
    fws[4..8].copy_from_slice(&len.to_le_bytes());

    let info = inspect_swf_bytes(&fws).expect("nbits=31 should not overflow");
    assert_eq!(info.width, ((2_147_483_647i64) / 20) as u32);
    assert_eq!(info.height, ((2_147_483_647i64) / 20) as u32);

    // 2. CWS decompression bomb (> 16 MiB decompressed zeros) is rejected
    let mut cws = vec![b'C', b'W', b'S', 10, 0, 0, 0, 0x02];
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![0u8; 64 * 1024];
    for _ in 0..(16 * 16 + 1) {
        enc.write_all(&chunk).unwrap();
    }
    let compressed = enc.finish().unwrap();
    assert!(compressed.len() < 64 * 1024);
    cws.extend_from_slice(&compressed);
    let err = inspect_swf_bytes(&cws).unwrap_err();
    assert!(err.to_string().contains("16 MiB"));
}

#[test]
fn t326_fte_atom_and_word_boundaries_preserve_combining_marks_and_zwj_emoji() {
    // "e\u{0301} и\u{0306} か\u{3099} 👨‍👩‍👧‍👦"
    // Offsets:
    // "e\u{0301}" -> 0..2
    // " "         -> 2..3
    // "и\u{0306}" -> 3..5
    // " "         -> 5..6
    // "か\u{3099}" -> 6..8
    // " "         -> 8..9
    // "👨‍👩‍👧‍👦" -> 9..20
    let raw = "e\u{0301} и\u{0306} か\u{3099} 👨‍👩‍👧‍👦";
    assert_eq!(raw.encode_utf16().count(), 20);

    let mut atoms = Vec::new();
    let mut idx = 0i32;
    while (idx as usize) < 20 {
        idx = find_next_atom_boundary(raw, idx).unwrap();
        atoms.push(idx);
    }
    assert_eq!(atoms, vec![2, 3, 5, 6, 8, 9, 20]);

    let mut rev_atoms = Vec::new();
    while idx > 0 {
        idx = find_previous_atom_boundary(raw, idx).unwrap();
        rev_atoms.push(idx);
    }
    rev_atoms.reverse();
    assert_eq!(rev_atoms, vec![0, 2, 3, 5, 6, 8, 9]);

    // Word boundaries never land inside combining sequences or ZWJ clusters either.
    let next_w0 = find_next_word_boundary(raw, 0).unwrap();
    assert_eq!(next_w0, 2);
    let prev_w_end = find_previous_word_boundary(raw, 20).unwrap();
    assert!(matches!(prev_w_end, 9 | 8));

    // FTE paragraph layout produces 7 grapheme atoms (never 18 code-unit atoms!).
    let req = FteLayoutRequest {
        runs: vec![FteInputElementRun {
            element_id: 1,
            is_graphic: false,
            text: raw.into(),
            graphic_width: 0.0,
            graphic_height: 0.0,
            font_name: "_sans".into(),
            font_size: 16.0,
            font_weight: "normal".into(),
            font_posture: "normal".into(),
            color: 0,
            alpha: 1.0,
            tracking_left: 0.0,
            tracking_right: 0.0,
            baseline_shift: 0.0,
            text_rotation: "auto".into(),
            break_opportunity: "auto".into(),
            digit_width: "default".into(),
            typographic_case: "default".into(),
            has_mirror: false,
        }],
        start_char: 0,
        max_width: 1000.0,
        line_offset: 0.0,
        fit_something: true,
        tab_stops: Vec::new(),
        justifier: FteJustifierSpec {
            kind: "space".into(),
            locale: "en".into(),
            line_justification: "unjustified".into(),
            letter_spacing: false,
            minimum_spacing: 0.5,
            optimum_spacing: 1.0,
            maximum_spacing: 2.0,
            justification_style: "pushInKinsoku".into(),
            compose_trailing_ideographic_spaces: false,
        },
        baseline_font_size: 16.0,
        baseline_zero: "roman".into(),
        bidi_level: 0,
        line_rotation: "rotate0".into(),
        apply_non_linear_font_scaling: true,
    };
    let resp = layout_text_line(&req).expect("layout_text_line");
    let line = resp.line.expect("non-empty line");
    assert_eq!(line.atoms.len(), 7);
    assert_eq!(line.raw_text_length, 20);
}
