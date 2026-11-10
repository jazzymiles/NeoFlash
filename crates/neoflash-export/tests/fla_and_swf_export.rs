use neoflash_core::{
    AssetWarpRig, BlendMode, CameraPose, ColorTransform, Easing, Element, ElementKind, FillRule,
    FilterSpec, FrameScript, FrameSound, GradientKind, GradientPaint, GradientStop, IkArmature,
    IkBone, InstancePlayback, Keyframe, Layer, LayerProperties, ObjectEvent, ObjectScript,
    PathCommand, Project, ResourceUnit, Script, ScriptLanguage, SoundEffect, SoundSync, SourceUnit,
    StrokeProfilePreset, StrokeWidthProfile, Symbol, SymbolKind, TextAlign, TextFormat, Timeline,
    Transform, TweenKind, TweenProperty, WarpPin, WarpPinKind,
};
use neoflash_export::{
    FlaExportOptions, FlaFeatureCategory, FlaFeatureDisposition, FlaTargetProfile, SwfCompression,
    SwfPublishOptions, SwfPublishProfile, SwfVmKind, export_fla_bytes_with_profile,
    export_fla_with_profile, export_legacy_binary_fla_bytes, import_fla_bytes, import_swf_bytes,
    play_swf_bytes, preflight_fla_export, preflight_fla_export_with_options, preflight_swf_publish,
    prepare_project_for_fla_profile, publish_swf, publish_swf_bytes,
};

fn make_tiny_png_bytes() -> Vec<u8> {
    let mut img = image::RgbaImage::new(4, 4);
    for y in 0..4 {
        for x in 0..4 {
            img.put_pixel(x, y, image::Rgba([220, 40, 90, 255]));
        }
    }
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .expect("encode tiny png");
    buf
}

