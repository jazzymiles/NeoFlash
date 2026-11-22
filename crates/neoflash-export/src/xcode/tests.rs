use super::*;
use crate::bitmap_encode::encode_rgba_png;
use neoflash_core::{
    BlendMode, Easing, Element, ElementKind, FillRule, FilterSpec, FrameSound, GradientKind,
    GradientPaint, GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties, ObjectEvent,
    PathCommand, Project, ResourceUnit, SoundSync, StrokeProfilePreset, StrokeWidthProfile, Symbol,
    SymbolKind, TextAlign, TextFormat, Timeline, Transform, TweenKind,
};
use std::collections::BTreeMap;
use std::process::Command;
use tempfile::TempDir;

/// Builds a valid minimal TrueType/SFNT binary with a `name` table containing PostScript name `TestEmbeddedSans-Regular`.
fn make_minimal_sfnt_font(postscript_name: &str) -> Vec<u8> {
    let utf16_bytes: Vec<u8> = postscript_name
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut name_table = Vec::new();
    name_table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    name_table.extend_from_slice(&1u16.to_be_bytes()); // count = 1
    name_table.extend_from_slice(&18u16.to_be_bytes()); // stringOffset = 6 + 12
    // NameRecord: platformID=3 (Windows), encodingID=1 (Unicode BMP), languageID=0x0409, nameID=6 (PostScript)
    name_table.extend_from_slice(&3u16.to_be_bytes());
    name_table.extend_from_slice(&1u16.to_be_bytes());
    name_table.extend_from_slice(&0x0409u16.to_be_bytes());
    name_table.extend_from_slice(&6u16.to_be_bytes());
    name_table.extend_from_slice(&(utf16_bytes.len() as u16).to_be_bytes());
    name_table.extend_from_slice(&0u16.to_be_bytes());
    name_table.extend_from_slice(&utf16_bytes);

    let mut sfnt = Vec::new();
    sfnt.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // TrueType version
    sfnt.extend_from_slice(&1u16.to_be_bytes()); // numTables = 1
    sfnt.extend_from_slice(&16u16.to_be_bytes()); // searchRange
    sfnt.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
    sfnt.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
    // TableDirectory entry for `name` at offset 28
    sfnt.extend_from_slice(b"name");
    sfnt.extend_from_slice(&0u32.to_be_bytes()); // checkSum
    sfnt.extend_from_slice(&28u32.to_be_bytes()); // offset = 12 + 16
    sfnt.extend_from_slice(&(name_table.len() as u32).to_be_bytes());
    sfnt.extend_from_slice(&name_table);
    while !sfnt.len().is_multiple_of(4) {
        sfnt.push(0);
    }
    sfnt
}

/// Builds a minimal valid PCM WAV byte buffer (44.1kHz mono 16-bit, 64 samples).
fn make_minimal_wav_bytes() -> Vec<u8> {
    let sample_rate: u32 = 44_100;
    let num_samples: u32 = 64;
    let data_bytes: u32 = num_samples * 2;
    let riff_size: u32 = 36 + data_bytes;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    wav.resize(44 + data_bytes as usize, 0);
    wav
}

