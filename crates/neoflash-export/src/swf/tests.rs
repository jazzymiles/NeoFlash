//! Unit tests for SWF dictionary capacity, layout, bytecode, and container encoding.

use super::*;

fn root_keyframe_mut(project: &mut Project) -> &mut neoflash_core::Keyframe {
    let root = project.root.clone();
    &mut project
        .symbol_mut(&root)
        .expect("blank project root")
        .timeline
        .layers[0]
        .keyframes[0]
}

fn tiny_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn wav_bytes(encoding: u16, channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
    let bytes_per_sample = usize::from(bits.div_ceil(8));
    let block_align = channels.saturating_mul(bits.div_ceil(8));
    let byte_rate = sample_rate.saturating_mul(u32::from(block_align));
    let data_len = usize::from(channels)
        .saturating_mul(bytes_per_sample)
        .max(1)
        * 4;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + u32::try_from(data_len).unwrap()).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&encoding.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&u32::try_from(data_len).unwrap().to_le_bytes());
    wav.resize(wav.len() + data_len, 0);
    wav
}

fn one_frame_mp3() -> Vec<u8> {
    // MPEG-1 Layer III, 128 kbps, 44.1 kHz, stereo: floor(144*128000/44100)=417 bytes.
    let mut frame = vec![0u8; 417];
    frame[0..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
    frame
}

fn project_with_sound(media_type: &str, extension: &str, bytes: &[u8]) -> Project {
    let mut project = Project::blank();
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes(
            "sound",
            format!("sound.{extension}"),
            media_type,
            bytes,
        )
        .unwrap(),
    );
    root_keyframe_mut(&mut project).sound = Some(FrameSound::new("sound"));
    project
}

#[test]
fn character_allocator_never_reuses_the_last_swf_dictionary_id() {
    let project = Project::blank();
    let options = SwfPublishOptions::default();
    let mut compiler = SwfDictionaryCompiler::new(&project, &options);
    compiler.next_char_id = u32::from(u16::MAX);

    assert_eq!(compiler.alloc_char_id(), u16::MAX);
    assert!(!compiler.dictionary_overflow);
    assert_eq!(compiler.alloc_char_id(), 0);
    assert!(compiler.dictionary_overflow);
    assert_eq!(compiler.alloc_char_id(), 0);
}

#[test]
fn preflight_refuses_frame_rates_that_the_swf_header_would_clamp() {
    let mut project = Project::blank();
    project.fps = 256;
    let options = SwfPublishOptions::default();
    let report = preflight_swf_publish(&project, &options);
    assert!(report.items.iter().any(|item| {
        item.code == "swf_frame_rate_limit" && item.disposition == FlaCompatibilityStatus::Blocking
    }));
    assert!(
        !report
            .items
            .iter()
            .any(|item| item.code == "swf_frame_rate_exact")
    );
    assert!(matches!(
        publish_swf_bytes(&project, &options),
        Err(ExportError::CompatibilityBlocked { .. })
    ));

    project.fps = 255;
    let exact = preflight_swf_publish(&project, &options);
    assert!(exact.items.iter().any(|item| {
        item.code == "swf_frame_rate_exact" && item.disposition == FlaCompatibilityStatus::Exact
    }));
}

