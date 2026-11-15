//! Comprehensive SWF export, re-import, native bytecode playback, `.nflash` JSON round-trip,
//! and HTML5 bundle verification for `T-325`.

use neoflash_core::{
    AuthoredBitmapFill, BlendMode, ColorTransform, Element, ElementKind, FillRule, FilterSpec,
    FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode,
    ObjectEvent, ObjectScript, PathCommand, Project, ResourceUnit, Script, ScriptLanguage, Symbol,
    SymbolKind, Timeline, Transform,
};
use neoflash_export::{
    FlaFeatureDisposition, SwfCompression, SwfPublishOptions, SwfPublishProfile, export_project,
    import_swf_bytes, play_swf_bytes, preflight_swf_publish, publish_swf_bytes,
};
use neoflash_import::{Severity, SwfBytecodePlayer, import_project, inspect};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        static ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-export-swf-fidelity-{prefix}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_mock_runtime(path: &Path) {
    fs::create_dir_all(path).expect("create mock runtime dir");
    fs::write(path.join("neoflash_web.js"), "var NeoFlashWasm;").expect("write neoflash_web.js");
    fs::write(path.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
        .expect("write neoflash_web_bg.wasm");
}

fn build_t325_authored_project(script_lang: ScriptLanguage) -> Project {
    let mut panel_sym = Symbol {
        id: "sym_panel".to_owned(),
        name: "ScaledPanel".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: if script_lang == ScriptLanguage::ActionScript3 {
            Some("ui.ScaledPanel".to_owned())
        } else {
            None
        },
        linkage_id: if script_lang == ScriptLanguage::ActionScript2 {
            Some("panel_export".to_owned())
        } else {
            None
        },
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "panel_layer".to_owned(),
                name: "PanelArt".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "panel_rect".to_owned(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 32.0,
                            height: 32.0,
                            fill: [60, 140, 220, 255],
                            stroke: [20, 40, 80, 255],
                            stroke_width: 2.0,
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
    panel_sym.set_scale9_grid(Some([4.0, 4.0, 24.0, 24.0]));

    let fx_sym = Symbol {
        id: "sym_fx".to_owned(),
        name: "FxBadge".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "fx_layer".to_owned(),
                name: "FxArt".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "fx_ellipse".to_owned(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Ellipse {
                            width: 24.0,
                            height: 24.0,
                            fill: [240, 120, 40, 255],
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

    let btn_sym = Symbol {
        id: "sym_btn".to_owned(),
        name: "ActionBtn".to_owned(),
        folder: String::new(),
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
                                width: 40.0,
                                height: 20.0,
                                fill: [80, 180, 100, 255],
                                stroke: [0, 0, 0, 255],
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
                            blend_mode: BlendMode::Screen,
                            filters: vec![FilterSpec::Glow {
                                color: 0x00FF88,
                                alpha: 1.0,
                                blur_x: 4.0,
                                blur_y: 4.0,
                                strength: 2.0,
                                quality: 2,
                                inner: false,
                                knockout: false,
                            }],
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 20.0,
                                fill: [110, 220, 130, 255],
                                stroke: [0, 0, 0, 255],
                                stroke_width: 1.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 2,
                        elements: vec![Element {
                            id: "btn_down".to_owned(),
                            blend_mode: BlendMode::Add,
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 20.0,
                                fill: [50, 130, 70, 255],
                                stroke: [0, 0, 0, 255],
                                stroke_width: 1.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![Element {
                            id: "btn_hit".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 20.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
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

    let hero_f0_filters = vec![
        FilterSpec::DropShadow {
            distance: 4.0,
            angle: 45.0,
            color: 0x112233,
            alpha: 0.75,
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 1.5,
            quality: 2,
            inner: false,
            knockout: false,
            hide_object: false,
        },
        FilterSpec::Blur {
            blur_x: 3.0,
            blur_y: 5.0,
            quality: 2,
        },
        FilterSpec::Glow {
            color: 0xFF3366,
            alpha: 1.0,
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 2.0,
            quality: 1,
            inner: true,
            knockout: false,
        },
        FilterSpec::Bevel {
            distance: 3.0,
            angle: 45.0,
            highlight_color: 0xFFFFFF,
            highlight_alpha: 1.0,
            shadow_color: 0x000000,
            shadow_alpha: 1.0,
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 1.0,
            quality: 2,
            placement: "inner".to_owned(),
            knockout: false,
        },
    ];

    let fx_f0_filters = vec![
        FilterSpec::GradientGlow {
            distance: 2.0,
            angle: 90.0,
            colors: vec![0x000000, 0x00FFAA],
            alphas: vec![0.0, 1.0],
            ratios: vec![0, 255],
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 1.5,
            quality: 2,
            placement: "outer".to_owned(),
            knockout: false,
        },
        FilterSpec::Convolution {
            matrix_x: 3,
            matrix_y: 3,
            matrix: vec![0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0],
            divisor: 1.0,
            bias: 0.0,
            preserve_alpha: true,
            clamp: true,
            color: 0x000000,
            alpha: 0.0,
        },
        FilterSpec::ColorMatrix {
            matrix: [
                1.0, 0.0, 0.0, 0.0, 10.0, //
                0.0, 1.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        },
        FilterSpec::GradientBevel {
            distance: 2.0,
            angle: 45.0,
            colors: vec![0xFF0000, 0xFFFFFF, 0x0000FF],
            alphas: vec![1.0, 0.5, 1.0],
            ratios: vec![0, 128, 255],
            blur_x: 3.0,
            blur_y: 3.0,
            strength: 1.0,
            quality: 1,
            placement: "full".to_owned(),
            knockout: false,
        },
    ];

    let hero_scripts = if script_lang == ScriptLanguage::ActionScript2 {
        vec![
            ObjectScript {
                event: ObjectEvent::Load,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('clip-load');".to_owned(),
                },
            },
            ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('clip-ef');".to_owned(),
                },
            },
            ObjectScript {
                event: ObjectEvent::Click,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "// onClipEvent(press)\ntrace('clip-press');".to_owned(),
                },
            },
        ]
    } else {
        Vec::new()
    };

    let btn_scripts = if script_lang == ScriptLanguage::ActionScript2 {
        vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "on (release) { trace('btn-release'); }".to_owned(),
            },
        }]
    } else {
        Vec::new()
    };

    let root_scripts = if script_lang == ScriptLanguage::ActionScript2 {
        vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "var f0 = 10 + 5; trace('f0=' + f0);".to_owned(),
                },
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('f1=' + heroPanel._x); stop();".to_owned(),
                },
            },
        ]
    } else {
        vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "var f0:int = 10 + 5; trace('as3-f0=' + f0);".to_owned(),
                },
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "trace('as3-f1=' + heroPanel.x); stop();".to_owned(),
                },
            },
        ]
    };

    let root_sym = Symbol {
        id: "root".to_owned(),
        name: "Scene 1".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![
                // Layer 0 (bottom): Masked Layer A
                Layer {
                    id: "layer_masked_a".to_owned(),
                    name: "MaskedA".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Normal,
                        mask: Some("layer_mask".to_owned()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "masked_rect_a".to_owned(),
                            kind: ElementKind::Rectangle {
                                width: 80.0,
                                height: 40.0,
                                fill: [200, 50, 50, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 1: Masked Layer B
                Layer {
                    id: "layer_masked_b".to_owned(),
                    name: "MaskedB".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Normal,
                        mask: Some("layer_mask".to_owned()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "masked_path_b".to_owned(),
                            kind: ElementKind::Path {
                                commands: vec![
                                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                    PathCommand::LineTo { x: 60.0, y: 0.0 },
                                    PathCommand::LineTo { x: 60.0, y: 60.0 },
                                    PathCommand::Close,
                                ],
                                fill_rule: FillRule::EvenOdd,
                                fill: [40, 200, 120, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 2: Static geometry Mask Layer clipping MaskedA + MaskedB
                Layer {
                    id: "layer_mask".to_owned(),
                    name: "ClipMask".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        mask_mode: MaskMode::Geometry,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "mask_window".to_owned(),
                            transform: Transform {
                                x: 8.0,
                                y: 8.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 50.0,
                                height: 50.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 3: Multi-frame heroPanel testing Frame 0 -> Frame 1 `is_move` resets!
                Layer {
                    id: "layer_hero".to_owned(),
                    name: "HeroLayer".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "inst_hero".to_owned(),
                                name: "heroPanel".to_owned(),
                                transform: Transform {
                                    x: 40.0,
                                    y: 50.0,
                                    alpha: 0.5,
                                    color_transform: ColorTransform {
                                        multipliers: [0.8, 1.0, 1.0, 1.0],
                                        offsets: [20.0, 0.0, 0.0, 0.0],
                                    },
                                    ..Transform::default()
                                },
                                visible: false,
                                cache_as_bitmap: true,
                                blend_mode: BlendMode::Multiply,
                                filters: hero_f0_filters,
                                scripts: hero_scripts.clone(),
                                kind: ElementKind::Instance {
                                    symbol_id: "sym_panel".to_owned(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 1,
                            elements: vec![Element {
                                id: "inst_hero".to_owned(),
                                name: "heroPanel".to_owned(),
                                transform: Transform {
                                    x: 125.0,
                                    y: 75.0,
                                    alpha: 1.0,
                                    color_transform: ColorTransform::default(),
                                    ..Transform::default()
                                },
                                visible: true,
                                cache_as_bitmap: false,
                                blend_mode: BlendMode::Normal,
                                filters: Vec::new(),
                                scripts: hero_scripts,
                                kind: ElementKind::Instance {
                                    symbol_id: "sym_panel".to_owned(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 4: Secondary instance carrying GradientGlow/Convolution/ColorMatrix/GradientBevel + Button
                Layer {
                    id: "layer_fx_btn".to_owned(),
                    name: "FxAndBtn".to_owned(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![
                            Element {
                                id: "inst_fx".to_owned(),
                                name: "fxPanel".to_owned(),
                                transform: Transform {
                                    x: 180.0,
                                    y: 40.0,
                                    ..Transform::default()
                                },
                                blend_mode: BlendMode::Overlay,
                                filters: fx_f0_filters,
                                kind: ElementKind::Instance {
                                    symbol_id: "sym_fx".to_owned(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "inst_btn".to_owned(),
                                name: "goBtn".to_owned(),
                                transform: Transform {
                                    x: 240.0,
                                    y: 120.0,
                                    ..Transform::default()
                                },
                                scripts: btn_scripts,
                                kind: ElementKind::Instance {
                                    symbol_id: "sym_btn".to_owned(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    }],
                },
            ],
            scripts: root_scripts,
            guides: Vec::new(),
        },
    };

    let mut project = Project::blank();
    project.name = "T325 Roundtrip".to_owned();
    project.width = 400;
    project.height = 300;
    project.fps = 24;
    project.root = "root".to_owned();
    project.scenes = Vec::new();
    project.symbols = vec![root_sym, panel_sym, fx_sym, btn_sym];
    project.upgrade_format_for_features();
    project.validate().expect("authored project must validate");
    project
}

#[test]
fn authored_filters_blends_masks_is_move_resets_scaling_grid_and_scripts_swf_roundtrip() {
    let compressions = [
        (SwfCompression::UncompressedFws, *b"FWS"),
        (SwfCompression::DeflateCws, *b"CWS"),
        (SwfCompression::LzmaZws, *b"ZWS"),
    ];

    // 1. AVM1 (AS2) across FWS, CWS, and ZWS
    let avm1_project = build_t325_authored_project(ScriptLanguage::ActionScript2);
    for (compression, expected_magic) in compressions {
        let opts = SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            compression,
            outline_text: true,
            ..SwfPublishOptions::default()
        };
        let (swf_bytes, report) =
            publish_swf_bytes(&avm1_project, &opts).expect("publish AVM1 SWF");
        assert!(report.can_proceed(), "preflight must succeed: {report:?}");
        assert_eq!(&swf_bytes[0..3], &expected_magic);

        // Native SWF player: verify Frame 0 filters/blend/visibility/alpha/mask + Frame 1 `is_move` resets!
        let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).expect("parse AVM1 SWF");
        let f0 = player.step_frame();
        let hero_f0 = f0
            .display_list
            .iter()
            .find(|e| e.name == "heroPanel")
            .expect("heroPanel on frame 0");
        assert_eq!(hero_f0.filters.len(), 4);
        assert!(matches!(hero_f0.filters[0], FilterSpec::DropShadow { .. }));
        assert!(matches!(hero_f0.filters[1], FilterSpec::Blur { .. }));
        assert!(matches!(hero_f0.filters[2], FilterSpec::Glow { .. }));
        assert!(matches!(hero_f0.filters[3], FilterSpec::Bevel { .. }));
        assert_eq!(hero_f0.blend_mode, BlendMode::Multiply);
        assert!(hero_f0.cache_as_bitmap);
        assert!(!hero_f0.visible);
        assert!((hero_f0.alpha - 50.0).abs() < 1.0);

        let fx_f0 = f0
            .display_list
            .iter()
            .find(|e| e.name == "fxPanel")
            .expect("fxPanel on frame 0");
        assert_eq!(fx_f0.filters.len(), 4);
        assert!(matches!(fx_f0.filters[0], FilterSpec::GradientGlow { .. }));
        assert!(matches!(fx_f0.filters[1], FilterSpec::Convolution { .. }));
        assert!(matches!(fx_f0.filters[2], FilterSpec::ColorMatrix { .. }));
        assert!(matches!(fx_f0.filters[3], FilterSpec::GradientBevel { .. }));
        assert_eq!(fx_f0.blend_mode, BlendMode::Overlay);

        // Static clipDepth mask must clip the 2 masked depths in native SWF player
        let mask_entry = f0
            .display_list
            .iter()
            .find(|e| e.clip_depth.is_some())
            .expect("static clipDepth mask entry");
        let clip_end = mask_entry.clip_depth.unwrap();
        let clipped_entries: Vec<_> = f0
            .display_list
            .iter()
            .filter(|e| e.depth > mask_entry.depth && e.depth <= clip_end)
            .collect();
        assert_eq!(clipped_entries.len(), 2);

        // Step to Frame 1: `is_move` must reset filters, blend_mode, cache_as_bitmap, visible, and alpha!
        let f1 = player.step_frame();
        let hero_f1 = f1
            .display_list
            .iter()
            .find(|e| e.name == "heroPanel")
            .expect("heroPanel on frame 1");
        assert_eq!(hero_f1.x, 125.0);
        assert_eq!(hero_f1.y, 75.0);
        assert!(
            hero_f1.filters.is_empty(),
            "is_move must clear filters when Frame 1 has empty filter list"
        );
        assert_eq!(hero_f1.blend_mode, BlendMode::Normal);
        assert!(!hero_f1.cache_as_bitmap);
        assert!(hero_f1.visible);
        assert!((hero_f1.alpha - 100.0).abs() < 1.0);
        assert!(hero_f1.color_transform.is_identity());

        // Verify Load + EnterFrame + Frame scripts + pointer click dispatch
        assert!(player.trace_output().contains(&"clip-load".to_owned()));
        assert!(player.trace_output().contains(&"clip-ef".to_owned()));
        assert!(player.trace_output().contains(&"f0=15".to_owned()));
        assert!(player.trace_output().contains(&"f1=125".to_owned()));
        player.trigger_pointer_click();
        assert!(player.trace_output().contains(&"clip-press".to_owned()));
        assert!(player.trace_output().contains(&"btn-release".to_owned()));

        // Re-import into `.nflash` Project, validate, JSON save/reopen, overlay, and re-publish round-trip!
        let reimported = import_swf_bytes(&swf_bytes).expect("re-import published AVM1 SWF");
        reimported
            .validate()
            .expect("re-imported AVM1 project must validate");
        let overlay = player.project_with_display_list_overlay(&reimported, &reimported.root, 1);
        overlay
            .validate()
            .expect("display_list overlay must validate");

        let json = reimported.to_json().expect("serialize reimported project");
        let reopened = Project::from_json(&json).expect("reopen reimported project");
        reopened
            .validate()
            .expect("reopened AVM1 project must validate");

        // Verify DefineScalingGrid (tag 78) preserved on nested MovieClip symbol
        assert!(
            reopened
                .symbols
                .iter()
                .any(|s| s.scale9_grid() == Some([4.0, 4.0, 24.0, 24.0])),
            "DefineScalingGrid must round-trip through SWF export -> import"
        );
        // Verify static LayerRole::Mask + 2 masked layers reconstructed on root
        let root_re = reopened.symbol(&reopened.root).expect("reopened root");
        let mask_layers: Vec<_> = root_re
            .timeline
            .layers
            .iter()
            .filter(|l| l.properties.role == LayerRole::Mask)
            .collect();
        assert_eq!(mask_layers.len(), 1);
        let masked_count = root_re
            .timeline
            .layers
            .iter()
            .filter(|l| l.properties.mask.as_deref() == Some(mask_layers[0].id.as_str()))
            .count();
        assert_eq!(masked_count, 2);

        // Re-export the re-imported project to SWF and confirm `// onClipEvent(press)` round-tripped!
        let (reexported_swf, reexport_rep) =
            publish_swf_bytes(&reopened, &opts).expect("re-export reopened AVM1 project");
        assert!(reexport_rep.can_proceed());
        let mut p_re = SwfBytecodePlayer::from_bytes(&reexported_swf).unwrap();
        p_re.step_frame();
        p_re.trigger_pointer_click();
        assert!(
            p_re.trace_output().contains(&"clip-press".to_owned()),
            "// onClipEvent(press) header must preserve ClipEventPress across SWF import -> re-export: {:?}",
            p_re.trace_output()
        );
        assert!(p_re.trace_output().contains(&"btn-release".to_owned()));
    }

    // 2. AVM2 (AS3) across FWS, CWS, and ZWS
    let avm2_project = build_t325_authored_project(ScriptLanguage::ActionScript3);
    for (compression, expected_magic) in compressions {
        let opts = SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            compression,
            outline_text: true,
            ..SwfPublishOptions::default()
        };
        let (swf_bytes, report) =
            publish_swf_bytes(&avm2_project, &opts).expect("publish AVM2 SWF");
        assert!(
            report.can_proceed(),
            "AVM2 preflight must succeed: {report:?}"
        );
        assert_eq!(&swf_bytes[0..3], &expected_magic);

        let play = play_swf_bytes(&swf_bytes, 2).expect("play AVM2 SWF");
        assert!(play.trace_output.contains(&"as3-f0=15".to_owned()));
        assert!(play.trace_output.contains(&"as3-f1=125".to_owned()));
        let hero_f1 = play.frames_executed[1]
            .display_list
            .iter()
            .find(|e| e.name == "heroPanel")
            .expect("AVM2 heroPanel on frame 1");
        assert!(hero_f1.filters.is_empty());
        assert_eq!(hero_f1.blend_mode, BlendMode::Normal);
        assert!(hero_f1.visible);

        let reimported = import_swf_bytes(&swf_bytes).expect("re-import AVM2 SWF");
        reimported
            .validate()
            .expect("re-imported AVM2 project must validate");
        assert!(
            reimported
                .symbols
                .iter()
                .any(|s| s.scale9_grid() == Some([4.0, 4.0, 24.0, 24.0]))
        );
    }
}

#[test]
fn reconstructed_as1_as2_as3_and_corpus_swfs_export_valid_html5_bundles_and_roundtrip_swf() {
    let temp = TempDirGuard::new("html5-and-corpus");
    let runtime_dir = temp.0.join("mock-runtime");
    create_mock_runtime(&runtime_dir);

    // 1. Real upstream Ruffle corpus SWF (`avm1-clip-events/test.swf`) -> `.nflash` -> HTML5 + SWF
    let corpus_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fla-corpus/upstream/ruffle/avm1-clip-events/test.swf");
    assert!(
        corpus_path.is_file(),
        "corpus SWF missing at {}",
        corpus_path.display()
    );
    let corpus_report = inspect(&corpus_path).expect("inspect corpus SWF");
    assert!(
        corpus_report
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error)
    );
    let corpus_project = import_project(&corpus_path).expect("import corpus SWF");
    corpus_project
        .validate()
        .expect("reconstructed corpus project must validate");

    let corpus_json = corpus_project.to_json().expect("corpus project to_json");
    let corpus_reopened = Project::from_json(&corpus_json).expect("corpus project from_json");
    corpus_reopened
        .validate()
        .expect("reopened corpus validate");

    let html_corpus_out = temp.0.join("html-corpus");
    export_project(&corpus_reopened, &html_corpus_out, &runtime_dir)
        .expect("export reconstructed corpus SWF project to HTML5");
    assert!(html_corpus_out.join("index.html").is_file());
    assert!(html_corpus_out.join("bundle.js").is_file());
    assert!(
        fs::metadata(html_corpus_out.join("bundle.js"))
            .unwrap()
            .len()
            > 64
    );

    let (re_swf, re_rep) = publish_swf_bytes(
        &corpus_reopened,
        &SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1),
    )
    .expect("re-publish reconstructed corpus project to SWF");
    assert!(re_rep.can_proceed());
    let original_play =
        play_swf_bytes(&fs::read(&corpus_path).unwrap(), 12).expect("play original corpus SWF");
    let re_play = play_swf_bytes(&re_swf, 12).expect("play re-published corpus SWF");
    assert_eq!(
        re_play
            .frames_executed
            .iter()
            .map(|frame| (frame.frame, &frame.traces))
            .collect::<Vec<_>>(),
        original_play
            .frames_executed
            .iter()
            .map(|frame| (frame.frame, &frame.traces))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        re_play.trace_output, original_play.trace_output,
        "Assigned event function bodies must run at their event, preserving the entire trace"
    );
    let trace_position = |line| re_play.trace_output.iter().position(|s| s == line).unwrap();
    assert!(trace_position("clip enterFrame") < trace_position("clip onEnterFrame"));
    assert!(trace_position("frame 3") < trace_position("clip onUnload"));
    assert!(trace_position("clip unload") < trace_position("clip onUnload"));
    assert!(trace_position("clip onUnload") < trace_position("frame 4"));

    // 2. Reconstructed AS2 and AS3 SWFs (with filters, blends, static masks, scaling grids, scripts)
    //    -> `.nflash` -> HTML5 export bundle!
    for (idx, (lang, profile)) in [
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
        ),
        (
            ScriptLanguage::ActionScript3,
            SwfPublishProfile::FlashPlayer10Avm2,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let authored = build_t325_authored_project(lang);
        let (swf_bytes, rep) = publish_swf_bytes(
            &authored,
            &SwfPublishOptions {
                profile,
                compression: SwfCompression::LzmaZws,
                outline_text: true,
                ..SwfPublishOptions::default()
            },
        )
        .expect("publish ZWS SWF");
        assert!(rep.can_proceed());

        let reconstructed = import_swf_bytes(&swf_bytes).expect("reconstruct ZWS SWF");
        reconstructed
            .validate()
            .expect("reconstructed ZWS SWF project must validate");
        let saved = reconstructed.to_json().expect("to_json");
        let reopened = Project::from_json(&saved).expect("from_json");
        reopened.validate().expect("reopened validate");

        let html_out = temp.0.join(format!("html-reconstructed-{idx}"));
        export_project(&reopened, &html_out, &runtime_dir)
            .expect("HTML5 export of reconstructed SWF project must succeed");
        let html = fs::read_to_string(html_out.join("index.html")).expect("read index.html");
        assert!(html.contains("bundle.js"));
        let bundle = fs::read_to_string(html_out.join("bundle.js")).expect("read bundle.js");
        assert!(bundle.contains("heroPanel"));
    }
}

#[test]
fn avm1_function_values_run_on_call_and_unsupported_closures_block() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let root = &mut project.symbols[0];
    root.timeline.frame_count = 1;
    root.timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
                var base = 40;
                var twice = function(value) { var result = value * 2; return result; };
                this.adjust = function(value) { return base + value; };
                trace("assigned");
                trace(twice(3));
                trace(this.adjust(2));
                stop();
            "#
            .to_owned(),
        },
    }];
    project.upgrade_format_for_features();
    project.validate().unwrap();
    let avm1_options = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (bytes, report) = publish_swf_bytes(&project, &avm1_options).unwrap();
    assert!(report.can_proceed());
    assert_eq!(
        play_swf_bytes(&bytes, 2).unwrap().trace_output,
        ["assigned", "6", "42"]
    );

    for (language, profile, source, issue) in [
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            "var f = function named() { trace('deferred'); };",
            "named function expressions",
        ),
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            "function make(x) { return function() { return x; }; } var f = make(7);",
            "nested function expressions",
        ),
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            "var f = function() { var x = 7; function g() { return x; } return g; };",
            "nested function declarations",
        ),
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            "var f = function() { onClipEvent(load) { trace('invalid'); } };",
            "object event handler syntax",
        ),
        (
            ScriptLanguage::ActionScript3,
            SwfPublishProfile::FlashPlayer10Avm2,
            "var f = function() { trace('deferred'); };",
            "AS3 function expressions",
        ),
    ] {
        project.script_language = Some(language);
        project.symbols[0].timeline.scripts[0].script = Script {
            language,
            source: source.to_owned(),
        };
        let options = SwfPublishOptions::for_profile(profile);
        let report = preflight_swf_publish(&project, &options);
        assert!(
            report.items.iter().any(|item| {
                item.disposition == FlaFeatureDisposition::Blocking
                    && item.location.ends_with("frame[0].script")
                    && item.message.contains(issue)
            }),
            "missing located block for {source}: {report:?}"
        );
        assert!(publish_swf_bytes(&project, &options).is_err());
    }
}

#[test]
fn authored_bitmap_fills_block_swf_publish_including_scriptless_mode() {
    let mut project = Project::blank();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([40, 90, 150, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    project
        .resources
        .push(ResourceUnit::from_bytes("paint", "paint.png", "image/png", png.get_ref()).unwrap());
    let key = &mut project.symbols[0].timeline.layers[0].keyframes[0];
    key.elements.push(Element {
        id: "painted-shape".to_owned(),
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    });
    key.bitmap_fills.push(AuthoredBitmapFill {
        element_id: "painted-shape".to_owned(),
        resource_id: "paint".to_owned(),
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        repeat: true,
        smoothing: false,
    });
    project.upgrade_format_for_features();
    project.validate().unwrap();
    for scriptless_mode in [false, true] {
        let options = SwfPublishOptions {
            scriptless_mode,
            ..SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1)
        };
        let report = preflight_swf_publish(&project, &options);
        let item = report
            .items
            .iter()
            .find(|item| item.code == "swf_bitmap_fill_unsupported")
            .expect("bitmap fill must not silently become its solid fallback");
        assert_eq!(item.disposition, FlaFeatureDisposition::Blocking);
        assert!(item.location.ends_with("frame[0]:element:painted-shape"));
        assert!(publish_swf_bytes(&project, &options).is_err());
    }
}
