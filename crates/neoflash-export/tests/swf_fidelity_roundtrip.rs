//! Comprehensive SWF export, re-import, native bytecode playback, `.nflash` JSON round-trip,
//! and HTML5 bundle verification for `T-325`.

use neoflash_core::{
    AuthoredBitmapFill, BlendMode, ColorTransform, Element, ElementKind, FillRule, FilterSpec,
    FrameScript, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode,
    ObjectEvent, ObjectScript, PathCommand, Project, ResourceUnit, Script, ScriptLanguage,
    SourceUnit, Symbol, SymbolKind, Timeline, Transform,
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
            "var f = function() { onClipEvent(load) { trace('invalid'); } };",
            "object event handler syntax",
        ),
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            "var f = () => { on(release) { trace('invalid'); } };",
            "object event handler syntax",
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
fn avm1_and_avm2_nested_lexical_closures_named_fn_exprs_and_arrows_execute_and_roundtrip() {
    let temp = TempDirGuard::new("closures-avm1-avm2");

    // 1. AVM1 (SWF 8): DefineFunction2 with mixed captured & uncaptured parameters (no register
    //    clobbering), shared mutable outer local across sibling closures, named recursive function
    //    expression, arrow functions, and 3-level curried closures.
    let mut avm1_proj = Project::blank();
    avm1_proj.script_language = Some(ScriptLanguage::ActionScript2);
    avm1_proj.symbols[0].timeline.frame_count = 1;
    avm1_proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
                function makeCounter(seed, uncapturedScale) {
                    var count = seed * uncapturedScale;
                    var calls = 0;
                    function bump(delta, localBonus) {
                        count += delta + localBonus;
                        calls++;
                        return count;
                    }
                    var snap = () => count * 100 + calls;
                    return { bump: bump, snap: snap };
                }
                var c1 = makeCounter(3, 10);
                var c2 = makeCounter(1, 5);
                trace(c1.bump(4, 1));
                trace(c1.bump(2, 0));
                trace(c1.snap());
                trace(c2.snap());

                var fact = function selfFact(n) {
                    if (n <= 1) {
                        return 1;
                    }
                    return n * selfFact(n - 1);
                };
                trace(fact(5));

                var triple = a => b => c => a + b + c;
                trace(triple(100)(20)(3));
                stop();
            "#
            .to_owned(),
        },
    }];
    avm1_proj.upgrade_format_for_features();
    avm1_proj.validate().unwrap();
    let avm1_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (avm1_bytes, avm1_rep) = publish_swf_bytes(&avm1_proj, &avm1_opts).unwrap();
    assert!(avm1_rep.can_proceed(), "{avm1_rep:?}");
    let avm1_play = play_swf_bytes(&avm1_bytes, 4).unwrap();
    assert!(
        avm1_play.unsupported_diagnostics.is_empty(),
        "unexpected AVM1 diagnostics: {:?}",
        avm1_play.unsupported_diagnostics
    );
    assert_eq!(
        avm1_play.trace_output,
        vec!["35", "37", "3702", "500", "120", "123"]
    );
    let avm1_swf_path = temp.0.join("closures_avm1.swf");
    std::fs::write(&avm1_swf_path, &avm1_bytes).unwrap();
    let avm1_reimported = neoflash_import::import_project(&avm1_swf_path).unwrap();
    avm1_reimported.validate().unwrap();

    // 2. AVM2 (SWF 10 / DoABC2): newactivation + Trait_Slot + getscopeobject / getouterscope +
    //    getslot / setslot for shared mutable outer locals/params, named function expressions,
    //    arrow functions, and multi-level nested closures.
    let mut avm2_proj = Project::blank();
    avm2_proj.script_language = Some(ScriptLanguage::ActionScript3);
    avm2_proj.symbols[0].timeline.frame_count = 1;
    avm2_proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"
                function makeAcc(seed:int, scale:int):Object {
                    var total:int = seed * scale;
                    var steps:int = 0;
                    var add:Function = function(delta:int, bonus:int):int {
                        total += delta + bonus;
                        steps++;
                        return total;
                    };
                    var read:Function = ():int => total * 100 + steps;
                    return { add: add, read: read };
                }
                var a1:Object = makeAcc(4, 10);
                var a2:Object = makeAcc(2, 3);
                trace(a1.add(5, 2));
                trace(a1.add(3, 0));
                trace(a1.read());
                trace(a2.read());

                var fib:Function = function selfFib(n:int):int {
                    if (n <= 1) {
                        return n;
                    }
                    return selfFib(n - 1) + selfFib(n - 2);
                };
                trace(fib(7));

                var curried:Function = (x:int):Function => (y:int):Function => (z:int):int => x + y + z;
                trace(curried(200)(30)(4));
                stop();
            "#
            .to_owned(),
        },
    }];
    avm2_proj.upgrade_format_for_features();
    avm2_proj.validate().unwrap();
    let avm2_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (avm2_bytes, avm2_rep) = publish_swf_bytes(&avm2_proj, &avm2_opts).unwrap();
    assert!(avm2_rep.can_proceed(), "{avm2_rep:?}");
    let avm2_play = play_swf_bytes(&avm2_bytes, 4).unwrap();
    assert!(
        avm2_play.unsupported_diagnostics.is_empty(),
        "unexpected AVM2 diagnostics: {:?}",
        avm2_play.unsupported_diagnostics
    );
    assert_eq!(
        avm2_play.trace_output,
        vec!["47", "50", "5002", "600", "13", "234"]
    );
    let avm2_swf_path = temp.0.join("closures_avm2.swf");
    std::fs::write(&avm2_swf_path, &avm2_bytes).unwrap();
    let avm2_reimported = neoflash_import::import_project(&avm2_swf_path).unwrap();
    avm2_reimported.validate().unwrap();

    // 3. AVM2 multi-frame seek/step/restart determinism with dynamic addChild, removeChild
    //    timeline tombstone, addFrameScript(1, null) frame-execution flag, and closure state:
    let mut det_avm2 = Project::blank();
    det_avm2.script_language = Some(ScriptLanguage::ActionScript3);
    det_avm2.symbols.push(neoflash_core::Symbol {
        id: "hero-sym".to_owned(),
        name: "HeroSym".to_owned(),
        kind: neoflash_core::SymbolKind::MovieClip,
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        timeline: neoflash_core::Timeline {
            frame_count: 1,
            layers: vec![neoflash_core::Layer {
                id: "hero-l0".to_owned(),
                name: "Layer 1".to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![neoflash_core::Keyframe {
                    frame: 0,
                    elements: vec![neoflash_core::Element {
                        id: "hero-rect".to_owned(),
                        name: String::new(),
                        visible: true,
                        kind: ElementKind::Rectangle {
                            width: 20.0,
                            height: 20.0,
                            fill: [220, 60, 40, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..neoflash_core::Element::default()
                    }],
                    ..neoflash_core::Keyframe::default()
                }],
                properties: neoflash_core::LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    });
    let root = &mut det_avm2.symbols[0];
    root.timeline.frame_count = 3;
    root.timeline.layers[0].keyframes = vec![
        neoflash_core::Keyframe {
            frame: 0,
            tween: Some(neoflash_core::Easing::Linear),
            elements: vec![neoflash_core::Element {
                id: "hero-inst".to_owned(),
                name: "hero".to_owned(),
                visible: true,
                kind: neoflash_core::ElementKind::Instance {
                    symbol_id: "hero-sym".to_owned(),
                    playback: neoflash_core::InstancePlayback::Loop,
                },
                transform: neoflash_core::Transform {
                    x: 15.0,
                    y: 20.0,
                    ..neoflash_core::Transform::default()
                },
                ..neoflash_core::Element::default()
            }],
            ..neoflash_core::Keyframe::default()
        },
        neoflash_core::Keyframe {
            frame: 2,
            elements: vec![neoflash_core::Element {
                id: "hero-inst".to_owned(),
                name: "hero".to_owned(),
                visible: true,
                kind: neoflash_core::ElementKind::Instance {
                    symbol_id: "hero-sym".to_owned(),
                    playback: neoflash_core::InstancePlayback::Loop,
                },
                transform: neoflash_core::Transform {
                    x: 85.0,
                    y: 20.0,
                    ..neoflash_core::Transform::default()
                },
                ..neoflash_core::Element::default()
            }],
            ..neoflash_core::Keyframe::default()
        },
    ];
    root.timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
                    var seq:int = 1;
                    this.nextSeq = ():int => {
                        seq += 10;
                        return seq;
                    };
                    trace("avm2_f0:" + this.nextSeq() + ":" + numChildren);
                "#
                .to_owned(),
            },
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
                    var dynSp:Sprite = new Sprite();
                    dynSp.name = "dynSp";
                    addChild(dynSp);
                    removeChild(hero);
                    addFrameScript(1, null);
                    trace("avm2_f1:" + this.nextSeq() + ":" + numChildren);
                "#
                .to_owned(),
            },
        },
        FrameScript {
            layer: None,
            frame: 2,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
                    trace("avm2_f2:" + this.nextSeq() + ":" + numChildren);
                    stop();
                "#
                .to_owned(),
            },
        },
    ];
    det_avm2.upgrade_format_for_features();
    det_avm2.validate().unwrap();
    let (det_bytes, det_rep) = publish_swf_bytes(&det_avm2, &avm2_opts).unwrap();
    assert!(det_rep.can_proceed(), "{det_rep:?}");

    let mut det_player = neoflash_import::SwfBytecodePlayer::from_bytes(&det_bytes).unwrap();
    let d0 = det_player.step_frame();
    let d1 = det_player.step_frame();
    let d2 = det_player.step_frame();
    assert_eq!(d0.traces, vec!["avm2_f0:11:1"]);
    assert_eq!(d1.traces, vec!["avm2_f1:21:1"]); // +dynSp, -hero -> 1 child (`dynSp`)
    assert_eq!(d2.traces, vec!["avm2_f2:31:1"]); // hero tombstoned on frame 2 Move!
    assert_eq!(d2.display_list.len(), 1);
    assert_eq!(d2.display_list[0].name, "dynSp");

    // Backward seek to 0 & 1 and restart() must reset `dynSp`, `hero` tombstone, `addFrameScript`,
    // and the captured `seq` closure slot cleanly:
    assert_eq!(det_player.seek_to_frame(0), d0);
    assert_eq!(det_player.step_frame(), d1);
    assert_eq!(det_player.step_frame(), d2);
    assert_eq!(det_player.seek_to_frame(1), d1);
    assert_eq!(det_player.step_frame(), d2);
    assert_eq!(det_player.restart(), d0);
    assert_eq!(det_player.step_frame(), d1);
    assert_eq!(det_player.step_frame(), d2);
}

