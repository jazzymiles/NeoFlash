use super::super::*;
use neoflash_core::{
    ColorTransform, Element, ElementKind, FrameSound, Keyframe, Layer, LayerProperties, Project,
    ResourceUnit, SoundEffect, SoundSync, TextAlign, TextFieldState, TextFormat, TextFormatRun,
    Timeline, new_id,
};
use std::io::{Cursor, Write as _};

fn build_zero_height_or_subset_sfnt(mapped_cp: u32, zero_height: bool) -> Vec<u8> {
    let mut g1 = Vec::new();
    g1.extend_from_slice(&1i16.to_be_bytes());
    g1.extend_from_slice(&0i16.to_be_bytes());
    g1.extend_from_slice(&0i16.to_be_bytes());
    g1.extend_from_slice(&500i16.to_be_bytes());
    g1.extend_from_slice(&700i16.to_be_bytes());
    g1.extend_from_slice(&2u16.to_be_bytes());
    g1.extend_from_slice(&0u16.to_be_bytes());
    g1.extend_from_slice(&[0x37, 0x37, 0x37]);
    g1.extend_from_slice(&[10, 200, 100]);
    g1.extend_from_slice(&[0, 250, 100]);
    if !g1.len().is_multiple_of(2) {
        g1.push(0);
    }
    let mut glyf = Vec::new();
    glyf.extend_from_slice(&g1);
    let end_off = glyf.len() as u32;
    let mut loca = Vec::new();
    for off in [0u32, 0u32, end_off] {
        loca.extend_from_slice(&off.to_be_bytes());
    }
    let mut head = vec![0u8; 54];
    head[1..2].copy_from_slice(&[1]);
    head[16..18].copy_from_slice(&0x000Bu16.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[1..2].copy_from_slice(&[1]);
    if zero_height {
        // ascent == descent == 0 => height_unscaled() == 0!
        hhea[4..6].copy_from_slice(&0i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&0i16.to_be_bytes());
    } else {
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    }
    hhea[34..36].copy_from_slice(&2u16.to_be_bytes());

    let mut maxp = vec![0u8; 32];
    maxp[1..2].copy_from_slice(&[1]);
    maxp[4..6].copy_from_slice(&2u16.to_be_bytes());
    let mut hmtx = Vec::new();
    for adv in [500u16, 600u16] {
        hmtx.extend_from_slice(&adv.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }
    let os2 = vec![0u8; 78];
    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());

    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&10u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&12u16.to_be_bytes());
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&28u32.to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&1u32.to_be_bytes());
    cmap.extend_from_slice(&mapped_cp.to_be_bytes());
    cmap.extend_from_slice(&mapped_cp.to_be_bytes());
    cmap.extend_from_slice(&1u32.to_be_bytes());

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"post", post),
    ];
    tables.sort_by_key(|(t, _)| *t);

    let mut out = Vec::new();
    out.extend_from_slice(&[0, 1, 0, 0]);
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    let mut off = 12 + tables.len() * 16;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(off as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        off += (data.len() + 3) & !3;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}

fn minimal_wav_bytes() -> Vec<u8> {
    let samples = 4410u32; // 0.1s mono 8-bit 44.1kHz
    let data_len = samples;
    let riff_len = 36 + data_len;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&riff_len.to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&44_100u32.to_le_bytes());
    b.extend_from_slice(&44_100u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 128u8);
    b
}