fn make_tiny_wav_bytes() -> Vec<u8> {
    let sample_rate = 44100u32;
    let num_samples = 64u32;
    let data_len = num_samples * 2;
    let riff_len = 36 + data_len;
    let mut wav = Vec::with_capacity((8 + riff_len) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_len.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..num_samples {
        let sample: i16 = if i % 2 == 0 { 1200 } else { -1200 };
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn strip_project_scripts(project: &mut Project) {
    project.sources.clear();
    project.document_class = None;
    for symbol in &mut project.symbols {
        symbol.timeline.scripts.clear();
        for element in symbol
            .timeline
            .layers
            .iter_mut()
            .flat_map(|layer| &mut layer.keyframes)
            .flat_map(|key| &mut key.elements)
        {
            element.scripts.clear();
        }
    }
}

fn build_rich_authoring_project(script_lang: ScriptLanguage) -> Project {
    let png_res = ResourceUnit::from_bytes(
        "res_bmp_1",
        "bitmap_1.png",
        "image/png",
        &make_tiny_png_bytes(),
    )
    .expect("png resource");
    let wav_res = ResourceUnit::from_bytes(
        "res_snd_1",
        "sound_1.wav",
        "audio/wav",
        &make_tiny_wav_bytes(),
    )
    .expect("wav resource");

    let badge_symbol = Symbol {
        id: "sym_badge".to_owned(),
        name: "BadgeClip".to_owned(),
        folder: "UI/Icons".to_owned(),
        kind: SymbolKind::MovieClip,
        class_name: if script_lang == ScriptLanguage::ActionScript3 {
            Some("ui.BadgeClip".to_owned())
        } else {
            None
        },
        linkage_id: Some("badge_export".to_owned()),
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "badge_layer_1".to_owned(),
                name: "BadgeShape".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![
                        Element {
                            id: "badge_rect".to_owned(),
                            name: String::new(),
                            transform: Transform {
                                x: 10.0,
                                y: 12.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 60.0,
                                height: 40.0,
                                fill: [40, 120, 240, 255],
                                stroke: [10, 20, 40, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "badge_oval".to_owned(),
                            name: String::new(),
                            transform: Transform {
                                x: 20.0,
                                y: 15.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 30.0,
                                height: 30.0,
                                fill: [250, 200, 40, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };

    let button_symbol = Symbol {
        id: "sym_btn".to_owned(),
        name: "ActionBtn".to_owned(),
        folder: "UI/Buttons".to_owned(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "btn_layer".to_owned(),
                name: "States".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "btn_up".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 80.0,
                                height: 32.0,
                                fill: [30, 180, 90, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 1.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 1,
                        elements: vec![Element {
                            id: "btn_over".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 80.0,
                                height: 32.0,
                                fill: [60, 220, 120, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 1.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };

    let gradient_paint = GradientPaint {
        kind: GradientKind::Linear,
        matrix: [0.05, 0.0, 0.0, 0.05, 50.0, 50.0],
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [255, 60, 60, 255],
            },
            GradientStop {
                ratio: 255,
                color: [60, 60, 255, 255],
            },
        ],
        focal_point_ratio: 0.0,
        ..GradientPaint::default()
    };

    let text_fmt = TextFormat {
        font: "Arial".to_owned(),
        size: 18.0,
        color: 0x112233,
        bold: true,
        align: TextAlign::Center,
        ..TextFormat::default()
    };

    let mut btn_instance = Element {
        id: "inst_btn".to_owned(),
        name: "playButton".to_owned(),
        transform: Transform {
            x: 180.0,
            y: 220.0,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: "sym_btn".to_owned(),
            playback: InstancePlayback::Independent,
        },
        ..Element::default()
    };
    if matches!(
        script_lang,
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
    ) {
        btn_instance.scripts.push(ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: script_lang,
                source: "gotoAndPlay(2);".to_owned(),
            },
        });
    }

    let scene1 = Symbol {
        id: "scene_1".to_owned(),
        name: "Scene 1".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            layers: vec![
                Layer {
                    id: "layer_bg".to_owned(),
                    name: "ArtAndTween".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            label: "intro".to_owned(),
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Classic,
                            sound: Some(FrameSound {
                                resource: "res_snd_1".to_owned(),
                                sync: SoundSync::Event,
                                loops: 2,
                                loop_forever: false,
                                effect: SoundEffect::FadeIn,
                                envelope: None,
                                in_point: None,
                                out_point: None,
                            }),
                            elements: vec![
                                Element {
                                    id: "inst_badge".to_owned(),
                                    name: "heroBadge".to_owned(),
                                    transform: Transform {
                                        x: 40.0,
                                        y: 50.0,
                                        alpha: 0.9,
                                        color_transform: ColorTransform {
                                            multipliers: [1.0, 0.9, 0.8, 0.9],
                                            offsets: [10.0, 0.0, 0.0, 0.0],
                                        },
                                        ..Transform::default()
                                    },
                                    blend_mode: BlendMode::Multiply,
                                    filters: vec![
                                        FilterSpec::Blur {
                                            blur_x: 4.0,
                                            blur_y: 4.0,
                                            quality: 2,
                                        },
                                        FilterSpec::DropShadow {
                                            distance: 5.0,
                                            angle: 45.0,
                                            color: 0x000000,
                                            alpha: 0.75,
                                            blur_x: 6.0,
                                            blur_y: 6.0,
                                            strength: 1.0,
                                            quality: 2,
                                            inner: false,
                                            knockout: false,
                                            hide_object: false,
                                        },
                                    ],
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym_badge".to_owned(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                                btn_instance,
                                Element {
                                    id: "grad_shape".to_owned(),
                                    name: String::new(),
                                    transform: Transform {
                                        x: 100.0,
                                        y: 80.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 70.0, y: 0.0 },
                                            PathCommand::QuadraticTo {
                                                cx: 90.0,
                                                cy: 35.0,
                                                x: 70.0,
                                                y: 70.0,
                                            },
                                            PathCommand::LineTo { x: 0.0, y: 70.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 60, 60, 255],
                                        stroke: [20, 20, 20, 255],
                                        fill_gradient: Some(gradient_paint),
                                        stroke_gradient: None,
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "bmp_el".to_owned(),
                                    name: String::new(),
                                    transform: Transform {
                                        x: 260.0,
                                        y: 40.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Bitmap {
                                        resource_id: "res_bmp_1".to_owned(),
                                        width: 4,
                                        height: 4,
                                        smoothing: true,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "title_text".to_owned(),
                                    name: "headlineTxt".to_owned(),
                                    transform: Transform {
                                        x: 50.0,
                                        y: 10.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::TextBox {
                                        text: "NeoFlash Title".to_owned(),
                                        width: 220.0,
                                        height: 36.0,
                                        format: text_fmt,
                                        word_wrap: true,
                                        multiline: false,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 5,
                            label: "end_pose".to_owned(),
                            elements: vec![Element {
                                id: "inst_badge".to_owned(),
                                name: "heroBadge".to_owned(),
                                transform: Transform {
                                    x: 240.0,
                                    y: 150.0,
                                    scale_x: 1.25,
                                    scale_y: 1.25,
                                    rotation: 15.0,
                                    alpha: 1.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym_badge".to_owned(),
                                    playback: InstancePlayback::Independent,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "layer_morph".to_owned(),
                    name: "MorphLayer".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Linear),
                            tween_kind: TweenKind::Shape,
                            elements: vec![Element {
                                id: "morph_a".to_owned(),
                                kind: ElementKind::Rectangle {
                                    width: 40.0,
                                    height: 40.0,
                                    fill: [255, 0, 128, 255],
                                    stroke: [0, 0, 0, 255],
                                    stroke_width: 1.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 5,
                            elements: vec![Element {
                                id: "morph_b".to_owned(),
                                kind: ElementKind::Rectangle {
                                    width: 90.0,
                                    height: 30.0,
                                    fill: [0, 200, 255, 255],
                                    stroke: [0, 0, 0, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
            ],
            scripts: vec![FrameScript {
                frame: 0,
                script: Script {
                    language: script_lang,
                    source: "trace(\"frame1-ready\");\nstop();".to_owned(),
                },
            }],
            guides: Vec::new(),
        },
    };

    let scene2 = Symbol {
        id: "scene_2".to_owned(),
        name: "Scene 2".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![Layer {
                id: "s2_layer".to_owned(),
                name: "OutroLayer".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "scene2_start".to_owned(),
                    elements: vec![Element {
                        id: "s2_rect".to_owned(),
                        kind: ElementKind::Rectangle {
                            width: 100.0,
                            height: 60.0,
                            fill: [120, 80, 220, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };

    Project {
        format_version: 9,
        name: "RichExportTest".to_owned(),
        width: 640,
        height: 480,
        fps: 30,
        background: [24, 32, 48, 255],
        root: "scene_1".to_owned(),
        scenes: vec!["scene_1".to_owned(), "scene_2".to_owned()],
        symbols: vec![scene1, scene2, badge_symbol, button_symbol],
        library_folders: vec![
            "UI".to_owned(),
            "UI/Icons".to_owned(),
            "UI/Buttons".to_owned(),
        ],
        sources: vec![SourceUnit {
            id: "src_1".to_owned(),
            path: "scripts/MainHelper.as".to_owned(),
            language: script_lang,
            source: if script_lang == ScriptLanguage::ActionScript3 {
                "package {\n  import flash.display.MovieClip;\n  public class MainHelper extends MovieClip {\n    trace(\"helper-ctor\");\n  }\n}\n".to_owned()
            } else {
                "var helperReady = 1;\ntrace(\"helper-avm1\");\n".to_owned()
            },
        }],
        resources: vec![png_res, wav_res],
        document_class: if script_lang == ScriptLanguage::ActionScript3 {
            Some("MainHelper".to_owned())
        } else {
            None
        },
    }
}

#[test]
fn t188_all_seven_profiles_round_trip_and_never_mutate_live_project() {
    let temp_root = std::env::temp_dir().join(format!("neoflash-t188-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).expect("create temp dir");

    for profile in FlaTargetProfile::ALL {
        let script_lang = if profile.supports_as3() {
            ScriptLanguage::ActionScript3
        } else {
            ScriptLanguage::ActionScript2
        };
        let project = build_rich_authoring_project(script_lang);
        let original_snapshot = project.clone();

        let preflight = preflight_fla_export(&project, profile);
        assert!(
            preflight.can_export(),
            "Profile {:?} should pass preflight without blocking items: {:?}",
            profile,
            preflight.blocking_items()
        );

        let options = FlaExportOptions::default();
        if matches!(profile, FlaTargetProfile::UncompressedXfl) {
            let xfl_dir = temp_root.join("uncompressed_project");
            let report = export_fla_with_profile(&project, profile, &options, &xfl_dir)
                .expect("export uncompressed XFL directory");
            assert!(report.can_export());
            assert!(xfl_dir.join("DOMDocument.xml").is_file());
            assert!(xfl_dir.join("META-INF/metadata.xml").is_file());
            assert!(xfl_dir.join("PublishSettings.xml").is_file());
            assert!(xfl_dir.join("LIBRARY").is_dir());

            let dom_xml = std::fs::read_to_string(xfl_dir.join("DOMDocument.xml")).unwrap();
            assert!(dom_xml.contains(&format!("xflVersion=\"{}\"", profile.xfl_version())));
            assert!(dom_xml.contains(&format!("creatorInfo=\"{}\"", profile.creator_info())));

            let reimported =
                neoflash_import::import_project(&xfl_dir).expect("reimport XFL directory");
            assert_eq!(reimported.width, 640);
            assert_eq!(reimported.height, 480);
            assert_eq!(reimported.fps, 30);
            assert_eq!(reimported.background, [24, 32, 48, 255]);
            assert_eq!(reimported.scenes.len(), 2);
        } else {
            let file_name = format!("export_{:?}.fla", profile);
            let out_path = temp_root.join(file_name);
            let report = export_fla_with_profile(&project, profile, &options, &out_path)
                .expect("export FLA file with profile");
            assert!(report.can_export());
            assert!(out_path.is_file());

            let (bytes, byte_report) = export_fla_bytes_with_profile(&project, profile, &options)
                .expect("export FLA bytes with profile");
            assert!(byte_report.can_export());
            assert!(!bytes.is_empty());

            let reimported = import_fla_bytes(&bytes).expect("reimport FLA bytes");
            assert_eq!(reimported.width, 640);
            assert_eq!(reimported.height, 480);
            assert_eq!(reimported.fps, 30);
            assert_eq!(reimported.background, [24, 32, 48, 255]);
            assert_eq!(reimported.scenes.len(), 2);
            assert!(reimported.symbols.iter().any(|s| s.name == "BadgeClip"));
            assert!(reimported.symbols.iter().any(|s| s.name == "ActionBtn"));
        }

        // Verify live project was never mutated
        assert_eq!(project, original_snapshot);
    }

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn t188_preflight_blocks_js_ts_and_mixed_vm_unless_strip_enabled() {
    let mut project = build_rich_authoring_project(ScriptLanguage::ActionScript3);
    // Add a TypeScript frame script -> should block Flash/Animate export by default
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::TypeScript,
            source: "const x: number = 42;".to_owned(),
        },
    });
    // Also add an AS2 object script -> mixed AVM1 + AVM2
    project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .scripts
        .push(ObjectScript {
            event: ObjectEvent::Load,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "stop();".to_owned(),
            },
        });

    let report = preflight_fla_export(&project, FlaTargetProfile::AnimateCC);
    assert!(!report.can_export());
    assert!(
        report
            .blocking_items()
            .iter()
            .any(|i| i.code == "script_web_language" && i.category == FlaFeatureCategory::Scripts)
    );
    assert!(
        report
            .blocking_items()
            .iter()
            .any(|i| i.code == "mixed_avm1_avm2_scripts")
    );

    // Ensure atomic target protection: existing file at target path is untouched when blocked
    let temp_file =
        std::env::temp_dir().join(format!("neoflash-blocked-{}.fla", std::process::id()));
    std::fs::write(&temp_file, b"ORIGINAL_SENTINEL").unwrap();
    let res = export_fla_with_profile(
        &project,
        FlaTargetProfile::AnimateCC,
        &FlaExportOptions::default(),
        &temp_file,
    );
    assert!(res.is_err());
    assert_eq!(
        std::fs::read(&temp_file).unwrap(),
        b"ORIGINAL_SENTINEL",
        "blocked export must never overwrite target file"
    );

    // Enabling strip_unsupported_scripts converts blocking script items to Omitted and succeeds
    let strip_opts = FlaExportOptions {
        strip_unsupported_scripts: true,
        ..FlaExportOptions::default()
    };
    let strip_report =
        preflight_fla_export_with_options(&project, FlaTargetProfile::AnimateCC, &strip_opts);
    assert!(strip_report.can_export());
    assert!(strip_report.count_by_disposition(FlaFeatureDisposition::Omitted) >= 2);
    let ok_report = export_fla_with_profile(
        &project,
        FlaTargetProfile::AnimateCC,
        &strip_opts,
        &temp_file,
    )
    .expect("export succeeds when strip_unsupported_scripts is true");
    assert!(ok_report.can_export());
    let _ = std::fs::remove_file(&temp_file);
}

#[test]
fn t188_motion_ik_warp_camera_and_text_outlining_bake_on_derived_copy() {
    let mut project = build_rich_authoring_project(ScriptLanguage::ActionScript3);
    {
        let layer = &mut project.symbols[0].timeline.layers[0];
        layer.keyframes[0].tween = Some(Easing::Linear);
        layer.keyframes[0].tween_kind = TweenKind::Motion;
        layer.keyframes[0].camera = Some(CameraPose {
            x: 15.0,
            y: -10.0,
            zoom: 1.2,
            rotation: 5.0,
            ..CameraPose::default()
        });
        layer.keyframes[0].ik_armatures.push(IkArmature {
            id: "arm_1".to_owned(),
            name: "Arm".to_owned(),
            bones: vec![IkBone {
                id: "bone_1".to_owned(),
                element_id: "inst_badge".to_owned(),
                parent_bone_id: None,
                length: 50.0,
                min_angle: None,
                max_angle: None,
                stiffness: 1.0,
            }],
            effector_bone_id: "bone_1".to_owned(),
            target_pos: [120.0, 90.0],
            pole_pos: None,
        });
        layer.keyframes[0].asset_warps.push(AssetWarpRig {
            element_id: "grad_shape".to_owned(),
            pins: vec![WarpPin {
                id: "pin_1".to_owned(),
                kind: WarpPinKind::Open,
                rest_pos: [10.0, 10.0],
                pos: [18.0, 14.0],
                rotation: 0.0,
                stiffness: 1.0,
            }],
            envelop_subdivisions: 4,
        });
        // Variable-width stroke on grad_shape
        layer.keyframes[0].elements[2].set_stroke_width_profile(StrokeWidthProfile::from_preset(
            StrokeProfilePreset::TaperBoth,
        ));
        layer.keyframes[1].keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    }
    let live_snapshot = project.clone();

    let opts = FlaExportOptions {
        bake_unsupported_tweens: true,
        outline_unsupported_text: true,
        ..FlaExportOptions::default()
    };
    let (bytes, report) =
        export_fla_bytes_with_profile(&project, FlaTargetProfile::FlashCS6, &opts)
            .expect("bake and outline FLA export");
    assert!(report.can_export());
    assert!(report.count_by_disposition(FlaFeatureDisposition::Baked) >= 4);
    assert!(report.count_by_disposition(FlaFeatureDisposition::Outlined) >= 1);

    let reimported = import_fla_bytes(&bytes).expect("reimport baked CS6 FLA");
    // Outlined text should have been converted to vector paths (no TextBox left)
    let has_textbox = reimported.symbols.iter().any(|s| {
        s.timeline.layers.iter().any(|l| {
            l.keyframes.iter().any(|k| {
                k.elements
                    .iter()
                    .any(|e| matches!(e.kind, ElementKind::TextBox { .. }))
            })
        })
    });
    assert!(!has_textbox, "TextBox should be outlined into vector paths");

    // Live project must still have Motion tween, IK, AssetWarp, Camera, and TextBox untouched
    assert_eq!(project, live_snapshot);
}

#[test]
fn t189_legacy_binary_fla_cs4_and_flash8_round_trip() {
    for profile in [
        FlaTargetProfile::LegacyFlashCS4,
        FlaTargetProfile::LegacyFlash8,
    ] {
        let lang = if profile == FlaTargetProfile::LegacyFlashCS4 {
            ScriptLanguage::ActionScript3
        } else {
            ScriptLanguage::ActionScript2
        };
        let project = build_rich_authoring_project(lang);
        let (bytes, report) =
            export_legacy_binary_fla_bytes(&project, profile).expect("write legacy binary FLA");
        assert!(report.can_export());
        // Verify OLE2 CFB magic signature
        assert_eq!(
            &bytes[0..8],
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
        );

        let reimported = import_fla_bytes(&bytes).expect("reimport legacy binary FLA");
        assert_eq!(reimported.width, 640);
        assert_eq!(reimported.height, 480);
        assert_eq!(reimported.fps, 30);
        assert_eq!(reimported.background, [24, 32, 48, 255]);
        assert_eq!(reimported.scenes.len(), 2);
        assert!(reimported.symbols.iter().any(|s| s.name == "BadgeClip"));
        assert!(reimported.symbols.iter().any(|s| s.name == "ActionBtn"));
        assert!(!reimported.resources.is_empty());

        let root_sym = reimported.symbol(&reimported.root).expect("root scene");
        assert_eq!(root_sym.timeline.frame_count, 6);
        assert!(
            root_sym
                .timeline
                .scripts
                .iter()
                .any(|fs| fs.script.source.contains("stop()")),
            "frame script must round-trip in {:?}",
            profile
        );
        let first_kf = &root_sym.timeline.layers[0].keyframes[0];
        assert_eq!(first_kf.label, "intro");
        assert!(first_kf.sound.is_some());
        assert!(
            first_kf
                .elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Instance { .. }))
        );
        assert!(
            first_kf
                .elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::TextBox { .. }))
        );
    }
}

#[test]
fn t192_publish_swf_profiles_compressions_and_bytecode_execution() {
    // 1. Test FlashPlayer8Avm1 across FWS, CWS, and ZWS + AVM1 bytecode execution
    let avm1_project = build_rich_authoring_project(ScriptLanguage::ActionScript2);
    for compression in [
        SwfCompression::UncompressedFws,
        SwfCompression::DeflateCws,
        SwfCompression::LzmaZws,
    ] {
        let opts = SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            compression,
            outline_text: true,
            ..SwfPublishOptions::default()
        };
        let (swf_bytes, report) =
            publish_swf_bytes(&avm1_project, &opts).expect("publish AVM1 SWF");
        assert!(report.can_proceed());
        match compression {
            SwfCompression::UncompressedFws => {
                assert_eq!(&swf_bytes[0..3], b"FWS");
                assert_eq!(swf_bytes[3], 8);
            }
            SwfCompression::DeflateCws => {
                assert_eq!(&swf_bytes[0..3], b"CWS");
                assert_eq!(swf_bytes[3], 8);
            }
            SwfCompression::LzmaZws => {
                assert_eq!(&swf_bytes[0..3], b"ZWS");
                assert!(swf_bytes[3] >= 13);
            }
        }

        let reimported = import_swf_bytes(&swf_bytes).expect("reimport AVM1 SWF");
        assert_eq!(reimported.width, 640);
        assert_eq!(reimported.height, 480);
        assert_eq!(reimported.fps, 30);
        assert_eq!(reimported.background, [24, 32, 48, 255]);

        let playback = play_swf_bytes(&swf_bytes, 6).expect("play AVM1 SWF");
        assert_eq!(playback.vm_kind, SwfVmKind::Avm1);
        assert!(
            !playback.frames_executed[0].playing,
            "stop() action on frame 0 must stop playback"
        );
        assert!(
            playback.trace_output.iter().any(|t| t == "frame1-ready"),
            "AVM1 trace output must include frame1-ready, got {:?}",
            playback.trace_output
        );
        assert!(!playback.frames_executed.is_empty());
        assert!(!playback.frames_executed[0].display_list.is_empty());
        assert!(!playback.audio_events.is_empty());
    }

    // 2. Test FlashPlayer9Avm2 and FlashPlayer10Avm2 + AVM2 DoABC2 / SymbolClass execution
    let avm2_project = build_rich_authoring_project(ScriptLanguage::ActionScript3);
    for profile in [
        SwfPublishProfile::FlashPlayer9Avm2,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        let opts = SwfPublishOptions {
            profile,
            compression: SwfCompression::DeflateCws,
            outline_text: true,
            ..SwfPublishOptions::default()
        };
        let (swf_bytes, report) =
            publish_swf_bytes(&avm2_project, &opts).expect("publish AVM2 SWF");
        assert!(report.can_proceed());
        assert_eq!(&swf_bytes[0..3], b"CWS");
        assert_eq!(swf_bytes[3], profile.swf_version());

        let reimported = import_swf_bytes(&swf_bytes).expect("reimport AVM2 SWF");
        assert_eq!(reimported.width, 640);
        assert_eq!(reimported.height, 480);
        assert_eq!(reimported.fps, 30);
        assert_eq!(reimported.document_class.as_deref(), Some("MainHelper"));
        assert!(!reimported.sources.is_empty());

        let playback = play_swf_bytes(&swf_bytes, 6).expect("play AVM2 SWF");
        assert_eq!(playback.vm_kind, SwfVmKind::Avm2);
        assert!(!playback.frames_executed[0].playing);
        assert!(
            playback.trace_output.iter().any(|t| t == "frame1-ready"),
            "AVM2 addFrameScript trace must execute, got {:?}",
            playback.trace_output
        );
    }

    // 3. Test atomic publish_swf file writer
    let tmp_swf = std::env::temp_dir().join(format!("neoflash-pub-{}.swf", std::process::id()));
    let pub_report = publish_swf(
        &avm2_project,
        &tmp_swf,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            outline_text: true,
            ..SwfPublishOptions::default()
        },
    )
    .expect("publish_swf to disk");
    assert!(pub_report.can_proceed());
    assert!(tmp_swf.is_file());
    let _ = std::fs::remove_file(&tmp_swf);
}

#[test]
fn t192_swf_preflight_blocks_incompatible_scripts_unless_scriptless_mode() {
    let mut project = build_rich_authoring_project(ScriptLanguage::ActionScript3);
    // Publishing AS3 project to FlashPlayer8Avm1 must block by default
    let avm1_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let report = preflight_swf_publish(&project, &avm1_opts);
    assert!(!report.can_proceed());
    assert!(publish_swf_bytes(&project, &avm1_opts).is_err());

    // With scriptless_mode = true, publishing visual-only SWF succeeds
    let scriptless_opts = SwfPublishOptions {
        scriptless_mode: true,
        outline_text: true,
        ..avm1_opts
    };
    let (swf_bytes, ok_report) =
        publish_swf_bytes(&project, &scriptless_opts).expect("scriptless SWF publish");
    assert!(ok_report.can_proceed());
    assert!(import_swf_bytes(&swf_bytes).is_ok());

    // Adding a JavaScript frame script blocks AVM2 publish unless scriptless_mode is true
    project.symbols[0].timeline.scripts.push(FrameScript {
        frame: 2,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "console.log('hi');".to_owned(),
        },
    });
    let avm2_opts = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer10Avm2,
        outline_text: true,
        ..SwfPublishOptions::default()
    };
    assert!(!preflight_swf_publish(&project, &avm2_opts).can_proceed());
    assert!(publish_swf_bytes(&project, &avm2_opts).is_err());

    let mut unsupported = Project::demo();
    strip_project_scripts(&mut unsupported);
    let root_id = unsupported.root.clone();
    unsupported
        .symbol_mut(&root_id)
        .expect("demo root")
        .timeline
        .scripts = vec![FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "if (ready) { trace(\"lost\"); }".to_owned(),
        },
    }];
    let unsupported_report = preflight_swf_publish(&unsupported, &avm2_opts);
    assert!(!unsupported_report.can_proceed());
    assert!(unsupported_report.items.iter().any(|item| {
        item.code == "swf_avm2_frame_script_unsupported_syntax"
            && item.location.contains("frame[0]")
    }));

    unsupported
        .symbol_mut(&root_id)
        .expect("demo root")
        .timeline
        .scripts[0]
        .script
        .source = "var x:Number = 1.5; x = 2; trace(x);".to_owned();
    let lossy_subset = preflight_swf_publish(&unsupported, &avm2_opts);
    assert!(lossy_subset.items.iter().any(|item| {
        item.code == "swf_avm2_frame_script_unsupported_syntax"
            && item.message.contains("bounded SWF compiler")
    }));
    assert!(publish_swf_bytes(&unsupported, &avm2_opts).is_err());

    unsupported
        .symbol_mut(&root_id)
        .expect("demo root")
        .timeline
        .scripts[0]
        .script
        .source = "trace(\"kept\"); stop(); gotoAndStop(1);".to_owned();
    let supported_report = preflight_swf_publish(&unsupported, &avm2_opts);
    assert!(
        supported_report.can_proceed(),
        "supported bounded statements should pass: {:?}",
        supported_report.items
    );
}

#[test]
fn t199_swf_preserves_dense_layers_and_changed_artwork_for_stable_ids() {
    let mut dense = Project::demo();
    dense.sources.clear();
    dense.document_class = None;
    let root_id = dense.root.clone();
    let dense_root = dense.symbol_mut(&root_id).expect("demo root");
    dense_root.timeline = Timeline {
        frame_count: 1,
        layers: vec![Layer {
            id: "dense-layer".to_owned(),
            name: "Dense".to_owned(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                elements: (0..16)
                    .map(|index| Element {
                        id: format!("dense-{index}"),
                        name: format!("dense-{index}"),
                        transform: Transform {
                            x: f64::from(index) * 12.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 10.0,
                            height: 10.0,
                            fill: [index * 8, 80, 180, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    })
                    .collect(),
                ..Keyframe::default()
            }],
        }],
        scripts: Vec::new(),
        guides: Vec::new(),
    };
    let options = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer10Avm2,
        compression: SwfCompression::UncompressedFws,
        scriptless_mode: true,
        ..SwfPublishOptions::default()
    };
    let (dense_bytes, _) = publish_swf_bytes(&dense, &options).expect("publish dense SWF");
    let dense_playback = play_swf_bytes(&dense_bytes, 1).expect("play dense SWF");
    assert_eq!(dense_playback.frames_executed[0].display_list.len(), 16);
    let depths: std::collections::BTreeSet<_> = dense_playback.frames_executed[0]
        .display_list
        .iter()
        .map(|entry| entry.depth)
        .collect();
    assert_eq!(depths.len(), 16);

    let mut changed = Project::demo();
    changed.sources.clear();
    changed.document_class = None;
    let root_id = changed.root.clone();
    let changed_root = changed.symbol_mut(&root_id).expect("demo root");
    let shape = |frame, fill| Keyframe {
        frame,
        elements: vec![Element {
            id: "stable-id".to_owned(),
            name: "stable-art".to_owned(),
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill,
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        }],
        ..Keyframe::default()
    };
    changed_root.timeline = Timeline {
        frame_count: 2,
        layers: vec![Layer {
            id: "changed-layer".to_owned(),
            name: "Changed".to_owned(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![shape(0, [255, 0, 0, 255]), shape(1, [0, 0, 255, 255])],
        }],
        scripts: Vec::new(),
        guides: Vec::new(),
    };
    let (changed_bytes, _) = publish_swf_bytes(&changed, &options).expect("publish changed SWF");
    let changed_playback = play_swf_bytes(&changed_bytes, 2).expect("play changed SWF");
    assert_eq!(changed_playback.frames_executed.len(), 2);
    assert_ne!(
        changed_playback.frames_executed[0].display_list[0].character_id,
        changed_playback.frames_executed[1].display_list[0].character_id,
        "a stable element id must not reuse stale SWF artwork"
    );
}

#[test]
fn t199_fla_preflight_never_claims_unserialized_filters_or_video_are_exact() {
    let mut project = Project::demo();
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).expect("demo root");
    let keyframe = &mut root.timeline.layers[0].keyframes[0];
    keyframe.elements[0].filters = vec![FilterSpec::ColorMatrix {
        matrix: neoflash_core::IDENTITY_COLOR_MATRIX,
    }];

    for profile in FlaTargetProfile::ALL {
        let report = preflight_fla_export(&project, profile);
        assert!(report.items.iter().any(|item| {
            item.code == "filter_not_serialized"
                && item.disposition == FlaFeatureDisposition::Blocking
                && item.message.contains("ColorMatrix")
        }));
        assert!(!report.can_export());
    }

    let root = project.symbol_mut(&root_id).expect("demo root");
    let keyframe = &mut root.timeline.layers[0].keyframes[0];
    let blur = FilterSpec::Blur {
        blur_x: 4.0,
        blur_y: 4.0,
        quality: 1,
    };
    keyframe.elements[0].filters = vec![blur.clone(); 3];
    keyframe.filters = vec![blur.clone(); 3];
    root.timeline.layers[0].properties.filters = vec![blur; 3];
    let overflow = preflight_fla_export(&project, FlaTargetProfile::AnimateCC);
    assert!(overflow.items.iter().any(|item| {
        item.code == "effective_filter_stack_too_large"
            && item.disposition == FlaFeatureDisposition::Blocking
    }));

    let root = project.symbol_mut(&root_id).expect("demo root");
    root.timeline.layers[0].properties.filters.clear();
    let keyframe = &mut root.timeline.layers[0].keyframes[0];
    keyframe.filters.clear();
    keyframe.elements[0].filters.clear();
    keyframe.elements[0].kind = ElementKind::Video {
        width: 320.0,
        height: 180.0,
    };
    let report = preflight_fla_export(&project, FlaTargetProfile::AnimateCC);
    let video = report
        .items
        .iter()
        .find(|item| item.code == "video_instance")
        .expect("video finding");
    assert_eq!(video.disposition, FlaFeatureDisposition::Approximated);
    assert_ne!(video.disposition, FlaFeatureDisposition::Exact);

    let strict = FlaExportOptions {
        allow_approximations: false,
        ..FlaExportOptions::default()
    };
    let strict_report =
        preflight_fla_export_with_options(&project, FlaTargetProfile::AnimateCC, &strict);
    assert!(strict_report.items.iter().any(|item| {
        item.code == "video_instance" && item.disposition == FlaFeatureDisposition::Blocking
    }));

    let mut filter_project = Project::demo();
    strip_project_scripts(&mut filter_project);
    let filter_root_id = filter_project.root.clone();
    let mut disabled_blur = FilterSpec::Blur {
        blur_x: 6.0,
        blur_y: 4.0,
        quality: 1,
    };
    disabled_blur.set_enabled(false);
    filter_project
        .symbol_mut(&filter_root_id)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes[0]
        .elements[0]
        .filters = vec![disabled_blur];
    filter_project.upgrade_format_for_features();
    let xfl_dir = std::env::temp_dir().join(format!(
        "neoflash-t199-disabled-filter-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&xfl_dir);
    export_fla_with_profile(
        &filter_project,
        FlaTargetProfile::UncompressedXfl,
        &FlaExportOptions::default(),
        &xfl_dir,
    )
    .expect("export disabled filter to XFL");
    let dom = std::fs::read_to_string(xfl_dir.join("DOMDocument.xml")).unwrap();
    assert!(dom.contains("<BlurFilter"));
    assert!(dom.contains("quality=\"1\""));
    assert!(dom.contains("isEnabled=\"false\""));
    let _ = std::fs::remove_dir_all(&xfl_dir);

    let stops: Vec<u32> = (0..16).map(|index| index * 0x010101).collect();
    let mut legacy = Project::demo();
    strip_project_scripts(&mut legacy);
    let legacy_filter_target = legacy
        .symbols
        .iter_mut()
        .flat_map(|symbol| &mut symbol.timeline.layers)
        .flat_map(|layer| &mut layer.keyframes)
        .flat_map(|key| &mut key.elements)
        .find(|element| matches!(element.kind, ElementKind::Instance { .. }))
        .expect("demo has a symbol instance for legacy filter serialization");
    legacy_filter_target.filters = vec![FilterSpec::GradientGlow {
        distance: 4.0,
        angle: 45.0,
        colors: stops,
        alphas: vec![1.0; 16],
        ratios: (0..16).map(|index| index * 17).collect(),
        blur_x: 4.0,
        blur_y: 4.0,
        strength: 1.0,
        quality: 1,
        placement: "outer".to_owned(),
        knockout: false,
    }];
    legacy.upgrade_format_for_features();
    let legacy_report = preflight_fla_export(&legacy, FlaTargetProfile::LegacyFlashCS4);
    assert!(legacy_report.items.iter().any(|item| {
        item.code == "legacy_filter_gradient_stop_limit"
            && item.disposition == FlaFeatureDisposition::Blocking
    }));
    assert!(
        export_legacy_binary_fla_bytes(&legacy, FlaTargetProfile::LegacyFlashCS4).is_err(),
        "a valid 16-stop NeoFlash filter must not be silently truncated to 15 in legacy FLA"
    );
}

#[test]
fn t199_fla_preflight_reports_frame_rate_clamp_before_export() {
    let mut project = Project::demo();
    project.fps = 121;

    let report = preflight_fla_export(&project, FlaTargetProfile::AnimateCC);
    assert!(report.items.iter().any(|item| {
        item.code == "frame_rate_clamped"
            && item.location == "Project.fps"
            && item.disposition == FlaFeatureDisposition::Approximated
            && item.message.contains("120 fps")
    }));
    assert!(!report.items.iter().any(|item| {
        item.code == "document_frame_rate_exact" && item.disposition == FlaFeatureDisposition::Exact
    }));

    let strict = FlaExportOptions {
        allow_approximations: false,
        ..FlaExportOptions::default()
    };
    let strict_report =
        preflight_fla_export_with_options(&project, FlaTargetProfile::AnimateCC, &strict);
    assert!(strict_report.items.iter().any(|item| {
        item.code == "frame_rate_clamped" && item.disposition == FlaFeatureDisposition::Blocking
    }));

    let (prepared, prepared_report) = prepare_project_for_fla_profile(
        &project,
        FlaTargetProfile::AnimateCC,
        &FlaExportOptions::default(),
    );
    assert_eq!(prepared.fps, 120);
    assert!(prepared_report.items.iter().any(|item| {
        item.code == "frame_rate_clamped" && item.disposition == FlaFeatureDisposition::Approximated
    }));

    project.fps = 120;
    let exact = preflight_fla_export(&project, FlaTargetProfile::AnimateCC);
    assert!(exact.items.iter().any(|item| {
        item.code == "document_frame_rate_exact" && item.disposition == FlaFeatureDisposition::Exact
    }));
    assert!(
        !exact
            .items
            .iter()
            .any(|item| item.code == "frame_rate_clamped")
    );
}