#[test]
fn authored_bitmap_fills_roundtrip_changed_and_held_keys_including_scriptless_mode() {
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
            stroke: [7, 17, 73, 255],
            stroke_width: 2.0,
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
    let mut changed = key.clone();
    changed.frame = 2;
    changed.bitmap_fills[0].matrix = [0.5, 0.25, -0.25, 1., 2., 4.];
    changed.bitmap_fills[0].repeat = false;
    changed.bitmap_fills[0].smoothing = true;
    project.symbols[0].timeline.layers[0]
        .keyframes
        .push(changed);
    project.symbols[0].timeline.frame_count = 4;
    project.upgrade_format_for_features();
    project.validate().unwrap();
    for scriptless_mode in [false, true] {
        let options = SwfPublishOptions {
            scriptless_mode,
            ..SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1)
        };
        let report = preflight_swf_publish(&project, &options);
        assert!(report.can_proceed(), "{:?}", report.items);
        let (bytes, _) = publish_swf_bytes(&project, &options).unwrap();
        let imported = import_swf_bytes(&bytes).unwrap();
        assert!(imported.resources.iter().any(|r| r.is_image()));
        for frame in 0..4 {
            let scene =
                neoflash_core::evaluate_scene(&imported, &imported.root, frame, false, true);
            let brush = scene
                .draws
                .iter()
                .find_map(|d| d.item.bitmap_fill.as_ref())
                .expect("bitmap paint on re-imported path");
            let expected = &project.symbols[0].timeline.layers[0].keyframes
                [usize::from(frame >= 2)]
            .bitmap_fills[0];
            assert_eq!(brush.matrix, expected.matrix);
            assert_eq!(
                (brush.repeat, brush.smoothing),
                (expected.repeat, expected.smoothing)
            );
            assert!(
                scene.draws.iter().any(|d| matches!(
                    d.item.kind,
                    ElementKind::Path {
                        stroke: [7, 17, 73, 255],
                        ..
                    } | ElementKind::PaintedPath {
                        stroke: [7, 17, 73, 255],
                        ..
                    }
                )),
                "authored stroke survives"
            );
        }
    }
}

#[test]
fn full_stage_bitmap_larger_than_a_swf_edge_record_remains_visible() {
    let mut project = Project::blank();
    project.width = 2160;
    project.height = 3840;
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(72, 128, image::Rgba([40, 90, 150, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    project.resources.push(
        ResourceUnit::from_bytes("poster", "poster.png", "image/png", png.get_ref()).unwrap(),
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "poster".to_owned(),
            kind: ElementKind::Bitmap {
                resource_id: "poster".to_owned(),
                width: 2160,
                height: 3840,
                smoothing: true,
            },
            ..Element::default()
        });
    let (bytes, _) = publish_swf_bytes(&project, &SwfPublishOptions::default()).unwrap();
    let imported = import_swf_bytes(&bytes).unwrap();
    let scene = neoflash_core::evaluate_scene(&imported, &imported.root, 0, false, true);
    let poster = scene
        .draws
        .iter()
        .find(|d| {
            matches!(
                d.item.kind,
                ElementKind::Bitmap {
                    width: 72,
                    height: 128,
                    ..
                }
            )
        })
        .expect("one fitted bitmap with a closed, subdivided rectangle");
    assert_eq!(poster.item.transform, [30., 0., 0., 30., 0., 0.]);
}

#[test]
fn t409_as3_and_as2_include_directives_with_project_sources_fallback_and_dedup() {
    // 1. AS3: `include "path.as";`, `include "PATH.AS"` (no semicolon, case-insensitive),
    //    and `#include 'path.as'`, pulling from both `project.resources` and `project.sources`
    //    without requiring package/class on included snippet `SourceUnit`s or double-running them.
    let mut as3_proj = Project::blank();
    as3_proj.script_language = Some(ScriptLanguage::ActionScript3);
    as3_proj.document_class = Some("app.IncludeMain".to_owned());
    as3_proj.resources.push(
        ResourceUnit::from_bytes(
            "res_inc_1",
            "inc/res_constants.as",
            "text/plain",
            b"var resBase:int = 100;\n",
        )
        .unwrap(),
    );
    as3_proj.sources = vec![
        SourceUnit {
            id: "src_inc_semi".to_owned(),
            path: "inc/SharedDefs.AS".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: "\u{FEFF}var srcBonus:int = 25;\ninclude \" no_semi_helper.as \"\n".to_owned(),
        },
        SourceUnit {
            id: "src_inc_nosemi".to_owned(),
            path: "inc/no_semi_helper.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: "var noSemiStep:int = 7;\n".to_owned(),
        },
        SourceUnit {
            id: "src_frame_inc".to_owned(),
            path: "frame_snippet.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: "trace(\"frame-inc:\" + (this.bootTotal + 3));\n".to_owned(),
        },
        SourceUnit {
            id: "src_doc_main".to_owned(),
            path: "app/IncludeMain.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package app {
                import flash.display.MovieClip;
                public dynamic class IncludeMain extends MovieClip {
                    public function IncludeMain() {
                        #include "../inc/res_constants.as"
                        include "../inc/shareddefs.as"; // case-insensitive + nested include without ';'
                        this.bootTotal = resBase + srcBonus + noSemiStep;
                        trace("ctor-inc:" + this.bootTotal);
                    }
                }
            }"#
            .to_owned(),
        },
    ];
    as3_proj.symbols[0].timeline.frame_count = 1;
    as3_proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "include 'FRAME_SNIPPET.AS'\nstop();\n".to_owned(),
        },
    }];
    as3_proj.upgrade_format_for_features();
    as3_proj.validate().unwrap();

    let avm2_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (as3_swf, as3_rep) =
        publish_swf_bytes(&as3_proj, &avm2_opts).expect("publish AS3 include SWF");
    assert!(as3_rep.can_proceed(), "{as3_rep:?}");
    let as3_play = play_swf_bytes(&as3_swf, 2).expect("play AS3 include SWF");
    assert!(as3_play.unsupported_diagnostics.is_empty());
    assert_eq!(as3_play.trace_output, vec!["ctor-inc:132", "frame-inc:135"]);

    // 2. AS2: case-insensitive `#include` fallback to `project.sources` without duplicate DoInitAction
    let mut as2_proj = Project::blank();
    as2_proj.script_language = Some(ScriptLanguage::ActionScript2);
    as2_proj.sources = vec![SourceUnit {
        id: "as2_inc_src".to_owned(),
        path: "scripts/CounterInit.AS".to_owned(),
        language: ScriptLanguage::ActionScript2,
        source: "if (bootHits == undefined) { var bootHits = 0; }\nbootHits += 1;\n".to_owned(),
    }];
    as2_proj.symbols[0].timeline.frame_count = 1;
    as2_proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source:
                "#include \"SCRIPTS/counterinit.as\"\ntrace(\"as2-hits:\" + bootHits);\nstop();\n"
                    .to_owned(),
        },
    }];
    as2_proj.upgrade_format_for_features();
    as2_proj.validate().unwrap();

    let avm1_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (as2_swf, as2_rep) =
        publish_swf_bytes(&as2_proj, &avm1_opts).expect("publish AS2 include SWF");
    assert!(as2_rep.can_proceed(), "{as2_rep:?}");
    let as2_play = play_swf_bytes(&as2_swf, 2).expect("play AS2 include SWF");
    assert_eq!(as2_play.trace_output, vec!["as2-hits:1"]);
}