#[test]
fn preflight_checks_only_used_media_and_refuses_unsupported_codecs() {
    let jpeg = neoflash_core::ResourceUnit::from_bytes(
        "photo",
        "photo.jpg",
        "image/jpeg",
        &[0xff, 0xd8, 0xff, 0xd9],
    )
    .unwrap();
    let ogg = neoflash_core::ResourceUnit::from_bytes(
        "music",
        "music.ogg",
        "audio/ogg",
        b"OggS\0unsupported",
    )
    .unwrap();

    let mut unused = Project::blank();
    unused.resources.extend([jpeg.clone(), ogg.clone()]);
    let unused_report = preflight_swf_publish(&unused, &SwfPublishOptions::default());
    assert!(unused_report.can_proceed(), "{:#?}", unused_report.items);

    let mut placed_jpeg = Project::blank();
    placed_jpeg.resources.push(jpeg);
    root_keyframe_mut(&mut placed_jpeg).elements.push(Element {
        id: "photo-placement".to_owned(),
        kind: ElementKind::Bitmap {
            resource_id: "photo".to_owned(),
            width: 1,
            height: 1,
            smoothing: true,
        },
        ..Element::default()
    });
    let jpeg_report = preflight_swf_publish(&placed_jpeg, &SwfPublishOptions::default());
    assert!(
        jpeg_report
            .items
            .iter()
            .any(|item| item.code == "swf_bitmap_data_unsupported")
    );
    assert!(!jpeg_report.can_proceed());

    let mut used_ogg = Project::blank();
    used_ogg.resources.push(ogg);
    root_keyframe_mut(&mut used_ogg).sound = Some(FrameSound::new("music"));
    let ogg_report = preflight_swf_publish(&used_ogg, &SwfPublishOptions::default());
    assert!(
        ogg_report
            .items
            .iter()
            .any(|item| item.code == "swf_sound_codec_unsupported")
    );
    assert!(!ogg_report.can_proceed());
}

#[test]
fn swf_sound_layouts_are_proven_instead_of_clamped_or_guessed() {
    let wav = wav_bytes(1, 1, 44_100, 16);
    let wav_info = analyze_swf_sound_resource("audio/wav", &wav).unwrap();
    assert_eq!(wav_info.sample_count, 4);
    assert_eq!(wav_info.sample_rate, 44_100);
    assert!(!wav_info.stereo);
    assert!(
        preflight_swf_publish(
            &project_with_sound("audio/wav", "wav", &wav),
            &SwfPublishOptions::default()
        )
        .can_proceed()
    );

    let mp3 = one_frame_mp3();
    let mp3_info = analyze_swf_sound_resource("audio/mpeg", &mp3).unwrap();
    assert_eq!(mp3_info.sample_count, 1152);
    assert_eq!(mp3_info.sample_rate, 44_100);
    assert!(mp3_info.stereo);
    let mp3_payload =
        build_define_sound_payload(7, &mp3_info, &mp3[mp3_info.data_start..mp3_info.data_end]);
    assert_eq!(
        u32::from_le_bytes(mp3_payload[3..7].try_into().unwrap()),
        1152
    );
    assert!(
        preflight_swf_publish(
            &project_with_sound("audio/mpeg", "mp3", &mp3),
            &SwfPublishOptions::default()
        )
        .can_proceed()
    );

    for (label, invalid) in [
        ("IEEE-float", wav_bytes(3, 1, 44_100, 32)),
        ("three-channel", wav_bytes(1, 3, 44_100, 16)),
        ("48-kHz", wav_bytes(1, 2, 48_000, 16)),
        ("24-bit", wav_bytes(1, 2, 44_100, 24)),
    ] {
        let report = preflight_swf_publish(
            &project_with_sound("audio/wav", "wav", &invalid),
            &SwfPublishOptions::default(),
        );
        assert!(
            report
                .items
                .iter()
                .any(|item| item.code == "swf_sound_layout_unsupported"),
            "{label}: {:#?}",
            report.items
        );
        assert!(!report.can_proceed(), "{label}");
    }
}