#[test]
fn t414_lottie_outlined_text_alpha_wrap_justify_multikey_subset_fallback_and_run_fonts() {
    // 1) Embedded subsetted font missing 'B' falls through to portable fallback instead of failing!
    // 2) Embedded zero-height font (`ascent == descent == 0`) outlines cleanly without div-by-zero!
    // 3) Outlined TextBox honors `tf_state.alpha`, `word_wrap`, `justify`, and multi-keyframe text/tint!
    // 4) Non-outlined TextBox registers `TextFormatRun` fonts in `fonts.list` and `chars`!
    let subset_only_a = build_zero_height_or_subset_sfnt(u32::from(b'A'), false);
    let zero_height_x = build_zero_height_or_subset_sfnt(u32::from(b'X'), true);

    let mut project = Project::blank();
    project.name = "T414 Lottie Text".into();
    project.resources.push(
        ResourceUnit::from_bytes(
            "sub-font",
            "fonts/SubsetCustom.ttf",
            "font/ttf",
            &subset_only_a,
        )
        .unwrap(),
    );
    project.resources.push(
        ResourceUnit::from_bytes(
            "zero-font",
            "fonts/ZeroHeight.ttf",
            "font/ttf",
            &zero_height_x,
        )
        .unwrap(),
    );

    // Multi-keyframe outlined TextBox with word_wrap=true, justify alignment + translucent Text alpha=128
    let mut tf0 = TextFieldState {
        text: "AA BB   \nCC".into(),
        word_wrap: true,
        multiline: true,
        alpha: 255,
        format: TextFormat {
            font: "SubsetCustom".into(), // lacks 'B'/'C' -> must fall through to built-in Inter!
            size: 20.0,
            color: 0xff0000,
            align: TextAlign::Left,
            letter_spacing: 4.0,
            ..Default::default()
        },
        ..Default::default()
    };
    tf0.format.set_align_mode(Some("justify".into()));
    let mut el0 = Element {
        id: "tb_multi".into(),
        kind: ElementKind::TextBox {
            text: tf0.text.clone(),
            width: 55.0,
            height: 80.0,
            format: tf0.format.clone(),
            word_wrap: true,
            multiline: true,
        },
        ..Element::default()
    };
    tf0.write_to_kind(&mut el0.kind);

    let mut tf1 = tf0.clone();
    tf1.text = "DD".into();
    let mut el1 = el0.clone();
    el1.transform.color_transform = ColorTransform {
        multipliers: [1.0, 1.0, 1.0, 1.0],
        offsets: [0.0, 200.0, 0.0, 0.0],
    };
    tf1.write_to_kind(&mut el1.kind);

    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline = Timeline {
        frame_count: 6,
        layers: vec![
            Layer {
                id: new_id("layer"),
                name: "OutlinedMultiKey".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![el0],
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![el1],
                        ..Default::default()
                    },
                ],
            },
            Layer {
                id: new_id("layer"),
                name: "ZeroHeightLayer".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("element"),
                        kind: ElementKind::Text {
                            text: "X".into(),
                            font_family: "ZeroHeight".into(),
                            font_size: 24.0,
                            color: [0, 255, 0, 128],
                        },
                        ..Element::default()
                    }],
                    ..Default::default()
                }],
            },
        ],
        scripts: Vec::new(),
        guides: Vec::new(),
    };
    project.format_version = 5;
    project.upgrade_format_for_features();

    let opts = LottieOptions {
        text_mode: LottieTextMode::Shapes,
        ..Default::default()
    };
    let (json_str, report) = export_lottie_json(&project, &opts).expect("export outlined Lottie");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_TEXT_OUTLINED") && n.message.contains("2 ")),
        "expected 2 outlined text elements in report: {:?}",
        report.notes
    );
    let doc: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let shapes_layer = doc["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "OutlinedMultiKey")
        .expect("OutlinedMultiKey layer");
    let top_groups = shapes_layer["shapes"].as_array().unwrap();
    assert_eq!(
        top_groups.len(),
        2,
        "2 distinct text/tint keyframes must emit 2 timed visibility groups"
    );
    assert_eq!(top_groups[0]["nm"], "Text Keyframe 0");
    assert_eq!(top_groups[1]["nm"], "Text Keyframe 3");

    // Check translucent `alpha == 128` (~50.1961% fill opacity `o.k`) on `ZeroHeightLayer` glyph fill!
    let zh_layer = doc["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "ZeroHeightLayer")
        .expect("ZeroHeightLayer");
    let first_glyph_items = zh_layer["shapes"][0]["it"].as_array().unwrap();
    let fl_item = first_glyph_items
        .iter()
        .find(|it| it["ty"] == "fl")
        .expect("glyph fill");
    let fill_o = fl_item["o"]["k"].as_f64().unwrap();
    assert!(
        (fill_o - (128.0 / 255.0 * 100.0)).abs() < 0.05,
        "expected fill opacity ~50.2 from alpha=128, got {fill_o}"
    );

    // 4) Non-outlined TextBox with `TextFormatRun` registers run font in `fonts.list` and `chars`!
    let mut run_proj = Project::blank();
    run_proj.name = "T414 Run Fonts".into();
    let mut rtf = TextFieldState {
        text: "Hello Mono".into(),
        format: TextFormat {
            font: "Inter".into(),
            size: 16.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut mono_fmt = rtf.format.clone();
    mono_fmt.font = "RobotoMono".into();
    mono_fmt.bold = true;
    rtf.format_runs = vec![
        TextFormatRun {
            start: 0,
            end: 6,
            format: rtf.format.clone(),
        },
        TextFormatRun {
            start: 6,
            end: 10,
            format: mono_fmt,
        },
    ];
    let mut rel = Element {
        id: new_id("element"),
        name: "dynField".into(),
        kind: ElementKind::TextBox {
            text: rtf.text.clone(),
            width: 160.0,
            height: 30.0,
            format: rtf.format.clone(),
            word_wrap: false,
            multiline: false,
        },
        ..Element::default()
    };
    rtf.write_to_kind(&mut rel.kind);
    let rroot = run_proj.root.clone();
    run_proj.symbol_mut(&rroot).unwrap().timeline.layers[0].keyframes[0].elements = vec![rel];
    run_proj.format_version = 5;
    run_proj.upgrade_format_for_features();

    let glyph_opts = LottieOptions {
        text_mode: LottieTextMode::TextLayers,
        outline_text_glyphs: Some(false),
        emit_glyph_chars: true,
        ..Default::default()
    };
    let (gjson, _) = export_lottie_json(&run_proj, &glyph_opts).unwrap();
    let gdoc: serde_json::Value = serde_json::from_str(&gjson).unwrap();
    let font_families: Vec<&str> = gdoc["fonts"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["fFamily"].as_str())
        .collect();
    assert!(font_families.contains(&"Inter"));
    assert!(
        font_families.contains(&"RobotoMono"),
        "TextFormatRun font must be registered in fonts.list: {font_families:?}"
    );
    assert!(
        gdoc["chars"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["fFamily"] == "RobotoMono" && c["ch"] == "M"),
        "TextFormatRun glyph chars must be recorded"
    );
}

#[test]
fn t414_lottie_audio_source_fps_monotonic_level_and_dotlottie_zip_bomb_guard() {
    // 1) P1-14: 24fps project exported with `options.fps = Some(60)`:
    //    Stream sound with `in_point = 0.0`, `out_point = 1.0` on a 48-frame timeline must trim `op = 24.0` and `tm` `to = 1.0`!
    //    Empty envelope emits 2D `[100.0, 100.0]`; 3+ simultaneous envelope points emit strictly monotonic `t`!
    let wav = minimal_wav_bytes();
    let mut project = Project::blank();
    project.name = "T414 Audio".into();
    project.fps = 24;
    project
        .resources
        .push(ResourceUnit::from_bytes("wav1", "audio/clip.wav", "audio/wav", &wav).unwrap());
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline = Timeline {
        frame_count: 48,
        layers: vec![
            Layer {
                id: new_id("layer"),
                name: "StreamEmptyEnv".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound {
                        resource: "wav1".into(),
                        sync: SoundSync::Stream,
                        loops: 1,
                        loop_forever: false,
                        effect: SoundEffect::None,
                        in_point: Some(0.0),
                        out_point: Some(1.0),
                        envelope: None,
                    }),
                    ..Default::default()
                }],
            },
            Layer {
                id: new_id("layer"),
                name: "StreamTripleZeroT".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound {
                        resource: "wav1".into(),
                        sync: SoundSync::Stream,
                        loops: 1,
                        loop_forever: false,
                        effect: SoundEffect::Custom,
                        in_point: None,
                        out_point: None,
                        envelope: Some(vec![[0.0, 1.0, 1.0], [0.0, 0.5, 0.5], [0.0, 0.1, 0.1]]),
                    }),
                    ..Default::default()
                }],
            },
        ],
        scripts: Vec::new(),
        guides: Vec::new(),
    };
    project.format_version = 6;
    project.upgrade_format_for_features();

    let opts = LottieOptions {
        player: LottieTargetPlayer::LottieWeb,
        fps: Some(60),
        ..Default::default()
    };
    let (json_str, _) = export_lottie_json(&project, &opts).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let layers = doc["layers"].as_array().unwrap();
    let empty_env_layer = layers.iter().find(|l| l["nm"] == "StreamEmptyEnv").unwrap();
    assert_eq!(empty_env_layer["op"].as_f64(), Some(24.0));
    assert_eq!(empty_env_layer["tm"]["k"][0]["e"], serde_json::json!([1.0]));
    assert_eq!(
        empty_env_layer["au"]["lv"],
        serde_json::json!({"a": 0, "k": [100.0, 100.0]})
    );

    let triple_layer = layers
        .iter()
        .find(|l| l["nm"] == "StreamTripleZeroT")
        .unwrap();
    let lv_kfs = triple_layer["au"]["lv"]["k"].as_array().unwrap();
    let ts: Vec<f64> = lv_kfs.iter().map(|k| k["t"].as_f64().unwrap()).collect();
    assert!(
        ts.windows(2).all(|w| w[0] < w[1]),
        "au.lv keyframe timestamps must be strictly monotonic: {ts:?}"
    );

    // 2) P2-17: `validate_dotlottie_bytes` rejects a deflated >64 MiB entry immediately
    let mut zbuf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut zbuf);
        let zopts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("manifest.json", zopts).unwrap();
        zw.write_all(br#"{"version":"1.0","generator":"test","animations":[{"id":"main"}]}"#)
            .unwrap();
        zw.start_file("animations/main.json", zopts).unwrap();
        let chunk = vec![b' '; 1024 * 1024];
        // Write 65 MiB of compressible spaces (~66 KiB deflated!)
        for _ in 0..65 {
            zw.write_all(&chunk).unwrap();
        }
        zw.finish().unwrap();
    }
    let bomb_bytes = zbuf.into_inner();
    assert!(bomb_bytes.len() < 256 * 1024);
    let err = validate_dotlottie_bytes(&bomb_bytes).expect_err("must reject >64 MiB ZIP entry");
    assert!(err.to_string().contains("exceeds uncompressed size budget"));
}