#[test]
fn t409_avm1_try_catch_finally_switch_for_in_operators_and_super_inheritance_roundtrip() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript2);
    proj.sources = vec![
        SourceUnit {
            id: "as2_v1".to_owned(),
            path: "sim/Vehicle.as".to_owned(),
            language: ScriptLanguage::ActionScript2,
            source: r#"class sim.Vehicle {
                var baseSpeed:Number;
                function Vehicle(seed:Number) {
                    this.baseSpeed = seed + 10;
                }
                function describe():String {
                    return "V" + this.baseSpeed;
                }
            }"#
            .to_owned(),
        },
        SourceUnit {
            id: "as2_v2".to_owned(),
            path: "sim/Craft.as".to_owned(),
            language: ScriptLanguage::ActionScript2,
            source: r#"class sim.Craft extends sim.Vehicle {
                var wingBoost:Number;
                function Craft(seed:Number, boost:Number) {
                    super(seed);
                    this.wingBoost = boost;
                }
                function describe():String {
                    return super.describe() + ":C" + this.wingBoost;
                }
            }"#
            .to_owned(),
        },
        SourceUnit {
            id: "as2_v3".to_owned(),
            path: "sim/Interceptor.as".to_owned(),
            language: ScriptLanguage::ActionScript2,
            source: r#"class sim.Interceptor extends sim.Craft {
                var callsign:String;
                function Interceptor(seed:Number, boost:Number, sign:String) {
                    super(seed, boost);
                    this.callsign = sign;
                }
                function describe():String {
                    return super.describe() + ":I-" + this.callsign;
                }
            }"#
            .to_owned(),
        },
    ];
    proj.symbols[0].timeline.frame_count = 1;
    proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
                var ship = new Interceptor(5, 3, "alpha");
                trace(ship.describe());
                trace((ship instanceof Interceptor) + "," + (ship instanceof Craft) + "," + (ship instanceof Vehicle));

                var log = "";
                function innerThrow(flag) {
                    try {
                        log += "T";
                        if (flag) {
                            throw new TypeError("bad-type");
                        }
                        return "ok";
                    } finally {
                        log += "F";
                    }
                }
                try {
                    innerThrow(true);
                } catch (re:RangeError) {
                    log += "R";
                } catch (te:TypeError) {
                    log += "C(" + te.message + ")";
                } finally {
                    log += "O";
                }
                var normalRes = innerThrow(false);
                trace(log + ":" + normalRes);

                function routeSwitch(code) {
                    var out = "";
                    switch (code) {
                        case 1:
                            out += "one";
                            break;
                        case 2:
                        case 3:
                            out += "twoThree";
                            break;
                        default:
                            out += "def";
                    }
                    return out;
                }
                trace(routeSwitch(1) + "|" + routeSwitch(2) + "|" + routeSwitch(9));

                var sum = 0;
                for (var i = 0; i < 6; i++) {
                    if (i == 1) {
                        continue;
                    }
                    if (i == 5) {
                        break;
                    }
                    sum += i;
                }
                var arr = [10, 20, 30];
                var delOk = delete arr[1];
                var eachSum = 0;
                for each (var v in arr) {
                    eachSum += v;
                }
                var bag = { a: 3, b: 4, temp: 9 };
                delete bag.temp;
                var keyCount = 0;
                for (var k in bag) {
                    keyCount++;
                }
                var bit = (~5 == -6) ? "yes" : "no";
                trace(sum + ":" + delOk + ":" + (1 in arr) + ":" + (0 in arr) + ":" + eachSum + ":" + keyCount + ":" + typeof ship + ":" + bit);
                stop();
            "#
            .to_owned(),
        },
    }];
    proj.upgrade_format_for_features();
    proj.validate().unwrap();

    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (swf_bytes, rep) = publish_swf_bytes(&proj, &opts).expect("publish AVM1 SWF");
    assert!(rep.can_proceed(), "{rep:?}");

    let play = play_swf_bytes(&swf_bytes, 2).expect("play AVM1 SWF");
    assert!(
        play.unsupported_diagnostics.is_empty(),
        "unexpected AVM1 diagnostics: {:?}",
        play.unsupported_diagnostics
    );
    assert_eq!(
        play.trace_output,
        vec![
            "V15:C3:I-alpha",
            "true,true,true",
            "TFC(bad-type)OTF:ok",
            "one|twoThree|def",
            "9:true:false:true:40:2:object:yes",
        ]
    );

    let reimported = import_swf_bytes(&swf_bytes).expect("re-import AVM1 SWF");
    reimported
        .validate()
        .expect("re-imported AVM1 project must validate");
}

#[test]
fn t409_avm2_try_catch_finally_switch_hasnext2_operators_and_multilevel_super_roundtrip() {
    let mut proj = Project::blank();
    proj.script_language = Some(ScriptLanguage::ActionScript3);
    proj.document_class = Some("app.MainApp".to_owned());
    // Intentionally place subclasses BEFORE base classes in `project.sources` to verify
    // topological class ordering in `build_abc_bytes`.
    proj.sources = vec![
        SourceUnit {
            id: "s_boss".to_owned(),
            path: "model/BossActor.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package model {
                public class BossActor extends Actor {
                    public var phase:int = 2;
                    public static var tagPrefix:String = "BOSS";
                    public static function formatTag(n:int):String {
                        return tagPrefix + "-" + n;
                    }
                    public function BossActor(hp:int, armor:int) {
                        super(hp, armor);
                    }
                    public override function compute(mult:int):int {
                        return super.compute(mult) + this.phase;
                    }
                }
            }"#
            .to_owned(),
        },
        SourceUnit {
            id: "s_actor".to_owned(),
            path: "model/Actor.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package model {
                public class Actor extends Entity {
                    public var armor:int;
                    public function Actor(hp:int, armor:int) {
                        super(hp);
                        this.armor = armor;
                    }
                    public override function compute(mult:int):int {
                        return super.compute(mult) + this.armor;
                    }
                }
            }"#
            .to_owned(),
        },
        SourceUnit {
            id: "s_entity".to_owned(),
            path: "model/Entity.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package model {
                public class Entity {
                    public var hp:int = 1;
                    public function Entity(hp:int) {
                        this.hp = hp;
                    }
                    public function compute(mult:int):int {
                        return this.hp * mult;
                    }
                }
            }"#
            .to_owned(),
        },
        SourceUnit {
            id: "s_main".to_owned(),
            path: "app/MainApp.as".to_owned(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package app {
                import flash.display.MovieClip;
                import flash.display.Sprite;
                import model.*;
                public dynamic class MainApp extends MovieClip {
                    public function MainApp() {
                        var boss:BossActor = new BossActor(10, 5);
                        trace(BossActor.formatTag(boss.compute(3)));
                        var checks:String = (boss is BossActor) + "," + (boss is Actor) + "," + (boss is Entity) + "," + (boss is Sprite) + "," + ((boss as Actor) !== null) + "," + ((boss as Sprite) === null) + "," + (boss instanceof Entity);
                        trace(checks);
                    }
                }
            }"#
            .to_owned(),
        },
    ];
    proj.symbols[0].timeline.frame_count = 1;
    proj.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"
                var flow:String = "";
                function runStep(mode:int):String {
                    try {
                        flow += "A";
                        if (mode == 1) {
                            throw new TypeError("type-fail");
                        }
                        if (mode == 2) {
                            return "early";
                        }
                        flow += "B";
                    } finally {
                        flow += "F";
                    }
                    return "done";
                }
                try {
                    runStep(1);
                } catch (re:RangeError) {
                    flow += "R";
                } catch (te:TypeError) {
                    flow += "T(" + te.message + ")";
                } finally {
                    flow += "Z";
                }
                var r2:String = runStep(2);
                trace(flow + ":" + r2);

                function classify(v:int):String {
                    var res:String = "";
                    switch (v) {
                        case 10:
                            res = "ten";
                            break;
                        case 20:
                        case 30:
                            res = "twentyThirty";
                            break;
                        default:
                            res = "other";
                    }
                    return res;
                }
                trace(classify(10) + "/" + classify(30) + "/" + classify(99));

                var acc:int = 0;
                for (var i:int = 0; i < 6; i++) {
                    if (i == 2) {
                        continue;
                    }
                    if (i == 5) {
                        break;
                    }
                    acc += i;
                }

                var nums:Array = [4, 8, 12];
                var idx:int = 1;
                var dynRead:int = nums[idx];
                nums[idx] = 99;
                var removed:Boolean = delete nums[idx];
                var valTotal:int = 0;
                for each (var n:int in nums) {
                    valTotal += n;
                }

                var dict:Object = { x: 1, y: 2, drop: 3 };
                delete dict.drop;
                var keysCount:int = 0;
                for (var k:String in dict) {
                    keysCount++;
                }
                var tern:String = (~0 == -1) ? "maskOk" : "maskBad";
                trace(acc + ":" + dynRead + ":" + removed + ":" + (1 in nums) + ":" + (2 in nums) + ":" + valTotal + ":" + keysCount + ":" + typeof nums + ":" + tern);
                stop();
            "#
            .to_owned(),
        },
    }];
    proj.upgrade_format_for_features();
    proj.validate().unwrap();

    let opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (swf_bytes, rep) = publish_swf_bytes(&proj, &opts).expect("publish AVM2 SWF");
    assert!(rep.can_proceed(), "{rep:?}");

    let play = play_swf_bytes(&swf_bytes, 2).expect("play AVM2 SWF");
    assert!(
        play.unsupported_diagnostics.is_empty(),
        "unexpected AVM2 diagnostics: {:?}",
        play.unsupported_diagnostics
    );
    assert_eq!(
        play.trace_output,
        vec![
            "BOSS-37",
            "true,true,true,false,true,true,true",
            "AFT(type-fail)ZAF:early",
            "ten/twentyThirty/other",
            "8:8:true:false:true:16:2:object:maskOk",
        ]
    );

    let reimported = import_swf_bytes(&swf_bytes).expect("re-import AVM2 SWF");
    reimported
        .validate()
        .expect("re-imported AVM2 project must validate");
}