#[test]
fn valid_png_and_intentional_basic_text_outlines_remain_publishable() {
    let mut project = Project::blank();
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes("image", "image.png", "image/png", &tiny_png())
            .unwrap(),
    );
    root_keyframe_mut(&mut project).elements.extend([
        Element {
            id: "image-placement".to_owned(),
            kind: ElementKind::Bitmap {
                resource_id: "image".to_owned(),
                width: 2,
                height: 2,
                smoothing: false,
            },
            ..Element::default()
        },
        Element {
            id: "static-text".to_owned(),
            kind: ElementKind::Text {
                text: "SWF".to_owned(),
                font_family: "Arial".to_owned(),
                font_size: 20.0,
                color: [10, 20, 30, 255],
            },
            ..Element::default()
        },
    ]);

    let default_report = preflight_swf_publish(&project, &SwfPublishOptions::default());
    assert!(default_report.can_proceed(), "{:#?}", default_report.items);
    assert!(
        default_report
            .items
            .iter()
            .any(|item| item.code == "swf_font3_embedded")
    );

    let mut non_bmp = project.clone();
    root_keyframe_mut(&mut non_bmp).elements.push(Element {
        id: "emoji-text".to_owned(),
        kind: ElementKind::Text {
            text: "SWF \u{1F680}".to_owned(),
            font_family: "Arial".to_owned(),
            font_size: 20.0,
            color: [10, 20, 30, 255],
        },
        ..Element::default()
    });
    let non_bmp_report = preflight_swf_publish(&non_bmp, &SwfPublishOptions::default());
    assert!(
        non_bmp_report
            .items
            .iter()
            .any(|item| item.code == "swf_text_requires_outlines")
    );
    assert!(!non_bmp_report.can_proceed());

    let options = SwfPublishOptions {
        outline_text: true,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };
    let report = preflight_swf_publish(&project, &options);
    assert!(report.can_proceed(), "{:#?}", report.items);
    assert!(
        report
            .items
            .iter()
            .any(|item| item.code == "swf_text_outlined")
    );
    let (bytes, _) = publish_swf_bytes(&project, &options).unwrap();
    assert!(bytes.starts_with(b"FWS"));
}

#[test]
fn system_outline_fallback_requires_glyph_coverage_and_reuses_the_installed_face() {
    let mut db = fontdb::Database::new();
    db.load_font_data(crate::fonts::fallback_font("Inter", false).to_vec());
    let mut fonts = SwfOutlineFonts {
        system: Some(db),
        ..Default::default()
    };
    let extract = |data: &[u8]| {
        crate::fonts::extract_text_outline_commands_with_diagnostics(
            "Caption",
            "Ω",
            "Requested",
            18.,
            Some(data),
        )
    };
    let a = fonts
        .system_outline("Requested", false, false, "Ω", extract)
        .unwrap();
    let b = fonts
        .system_outline("Requested", false, false, "Ω", extract)
        .unwrap();
    assert!(!a.is_empty() && a == b);
    assert_eq!(fonts.recent.len(), 1);
    assert_eq!(fonts.substitutions.len(), 1);
    assert!(
        fonts
            .system_outline("Requested", false, false, "中", |_| Ok(Vec::new()))
            .is_none()
    );
}

#[test]
fn text_outline_preflight_converts_named_fields_but_blocks_unpreserved_rich_visuals() {
    let mut project = Project::blank();
    root_keyframe_mut(&mut project).elements.push(Element {
        id: "editable-text".to_owned(),
        name: "scoreField".to_owned(),
        kind: ElementKind::TextBox {
            text: "42".to_owned(),
            width: 100.0,
            height: 30.0,
            format: neoflash_core::TextFormat::default(),
            word_wrap: false,
            multiline: false,
        },
        ..Element::default()
    });
    let outlined = SwfPublishOptions {
        outline_text: true,
        ..SwfPublishOptions::default()
    };
    let report = preflight_swf_publish(&project, &outlined);
    assert!(report.can_proceed(), "{:#?}", report.items);
    assert!(
        report
            .items
            .iter()
            .any(|item| item.code == "swf_text_outlined")
    );

    let element = &mut root_keyframe_mut(&mut project).elements[0];
    let ElementKind::TextBox { format, .. } = &mut element.kind else {
        unreachable!()
    };
    let field_options = neoflash_core::AuthoredTextFieldOptions {
        html: true,
        ..neoflash_core::AuthoredTextFieldOptions::default()
    };
    format.set_field_options(Some(field_options));
    let rich_report = preflight_swf_publish(&project, &outlined);
    assert!(rich_report.items.iter().any(|item| {
        item.code == "swf_text_outline_unsupported" && item.message.contains("HTML")
    }));
}