fn build_rich_multi_symbol_project() -> (Project, XcodeScriptBundle) {
    // 1. Embedded resources: 4x4 RGBA PNG, PCM WAV, and minimal embedded TTF font
    let mut rgba = vec![0u8; 4 * 4 * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        *px = [30, 180, 255, 255];
    }
    let png_bytes = encode_rgba_png(&rgba, 4, 4).expect("valid 4x4 PNG");
    let png_res =
        ResourceUnit::from_bytes("res-badge-png", "images/badge.png", "image/png", &png_bytes)
            .expect("png resource");

    let wav_bytes = make_minimal_wav_bytes();
    let wav_res =
        ResourceUnit::from_bytes("res-chime-wav", "audio/chime.wav", "audio/wav", &wav_bytes)
            .expect("wav resource");

    let font_bytes = make_minimal_sfnt_font("TestEmbeddedSans-Regular");
    let font_res = ResourceUnit::from_bytes(
        "res-font-sans",
        "fonts/TestEmbeddedSans.ttf",
        "font/ttf",
        &font_bytes,
    )
    .expect("font resource");

    // 2. Symbol 1: `SymHeroBadge` (MovieClip with focal radial gradient + negative registration point)
    let badge_sym = Symbol {
        id: "sym-hero-badge".into(),
        name: "HeroBadge".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("HeroBadgeLink".into()),
        timeline: Timeline {
            frame_count: 1,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-badge".into(),
                name: "Art".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-badge-orb".into(),
                        name: String::new(),
                        transform: Transform {
                            x: -24.0,
                            y: -24.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::PaintedPath {
                            commands: vec![
                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                PathCommand::LineTo { x: 48.0, y: 0.0 },
                                PathCommand::LineTo { x: 48.0, y: 48.0 },
                                PathCommand::LineTo { x: 0.0, y: 48.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [255, 120, 40, 255],
                            stroke: [255, 255, 255, 255],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Radial,
                                matrix: [0.03, 0.0, 0.0, 0.03, 24.0, 24.0],
                                stops: vec![
                                    GradientStop {
                                        ratio: 0,
                                        color: [255, 250, 180, 255],
                                    },
                                    GradientStop {
                                        ratio: 255,
                                        color: [220, 60, 20, 255],
                                    },
                                ],
                                focal_point_ratio: -0.65,
                                spread: neoflash_core::GradientSpread::Extend,
                                linear_rgb: false,
                            }),
                            stroke_gradient: None,
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };

    // 3. Symbol 2: `SymPlayButton` (4-frame Flash Button: Up / Over / Down / Hit)
    let make_btn_frame = |frame: u32, color: [u8; 4]| Keyframe {
        frame,
        elements: vec![Element {
            id: format!("el-btn-{frame}"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 44.0,
                fill: color,
                stroke: [20, 30, 50, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        }],
        ..Keyframe::default()
    };
    let play_btn_sym = Symbol {
        id: "sym-play-btn".into(),
        name: "PlayButton".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-btn".into(),
                name: "States".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    make_btn_frame(0, [40, 130, 240, 255]),
                    make_btn_frame(1, [70, 160, 255, 255]),
                    make_btn_frame(2, [25, 95, 190, 255]),
                    make_btn_frame(3, [0, 0, 0, 255]),
                ],
            }],
        },
    };

    // 4. Symbol 3: `SymSpinnerGraphic` (Graphic symbol with 4 frames)
    let spinner_sym = Symbol {
        id: "sym-spinner-gfx".into(),
        name: "SpinnerGraphic".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-spinner".into(),
                name: "Tick".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::Linear),
                        tween_kind: TweenKind::Classic,
                        elements: vec![Element {
                            id: "el-spin-bar".into(),
                            name: String::new(),
                            transform: Transform {
                                rotation: 0.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 8.0,
                                fill: [80, 220, 160, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![Element {
                            id: "el-spin-bar".into(),
                            name: String::new(),
                            transform: Transform {
                                rotation: 90.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 8.0,
                                fill: [80, 220, 160, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            }],
        },
    };

    // 5. Root Symbol (`MainScene`, 12 frames):
    // - Layer 0: Compound PaintedPath with EvenOdd hole + Linear gradient + TaperBoth variable-width stroke
    // - Layer 1: Shape morph tween (frames 0 -> 11)
    // - Layer 2: Classic tween of `heroBadge` (frames 0 -> 11, x: 80 -> 300) + `tweenRunner` (untouched by script!)
    // - Layer 3: UI Controls (`playBtn`, `titleField` TextBox, `spinnerGfx` Graphic Loop, `badgeBmp` Bitmap) + labels ("intro" @ 0, "gameplay" @ 6) + FrameSound @ 0
    let mut compound_donut = Element {
        id: "el-donut".into(),
        name: "donutRing".into(),
        transform: Transform {
            x: 200.0,
            y: 150.0,
            skew: 8.0,
            ..Transform::default()
        },
        kind: ElementKind::PaintedPath {
            commands: vec![
                // Outer contour: 0..100
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 100.0 },
                PathCommand::LineTo { x: 0.0, y: 100.0 },
                PathCommand::Close,
                // Inner hole: 30..70
                PathCommand::MoveTo { x: 30.0, y: 30.0 },
                PathCommand::LineTo { x: 70.0, y: 30.0 },
                PathCommand::LineTo { x: 70.0, y: 70.0 },
                PathCommand::LineTo { x: 30.0, y: 70.0 },
                PathCommand::Close,
            ],
            fill_rule: FillRule::EvenOdd,
            fill: [60, 120, 250, 255],
            stroke: [255, 200, 60, 255],
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                matrix: [0.06, 0.0, 0.0, 0.06, 50.0, 50.0],
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: [40, 180, 255, 255],
                    },
                    GradientStop {
                        ratio: 255,
                        color: [140, 60, 255, 255],
                    },
                ],
                focal_point_ratio: 0.0,
                spread: neoflash_core::GradientSpread::Extend,
                linear_rgb: true,
            }),
            stroke_gradient: None,
            stroke_width: 4.0,
        },
        ..Element::default()
    };
    compound_donut.set_stroke_width_profile(StrokeWidthProfile::from_preset(
        StrokeProfilePreset::TaperBoth,
    ));

    let morph_start = Element {
        id: "el-morph-a".into(),
        name: String::new(),
        transform: Transform {
            x: 40.0,
            y: 260.0,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 50.0,
            height: 50.0,
            fill: [255, 80, 120, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let morph_end = Element {
        id: "el-morph-b".into(),
        name: String::new(),
        transform: Transform {
            x: 120.0,
            y: 260.0,
            ..Transform::default()
        },
        kind: ElementKind::Ellipse {
            width: 70.0,
            height: 40.0,
            fill: [80, 220, 255, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Element::default()
    };

    let root_sym = Symbol {
        id: "sym-root".into(),
        name: "Scene 1".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 12,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![
                Layer {
                    id: "lyr-donut".into(),
                    name: "VectorArt".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![compound_donut],
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "lyr-morph".into(),
                    name: "ShapeMorph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![morph_start],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![morph_end],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "lyr-classic".into(),
                    name: "ClassicTween".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Linear),
                            tween_kind: TweenKind::Classic,
                            elements: vec![
                                Element {
                                    id: "el-hero-inst".into(),
                                    name: "heroBadge".into(),
                                    transform: Transform {
                                        x: 80.0,
                                        y: 80.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-hero-badge".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "el-runner-inst".into(),
                                    name: "tweenRunner".into(),
                                    transform: Transform {
                                        x: 50.0,
                                        y: 420.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-hero-badge".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![
                                Element {
                                    id: "el-hero-inst".into(),
                                    name: "heroBadge".into(),
                                    transform: Transform {
                                        x: 300.0,
                                        y: 80.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-hero-badge".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "el-runner-inst".into(),
                                    name: "tweenRunner".into(),
                                    transform: Transform {
                                        x: 270.0,
                                        y: 420.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-hero-badge".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "lyr-ui".into(),
                    name: "Controls".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            label: "intro".into(),
                            sound: Some(FrameSound {
                                resource: "res-chime-wav".into(),
                                sync: SoundSync::Start,
                                loops: 1,
                                loop_forever: false,
                                effect: neoflash_core::SoundEffect::None,
                                envelope: None,
                                in_point: None,
                                out_point: None,
                            }),
                            elements: vec![
                                Element {
                                    id: "el-play-btn".into(),
                                    name: "playBtn".into(),
                                    transform: Transform {
                                        x: 340.0,
                                        y: 500.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-play-btn".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "el-spinner".into(),
                                    name: "spinnerGfx".into(),
                                    transform: Transform {
                                        x: 520.0,
                                        y: 515.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-spinner-gfx".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "el-title-text".into(),
                                    name: "titleField".into(),
                                    transform: Transform {
                                        x: 40.0,
                                        y: 24.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::TextBox {
                                        text: "NeoFlash Swift 6 Stage".into(),
                                        width: 320.0,
                                        height: 40.0,
                                        format: TextFormat {
                                            font: "TestEmbeddedSans".into(),
                                            size: 22.0,
                                            color: 0xFFFFFF,
                                            bold: true,
                                            align: TextAlign::Left,
                                            ..TextFormat::default()
                                        },
                                        word_wrap: true,
                                        multiline: false,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "el-bmp-icon".into(),
                                    name: "badgeBmp".into(),
                                    transform: Transform {
                                        x: 720.0,
                                        y: 24.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Bitmap {
                                        resource_id: "res-badge-png".into(),
                                        width: 32,
                                        height: 32,
                                        smoothing: true,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 6,
                            label: "gameplay".into(),
                            elements: vec![Element {
                                id: "el-gameplay-label".into(),
                                name: String::new(),
                                transform: Transform {
                                    x: 40.0,
                                    y: 68.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "Gameplay Active".into(),
                                    font_family: "TestEmbeddedSans".into(),
                                    font_size: 18.0,
                                    color: [120, 255, 180, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
            ],
        },
    };

    let project = Project {
        format_version: 14,
        name: "Liquid Arcade Demo".into(),
        width: 800,
        height: 600,
        fps: 24,
        background: [18, 22, 34, 255],
        root: "sym-root".into(),
        symbols: vec![root_sym, badge_sym, play_btn_sym, spinner_sym],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: vec![png_res, wav_res, font_res],
        document_class: None,
        scenes: Vec::new(),
        script_language: None,
        also_compile_compatible_scripts: true,
    };

    let mut root_frames = BTreeMap::new();
    root_frames.insert(
        0,
        r#"if let st = self.stage {
    st.scaleMode = .noScale
    st.align = .topLeft
    st.addListener(self)
}
self.dynamicState.phase = "intro"
self.host?.trackEvent(name: "scene_boot")"#
            .to_owned(),
    );

    let scripts = XcodeScriptBundle {
        host_methods: vec![
            XcodeSwiftProxyMethod {
                name: "trackEvent".into(),
                params: vec![XcodeSwiftProxyParam {
                    name: "name".into(),
                    swift_type: "String".into(),
                }],
                return_swift_type: None,
            },
            XcodeSwiftProxyMethod {
                name: "currentTier".into(),
                params: Vec::new(),
                return_swift_type: Some("String".into()),
            },
        ],
        symbols: vec![XcodeSymbolScriptBundle {
            symbol_id: "sym-root".into(),
            swift_class_name: "MainScene".into(),
            properties_swift:
                "public var resizeCount: Int = 0\npublic var dynamicState: NFValue = [\"score\": 0]"
                    .into(),
            frame_methods_swift: root_frames,
            helper_methods_swift: r#"public func onResize() {
    guard let st = self.stage else { return }
    self.resizeCount += 1
    self.heroBadge.x = st.stageWidth - st.safeRight - 60.0
    self.playBtn.y = st.stageHeight - st.safeBottom - 56.0
}"#
            .into(),
            instance_handlers_swift: vec![(
                "playBtn".into(),
                ObjectEvent::Click,
                "self.dynamicState.score += 10\nself.gotoAndStop(\"gameplay\")".into(),
            )],
        }],
        source_classes_swift: String::new(),
    };

    (project, scripts)
}

fn assert_plutil_lint(path: &Path) {
    let out = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(path)
        .output()
        .expect("run /usr/bin/plutil");
    assert!(
        out.status.success(),
        "plutil -lint failed for {}:\nstdout: {}\nstderr: {}",
        path.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn assert_xcodebuild_succeeds(xcodeproj_path: &Path, scheme: &str, derived_data_dir: &Path) {
    let out = Command::new("/usr/bin/xcodebuild")
        .arg("-project")
        .arg(xcodeproj_path)
        .arg("-scheme")
        .arg(scheme)
        .arg("-configuration")
        .arg("Debug")
        .arg("-destination")
        .arg("platform=macOS,arch=arm64")
        .arg("-derivedDataPath")
        .arg(derived_data_dir)
        .arg("CODE_SIGNING_ALLOWED=NO")
        .arg("build")
        .output()
        .expect("run /usr/bin/xcodebuild");
    assert!(
        out.status.success(),
        "xcodebuild failed for {} (scheme {}):\nstdout:\n{}\nstderr:\n{}",
        xcodeproj_path.display(),
        scheme,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn preflight_detects_macos_fonts_blend_modes_displacement_filters_and_video_codecs() {
    let mut project = Project::demo();
    project.resources.push(
        ResourceUnit::from_bytes("res-webm", "video/clip.webm", "video/webm", b"webmdata")
            .expect("webm resource"),
    );

    let disp_filter: FilterSpec = serde_json::from_value(serde_json::json!({
        "type": "displacement",
        "map_width": 4,
        "map_height": 4,
        "map_rgba": vec![128u8; 64],
        "map_point": [0, 0],
        "component_x": 1,
        "component_y": 2,
        "scale_x": 12.0,
        "scale_y": 12.0,
        "mode": "clamp",
        "color": 0,
        "alpha": 0.0
    }))
    .expect("valid displacement FilterSpec");

    let root = project.symbol_mut(&project.root.clone()).expect("root");
    root.timeline.layers.push(Layer {
        id: "lyr-diag".into(),
        name: "DiagLayer".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            blend_mode: BlendMode::Invert,
            ..LayerProperties::default()
        },
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "el-monaco".into(),
                name: "debugLabel".into(),
                transform: Transform::default(),
                kind: ElementKind::Text {
                    text: "FPS: 60".into(),
                    font_family: "Monaco".into(),
                    font_size: 14.0,
                    color: [255, 255, 255, 255],
                },
                blend_mode: BlendMode::Erase,
                filters: vec![disp_filter],
                ..Element::default()
            }],
            ..Keyframe::default()
        }],
    });

    let items = preflight_xcode_project(&project);
    let codes: Vec<&str> = items.iter().map(|i| i.code.as_str()).collect();
    assert!(codes.contains(&"unembedded_macos_font"), "got: {items:?}");
    assert!(
        codes.contains(&"unsupported_blend_invert"),
        "got: {items:?}"
    );
    assert!(codes.contains(&"unsupported_blend_erase"), "got: {items:?}");
    assert!(
        codes.contains(&"unsupported_displacement_map_filter"),
        "got: {items:?}"
    );
    assert!(codes.contains(&"unsupported_video_codec"), "got: {items:?}");

    let monaco_item = items
        .iter()
        .find(|i| i.code == "unembedded_macos_font")
        .unwrap();
    assert!(
        monaco_item
            .location
            .contains("DiagLayer / frame 1 / debugLabel"),
        "expected exact symbol/layer/frame/element location, got: {}",
        monaco_item.location
    );
}

#[test]
fn swift_host_scanner_ignores_comments_and_strings_when_appending_stubs() {
    let custom_host = r##"import Foundation

// Commented-out stub MUST NOT trick scanner:
// func openPaywall(tier: String) {}
/* Outer nested block comment
   /* func openPaywall(tier: String) {} */
   func rewardCoins(amount: Int) -> Bool { return true }
*/

@MainActor
final class AppSwiftHost: SwiftHostProtocol {
    var events: [String] = []
    let fakeNote = "func openPaywall(tier: String) { }"
    let rawNote = #"func rewardCoins(amount: Int) -> Bool"#

    func trackEvent(name: String) {
        if !name.isEmpty {
            events.append(name)
        }
    }
}
"##;

    let methods = vec![
        XcodeSwiftProxyMethod {
            name: "trackEvent".into(),
            params: vec![XcodeSwiftProxyParam {
                name: "name".into(),
                swift_type: "String".into(),
            }],
            return_swift_type: None,
        },
        XcodeSwiftProxyMethod {
            name: "openPaywall".into(),
            params: vec![XcodeSwiftProxyParam {
                name: "tier".into(),
                swift_type: "String".into(),
            }],
            return_swift_type: None,
        },
        XcodeSwiftProxyMethod {
            name: "rewardCoins".into(),
            params: vec![XcodeSwiftProxyParam {
                name: "amount".into(),
                swift_type: "Int".into(),
            }],
            return_swift_type: Some("Bool".into()),
        },
    ];

    let (updated, appended) = app_shell::append_missing_host_stubs(custom_host, &methods);
    assert_eq!(appended, vec!["openPaywall", "rewardCoins"]);
    assert!(updated.contains("events.append(name)"));
    assert!(updated.contains("func openPaywall(tier: String) {"));
    assert!(updated.contains("func rewardCoins(amount: Int) -> Bool {"));
    assert!(updated.contains("return false"));

    // Re-running with the exact same method list must be a pure idempotent no-op!
    let (re_updated, re_appended) = app_shell::append_missing_host_stubs(&updated, &methods);
    assert!(re_appended.is_empty());
    assert_eq!(re_updated, updated);
}

#[test]
fn xcode_export_end_to_end_plutil_xcodebuild_roundtrip_and_runtime_semantics() {
    let tmp = TempDir::new().expect("tempdir");
    let out_dir = tmp.path().join("LiquidArcadeExport");
    let derived_data_dir = tmp.path().join("DerivedData");

    let (project, mut scripts) = build_rich_multi_symbol_project();
    let opts = XcodeExportOptions::default();

    // 1. Initial Export
    let report = export_xcode_project(&project, &out_dir, &opts, Some(&scripts))
        .expect("initial Xcode export");
    assert_eq!(report.scheme_name, "LiquidArcadeDemo");
    assert_eq!(
        report.bundle_identifier,
        "com.neoflash.generated.liquidarcadedemo"
    );
    assert_eq!(report.symbol_count, 4);
    assert!(report.preserved_app_files.is_empty());
    assert!(report.appended_host_methods.is_empty());
    assert!(report.pbxproj_path.is_file());
    assert!(report.info_plist_path.is_file());

    let info_plist_text = fs::read_to_string(&report.info_plist_path).unwrap();
    assert!(info_plist_text.contains("<key>UIAppFonts</key>"));
    assert!(info_plist_text.contains("<key>ATSApplicationFontsPath</key>"));
    assert!(info_plist_text.contains("TestEmbeddedSans.ttf"));

    // 2. Validate both project.pbxproj and Info.plist via /usr/bin/plutil -lint
    assert_plutil_lint(&report.pbxproj_path);
    assert_plutil_lint(&report.info_plist_path);

    // 3. Real Xcode 26.2 / Swift 6.2.3 build of initial export
    assert_xcodebuild_succeeds(
        &report.xcodeproj_path,
        &report.scheme_name,
        &derived_data_dir,
    );

    // 4. Modify App/ContentView.swift and App/AppSwiftHost.swift with custom developer code,
    //    add a NEW proxy method `openPaywall(tier: String)` to `XcodeScriptBundle`, and re-export
    //    with `overwrite_app_shell: false`.
    let content_view_path = out_dir.join("App/ContentView.swift");
    let custom_content_view = r#"import SwiftUI

// CUSTOM_DEVELOPER_VIEW_MARKER_T455
struct ContentView: View {
    @State private var scene: MainScene
    @State private var host: AppSwiftHost
    @State private var nativeTapCount: Int = 0

    init() {
        let initialHost = AppSwiftHost()
        let initialScene = MainScene()
        initialScene.host = initialHost
        _host = State(initialValue: initialHost)
        _scene = State(initialValue: initialScene)
    }

    var body: some View {
        ZStack {
            Button("Underneath Native Button \(nativeTapCount)") {
                nativeTapCount += 1
            }
            NeoFlashStageView(scene: scene)
                .ignoresSafeArea()
        }
    }
}
"#;
    fs::write(&content_view_path, custom_content_view).expect("write custom ContentView.swift");

    let app_host_path = out_dir.join("App/AppSwiftHost.swift");
    let custom_app_host = r#"import Foundation

// CUSTOM_DEVELOPER_HOST_MARKER_T455
// Note: commenting out `func openPaywall(tier: String)` must not block stub insertion!
@MainActor
final class AppSwiftHost: SwiftHostProtocol {
    var trackedEvents: [String] = []
    let docString = "func openPaywall(tier: String)"

    init() {}

    func trackEvent(name: String) {
        trackedEvents.append("custom:\(name)")
    }

    func currentTier() -> String {
        return "pro_annual"
    }
}
"#;
    fs::write(&app_host_path, custom_app_host).expect("write custom AppSwiftHost.swift");

    // Add NEW proxy method `openPaywall(tier: String)` and call it from frame 6 script!
    scripts.host_methods.push(XcodeSwiftProxyMethod {
        name: "openPaywall".into(),
        params: vec![XcodeSwiftProxyParam {
            name: "tier".into(),
            swift_type: "String".into(),
        }],
        return_swift_type: None,
    });
    scripts.symbols[0].frame_methods_swift.insert(
        6,
        "let tier = self.host?.currentTier() ?? \"free\"\nself.host?.openPaywall(tier: tier)"
            .into(),
    );

    let reexport_report = export_xcode_project(&project, &out_dir, &opts, Some(&scripts))
        .expect("non-destructive re-export");

    assert_eq!(reexport_report.preserved_app_files.len(), 3);
    assert_eq!(reexport_report.appended_host_methods, vec!["openPaywall"]);

    let preserved_cv = fs::read_to_string(&content_view_path).unwrap();
    assert_eq!(
        preserved_cv, custom_content_view,
        "App/ContentView.swift must remain 100% untouched on re-export!"
    );

    let updated_host = fs::read_to_string(&app_host_path).unwrap();
    assert!(
        updated_host.contains("CUSTOM_DEVELOPER_HOST_MARKER_T455"),
        "Developer marker in AppSwiftHost.swift must be preserved!"
    );
    assert!(
        updated_host.contains("trackedEvents.append(\"custom:\\(name)\")"),
        "Developer implementation of trackEvent must be 100% preserved!"
    );
    assert!(
        updated_host.contains("return \"pro_annual\""),
        "Developer implementation of currentTier must be 100% preserved!"
    );
    assert!(
        updated_host.contains("func openPaywall(tier: String)"),
        "Newly added SwiftHostProtocol method openPaywall(tier: String) stub must be appended!"
    );

    assert_plutil_lint(&reexport_report.pbxproj_path);
    assert_plutil_lint(&reexport_report.info_plist_path);

    // Re-run real xcodebuild after non-destructive re-export!
    assert_xcodebuild_succeeds(
        &reexport_report.xcodeproj_path,
        &reexport_report.scheme_name,
        &derived_data_dir,
    );

    // 5. Compile & execute a Swift 6 runtime verification binary against the generated Swift files
    //    to prove runtime semantics (`isScriptControlled`, Liquid Stage `onResize`, ProMotion 120Hz
    //    fixed-FPS accumulator, `.evenOdd` compound hole hit-testing pass-through, and CGContext rendering)!
    let verifier_swift_path = tmp.path().join("RuntimeVerifyMain.swift");
    let verifier_bin_path = tmp.path().join("RuntimeVerifyBin");
    let verifier_swift = r#"import Foundation
import CoreGraphics

@main
@MainActor
struct RuntimeVerifyMain {
    static func main() {
        // 1. Verify NFValue dynamic member lookup, subscripts, truthiness & math
        var bag: NFValue = ["count": 2, "tag": "hero"]
        bag.count += 3
        assert(bag.count == 5, "Expected bag.count == 5")
        assert((bag.tag + "_1") == "hero_1", "Expected string concat")
        var arr: NFValue = [10, 20]
        arr[2] = 30
        assert(arr["length"] == 3 && arr[2] == 30, "Expected dynamic array growth")

        // 2. Mount MainScene + AppSwiftHost onto NFStage
        let host = AppSwiftHost()
        let scene = MainScene()
        scene.host = host
        let stage = NFStage(scene: scene)
        assert(host.trackedEvents == ["custom:scene_boot"], "Expected frame 0 host call, got \(host.trackedEvents)")
        assert(stage.scaleMode == .noScale && stage.align == .topLeft, "Expected frame 0 liquid stage setup")

        // 3. Trigger Liquid Stage resize -> verifies `onResize()` mutates `heroBadge.x` and sets `isScriptControlled`!
        let initialRunnerX = scene.tweenRunner.x
        stage.updateViewport(width: 1280, height: 720, safeTop: 44, safeLeft: 0, safeBottom: 34, safeRight: 20)
        assert(scene.resizeCount >= 1, "Expected onResize to fire")
        assert(abs(scene.heroBadge.x - 1200.0) < 1e-6, "Expected heroBadge.x == 1200.0, got \(scene.heroBadge.x)")
        assert(scene.heroBadge.isScriptControlled == true, "Script mutation MUST set isScriptControlled = true")
        assert(scene.tweenRunner.isScriptControlled == false, "Untouched tween instance must stay timeline-controlled")

        // 4. Verify 120Hz ProMotion fixed-FPS accumulator: 5 ticks of 1/120s at 24fps == exactly 1 timeline frame!
        var totalSteps = 0
        for _ in 0..<5 {
            totalSteps += stage.advance(deltaSeconds: 1.0 / 120.0)
        }
        assert(totalSteps == 1, "5 ticks at 120Hz on a 24fps stage must advance 1 frame, got \(totalSteps)")
        assert(scene.currentFrame == 2, "Expected currentFrame == 2, got \(scene.currentFrame)")

        // Step 4 more frames (total frame 6) and prove `heroBadge.x` was NOT overwritten by the Classic tween,
        // while `tweenRunner.x` DID advance along the Classic tween!
        for _ in 0..<4 {
            _ = stage.advance(deltaSeconds: 1.0 / 24.0)
        }
        assert(scene.currentFrame == 6, "Expected currentFrame == 6")
        assert(abs(scene.heroBadge.x - 1200.0) < 1e-6, "Timeline MUST NOT overwrite script-controlled heroBadge.x!")
        assert(scene.tweenRunner.x > initialRunnerX + 50.0, "Non-script-controlled tweenRunner.x MUST interpolate!")

        // 5. Verify mathematical `.evenOdd` compound hole hit-testing & transparent pass-through!
        // Empty top-right region -> nil (passes through to native SwiftUI view underneath)
        assert(stage.hitTestViewPoint(CGPoint(x: 900, y: 20)) == nil, "Empty stage region must pass through")
        // Center of compound donut hole (local (50, 50) inside 30..70 hole, translated by (200, 150) w/ skew 8 deg):
        let holeStagePt = scene.donutRing.localToGlobal(CGPoint(x: 50, y: 50))
        assert(stage.hitTestViewPoint(holeStagePt) == nil, "EvenOdd hole interior MUST pass through!")
        let ringStagePt = scene.donutRing.localToGlobal(CGPoint(x: 15, y: 15))
        assert(stage.hitTestViewPoint(ringStagePt) === scene.donutRing, "Painted donut ring MUST hit donutRing!")

        // 6. Click `playBtn` -> jumps to `"gameplay"` (frame 7, 0-based index 6) and invokes newly appended `openPaywall`!
        let btnCenter = scene.playBtn.localToGlobal(CGPoint(x: 60, y: 22))
        stage.handlePointerDown(viewPoint: btnCenter)
        stage.handlePointerUp(viewPoint: btnCenter)
        assert(scene.currentFrame == 7, "Clicking playBtn should gotoAndStop('gameplay') at frame 7, got \(scene.currentFrame)")
        assert(scene.dynamicState.score == 10, "Click handler should increment dynamicState.score")

        // 7. Render into a real RGBA8 CoreGraphics bitmap context to prove zero-crash vector/gradient/text rendering!
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        guard let ctx = CGContext(
            data: nil,
            width: 400,
            height: 300,
            bitsPerComponent: 8,
            bytesPerRow: 400 * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            fatalError("Failed to allocate CGContext")
        }
        // Flip top-left Y-down to match NeoFlashPlatformView
        ctx.translateBy(x: 0, y: 300)
        ctx.scaleBy(x: 1, y: -1)
        NFRenderer.render(stage: stage, in: ctx, viewSize: CGSize(width: 400, height: 300), drawsBackground: true)
        print("RUNTIME_VERIFY_OK")
    }
}
"#;
    fs::write(&verifier_swift_path, verifier_swift).expect("write RuntimeVerifyMain.swift");

    let swiftc_out = Command::new("/usr/bin/swiftc")
        .arg("-swift-version")
        .arg("6")
        .arg("-strict-concurrency=complete")
        .arg("-warnings-as-errors")
        .arg(out_dir.join("Generated/NeoFlashKit/NFValue.swift"))
        .arg(out_dir.join("Generated/NeoFlashKit/NFDisplayList.swift"))
        .arg(out_dir.join("Generated/NeoFlashKit/NFRenderer.swift"))
        .arg(out_dir.join("Generated/NeoFlashKit/NFAudio.swift"))
        .arg(out_dir.join("Generated/Scenes/SwiftHostContract.swift"))
        .arg(out_dir.join("Generated/Scenes/GeneratedScenes.swift"))
        .arg(&app_host_path)
        .arg(&verifier_swift_path)
        .arg("-o")
        .arg(&verifier_bin_path)
        .output()
        .expect("compile Swift 6 runtime verifier");
    assert!(
        swiftc_out.status.success(),
        "swiftc runtime verifier failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&swiftc_out.stdout),
        String::from_utf8_lossy(&swiftc_out.stderr)
    );

    let run_out = Command::new(&verifier_bin_path)
        .output()
        .expect("run Swift 6 runtime verifier");
    assert!(
        run_out.status.success()
            && String::from_utf8_lossy(&run_out.stdout).contains("RUNTIME_VERIFY_OK"),
        "RuntimeVerifyBin failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_out.stdout),
        String::from_utf8_lossy(&run_out.stderr)
    );

    // 6. Also verify `#elseif os(iOS)` UIKit branch compiles cleanly with zero warnings under Swift 6!
    if let Ok(sdk_out) = Command::new("/usr/bin/xcrun")
        .args(["--sdk", "iphonesimulator", "--show-sdk-path"])
        .output()
        && sdk_out.status.success()
    {
        let ios_sdk = String::from_utf8_lossy(&sdk_out.stdout).trim().to_owned();
        if !ios_sdk.is_empty() {
            let ios_check = Command::new("/usr/bin/swiftc")
                .arg("-typecheck")
                .arg("-sdk")
                .arg(&ios_sdk)
                .arg("-target")
                .arg("arm64-apple-ios17.0-simulator")
                .arg("-swift-version")
                .arg("6")
                .arg("-strict-concurrency=complete")
                .arg("-warnings-as-errors")
                .arg(out_dir.join("Generated/NeoFlashKit/NFValue.swift"))
                .arg(out_dir.join("Generated/NeoFlashKit/NFDisplayList.swift"))
                .arg(out_dir.join("Generated/NeoFlashKit/NFRenderer.swift"))
                .arg(out_dir.join("Generated/NeoFlashKit/NFAudio.swift"))
                .arg(out_dir.join("Generated/Scenes/SwiftHostContract.swift"))
                .arg(out_dir.join("Generated/Scenes/GeneratedScenes.swift"))
                .arg(out_dir.join("App/LiquidArcadeDemoApp.swift"))
                .arg(&content_view_path)
                .arg(&app_host_path)
                .output()
                .expect("swiftc iOS simulator typecheck");
            assert!(
                ios_check.status.success(),
                "iOS UIKit Swift 6 typecheck failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&ios_check.stdout),
                String::from_utf8_lossy(&ios_check.stderr)
            );
        }
    }
}

#[test]
fn multi_scene_flattening_and_motion_guide_orient_to_path_parity() {
    let tmp = TempDir::new().expect("tempdir");
    let out_dir = tmp.path().join("GuidedMultiScene");

    let rocket_sym = Symbol {
        id: "sym-rocket".into(),
        name: "Rocket".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-r".into(),
                name: "Hull".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-r".into(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 24.0,
                            height: 10.0,
                            fill: [255, 90, 50, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };

    let guide_layer = Layer {
        id: "lyr-guide".into(),
        name: "ArcGuide".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: neoflash_core::LayerRole::Guide,
            ..LayerProperties::default()
        },
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "el-guide-curve".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 20.0, y: 200.0 },
                        PathCommand::QuadraticTo {
                            cx: 200.0,
                            cy: 20.0,
                            x: 380.0,
                            y: 200.0,
                        },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: [100, 200, 255, 255],
                    stroke_width: 1.0,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        }],
    };

    let guided_layer = Layer {
        id: "lyr-flight".into(),
        name: "Flight".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            motion_guide: Some("lyr-guide".into()),
            ..LayerProperties::default()
        },
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Classic,
                orient_to_path: true,
                snap_to_guide: true,
                elements: vec![Element {
                    id: "el-ship".into(),
                    name: "ship".into(),
                    transform: Transform {
                        x: 20.0,
                        y: 200.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-rocket".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 8,
                elements: vec![Element {
                    id: "el-ship".into(),
                    name: "ship".into(),
                    transform: Transform {
                        x: 380.0,
                        y: 200.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-rocket".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
        ],
    };

    let scene1 = Symbol {
        id: "sym-scene-1".into(),
        name: "IntroScene".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 9,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![guide_layer, guided_layer],
        },
    };

    let scene2 = Symbol {
        id: "sym-scene-2".into(),
        name: "OutroScene".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 5,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-outro".into(),
                name: "Outro".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "credits".into(),
                    elements: vec![Element {
                        id: "el-outro-box".into(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 100.0,
                            height: 40.0,
                            fill: [40, 200, 120, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };

    let project = Project {
        format_version: 14,
        name: "Guided Multi Scene".into(),
        width: 400,
        height: 300,
        fps: 24,
        background: [0, 0, 0, 255],
        root: "sym-scene-1".into(),
        symbols: vec![scene1, scene2, rocket_sym],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: vec!["sym-scene-1".into(), "sym-scene-2".into()],
        script_language: None,
        also_compile_compatible_scripts: true,
    };

    let report = export_xcode_project(&project, &out_dir, &XcodeExportOptions::default(), None)
        .expect("export multi-scene guided project");
    // Secondary scene (`sym-scene-2`) is flattened into `MainScene`, leaving `MainScene` + `SymRocket` (2 symbols).
    assert_eq!(report.symbol_count, 2);
    assert_plutil_lint(&report.pbxproj_path);

    let generated_scenes =
        fs::read_to_string(out_dir.join("Generated/Scenes/GeneratedScenes.swift")).unwrap();
    assert!(generated_scenes.contains("totalFrames: 14"));
    assert!(
        generated_scenes
            .contains("NFSceneRange(name: \"IntroScene\", startFrame: 0, frameCount: 9)")
    );
    assert!(
        generated_scenes
            .contains("NFSceneRange(name: \"OutroScene\", startFrame: 9, frameCount: 5)")
    );
    assert!(generated_scenes.contains("\"credits\": 10"));
    assert!(
        !generated_scenes.contains("ArcGuide"),
        "Guide layers must never be published artwork!"
    );
}

#[test]
fn end_to_end_ast_to_xcodebuild_for_starter_scene_and_tetris_as2_as3() {
    let tmp = TempDir::new().expect("tempdir");

    // 1. Starter `new_xcode_swift_scene` (AS3 with SwiftHost.as + liquid onResize)
    let starter =
        Project::new_xcode_swift_scene(1280, 800, 60, neoflash_core::ScriptLanguage::ActionScript3);
    let starter_dir = tmp.path().join("StarterAS3");
    let starter_report =
        export_xcode_project(&starter, &starter_dir, &XcodeExportOptions::default(), None)
            .expect("export starter AS3 Xcode scene");
    let contract_src =
        fs::read_to_string(starter_dir.join("Generated/Scenes/SwiftHostContract.swift")).unwrap();
    assert!(
        contract_src.contains("func notifySceneReady(width: Double, height: Double)"),
        "SwiftHost.as contract must extract notifySceneReady: {contract_src}"
    );
    assert!(
        contract_src.contains("func hapticImpact(style: String)"),
        "SwiftHost.as contract must extract hapticImpact: {contract_src}"
    );
    assert_xcodebuild_succeeds(
        &starter_report.xcodeproj_path,
        &starter_report.scheme_name,
        &tmp.path().join("dd-starter"),
    );

    // 2. Real `examples/tetris-as2.nflash` -> AST -> Swift 6 -> xcodebuild!
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tetris_as2_json =
        fs::read_to_string(repo_root.join("examples/tetris-as2.nflash")).expect("read tetris-as2");
    let tetris_as2 = Project::from_json(&tetris_as2_json).expect("parse tetris-as2");
    let as2_dir = tmp.path().join("TetrisAS2");
    let as2_report =
        export_xcode_project(&tetris_as2, &as2_dir, &XcodeExportOptions::default(), None)
            .expect("export tetris-as2 to Xcode");
    assert_xcodebuild_succeeds(
        &as2_report.xcodeproj_path,
        &as2_report.scheme_name,
        &tmp.path().join("dd-as2"),
    );

    // 3. Real `examples/tetris-as3.nflash` -> AST -> Swift 6 -> xcodebuild!
    let tetris_as3_json =
        fs::read_to_string(repo_root.join("examples/tetris-as3.nflash")).expect("read tetris-as3");
    let tetris_as3 = Project::from_json(&tetris_as3_json).expect("parse tetris-as3");
    let as3_dir = tmp.path().join("TetrisAS3");
    let as3_report =
        export_xcode_project(&tetris_as3, &as3_dir, &XcodeExportOptions::default(), None)
            .expect("export tetris-as3 to Xcode");
    assert_xcodebuild_succeeds(
        &as3_report.xcodeproj_path,
        &as3_report.scheme_name,
        &tmp.path().join("dd-as3"),
    );
}