#[test]
fn t409_dynamic_movieclip_and_sprite_stage_projection_avm1_and_avm2() {
    // 1. AVM1 dynamic `createEmptyMovieClip` + runtime vector drawing + `attachMovie` with
    //    `Object.registerClass` + `duplicateMovieClip` + `removeMovieClip` Stage projection!
    let mut avm1_proj = Project::blank();
    avm1_proj.script_language = Some(ScriptLanguage::ActionScript2);
    avm1_proj.symbols.push(Symbol {
        id: "sym_badge".to_owned(),
        name: "BadgeClip".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("linked_badge".to_owned()),
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "badge_l".to_owned(),
                name: "Art".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "badge_rect".to_owned(),
                        kind: ElementKind::Rectangle {
                            width: 18.0,
                            height: 18.0,
                            fill: [40, 200, 90, 255],
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
    });
    avm1_proj.sources = vec![SourceUnit {
        id: "src_badge_ctrl".to_owned(),
        path: "BadgeController.as".to_owned(),
        language: ScriptLanguage::ActionScript2,
        source: r#"class BadgeController {
            function BadgeController() {
                this._x = 140;
                trace("badge-ctor:" + this._name);
            }
        }"#
        .to_owned(),
    }];
    avm1_proj.symbols[0].timeline.frame_count = 2;
    avm1_proj.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: r#"
                    Object.registerClass("linked_badge", BadgeController);
                    var canvas = this.createEmptyMovieClip("canvasClip", 10);
                    canvas._x = 25;
                    canvas._y = 35;
                    canvas.lineStyle(2, 0x112233, 100);
                    canvas.beginFill(0x2288FF, 100);
                    canvas.moveTo(0, 0);
                    canvas.lineTo(40, 0);
                    canvas.lineTo(40, 30);
                    canvas.lineTo(0, 30);
                    canvas.endFill();

                    var attached = this.attachMovie("linked_badge", "b1", 20);
                    duplicateMovieClip("canvasClip", "canvasCopy", 30);
                    this.canvasCopy._x = 90;
                    trace("f0-dyn:" + attached._x + ":" + this.canvasCopy._x);
                "#
                .to_owned(),
            },
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: r#"
                    this.b1.removeMovieClip();
                    stop();
                "#
                .to_owned(),
            },
        },
    ];
    avm1_proj.upgrade_format_for_features();
    avm1_proj.validate().unwrap();

    let avm1_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (avm1_swf, _) = publish_swf_bytes(&avm1_proj, &avm1_opts).unwrap();
    let avm1_reimported = import_swf_bytes(&avm1_swf).unwrap();
    let mut avm1_player = SwfBytecodePlayer::from_bytes(&avm1_swf).unwrap();

    let f0 = avm1_player.step_frame();
    assert_eq!(
        avm1_player.trace_output(),
        &["badge-ctor:b1", "f0-dyn:140:90"]
    );
    assert_eq!(f0.display_list.len(), 3);
    let scene_f0 =
        avm1_player.evaluate_stage_scene(&avm1_reimported, &avm1_reimported.root, 0, true);
    assert!(
        scene_f0.draws.len() >= 3,
        "runtime drawn canvasClip, duplicated canvasCopy, and attached b1 must project onto Stage: {}",
        scene_f0.draws.len()
    );
    let overlay_f0 =
        avm1_player.project_with_display_list_overlay(&avm1_reimported, &avm1_reimported.root, 0);
    overlay_f0
        .validate()
        .expect("AVM1 dynamic overlay project must validate");

    let f1 = avm1_player.step_frame();
    assert_eq!(f1.display_list.len(), 2, "b1 must be removed on frame 1");
    assert!(f1.display_list.iter().all(|e| e.name != "b1"));

    // 2. AVM2 dynamic `new Sprite()` + `graphics.drawRect(...)` + `new LinkedCard()` (`SymbolClass`)
    //    + `addChild` / `removeChild` Stage projection!
    let mut avm2_proj = Project::blank();
    avm2_proj.script_language = Some(ScriptLanguage::ActionScript3);
    avm2_proj.symbols.push(Symbol {
        id: "sym_card".to_owned(),
        name: "LinkedCard".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: Some("ui.LinkedCard".to_owned()),
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "card_l".to_owned(),
                name: "Art".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "card_rect".to_owned(),
                        kind: ElementKind::Rectangle {
                            width: 24.0,
                            height: 16.0,
                            fill: [250, 180, 30, 255],
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
    });
    avm2_proj.sources = vec![SourceUnit {
        id: "src_card".to_owned(),
        path: "ui/LinkedCard.as".to_owned(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package ui {
            import flash.display.MovieClip;
            public class LinkedCard extends MovieClip {
                public function LinkedCard() {
                    super();
                    this.x = 160;
                }
            }
        }"#
        .to_owned(),
    }];
    avm2_proj.symbols[0].timeline.frame_count = 2;
    avm2_proj.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
                    var sp:Sprite = new Sprite();
                    sp.name = "drawnSprite";
                    sp.x = 30;
                    sp.y = 45;
                    sp.graphics.beginFill(0xFF4422, 1.0);
                    sp.graphics.drawRect(0, 0, 50, 25);
                    sp.graphics.endFill();
                    addChild(sp);

                    this.card = new ui.LinkedCard();
                    this.card.name = "cardInst";
                    addChild(this.card);
                    trace("avm2-dyn:" + numChildren + ":" + this.card.x + ":" + sp.x);
                "#
                .to_owned(),
            },
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"
                    removeChild(this.card);
                    trace("avm2-after-rm:" + numChildren);
                    stop();
                "#
                .to_owned(),
            },
        },
    ];
    avm2_proj.upgrade_format_for_features();
    avm2_proj.validate().unwrap();

    let avm2_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (avm2_swf, _) = publish_swf_bytes(&avm2_proj, &avm2_opts).unwrap();
    let avm2_reimported = import_swf_bytes(&avm2_swf).unwrap();
    let mut avm2_player = SwfBytecodePlayer::from_bytes(&avm2_swf).unwrap();

    let af0 = avm2_player.step_frame();
    assert_eq!(af0.traces, vec!["avm2-dyn:2:160:30"]);
    assert_eq!(af0.display_list.len(), 2);
    let avm2_scene_f0 =
        avm2_player.evaluate_stage_scene(&avm2_reimported, &avm2_reimported.root, 0, true);
    assert!(
        avm2_scene_f0.draws.len() >= 2,
        "AVM2 Sprite.graphics + LinkedCard SymbolClass instance must project onto Stage: {}",
        avm2_scene_f0.draws.len()
    );
    let avm2_overlay_f0 =
        avm2_player.project_with_display_list_overlay(&avm2_reimported, &avm2_reimported.root, 0);
    avm2_overlay_f0
        .validate()
        .expect("AVM2 dynamic overlay project must validate");

    let af1 = avm2_player.step_frame();
    assert_eq!(af1.traces, vec!["avm2-after-rm:1"]);
    assert_eq!(af1.display_list.len(), 1);
    assert_eq!(af1.display_list[0].name, "drawnSprite");
}