#[test]
fn baked_shape_tween_depth_band_reserves_both_endpoint_sets() {
    let element = Element::default();
    let timeline = Timeline {
        frame_count: 3,
        layers: vec![
            neoflash_core::Layer {
                id: "shape".to_owned(),
                name: "Shape".to_owned(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![
                    neoflash_core::Keyframe {
                        frame: 0,
                        elements: vec![element.clone(), element.clone()],
                        tween: Some(Easing::Linear),
                        tween_kind: TweenKind::Shape,
                        ..Default::default()
                    },
                    neoflash_core::Keyframe {
                        frame: 2,
                        elements: vec![element.clone(), element.clone(), element.clone()],
                        ..Default::default()
                    },
                ],
            },
            neoflash_core::Layer {
                id: "following".to_owned(),
                name: "Following".to_owned(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![neoflash_core::Keyframe {
                    frame: 0,
                    elements: vec![element],
                    ..Default::default()
                }],
            },
        ],
        scripts: Vec::new(),
        guides: Vec::new(),
    };

    assert_eq!(timeline_depth_bands(&timeline), Some(vec![(1, 5), (6, 6)]));
}

#[test]
fn button_depths_support_more_than_4096_single_element_layers_without_overflow() {
    let mut project = Project::blank();
    let element = Element {
        id: "button-art".to_owned(),
        kind: ElementKind::Rectangle {
            width: 8.0,
            height: 8.0,
            fill: [20, 90, 180, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let layers = (0..4097)
        .map(|index| {
            let mut layer_element = element.clone();
            layer_element.id = format!("button-art-{index}");
            neoflash_core::Layer {
                id: format!("button-layer-{index}"),
                name: format!("Button layer {index}"),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![neoflash_core::Keyframe {
                    frame: 0,
                    elements: vec![layer_element],
                    ..Default::default()
                }],
            }
        })
        .collect();
    project.symbols.push(Symbol {
        id: "large-button".to_owned(),
        name: "Large button".to_owned(),
        folder: String::new(),
        kind: SymbolKind::Button,
        timeline: Timeline {
            frame_count: 4,
            layers,
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    project.validate().unwrap();

    let options = SwfPublishOptions {
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };
    let (bytes, report) = publish_swf_bytes(&project, &options).unwrap();
    assert!(report.can_proceed());
    assert!(bytes.starts_with(b"FWS"));

    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "large-button-instance".to_owned(),
            kind: ElementKind::Instance {
                symbol_id: "large-button".to_owned(),
                playback: neoflash_core::InstancePlayback::Independent,
            },
            scripts: vec![ObjectScript {
                event: ObjectEvent::Click,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "stop();".to_owned(),
                },
            }],
            ..Element::default()
        });
    project.validate().unwrap();
    let avm1_options = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer8Avm1,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };
    let error = publish_swf_bytes(&project, &avm1_options).unwrap_err();
    assert!(
        error.to_string().contains("state records occupy"),
        "got: {error}"
    );
}

#[test]
fn clip_actions_use_spec_event_flag_bits() {
    let scripts = [
        ObjectScript {
            event: ObjectEvent::Load,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('load');".to_owned(),
            },
        },
        ObjectScript {
            event: ObjectEvent::EnterFrame,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('frame');".to_owned(),
            },
        },
        ObjectScript {
            event: ObjectEvent::Click,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('click');".to_owned(),
            },
        },
    ];
    let project = Project::blank();
    let encoded = encode_clip_actions(&project, &scripts).unwrap().unwrap();
    assert_eq!(
        u32::from_le_bytes(encoded[2..6].try_into().unwrap()),
        0x0803
    );
    let mut offset = 6;
    for expected in [0x01_u32, 0x02, 0x0800] {
        assert_eq!(
            u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap()),
            expected
        );
        let size = u32::from_le_bytes(encoded[offset + 4..offset + 8].try_into().unwrap()) as usize;
        offset += 8 + size;
    }
    assert_eq!(
        u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap()),
        0
    );
}

