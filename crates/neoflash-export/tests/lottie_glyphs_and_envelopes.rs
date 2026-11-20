//! Integration tests for Lottie / dotLottie vector-glyph Bezier text outlines (`ty: "gr"` / `ty: "sh"`
//! + `chars`/`t` text layer modes) and multi-point custom stereo sound envelopes (`au.lv`).

use neoflash_core::{
    AppearancePaintEntry, AuthoredTextFieldOptions, Element, ElementAppearance, ElementKind,
    FrameSound, Keyframe, Project, ResourceUnit, SoundEffect, SoundSync, TextAlign, TextFormat,
    TextFormatRun, Transform,
};
use neoflash_export::{
    LottieFormat, LottieOptions, LottieTextMode, export_dotlottie_bytes, export_lottie,
    validate_dotlottie_bytes,
};

const EMBEDDED_TTF: &[u8] = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");

fn tiny_pcm_wav() -> Vec<u8> {
    let sample_rate = 8_000u32;
    let num_samples = 800u32; // 0.1s mono 16-bit PCM
    let data_bytes = num_samples * 2;
    let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // 1 channel
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..num_samples {
        let sample = if i % 20 < 10 { 4096i16 } else { -4096i16 };
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[test]
fn smart_default_outlines_embedded_font_text_with_fill_and_stroke_while_preserving_text_layers_when_requested()
 {
    let mut project = Project::blank();
    project.format_version = 9;
    project.resources.push(
        ResourceUnit::from_bytes("font-inter", "fonts/Inter.ttf", "font/ttf", EMBEDDED_TTF)
            .expect("embedded Inter font resource"),
    );

    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 12;
        root.timeline.layers[0].name = "Headline".into();
        root.timeline.layers[0].keyframes[0] = Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "hero-txt".into(),
                name: "HeroLabel".into(),
                transform: Transform {
                    x: 40.0,
                    y: 60.0,
                    ..Transform::default()
                },
                kind: ElementKind::Text {
                    text: "AB".into(),
                    font_family: "Inter".into(),
                    font_size: 36.0,
                    color: [240, 64, 96, 255],
                },
                ..Element::default()
            }],
            appearances: vec![ElementAppearance {
                element_id: "hero-txt".into(),
                entries: vec![AppearancePaintEntry::Stroke {
                    color: [16, 24, 40, 255],
                    gradient: None,
                    swatch_id: None,
                    width: 2.0,
                    offset_distance: 0.0,
                    opacity: 1.0,
                    pattern_brush: None,
                }],
                ..ElementAppearance::default()
            }],
            ..Keyframe::default()
        };
    }
    project.upgrade_format_for_features();
    project.validate().unwrap();

    // 1. Smart default (`outline_text_glyphs: None`): embedded FontResource automatically converts
    //    `Text` into per-glyph `ty: "gr"` / `ty: "sh"` Bezier contours with fill + stroke!
    let (smart_val, smart_report) = export_lottie(&project, &LottieOptions::default()).unwrap();
    assert!(
        !smart_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_TEXT_FONT_DEPENDENCY")),
        "auto-outlined embedded font text must not depend on external runtime web fonts"
    );
    assert!(
        smart_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_TEXT_OUTLINED"))
    );
    let shape_layer = smart_val["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "Headline · HeroLabel")
        .expect("outlined text shape layer");
    assert_eq!(shape_layer["ty"], 4);
    let glyph_groups = shape_layer["shapes"].as_array().expect("glyph groups");
    assert_eq!(glyph_groups.len(), 2, "two visible glyphs ('A' and 'B')");
    for grp in glyph_groups {
        assert_eq!(grp["ty"], "gr");
        let it = grp["it"].as_array().unwrap();
        let sh = it
            .iter()
            .find(|item| item["ty"] == "sh")
            .expect("glyph sh item");
        let bez = &sh["ks"]["k"];
        assert_eq!(bez["c"], true);
        assert!(bez["v"].as_array().unwrap().len() >= 3);
        assert_eq!(
            bez["v"].as_array().unwrap().len(),
            bez["i"].as_array().unwrap().len()
        );
        assert_eq!(
            bez["v"].as_array().unwrap().len(),
            bez["o"].as_array().unwrap().len()
        );
        assert!(it.iter().any(|item| item["ty"] == "fl"));
        assert!(it.iter().any(|item| item["ty"] == "st"));
        assert!(it.iter().any(|item| item["ty"] == "tr"));
    }

    // 2. Explicit `outline_text_glyphs: Some(false)` + `emit_glyph_chars: true`: keeps `ty: 5` `"t"`
    //    text layer AND emits root `"chars"` vector glyph outline table!
    let (chars_val, _) = export_lottie(
        &project,
        &LottieOptions {
            outline_text_glyphs: Some(false),
            emit_glyph_chars: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    let t_layer = chars_val["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "Headline · HeroLabel")
        .expect("ty:5 text layer");
    assert_eq!(t_layer["ty"], 5);
    assert_eq!(t_layer["t"]["d"]["k"][0]["s"]["t"], "AB");
    let chars_arr = chars_val["chars"]
        .as_array()
        .expect("root chars glyph definitions");
    assert_eq!(chars_arr.len(), 2);
    assert!(chars_arr.iter().any(|c| c["ch"] == "A"
        && c["data"]["shapes"][0]["ty"] == "gr"
        && c["w"].as_f64().unwrap() > 10.0));
    assert!(chars_arr.iter().any(|c| c["ch"] == "B"));
}

#[test]
fn explicit_outline_option_converts_system_font_and_rich_textbox_runs_and_exports_multipoint_sound_envelopes()
 {
    let mut project = Project::blank();
    project.fps = 24;
    project.format_version = 7;
    project.resources.push(
        ResourceUnit::from_bytes(
            "sfx-stereo",
            "audio/theme.wav",
            "audio/wav",
            &tiny_pcm_wav(),
        )
        .unwrap(),
    );

    let mut fmt_red = TextFormat {
        font: "Helvetica".into(),
        size: 28.0,
        color: 0xFF2040,
        align: TextAlign::Center,
        ..TextFormat::default()
    };
    let fmt_blue = TextFormat {
        font: "Helvetica".into(),
        size: 28.0,
        bold: true,
        color: 0x2080FF,
        align: TextAlign::Center,
        ..TextFormat::default()
    };
    fmt_red.set_field_options(Some(AuthoredTextFieldOptions {
        selectable: false,
        format_runs: vec![
            TextFormatRun {
                start: 0,
                end: 2,
                format: fmt_red.clone(),
            },
            TextFormatRun {
                start: 2,
                end: 4,
                format: fmt_blue,
            },
        ],
        ..AuthoredTextFieldOptions::default()
    }));

    let root_id = project.root.clone();
    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 20;
        root.timeline.layers[0].name = "RichTitle".into();
        root.timeline.layers[0].keyframes[0] = Keyframe {
            frame: 0,
            sound: Some(FrameSound {
                resource: "sfx-stereo".into(),
                sync: SoundSync::Stream,
                loops: 1,
                loop_forever: false,
                effect: SoundEffect::Custom,
                envelope: Some(vec![
                    [0.0, 0.25, 0.80],
                    [0.5, 0.90, 0.35],
                    [0.8, 0.10, 1.00],
                ]),
                in_point: Some(0.0),
                out_point: None,
            }),
            elements: vec![Element {
                id: "rich-box".into(),
                name: String::new(),
                transform: Transform {
                    x: 20.0,
                    y: 30.0,
                    ..Transform::default()
                },
                kind: ElementKind::TextBox {
                    text: "HiGo".into(),
                    width: 240.0,
                    height: 50.0,
                    format: fmt_red,
                    word_wrap: false,
                    multiline: false,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        };
    }
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let opts = LottieOptions {
        outline_text_glyphs: Some(true),
        ..LottieOptions::default()
    };
    let (val, report) = export_lottie(&project, &opts).unwrap();
    assert!(
        !report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_TIMELINE_SOUND")),
        "streamed WAV with custom stereo envelope must export natively on LottieWeb"
    );

    // Check rich-text per-glyph shape groups preserve distinct run colors
    let text_shape_layer = val["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["ty"] == 4)
        .expect("outlined rich TextBox layer");
    let groups = text_shape_layer["shapes"].as_array().unwrap();
    assert_eq!(groups.len(), 4, "4 glyphs in 'HiGo'");
    let first_fill = &groups[0]["it"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["ty"] == "fl")
        .unwrap()["c"]["k"];
    let last_fill = &groups[3]["it"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["ty"] == "fl")
        .unwrap()["c"]["k"];
    assert!(first_fill[0].as_f64().unwrap() > 0.9 && first_fill[2].as_f64().unwrap() < 0.35);
    assert!(last_fill[2].as_f64().unwrap() > 0.9 && last_fill[0].as_f64().unwrap() < 0.2);

    // Check multi-point custom stereo sound envelope on `ty: 6` audio layer (`au.lv`)
    let audio_layer = val["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["ty"] == 6)
        .expect("Lottie audio layer");
    let lv = &audio_layer["au"]["lv"];
    assert_eq!(lv["a"], 1, "custom sound envelope must animate au.lv");
    let kfs = lv["k"].as_array().expect("au.lv keyframes");
    // Points at pos 0.0 (t=0), 0.5 (t=10), 0.8 (t=16), plus hold at op (t=20)
    assert_eq!(kfs.len(), 4, "expected 4 stereo volume keyframes: {kfs:?}");
    assert_eq!(kfs[0]["t"].as_f64(), Some(0.0));
    assert_eq!(kfs[0]["s"], serde_json::json!([25.0, 80.0]));
    assert_eq!(kfs[0]["e"], serde_json::json!([90.0, 35.0]));
    assert_eq!(kfs[1]["t"].as_f64(), Some(10.0));
    assert_eq!(kfs[1]["s"], serde_json::json!([90.0, 35.0]));
    assert_eq!(kfs[2]["t"].as_f64(), Some(16.0));
    assert_eq!(kfs[2]["s"], serde_json::json!([10.0, 100.0]));
    assert_eq!(kfs[3]["t"].as_f64(), Some(20.0));
    assert_eq!(kfs[3]["s"], serde_json::json!([10.0, 100.0]));

    // Also verify dotLottie (.lottie) packaging with TextMode::Shapes
    let (dot_bytes, _) = export_dotlottie_bytes(
        &project,
        &LottieOptions {
            format: LottieFormat::DotLottie,
            text_mode: LottieTextMode::Shapes,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    validate_dotlottie_bytes(&dot_bytes).expect("valid dotLottie archive");
}