#[test]
fn labelled_frame_navigation_roundtrips_and_restarts_in_each_vm() {
    use neoflash_core::FrameLabelType;

    for (language, profile, destination, primary_label, trace) in [
        (
            ScriptLanguage::ActionScript2,
            SwfPublishProfile::FlashPlayer8Avm1,
            2,
            "Near",
            "near",
        ),
        (
            ScriptLanguage::ActionScript3,
            SwfPublishProfile::FlashPlayer10Avm2,
            3,
            "Far",
            "far",
        ),
    ] {
        let mut project =
            Project::from_json(include_str!("fixtures/swf-label-navigation.nflash")).unwrap();
        project.script_language = Some(language);
        for entry in &mut project.symbols[0].timeline.scripts {
            entry.script.language = language;
        }
        let options = SwfPublishOptions {
            profile,
            ..Default::default()
        };
        let (bytes, report) = publish_swf_bytes(&project, &options).unwrap();
        assert!(report.can_proceed(), "{report:?}");

        let imported = import_swf_bytes(&bytes).unwrap();
        let saved = Project::from_json(&imported.to_json().unwrap()).unwrap();
        let timeline = &saved.symbol(&saved.root).unwrap().timeline;
        let labels = timeline.navigable_labels();
        for (name, frame) in [
            ("Home", 0),
            ("Near", 2),
            ("Landing", 2),
            ("Far", 3),
            ("landing", 3),
        ] {
            assert!(labels.contains(&(name, frame)), "{labels:?}");
        }
        assert!(
            timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .any(|k| k.label == "Landing" && k.label_type == FrameLabelType::Anchor)
        );

        let (republished, report) = publish_swf_bytes(&saved, &options).unwrap();
        assert!(report.can_proceed(), "{report:?}");
        for movie in [&bytes, &republished] {
            let mut player = SwfBytecodePlayer::from_bytes(movie).unwrap();
            let start = player.step_frame();
            assert_eq!(start.traces, ["start"]);
            let reached = player.step_frame();
            assert_eq!(reached.frame, destination);
            assert_eq!(reached.label.as_deref(), Some(primary_label));
            assert_eq!(reached.traces, ["step", trace]);
            assert!(!reached.playing);
            assert_eq!(reached.display_list, start.display_list);
            assert!(reached.unsupported_diagnostics.is_empty());
            assert_eq!(player.seek_to_frame(0), start);
            assert_eq!(player.step_frame(), reached);
            assert_eq!(player.seek_to_frame(1), reached);
            assert_eq!(player.restart(), start);
            assert_eq!(player.step_frame(), reached);
        }
    }
}