#[test]
fn clip_event_header_override_preserves_comma_quote_and_numeric_key_codes() {
    assert_eq!(
        parse_clip_event_header_override("// onClipEvent(keyPress \",\")\ntrace('comma');"),
        Some((0x0002_0000, Some(b',')))
    );
    assert_eq!(
        parse_clip_event_header_override("// onClipEvent(press, keyPress \"key#200\")"),
        Some((0x0002_0400, Some(200)))
    );
    assert_eq!(
        parse_clip_event_header_override("// onClipEvent(keyPress \"\"\")"),
        Some((0x0002_0000, Some(b'"')))
    );
    assert_eq!(
        parse_clip_event_header_override("// onClipEvent(keyPress \"key#999\")"),
        None
    );

    let project = Project::blank();
    for (source, expected_key) in [
        ("// onClipEvent(keyPress \",\")\ntrace('comma');", b','),
        (
            "// onClipEvent(keyPress \"key#200\")\ntrace('numeric');",
            200,
        ),
    ] {
        let encoded = encode_clip_actions(
            &project,
            &[ObjectScript {
                event: ObjectEvent::Load,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: source.to_owned(),
                },
            }],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(encoded[2..6].try_into().unwrap()),
            0x0002_0000
        );
        assert_eq!(
            u32::from_le_bytes(encoded[6..10].try_into().unwrap()),
            0x0002_0000
        );
        assert_eq!(encoded[14], expected_key);
    }
}

#[test]
fn avm1_record_lengths_refuse_oversized_strings_labels_and_conditional_blocks() {
    let mut project = Project::blank();
    let oversized_string = "x".repeat(usize::from(u16::MAX) - 1);
    let trace_source = format!("trace(\"{oversized_string}\");");
    let issue = swf_script_compilation_issue(
        &project,
        ScriptLanguage::ActionScript2,
        "",
        &trace_source,
        false,
        false,
    )
    .expect("oversized ActionPush must fail preflight");
    assert!(issue.contains("ActionPush"), "got: {issue}");
    assert!(compile_avm1_actions(&trace_source).is_err());

    let oversized_label = "l".repeat(usize::from(u16::MAX));
    let goto_source = format!("gotoAndStop(\"{oversized_label}\");");
    let issue = swf_script_compilation_issue(
        &project,
        ScriptLanguage::ActionScript1,
        "",
        &goto_source,
        false,
        false,
    )
    .expect("oversized ActionGotoLabel must fail preflight");
    assert!(issue.contains("ActionGotoLabel"), "got: {issue}");

    let many_actions = "stop();".repeat(usize::from(u16::MAX));
    let scripts = vec![
        ObjectScript {
            event: ObjectEvent::Click,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: many_actions,
            },
        },
        ObjectScript {
            event: ObjectEvent::Click,
            script: neoflash_core::Script {
                language: ScriptLanguage::ActionScript2,
                source: "on (rollOver) { play(); }".to_owned(),
            },
        },
    ];
    let error = encode_button2_cond_actions(&project, &scripts).unwrap_err();
    assert!(error.contains("BUTTONCONDACTION"), "got: {error}");

    let bad_btn_event = vec![ObjectScript {
        event: ObjectEvent::Load,
        script: neoflash_core::Script {
            language: ScriptLanguage::ActionScript2,
            source: "play();".to_owned(),
        },
    }];
    let bad_err = encode_button2_cond_actions(&project, &bad_btn_event).unwrap_err();
    assert!(bad_err.contains("Button instance"), "got: {bad_err}");

    let mut button = project.symbols[0].clone();
    button.id = "button".to_owned();
    button.name = "Button".to_owned();
    button.kind = SymbolKind::Button;
    project.symbols.push(button);
    root_keyframe_mut(&mut project).elements.push(Element {
        id: "button-instance".to_owned(),
        kind: ElementKind::Instance {
            symbol_id: "button".to_owned(),
            playback: neoflash_core::InstancePlayback::Independent,
        },
        scripts,
        ..Element::default()
    });
    let report = preflight_swf_publish(
        &project,
        &SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1),
    );
    assert!(
        report
            .items
            .iter()
            .any(|item| item.code == "swf_avm1_button_action_capacity")
    );
}