fn collect_swf_tags_raw(swf_bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    assert!(swf_bytes.len() >= 8);
    let body: Vec<u8> = match &swf_bytes[..3] {
        b"FWS" => swf_bytes[8..].to_vec(),
        b"CWS" => {
            use std::io::Read;
            let mut dec = flate2::read::ZlibDecoder::new(&swf_bytes[8..]);
            let mut out = Vec::new();
            dec.read_to_end(&mut out).expect("decompress CWS");
            out
        }
        other => panic!("unexpected SWF signature in tag collector: {other:?}"),
    };
    let nbits = usize::from(body[0] >> 3);
    let rect_bytes = (5 + nbits * 4).div_ceil(8);
    let mut pos = rect_bytes + 4; // frame_rate(2) + frame_count(2)
    let mut out = Vec::new();
    while pos + 2 <= body.len() {
        let hdr = u16::from_le_bytes([body[pos], body[pos + 1]]);
        pos += 2;
        let code = hdr >> 6;
        let short_len = usize::from(hdr & 0x3f);
        let len = if short_len == 0x3f {
            let l = u32::from_le_bytes(body[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            l
        } else {
            short_len
        };
        let end = (pos + len).min(body.len());
        out.push((code, body[pos..end].to_vec()));
        pos = end;
        if code == 0 {
            break;
        }
    }
    out
}

fn make_wav_pcm_bytes(sample_rate: u32, samples: usize) -> Vec<u8> {
    let channels = 1u16;
    let bits_per_sample = 16u16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    let block_align = channels * (bits_per_sample / 8);
    let data_len = (samples * 2) as u32;
    let riff_len = 36 + data_len;
    let mut out = Vec::with_capacity(44 + samples * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let val = ((i as f64 * 0.15).sin() * 12000.0) as i16;
        out.extend_from_slice(&val.to_le_bytes());
    }
    out
}

fn make_mp3_44k_frames(frame_count: usize) -> Vec<u8> {
    // MPEG-1 Layer III, 128 kbps, 44100 Hz, no padding -> 417 bytes/frame, 1152 samples/frame
    let frame_len = 417usize;
    let mut out = Vec::with_capacity(frame_count * frame_len);
    for _ in 0..frame_count {
        let mut f = vec![0u8; frame_len];
        f[0] = 0xFF;
        f[1] = 0xFB; // MPEG-1, Layer III, no CRC
        f[2] = 0x90; // 128 kbps (9), 44100 Hz (0), padding=0
        f[3] = 0xC0; // Mono
        out.extend_from_slice(&f);
    }
    out
}

#[test]
fn t451_define_font3_multilingual_glyphs_define_text2_and_edit_text_outlines_roundtrip() {
    use neoflash_core::{
        AuthoredTextFieldOptions, Player as CorePlayer, TextFieldType, TextFormat,
    };

    let mut proj = Project::blank();
    proj.width = 480;
    proj.height = 200;

    let mut dyn_fmt = TextFormat {
        font: "Arial".to_owned(),
        size: 18.0,
        color: 0x104090,
        ..TextFormat::default()
    };
    dyn_fmt.set_field_options(Some(AuthoredTextFieldOptions {
        field_type: TextFieldType::Dynamic,
        variable: Some("hudLabel".to_owned()),
        embed_fonts: true,
        ..AuthoredTextFieldOptions::default()
    }));

    let static_el = Element {
        id: "txt_static_multi".to_owned(),
        name: String::new(),
        transform: Transform {
            x: 16.0,
            y: 16.0,
            alpha: 0.75,
            ..Transform::default()
        },
        kind: ElementKind::TextBox {
            text: "Hello SWF 123 - Привет - Ωμέγα".to_owned(),
            width: 420.0,
            height: 44.0,
            format: TextFormat {
                font: "Arial".to_owned(),
                size: 20.0,
                color: 0xCC2244,
                ..TextFormat::default()
            },
            word_wrap: false,
            multiline: false,
        },
        ..Element::default()
    };

    let dyn_el = Element {
        id: "txt_dyn_multi".to_owned(),
        name: "dynField".to_owned(),
        transform: Transform {
            x: 16.0,
            y: 80.0,
            alpha: 0.8,
            ..Transform::default()
        },
        kind: ElementKind::TextBox {
            text: "Dynamic Text 42".to_owned(),
            width: 320.0,
            height: 36.0,
            format: dyn_fmt,
            word_wrap: false,
            multiline: false,
        },
        ..Element::default()
    };

    let root_id = proj.root.clone();
    proj.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements =
        vec![static_el, dyn_el];
    proj.upgrade_format_for_features();
    proj.validate()
        .expect("multilingual text project validates");

    // 1. outline_text: false -> real DefineFont3 (tag 75) with non-zero glyphs + DefineText2 (tag 33) + DefineEditText (tag 37, UseOutlines=1)
    let glyph_opts = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer8Avm1,
        compression: SwfCompression::UncompressedFws,
        outline_text: false,
        ..SwfPublishOptions::default()
    };
    let preflight = preflight_swf_publish(&proj, &glyph_opts);
    assert!(
        preflight.can_proceed(),
        "DefineFont3 preflight blocked: {:?}",
        preflight.items
    );
    let (swf_glyphs, rep_glyphs) =
        publish_swf_bytes(&proj, &glyph_opts).expect("publish DefineFont3 SWF");
    assert!(rep_glyphs.can_proceed());

    let tags = collect_swf_tags_raw(&swf_glyphs);
    let font3_payloads: Vec<&Vec<u8>> = tags
        .iter()
        .filter_map(|(code, p)| (*code == 75).then_some(p))
        .collect();
    assert!(
        !font3_payloads.is_empty(),
        "outline_text=false must emit DefineFont3 (tag 75)"
    );
    // Check DefineFont3 header: FontID(2), Flags(1: HasLayout=0x80 | WideOffsets=0x08 | WideCodes=0x04), Lang(1), NameLen(1), Name(N), NumGlyphs(2)
    let fp = font3_payloads[0];
    let flags = fp[2];
    assert_eq!(
        flags & 0x8C,
        0x8C,
        "DefineFont3 must set FontFlagsHasLayout | WideOffsets | WideCodes (got 0x{flags:02x})"
    );
    let name_len = usize::from(fp[4]);
    let num_glyphs = u16::from_le_bytes([fp[5 + name_len], fp[6 + name_len]]);
    assert!(
        num_glyphs >= 15,
        "DefineFont3 must embed real subdivided vector glyphs for Latin/Cyrillic/Greek chars (got {num_glyphs})"
    );
    assert!(
        tags.iter().any(|(code, _)| *code == 33),
        "static TextBox with outline_text=false must emit DefineText2 (tag 33)"
    );
    let edit_tag = tags
        .iter()
        .find(|(code, _)| *code == 37)
        .expect("dynamic TextBox must emit DefineEditText (tag 37)");
    // DefineEditText payload: CharID(2), Bounds(RECT), flags0(1), flags1(1) where UseOutlines is bit 0x01 of flags1
    let nbits = usize::from(edit_tag.1[2] >> 3);
    let rect_bytes = (5 + nbits * 4).div_ceil(8);
    let edit_flags1 = edit_tag.1[2 + rect_bytes + 1];
    assert_ne!(
        edit_flags1 & 0x01,
        0,
        "DefineEditText paired with DefineFont3 must set UseOutlines=1 (flags1=0x{edit_flags1:02x})"
    );

    let reimported = import_swf_bytes(&swf_glyphs).expect("reimport DefineFont3 SWF");
    reimported
        .validate()
        .expect("reimported DefineFont3 project validates");
    let re_els = &reimported.symbol(&reimported.root).unwrap().timeline.layers;
    assert!(re_els.len() >= 2);
    // Verify non-opaque TextColor alpha preserved across export -> import!
    let re_static = &re_els[0].keyframes[0].elements[0];
    assert!(
        (re_static.transform.alpha - 0.75).abs() < 0.02,
        "reimported DefineText2 alpha expected ~0.75, got {}",
        re_static.transform.alpha
    );

    // 2. outline_text: true -> static shape outlines also succeed and render visible RGBA pixels
    let outline_opts = SwfPublishOptions {
        outline_text: true,
        ..glyph_opts
    };
    let (swf_outlined, _) = publish_swf_bytes(&proj, &outline_opts).expect("publish outlined SWF");
    let re_outlined = import_swf_bytes(&swf_outlined).expect("reimport outlined SWF");
    re_outlined.validate().unwrap();
    let cp = CorePlayer::new(&re_outlined);
    let rgba = cp
        .render_snapshot_rgba(&re_outlined, re_outlined.width, re_outlined.height)
        .expect("render outlined text RGBA");
    assert!(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .any(|&px| px != [255, 255, 255, 255]),
        "outlined multilingual text must render visible pixels"
    );
}

#[test]
fn t451_stream_sound_head2_and_per_frame_wav_pcm_and_mp3_blocks_and_strict_preflight() {
    use neoflash_core::{FrameSound, SoundSync};

    let make_stream_project =
        |media_type: &str, filename: &str, raw_bytes: Vec<u8>, frames: u32| -> Project {
            let mut proj = Project::blank();
            proj.fps = 24;
            let snd_res = ResourceUnit::from_bytes("res_stream", filename, media_type, &raw_bytes)
                .expect("audio resource");
            proj.resources.push(snd_res);
            let root_id = proj.root.clone();
            let tl = &mut proj.symbol_mut(&root_id).unwrap().timeline;
            tl.frame_count = frames;
            tl.layers[0].keyframes[0].sound = Some(FrameSound {
                resource: "res_stream".to_owned(),
                sync: SoundSync::Stream,
                loops: 1,
                loop_forever: false,
                effect: Default::default(),
                in_point: None,
                out_point: None,
                envelope: None,
            });
            proj.upgrade_format_for_features();
            proj.validate().expect("stream sound project validates");
            proj
        };

    let opts = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer8Avm1,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };

    // 1. WAV PCM 22050 Hz across 5 frames (~919 samples/frame -> 4600 samples)
    let wav_proj = make_stream_project(
        "audio/wav",
        "stream.wav",
        make_wav_pcm_bytes(22_050, 4600),
        5,
    );
    let (wav_swf, wav_rep) = publish_swf_bytes(&wav_proj, &opts).expect("publish WAV stream SWF");
    assert!(wav_rep.can_proceed(), "{:?}", wav_rep.items);
    let wav_tags = collect_swf_tags_raw(&wav_swf);
    assert_eq!(
        wav_tags.iter().filter(|(c, _)| *c == 45).count(),
        1,
        "must emit SoundStreamHead2 (tag 45)"
    );
    let wav_blocks = wav_tags.iter().filter(|(c, _)| *c == 19).count();
    assert!(
        wav_blocks >= 4,
        "must emit per-frame SoundStreamBlock (tag 19) tags across stream frames, got {wav_blocks}"
    );
    let wav_play = play_swf_bytes(&wav_swf, 5).expect("play WAV stream SWF");
    assert!(
        wav_play
            .audio_events
            .iter()
            .filter(|e| e.sync == "stream")
            .count()
            >= 4,
        "SwfBytecodePlayer must record per-frame stream audio events: {:?}",
        wav_play.audio_events
    );
    let wav_reimported = import_swf_bytes(&wav_swf).expect("reimport WAV stream SWF");
    wav_reimported
        .validate()
        .expect("reimported WAV stream validates");
    let has_reimported_stream = wav_reimported
        .symbol(&wav_reimported.root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .any(|k| {
            k.sound
                .as_ref()
                .is_some_and(|s| s.sync == SoundSync::Stream)
        });
    assert!(
        has_reimported_stream,
        "reimported SWF must reconstruct SoundSync::Stream"
    );

    // 2. MP3 44100 Hz across 4 frames (8 MP3 frames = 9216 samples)
    let mp3_proj = make_stream_project("audio/mpeg", "stream.mp3", make_mp3_44k_frames(8), 4);
    let (mp3_swf, mp3_rep) = publish_swf_bytes(&mp3_proj, &opts).expect("publish MP3 stream SWF");
    assert!(mp3_rep.can_proceed(), "{:?}", mp3_rep.items);
    let mp3_tags = collect_swf_tags_raw(&mp3_swf);
    let head2 = mp3_tags
        .iter()
        .find(|(c, _)| *c == 45)
        .expect("MP3 SoundStreamHead2");
    assert_eq!(
        head2.1[1] >> 4,
        2,
        "MP3 SoundStreamHead2 must declare StreamSoundCompression=2 (MP3)"
    );
    assert!(
        mp3_tags.iter().filter(|(c, _)| *c == 19).count() >= 3,
        "MP3 stream must emit per-frame SoundStreamBlock tags"
    );
    let mp3_play = play_swf_bytes(&mp3_swf, 4).expect("play MP3 stream SWF");
    assert!(mp3_play.audio_events.iter().any(|e| e.sync == "stream"));

    // 3. Strict preflight rejection on unsupported codec (audio/ogg) and non-SWF 48000 Hz WAV
    let ogg_proj = make_stream_project("audio/ogg", "bad.ogg", b"OggS\x00\x02dummy".to_vec(), 3);
    assert!(
        !preflight_swf_publish(&ogg_proj, &opts).can_proceed(),
        "preflight must strictly reject unsupported OGG stream sound"
    );
    let wav48_proj = make_stream_project(
        "audio/wav",
        "bad48k.wav",
        make_wav_pcm_bytes(48_000, 2400),
        3,
    );
    assert!(
        !preflight_swf_publish(&wav48_proj, &opts).can_proceed(),
        "preflight must strictly reject non-SWF 48 kHz WAV stream sound"
    );
}

#[test]
fn t451_avm1_and_avm2_for_each_switch_fallthrough_delete_builtins_clip_events_and_drawing_api() {
    use neoflash_core::Player as CorePlayer;

    // 1. AVM1 (FlashPlayer8Avm1): for each..in, switch fallthrough, delete, Array/String/Math builtins,
    //    SWF 6/7+ clip & button handlers, and dynamic MovieClip drawing API!
    let mut avm1_proj = Project::blank();
    avm1_proj.script_language = Some(ScriptLanguage::ActionScript2);

    let btn_sym = Symbol {
        id: "sym_btn7".to_owned(),
        name: "Btn7".to_owned(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![Layer {
                id: "b_l".to_owned(),
                name: "L1".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: (0..4)
                    .map(|f| Keyframe {
                        frame: f,
                        elements: vec![Element {
                            id: format!("b_rect_{f}"),
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 18.0,
                                fill: [40, 140, 220, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    })
                    .collect(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };
    let clip_sym = Symbol {
        id: "sym_clip7".to_owned(),
        name: "Clip7".to_owned(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: "c_l".to_owned(),
                name: "L1".to_owned(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "c_rect".to_owned(),
                        kind: ElementKind::Rectangle {
                            width: 24.0,
                            height: 24.0,
                            fill: [200, 80, 60, 255],
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
    avm1_proj.symbols.push(btn_sym);
    avm1_proj.symbols.push(clip_sym);

    let root_id = avm1_proj.root.clone();
    {
        let root_tl = &mut avm1_proj.symbol_mut(&root_id).unwrap().timeline;
        root_tl.frame_count = 2;
        root_tl.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![
                    Element {
                        id: "inst_btn7".to_owned(),
                        name: "actBtn".to_owned(),
                        transform: Transform {
                            x: 10.0,
                            y: 10.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: "sym_btn7".to_owned(),
                            playback: InstancePlayback::Independent,
                        },
                        scripts: vec![ObjectScript {
                            event: ObjectEvent::Click,
                            script: Script {
                                language: ScriptLanguage::ActionScript2,
                                source: r#"
                                    on (rollOver) { trace("btn:over"); }
                                    on (rollOut) { trace("btn:out"); }
                                    on (keyPress "<Space>") { trace("btn:space"); }
                                "#
                                .to_owned(),
                            },
                        }],
                        ..Element::default()
                    },
                    Element {
                        id: "inst_clip7".to_owned(),
                        name: "actClip".to_owned(),
                        transform: Transform {
                            x: 60.0,
                            y: 10.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: "sym_clip7".to_owned(),
                            playback: InstancePlayback::Independent,
                        },
                        scripts: vec![ObjectScript {
                            event: ObjectEvent::Load,
                            script: Script {
                                language: ScriptLanguage::ActionScript2,
                                source: r#"
                                    onClipEvent (unload) { trace("clip:unload"); }
                                    on (releaseOutside) { trace("clip:relOut"); }
                                "#
                                .to_owned(),
                            },
                        }],
                        ..Element::default()
                    },
                ],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 1,
                elements: Vec::new(), // removes both on frame 1 -> triggers clip:unload!
                ..Keyframe::default()
            },
        ];
        root_tl.scripts = vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: r#"
                        var nums = [3, 5, 7];
                        var sum = 0;
                        for each (var v in nums) {
                            sum += v;
                        }
                        var sw = 0;
                        for (var k = 1; k <= 3; k++) {
                            switch (k) {
                                case 1:
                                    sw += 10;
                                case 2:
                                    sw += 100;
                                    break;
                                default:
                                    sw += 1;
                            }
                        }
                        var bag = { a: 9, b: 4, c: 2 };
                        delete bag.b;
                        var dynKey = "c";
                        delete bag[dynKey];
                        var bagCount = 0;
                        for (var bk in bag) {
                            bagCount += bag[bk];
                        }
                        var arr = [10, 20, 30, 20];
                        var sp = arr.splice(1, 1, 99);
                        var cat = arr.concat([40]);
                        var s = String.fromCharCode(65, 66, 67).concat("-xy-AB");
                        var mm = Math.min(9, 4, 2, 7) + ":" + Math.max(1, 8, 3, 5);

                        var canvas = createEmptyMovieClip("canvas", 50);
                        canvas.lineStyle(2, 0x112233, 100);
                        canvas.beginFill(0x22AA66, 100);
                        canvas.moveTo(100, 60);
                        canvas.lineTo(160, 60);
                        canvas.curveTo(170, 90, 160, 120);
                        canvas.lineTo(100, 120);
                        canvas.lineTo(100, 60);
                        canvas.endFill();

                        trace("avm1:" + sum + ":" + sw + ":" + bagCount + ":" + sp[0] + ":" + cat.length + ":" + arr.indexOf(99) + ":" + s.lastIndexOf("AB") + ":" + mm);
                    "#
                    .to_owned(),
                },
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace(\"avm1:f2\"); stop();".to_owned(),
                },
            },
        ];
    }
    avm1_proj.upgrade_format_for_features();
    avm1_proj.validate().unwrap();

    let avm1_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1);
    let (avm1_swf, avm1_rep) =
        publish_swf_bytes(&avm1_proj, &avm1_opts).expect("publish AVM1 T-451 SWF");
    assert!(avm1_rep.can_proceed(), "{:?}", avm1_rep.items);
    let avm1_reimp = import_swf_bytes(&avm1_swf).expect("reimport AVM1 T-451 SWF");
    avm1_reimp.validate().unwrap();

    let mut p1 = SwfBytecodePlayer::from_bytes(&avm1_swf).unwrap();
    let s0 = p1.step_frame();
    // sum=15, sw=(10+100)+(100)+(1)=211, bagCount=9, sp[0]=20, cat.length=5, indexOf(99)=1, "ABC-xy-AB".lastIndexOf("AB")=7, mm=2:8
    assert_eq!(s0.traces, vec!["avm1:15:211:9:20:5:1:7:2:8"]);
    let overlay1 = p1.project_with_display_list_overlay(&avm1_reimp, &avm1_reimp.root, 0);
    overlay1.validate().unwrap();
    let rgba1 = CorePlayer::new(&overlay1)
        .render_snapshot_rgba(&overlay1, overlay1.width, overlay1.height)
        .unwrap();
    // Sample inside dynamically drawn polygon [100..160, 60..120]
    let px_idx = (90usize * overlay1.width as usize + 130usize) * 4;
    assert_ne!(
        &rgba1[px_idx..px_idx + 4],
        &[255, 255, 255, 255],
        "AVM1 dynamic beginFill/moveTo/lineTo/curveTo/endFill must rasterize onto Stage"
    );

    let before_trig = p1.trace_output().len();
    p1.trigger_pointer_roll_over();
    p1.trigger_pointer_roll_out();
    p1.trigger_pointer_release_outside();
    p1.trigger_key_press(32);
    assert_eq!(
        &p1.trace_output()[before_trig..],
        &["btn:over", "btn:out", "clip:relOut", "btn:space"]
    );
    let s1 = p1.step_frame();
    assert_eq!(s1.traces, vec!["clip:unload", "avm1:f2"]);

    // 2. AVM2 (FlashPlayer10Avm2): for each..in, switch fallthrough, delete, Array/String/Math builtins, and Sprite.graphics
    let mut avm2_proj = Project::blank();
    avm2_proj.script_language = Some(ScriptLanguage::ActionScript3);
    let root2 = avm2_proj.root.clone();
    avm2_proj.symbol_mut(&root2).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"
                var items:Array = [4, 6, 10];
                var total:int = 0;
                for each (var item:int in items) {
                    total += item;
                }
                var sw:int = 0;
                for (var i:int = 1; i <= 3; i++) {
                    switch (i) {
                        case 1:
                            sw += 5;
                        case 2:
                            sw += 50;
                            break;
                        default:
                            sw += 2;
                    }
                }
                var dict:Object = { keep: 12, dropA: 99, dropB: 88 };
                delete dict.dropA;
                var dk:String = "dropB";
                delete dict[dk];
                var dsum:int = 0;
                for each (var dv:int in dict) {
                    dsum += dv;
                }
                var arr:Array = [1, 2, 3, 2];
                var cut:Array = arr.splice(1, 1, 77);
                var joined:Array = arr.concat([9]);
                var txt:String = String.fromCharCode(88, 89, 90).concat("-XY");
                var mm:String = Math.min(8, 3, 6, 1) + ":" + Math.max(4, 2, 9, 5);

                var sp:Sprite = new Sprite();
                sp.graphics.lineStyle(2, 0x223344, 1.0);
                sp.graphics.beginFill(0xDD3355, 1.0);
                sp.graphics.moveTo(40, 40);
                sp.graphics.lineTo(100, 40);
                sp.graphics.curveTo(110, 70, 100, 100);
                sp.graphics.lineTo(40, 100);
                sp.graphics.endFill();
                sp.graphics.beginFill(0x3388EE, 1.0);
                sp.graphics.drawCircle(180, 70, 20);
                sp.graphics.endFill();
                addChild(sp);

                trace("avm2:" + total + ":" + sw + ":" + dsum + ":" + cut[0] + ":" + joined.length + ":" + arr.indexOf(77) + ":" + txt.lastIndexOf("XY") + ":" + mm);
                stop();
            "#
            .to_owned(),
        },
    }];
    avm2_proj.upgrade_format_for_features();
    avm2_proj.validate().unwrap();

    let avm2_opts = SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer10Avm2);
    let (avm2_swf, avm2_rep) =
        publish_swf_bytes(&avm2_proj, &avm2_opts).expect("publish AVM2 T-451 SWF");
    assert!(avm2_rep.can_proceed(), "{:?}", avm2_rep.items);
    let avm2_reimp = import_swf_bytes(&avm2_swf).expect("reimport AVM2 T-451 SWF");
    avm2_reimp.validate().unwrap();

    let mut p2 = SwfBytecodePlayer::from_bytes(&avm2_swf).unwrap();
    let a0 = p2.step_frame();
    // total=20, sw=(5+50)+(50)+(2)=107, dsum=12, cut[0]=2, joined.length=5, indexOf(77)=1, "XYZ-XY".lastIndexOf("XY")=4, mm=1:9
    assert_eq!(a0.traces, vec!["avm2:20:107:12:2:5:1:4:1:9"]);
    let overlay2 = p2.project_with_display_list_overlay(&avm2_reimp, &avm2_reimp.root, 0);
    overlay2.validate().unwrap();
    let rgba2 = CorePlayer::new(&overlay2)
        .render_snapshot_rgba(&overlay2, overlay2.width, overlay2.height)
        .unwrap();
    let px_curve = (70usize * overlay2.width as usize + 70usize) * 4;
    let px_circle = (70usize * overlay2.width as usize + 180usize) * 4;
    assert_ne!(&rgba2[px_curve..px_curve + 4], &[255, 255, 255, 255]);
    assert_ne!(&rgba2[px_circle..px_circle + 4], &[255, 255, 255, 255]);
}

#[test]
fn t451_multi_fixture_ruffle_differential_swf_import_and_republish_roundtrip() {
    use neoflash_core::Player as CorePlayer;

    let ruffle_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("examples/fla-corpus/upstream/ruffle");

    // 1. Trace-oracle differential round-trip: original SWF -> imported .nflash -> republished SWF
    let trace_roundtrip_cases: [(&str, SwfPublishProfile, usize); 3] = [
        ("avm1-frame-labels", SwfPublishProfile::FlashPlayer8Avm1, 4),
        ("avm1-clip-events", SwfPublishProfile::FlashPlayer8Avm1, 4),
        (
            "avm1-nested-button-text",
            SwfPublishProfile::FlashPlayer8Avm1,
            1,
        ),
    ];
    for (name, profile, ticks) in trace_roundtrip_cases {
        let swf_path = ruffle_dir.join(name).join("test.swf");
        let oracle_path = ruffle_dir.join(name).join("output.txt");
        let expected: Vec<String> = fs::read_to_string(&oracle_path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();

        let imported = import_project(&swf_path).unwrap();
        imported.validate().unwrap();
        let saved = Project::from_json(&imported.to_json().unwrap()).unwrap();
        saved.validate().unwrap();

        let (repub_swf, rep) =
            publish_swf_bytes(&saved, &SwfPublishOptions::for_profile(profile)).unwrap();
        assert!(
            rep.can_proceed(),
            "{name} republish blocked: {:?}",
            rep.items
        );

        let mut player = SwfBytecodePlayer::from_bytes(&repub_swf).unwrap();
        let mut traces = Vec::new();
        for _ in 0..ticks {
            traces.extend(player.step_frame().traces);
        }
        assert_eq!(
            traces, expected,
            "republished SWF trace differential mismatch against Flash oracle on {name}"
        );
    }

    // 1b. AVM2 Ruffle SWF fixtures: original SWF == Flash oracle (28/28 & 18/18 lines),
    //     imported .nflash validates & round-trips JSON, and exports valid HTML5 bundles!
    let temp = TempDirGuard::new("t451-avm2-ruffle");
    let runtime_dir = temp.0.join("runtime");
    create_mock_runtime(&runtime_dir);
    for (name, ticks, expected_lines) in [
        ("avm2-button-frame-scripts", 2usize, 28usize),
        ("avm2-nested-construction", 3usize, 18usize),
    ] {
        let swf_path = ruffle_dir.join(name).join("test.swf");
        let oracle_path = ruffle_dir.join(name).join("output.txt");
        let expected: Vec<String> = fs::read_to_string(&oracle_path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(expected.len(), expected_lines);

        let orig_bytes = fs::read(&swf_path).unwrap();
        let mut p_orig = SwfBytecodePlayer::from_bytes(&orig_bytes).unwrap();
        let mut orig_traces = Vec::new();
        for _ in 0..ticks {
            orig_traces.extend(p_orig.step_frame().traces);
        }
        assert_eq!(
            orig_traces, expected,
            "original {name} SWF must match Flash oracle"
        );

        let imported = import_project(&swf_path).unwrap();
        imported.validate().unwrap();
        let saved = Project::from_json(&imported.to_json().unwrap()).unwrap();
        saved.validate().unwrap();
        assert!(
            !saved.sources.is_empty(),
            "{name} must decompile DoABC2 into AS3 sources"
        );

        let html_out = temp.0.join(name);
        export_project(&saved, &html_out, &runtime_dir)
            .expect("export AVM2 Ruffle .nflash to HTML5");
        assert!(html_out.join("bundle.js").is_file());
    }

    // 2. Visual RGBA differential round-trip on MX SWFs (original SWF vs imported .nflash vs republished SWF)
    let visual_cases = [
        ("mx-button/DefineButton-MX.swf", 1usize),
        ("mx-shape-tween/DefineMorphShape-MX.swf", 5usize),
        ("mx-static-text/DefineText2-MX.swf", 1usize),
    ];
    for (rel, frames) in visual_cases {
        let swf_path = ruffle_dir.join(rel);
        let orig_bytes = fs::read(&swf_path).unwrap();
        let imported = import_project(&swf_path).unwrap();
        imported.validate().unwrap();

        let (repub_bytes, rep) = publish_swf_bytes(
            &imported,
            &SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1),
        )
        .unwrap();
        assert!(
            rep.can_proceed(),
            "{rel} republish blocked: {:?}",
            rep.items
        );
        let reimp = import_swf_bytes(&repub_bytes).unwrap();
        reimp.validate().unwrap();

        let mut p_orig = SwfBytecodePlayer::from_bytes(&orig_bytes).unwrap();
        let mut p_repub = SwfBytecodePlayer::from_bytes(&repub_bytes).unwrap();
        for f in 0..frames {
            let st_o = p_orig.step_frame();
            let st_r = p_repub.step_frame();
            let ov_o =
                p_orig.project_with_display_list_overlay(&imported, &imported.root, st_o.frame);
            let ov_r = p_repub.project_with_display_list_overlay(&reimp, &reimp.root, st_r.frame);
            let rgba_o = CorePlayer::new(&ov_o)
                .render_snapshot_rgba(&ov_o, ov_o.width, ov_o.height)
                .unwrap();
            let rgba_r = CorePlayer::new(&ov_r)
                .render_snapshot_rgba(&ov_r, ov_r.width, ov_r.height)
                .unwrap();
            let fg_o = rgba_o
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|&&px| px != ov_o.background)
                .count();
            let fg_r = rgba_r
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|&&px| px != ov_r.background)
                .count();
            assert!(
                fg_o > 0 && fg_r > 0,
                "{rel} frame {f} foreground pixel count zero (orig={fg_o}, repub={fg_r})"
            );
            let ratio = (fg_r as f64) / (fg_o as f64);
            assert!(
                (0.75..=1.35).contains(&ratio),
                "{rel} frame {f} RGBA foreground coverage drifted (orig={fg_o}, repub={fg_r}, ratio={ratio:.3})"
            );
        }
    }
}