#[test]
fn bounded_script_preflight_rejects_trailing_as3_and_keeps_avm1_associativity() {
    let project = Project::blank();
    let valid_external = "package app {\nimport flash.display.MovieClip;\npublic class Main extends MovieClip {\ntrace(-1);\n}\n}";
    assert_eq!(
        swf_script_compilation_issue(
            &project,
            ScriptLanguage::ActionScript3,
            "",
            valid_external,
            true,
            false,
        ),
        None
    );
    assert_eq!(
        swf_script_compilation_issue(
            &project,
            ScriptLanguage::ActionScript3,
            "",
            "package app { public class Main { trace(1.5); } }",
            true,
            false,
        ),
        None
    );
    assert!(
        swf_script_compilation_issue(
            &project,
            ScriptLanguage::ActionScript3,
            "",
            "package app { public class Main { with (stage) { trace(1); } } }",
            true,
            false,
        )
        .is_some()
    );
    assert!(
        swf_script_compilation_issue(
            &project,
            ScriptLanguage::ActionScript3,
            "",
            "import flash.display.MovieClip; trace(1);",
            true,
            false,
        )
        .is_some()
    );
    assert!(
        swf_script_compilation_issue(
            &project,
            ScriptLanguage::ActionScript3,
            "",
            "evil public class Main extends MovieClip {",
            true,
            false,
        )
        .is_some()
    );

    let stmts = parse_swf_statements_strict("trace(8 - 4 - 2); trace(8 / 4 / 2);", false)
        .expect("parse left-associative binary ops");
    assert_eq!(stmts.len(), 2);
    assert_eq!(unquote_literal("\"a\" + \"b\""), None);

    assert!(compile_avm1_actions("gotoAndStop(65536);").is_ok());
    assert!(compile_avm1_actions("gotoAndStop(3.0);").is_ok());
    assert!(compile_avm1_actions("gotoAndStop(0);").is_err());
    assert!(compile_avm1_actions("gotoAndStop(-1);").is_err());
    let issue = swf_script_compilation_issue(
        &project,
        ScriptLanguage::ActionScript2,
        "",
        "gotoAndStop(65537);",
        false,
        false,
    )
    .expect("out-of-range numeric AVM1 frame must fail preflight");
    assert!(issue.contains("65536 one-based frames"), "got: {issue}");

    let unqual_goto = compile_avm1_actions("gotoAndStop(3);").unwrap();
    assert!(
        unqual_goto.contains(&0x81),
        "unqualified gotoAndStop uses ActionGotoFrame"
    );
    let local_goto = compile_avm1_actions("this.gotoAndStop(3);").unwrap();
    assert!(
        local_goto.contains(&0x52) && !local_goto.contains(&0x81),
        "this.gotoAndStop uses ActionCallMethod on `this` instead of lexical ActionGotoFrame"
    );
    let root_goto = compile_avm1_actions("_root.gotoAndStop(3);").unwrap();
    assert!(
        root_goto.contains(&0x52),
        "_root.gotoAndStop must call the root MovieClip method"
    );
    assert!(
        !root_goto.contains(&0x81),
        "_root.gotoAndStop must not retarget the current nested timeline"
    );
    let root_x = compile_avm1_actions("_root._x = 25;").unwrap();
    assert!(root_x.contains(&0x4f), "_root._x uses ActionSetMember");
    assert!(!root_x.contains(&0x1d), "_root._x is not a local variable");
}

#[test]
fn avm2_integer_emission_preserves_negative_and_large_i32_values() {
    let mut pool = AbcPoolBuilder::default();
    let mut code = SwfBitWriter::new();
    emit_avm2_int(&mut pool, &mut code, -1);
    assert_eq!(code.buf.first(), Some(&0x25));
    assert!(pool.ints.is_empty());

    let mut large_code = SwfBitWriter::new();
    emit_avm2_int(&mut pool, &mut large_code, i32::MAX);
    assert_eq!(large_code.buf, vec![0x2D, 0x01]);
    assert_eq!(pool.ints, vec![i32::MAX]);
    assert_eq!(pool.add_int(i32::MAX), 1);
    assert!(
        swf_script_compilation_issue(
            &Project::blank(),
            ScriptLanguage::ActionScript3,
            "",
            "trace(2147483647);",
            false,
            false,
        )
        .is_none()
    );
}

#[test]
fn avm1_and_avm2_logical_operators_short_circuit_rhs_evaluation() {
    let mut avm1_and = Vec::new();
    avm1_compile_expr(
        &mut avm1_and,
        &SwfExpr::Binary {
            op: SwfBinaryOp::And,
            lhs: Box::new(SwfExpr::Bool(false)),
            rhs: Box::new(SwfExpr::Int(7)),
        },
        &HashMap::new(),
    )
    .unwrap();
    assert!(avm1_and.contains(&0x9d), "AVM1 && uses ActionIf");
    assert!(!avm1_and.contains(&0x10), "AVM1 && is not eager ActionAnd");

    let mut avm1_or = Vec::new();
    avm1_compile_expr(
        &mut avm1_or,
        &SwfExpr::Binary {
            op: SwfBinaryOp::Or,
            lhs: Box::new(SwfExpr::Bool(true)),
            rhs: Box::new(SwfExpr::Int(7)),
        },
        &HashMap::new(),
    )
    .unwrap();
    assert!(avm1_or.contains(&0x9d), "AVM1 || uses ActionIf");
    assert!(!avm1_or.contains(&0x11), "AVM1 || is not eager ActionOr");

    fn compile(op: SwfBinaryOp, lhs: bool) -> Vec<u8> {
        let mut pool = AbcPoolBuilder::default();
        let mut code = SwfBitWriter::new();
        let mut state = Avm2CompilerState::default();
        let mut env = Avm2LexicalEnv {
            init_scope_depth: 1,
            needs_activation: false,
            uses_dxns: false,
            act_reg: None,
            active_catch_regs: Vec::new(),
            max_catch_scopes: 0,
            next_temp_reg: 1,
            declared_names: HashSet::new(),
            local_regs: HashMap::new(),
            own_slots: HashMap::new(),
            outer_slots: HashMap::new(),
            activation_traits: Vec::new(),
            exceptions: Vec::new(),
            flow_stack: Vec::new(),
            finally_stack: Vec::new(),
            imports: Avm2ImportCtx::default(),
            post_super: None,
        };
        compile_avm2_expr(
            &mut pool,
            &SwfExpr::Binary {
                op,
                lhs: Box::new(SwfExpr::Bool(lhs)),
                rhs: Box::new(SwfExpr::Int(7)),
            },
            &mut code,
            &mut state,
            &mut env,
        )
        .unwrap();
        code.buf
    }

    let and_code = compile(SwfBinaryOp::And, false);
    assert_eq!(&and_code[..4], &[0x27, 0x82, 0x2A, 0x12]);
    assert!(and_code.contains(&0x29), "non-short-circuit path drops lhs");
    assert!(
        !and_code.contains(&0x76),
        "logical && preserves operand value without convert_b"
    );
    assert!(!and_code.contains(&0xA8), "logical && is not eager bitand");

    let or_code = compile(SwfBinaryOp::Or, true);
    assert_eq!(&or_code[..4], &[0x26, 0x82, 0x2A, 0x11]);
    assert!(or_code.contains(&0x29), "non-short-circuit path drops lhs");
    assert!(
        !or_code.contains(&0x76),
        "logical || preserves operand value without convert_b"
    );
    assert!(!or_code.contains(&0xA9), "logical || is not eager bitor");
}
