//! `[T-472]` Phase 1 Track E2 integration suite for the Experimental Xcode / Swift 6 (`@MainActor`)
//! exporter & `NeoFlashKit` runtime across macOS 15+ (`macosx`) and iOS 17+ (`iphonesimulator`).

use neoflash_core::{
    AuthoredTextFieldOptions, BlendMode, ColorTransform, Element, ElementKind, FillRule,
    FilterSpec, FrameScript, FrameSound, InstancePlayback, Keyframe, Layer, LayerProperties,
    LayerRole, ObjectEvent, ObjectScript, PathCommand, Project, ResourceUnit, Script,
    ScriptLanguage, SoundSync, SourceUnit, Symbol, SymbolKind, TextAlign, TextFieldType,
    TextFormat, Timeline, Transform,
};
use neoflash_export::{
    XcodeExportOptions, XcodePreflightSeverity, export_xcode_project, preflight_xcode_project,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Builds a valid 44.1 kHz mono 16-bit PCM WAV buffer with a non-silent sine-like tone (`num_samples` frames).
fn make_pcm_wav_bytes(num_samples: u32, freq_step: i16) -> Vec<u8> {
    let sample_rate: u32 = 44_100;
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
    for i in 0..num_samples {
        let sample = ((i as i32 * i32::from(freq_step)) % 16_000 - 8_000) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn should_skip_xcodebuild() -> bool {
    if std::env::var("NEOFLASH_SKIP_XCODEBUILD")
        .ok()
        .is_some_and(|v| !v.trim().is_empty() && v.trim() != "0")
    {
        return true;
    }
    !Command::new("/usr/bin/xcodebuild")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn sdk_path(sdk_name: &str) -> Option<String> {
    let out = Command::new("/usr/bin/xcrun")
        .args(["--sdk", sdk_name, "--show-sdk-path"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let trimmed = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn assert_plutil_lint(path: &Path) {
    let out = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(path)
        .output()
        .expect("invoke /usr/bin/plutil");
    assert!(
        out.status.success(),
        "plutil -lint failed for {}:\nstdout: {}\nstderr: {}",
        path.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[allow(clippy::too_many_lines)]
fn build_t472_deep_as3_project() -> Project {
    let chime_wav = make_pcm_wav_bytes(8_820, 310); // 200ms @ 44.1kHz
    let laser_wav = make_pcm_wav_bytes(6_615, 520); // 150ms @ 44.1kHz
    let chime_res =
        ResourceUnit::from_bytes("res-chime", "audio/chime.wav", "audio/wav", &chime_wav)
            .expect("chime wav resource");
    let laser_res =
        ResourceUnit::from_bytes("res-laser", "audio/laser.wav", "audio/wav", &laser_wav)
            .expect("laser wav resource");

    // 1. Nested Leaf MovieClip (`SymNestedCounter`, 4 frames, independent clock + frame script + label)
    let nested_counter = Symbol {
        id: "sym-nested-counter".into(),
        name: "NestedCounter".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: Some("NestedCounterLink".into()),
        timeline: Timeline {
            frame_count: 4,
            guides: Vec::new(),
            scripts: vec![
                FrameScript {
                    layer: None,
                    frame: 0,
                    script: Script {
                        language: ScriptLanguage::ActionScript3,
                        source: "var visitCount:int = visitCount + 1;".into(),
                    },
                },
                FrameScript {
                    layer: None,
                    frame: 2,
                    script: Script {
                        language: ScriptLanguage::ActionScript3,
                        source: "visitCount = visitCount + 10;".into(),
                    },
                },
            ],
            layers: vec![Layer {
                id: "lyr-nc".into(),
                name: "CounterArt".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        label: "boot".into(),
                        elements: vec![Element {
                            id: "el-nc-rect".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: [200, 200, 200, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 2,
                        label: "boosted".into(),
                        elements: vec![Element {
                            id: "el-nc-rect".into(),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: [255, 240, 120, 255],
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

    // 2. Parent Container MovieClip (`SymParentPod`, 3 frames) holding `nestedCounter` with affine + ColorTransform
    let parent_pod = Symbol {
        id: "sym-parent-pod".into(),
        name: "ParentPod".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 3,
            guides: Vec::new(),
            scripts: vec![FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "stop();".into(),
                },
            }],
            layers: vec![Layer {
                id: "lyr-pod".into(),
                name: "PodChild".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-nested-mc".into(),
                        name: "nestedCounter".into(),
                        transform: Transform {
                            x: 20.0,
                            y: 10.0,
                            scale_x: 1.5,
                            scale_y: 0.5,
                            color_transform: ColorTransform {
                                multipliers: [0.5, 1.0, 0.25, 0.8],
                                offsets: [40.0, 0.0, 10.0, 0.0],
                            },
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: "sym-nested-counter".into(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }],
        },
    };

    // 3. Synchronized Graphic Symbol (`SymSyncMeter`, 4 frames)
    let make_meter_key = |f: u32, w: f64| Keyframe {
        frame: f,
        elements: vec![Element {
            id: format!("el-meter-{f}"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: w,
                height: 12.0,
                fill: [60, 220, 180, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        }],
        ..Keyframe::default()
    };
    let sync_meter = Symbol {
        id: "sym-sync-meter".into(),
        name: "SyncMeter".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-meter".into(),
                name: "Bars".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    make_meter_key(0, 10.0),
                    make_meter_key(1, 20.0),
                    make_meter_key(2, 30.0),
                    make_meter_key(3, 40.0),
                ],
            }],
        },
    };

    // 4. Interactive 4-Frame SimpleButton (`SymActionButton`: Up, Over, Down, Hit)
    let make_btn_key = |f: u32, fill: [u8; 4], w: f64, h: f64| Keyframe {
        frame: f,
        elements: vec![Element {
            id: format!("el-act-btn-{f}"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: w,
                height: h,
                fill,
                stroke: [255, 255, 255, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        }],
        ..Keyframe::default()
    };
    let action_btn = Symbol {
        id: "sym-action-btn".into(),
        name: "ActionButton".into(),
        folder: String::new(),
        kind: SymbolKind::Button,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-act-btn".into(),
                name: "States".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    make_btn_key(0, [40, 110, 220, 255], 120.0, 44.0),
                    make_btn_key(1, [70, 150, 255, 255], 120.0, 44.0),
                    make_btn_key(2, [20, 80, 170, 255], 120.0, 44.0),
                    // Dedicated Hit state (frame 4 / index 3) slightly wider (140x50)
                    make_btn_key(3, [0, 0, 0, 255], 140.0, 50.0),
                ],
            }],
        },
    };

    // 5. Masked Target MovieClip (`SymMaskedBanner`)
    let masked_banner = Symbol {
        id: "sym-masked-banner".into(),
        name: "MaskedBanner".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 1,
            scripts: Vec::new(),
            guides: Vec::new(),
            layers: vec![Layer {
                id: "lyr-mb".into(),
                name: "Fill".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-mb-rect".into(),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 200.0,
                            height: 200.0,
                            fill: [240, 30, 40, 255],
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

    // Dynamic & Input TextFormat configurations
    let mut dynamic_fmt = TextFormat {
        font: "_sans".into(),
        size: 18.0,
        color: 0xEEFFAA,
        bold: true,
        align: TextAlign::Left,
        ..TextFormat::default()
    };
    dynamic_fmt.set_field_options(Some(AuthoredTextFieldOptions {
        field_type: TextFieldType::Dynamic,
        selectable: false,
        border: true,
        border_color: 0x335577,
        background: true,
        background_color: 0x101824,
        ..AuthoredTextFieldOptions::default()
    }));

    let mut input_fmt = TextFormat {
        font: "_sans".into(),
        size: 16.0,
        color: 0x111111,
        align: TextAlign::Left,
        ..TextFormat::default()
    };
    input_fmt.set_field_options(Some(AuthoredTextFieldOptions {
        field_type: TextFieldType::Input,
        selectable: true,
        max_chars: 12,
        restrict: Some("A-Za-z0-9_".into()),
        border: true,
        border_color: 0x2266CC,
        background: true,
        background_color: 0xFFFFFF,
        ..AuthoredTextFieldOptions::default()
    }));

    // Root Scene (`MainScene`, 6 frames):
    // - Layer 0 (`BackdropBlend`): Screen + DropShadow + Blur filter cards
    // - Layer 1 (`MaskClip`, LayerRole::Mask): EvenOdd donut mask on frames 0..5, EMPTY blank keyframe on frame 5!
    // - Layer 2 (`MaskedContent`, masked by `lyr-mask`): `maskedBanner` covering (80..280, 80..280)
    // - Layer 3 (`InteractiveAndNested`): `parentPod`, `gfxLoop`, `gfxOnce`, `gfxSingle`, `statusField`, `playerInput`, `actionBtn`, plus FrameSound @ 0
    let blend_filter_layer = Layer {
        id: "lyr-filters-blends".into(),
        name: "FiltersAndBlends".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![
                // Light backdrop mat at (300, 68, 140x132) so black DropShadow contrast & direction are unmistakable
                Element {
                    id: "el-shadow-mat".into(),
                    name: String::new(),
                    transform: Transform {
                        x: 300.0,
                        y: 68.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 140.0,
                        height: 132.0,
                        fill: [200, 210, 225, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
                // Base dark-magenta rectangle at (310, 80, 80x80) casting a 90-deg downward DropShadow
                Element {
                    id: "el-blend-base".into(),
                    name: String::new(),
                    transform: Transform {
                        x: 310.0,
                        y: 80.0,
                        ..Transform::default()
                    },
                    filters: vec![FilterSpec::DropShadow {
                        distance: 10.0,
                        angle: 90.0,
                        color: 0x000000,
                        alpha: 0.85,
                        blur_x: 6.0,
                        blur_y: 6.0,
                        strength: 1.0,
                        quality: 2,
                        inner: false,
                        knockout: false,
                        hide_object: false,
                    }],
                    kind: ElementKind::Rectangle {
                        width: 80.0,
                        height: 80.0,
                        fill: [160, 40, 120, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
                // Overlapping Screen-blended rectangle at (345, 100, 80x80)
                Element {
                    id: "el-blend-screen".into(),
                    name: "screenCard".into(),
                    transform: Transform {
                        x: 345.0,
                        y: 100.0,
                        ..Transform::default()
                    },
                    blend_mode: BlendMode::Screen,
                    kind: ElementKind::Rectangle {
                        width: 80.0,
                        height: 80.0,
                        fill: [120, 150, 60, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
                // Blurred bright cyan square at (470, 90, 50x50)
                Element {
                    id: "el-blur-tile".into(),
                    name: "blurTile".into(),
                    transform: Transform {
                        x: 470.0,
                        y: 90.0,
                        ..Transform::default()
                    },
                    filters: vec![FilterSpec::Blur {
                        blur_x: 10.0,
                        blur_y: 10.0,
                        quality: 2,
                    }],
                    kind: ElementKind::Rectangle {
                        width: 50.0,
                        height: 50.0,
                        fill: [0, 240, 255, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                },
            ],
            ..Keyframe::default()
        }],
    };

    let mask_layer = Layer {
        id: "lyr-mask".into(),
        name: "DonutMask".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: LayerRole::Mask,
            ..LayerProperties::default()
        },
        keyframes: vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "el-donut-mask".into(),
                    name: String::new(),
                    transform: Transform {
                        x: 100.0,
                        y: 100.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Path {
                        commands: vec![
                            // Outer mask square: Stage (100..260, 100..260)
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::LineTo { x: 160.0, y: 0.0 },
                            PathCommand::LineTo { x: 160.0, y: 160.0 },
                            PathCommand::LineTo { x: 0.0, y: 160.0 },
                            PathCommand::Close,
                            // Inner EvenOdd hole: Stage (150..210, 150..210)
                            PathCommand::MoveTo { x: 50.0, y: 50.0 },
                            PathCommand::LineTo { x: 110.0, y: 50.0 },
                            PathCommand::LineTo { x: 110.0, y: 110.0 },
                            PathCommand::LineTo { x: 50.0, y: 110.0 },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::EvenOdd,
                        fill: [0, 0, 0, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            // Blank mask keyframe on frame 5 (1-based frame 6) -> must clip out 100% of masked layer!
            Keyframe {
                frame: 5,
                elements: Vec::new(),
                ..Keyframe::default()
            },
        ],
    };

    let masked_content_layer = Layer {
        id: "lyr-masked-content".into(),
        name: "MaskedContent".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            mask: Some("lyr-mask".into()),
            ..LayerProperties::default()
        },
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "el-masked-banner".into(),
                name: "maskedBanner".into(),
                transform: Transform {
                    x: 80.0,
                    y: 80.0,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: "sym-masked-banner".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        }],
    };

    let interactive_layer = Layer {
        id: "lyr-interactive".into(),
        name: "UIAndNested".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            sound: Some(FrameSound {
                resource: "res-chime".into(),
                sync: SoundSync::Event,
                loops: 1,
                loop_forever: false,
                effect: neoflash_core::SoundEffect::None,
                envelope: None,
                in_point: None,
                out_point: None,
            }),
            elements: vec![
                Element {
                    id: "el-parent-pod".into(),
                    name: "parentPod".into(),
                    transform: Transform {
                        x: 560.0,
                        y: 80.0,
                        scale_x: 2.0,
                        scale_y: 2.0,
                        color_transform: ColorTransform {
                            multipliers: [1.0, 0.5, 1.0, 0.9],
                            offsets: [10.0, 20.0, 0.0, 0.0],
                        },
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-parent-pod".into(),
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-gfx-loop".into(),
                    name: "gfxLoop".into(),
                    transform: Transform {
                        x: 560.0,
                        y: 220.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-sync-meter".into(),
                        playback: InstancePlayback::Loop,
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-gfx-once".into(),
                    name: "gfxOnce".into(),
                    transform: Transform {
                        x: 560.0,
                        y: 250.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-sync-meter".into(),
                        playback: InstancePlayback::PlayOnce,
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-gfx-single".into(),
                    name: "gfxSingle".into(),
                    transform: Transform {
                        x: 560.0,
                        y: 280.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-sync-meter".into(),
                        playback: InstancePlayback::SingleFrame(2),
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-status-tf".into(),
                    name: "statusField".into(),
                    transform: Transform {
                        x: 60.0,
                        y: 340.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::TextBox {
                        text: "Booting".into(),
                        width: 320.0,
                        height: 44.0,
                        format: dynamic_fmt,
                        word_wrap: true,
                        multiline: true,
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-input-tf".into(),
                    name: "playerInput".into(),
                    transform: Transform {
                        x: 60.0,
                        y: 410.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::TextBox {
                        text: "Guest".into(),
                        width: 240.0,
                        height: 36.0,
                        format: input_fmt,
                        word_wrap: false,
                        multiline: false,
                    },
                    ..Element::default()
                },
                Element {
                    id: "el-action-btn".into(),
                    name: "actionBtn".into(),
                    transform: Transform {
                        x: 60.0,
                        y: 480.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: "sym-action-btn".into(),
                        playback: InstancePlayback::Independent,
                    },
                    scripts: vec![ObjectScript {
                        event: ObjectEvent::Click,
                        script: Script {
                            language: ScriptLanguage::ActionScript2,
                            source: r#"
on (press) {
    pressCount = pressCount + 1;
    SwiftHost.notifyAction("btn_press");
}
on (release) {
    releaseCount = releaseCount + 1;
    fscommand("pulseHaptic", "medium");
}
"#
                            .into(),
                        },
                    }],
                    ..Element::default()
                },
            ],
            ..Keyframe::default()
        }],
    };

    let root_frame0_script = r#"
import flash.events.Event;
import flash.events.MouseEvent;
import flash.events.TouchEvent;

var engine:ScoreEngine = new ScoreEngine(15);
var pressCount:int = 0;
var releaseCount:int = 0;
var clickEvents:int = 0;
var downEvents:int = 0;
var upEvents:int = 0;
var moveEvents:int = 0;
var tapEvents:int = 0;
var lastTapId:int = -1;
var inputChangeCount:int = 0;
var computedScore:Number = 0;
var caughtErrorMsg:String = "";

engine.bonus = 20;
var firstPts:Number = engine.record("boost", 40);
var secondPts:Number = engine.record("normal", 10);
computedScore = engine.sumSamples();
caughtErrorMsg = engine.tryInvalid(-5);

var hostTier:String = SwiftHost.reportScore(int(computedScore), engine.samples);
statusField.htmlText = "<b>Tier:</b> " + hostTier + " &amp; Score=" + String(int(computedScore)) + "<br/>Ready";
statusField.alpha = 0.85;

actionBtn.addEventListener(MouseEvent.CLICK, function(e:MouseEvent):void {
    clickEvents = clickEvents + 1;
});
stage.addEventListener(MouseEvent.MOUSE_DOWN, function(e:MouseEvent):void {
    downEvents = downEvents + 1;
});
stage.addEventListener(MouseEvent.MOUSE_UP, function(e:MouseEvent):void {
    upEvents = upEvents + 1;
});
stage.addEventListener(MouseEvent.MOUSE_MOVE, function(e:MouseEvent):void {
    moveEvents = moveEvents + 1;
});
stage.addEventListener(TouchEvent.TOUCH_TAP, function(e:TouchEvent):void {
    tapEvents = tapEvents + 1;
    lastTapId = e.touchPointID;
});
playerInput.addEventListener(Event.CHANGE, function(e:Event):void {
    inputChangeCount = inputChangeCount + 1;
    statusField.text = "Player:" + playerInput.text;
});
"#;

    let root_symbol = Symbol {
        id: "sym-root".into(),
        name: "Main".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            guides: Vec::new(),
            scripts: vec![FrameScript {
                layer: None,
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: root_frame0_script.into(),
                },
            }],
            layers: vec![
                blend_filter_layer,
                mask_layer,
                masked_content_layer,
                interactive_layer,
            ],
        },
    };

    let swift_host_as = SourceUnit {
        id: "src-swifthost".into(),
        path: "SwiftHost.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package {
    public class SwiftHost {
        public static function reportScore(score:int, samples:Vector.<Number>):String {
            return "mock";
        }
        public static function notifyAction(tag:String):void {}
    }
}
"#
        .into(),
    };

    let score_engine_as = SourceUnit {
        id: "src-score-engine".into(),
        path: "game/ScoreEngine.as".into(),
        language: ScriptLanguage::ActionScript3,
        source: r#"package game {
    public class ScoreEngine {
        public var multiplier:Number = 1.5;
        public var samples:Vector.<Number> = new Vector.<Number>();
        private var _bonus:int = 0;
        public var finallyCount:int = 0;

        public function ScoreEngine(initialBonus:int = 10) {
            this._bonus = initialBonus;
        }

        public function get bonus():int {
            return this._bonus;
        }

        public function set bonus(v:int):void {
            this._bonus = int(Math.max(0, v));
        }

        public function validateNonNegative(raw:Number):Number {
            if (raw < 0) {
                throw new Error("negative_points");
            }
            return raw;
        }

        public function record(mode:String, points:Number):Number {
            var factor:Number = 1.0;
            switch (mode) {
                case "super":
                case "boost":
                    factor = 2.0;
                    break;
                case "penalty":
                    factor = 0.5;
                    break;
                default:
                    factor = 1.0;
            }
            var trig:Number = Math.sin(0.0) + Math.cos(0.0) * (Math.SQRT2 / Math.SQRT2);
            var computed:Number = Math.round(points * this.multiplier * factor * trig) + this._bonus;
            this.samples.push(computed);
            return computed;
        }

        public function sumSamples():Number {
            var total:Number = 0.0;
            for each (var item:Number in this.samples) {
                total = total + item;
            }
            var weights:Object = { a: 2, b: 3 };
            var keyCount:int = 0;
            for (var k:String in weights) {
                if (k.length > 0) {
                    keyCount = keyCount + 1;
                }
            }
            return total + keyCount;
        }

        public function tryInvalid(badValue:Number):String {
            var captured:String = "none";
            try {
                var clean:Number = this.validateNonNegative(badValue);
                captured = String(clean);
            } catch (err:Error) {
                captured = err.message;
            } finally {
                this.finallyCount = this.finallyCount + 1;
            }
            return captured;
        }
    }
}
"#
        .into(),
    };

    Project {
        format_version: 14,
        name: "Deep Swift Showcase".into(),
        width: 800,
        height: 600,
        fps: 24,
        background: [16, 20, 28, 255],
        root: "sym-root".into(),
        symbols: vec![
            root_symbol,
            nested_counter,
            parent_pod,
            sync_meter,
            action_btn,
            masked_banner,
        ],
        library_folders: Vec::new(),
        sources: vec![swift_host_as, score_engine_as],
        resources: vec![chime_res, laser_res],
        document_class: None,
        scenes: Vec::new(),
        script_language: Some(ScriptLanguage::ActionScript3),
        also_compile_compatible_scripts: true,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn t472_deep_scene_nested_mc_mask_filters_blends_textfield_touch_audio_and_xcodebuild() {
    let tmp = TempDir::new().expect("tempdir");
    let out_dir = tmp.path().join("DeepSwiftShowcaseExport");
    let derived_mac = tmp.path().join("DerivedDataMac");
    let derived_ios = tmp.path().join("DerivedDataIOS");

    let mut project = build_t472_deep_as3_project();
    let preflight = preflight_xcode_project(&project);
    assert!(
        preflight
            .iter()
            .all(|item| item.severity == XcodePreflightSeverity::Info),
        "Expected zero portability warnings on clean AS3 showcase, got: {preflight:?}"
    );

    // 1. Initial Export (auto-compiling AS3 + AS2 scripts + custom ScoreEngine.as + SwiftHost.as)
    let opts = XcodeExportOptions::default();
    let report =
        export_xcode_project(&project, &out_dir, &opts, None).expect("initial T-472 Xcode export");
    assert_eq!(report.scheme_name, "DeepSwiftShowcase");
    assert_eq!(report.symbol_count, 6);
    assert!(report.preserved_app_files.is_empty());

    // Verify SwiftHostContract.swift contains explicit SwiftHost.as + ad-hoc fscommand("pulseHaptic", ...)
    let contract_src =
        fs::read_to_string(out_dir.join("Generated/Scenes/SwiftHostContract.swift")).unwrap();
    assert!(
        contract_src.contains("func reportScore(score: Int, samples: [Double]) -> String"),
        "Expected typed Vector.<Number> -> [Double] in SwiftHostContract.swift:\n{contract_src}"
    );
    assert!(
        contract_src.contains("func notifyAction(tag: String)"),
        "Expected notifyAction in SwiftHostContract.swift:\n{contract_src}"
    );
    assert!(
        contract_src.contains("func pulseHaptic("),
        "Expected ad-hoc fscommand pulseHaptic in SwiftHostContract.swift:\n{contract_src}"
    );

    // Verify ScoreEngine.as compiled into GeneratedScenes.swift with [Double] and paired bonus get/set
    let scenes_src =
        fs::read_to_string(out_dir.join("Generated/Scenes/GeneratedScenes.swift")).unwrap();
    assert!(
        scenes_src.contains("public class ScoreEngine"),
        "Expected compiled ScoreEngine class in GeneratedScenes.swift"
    );
    assert!(
        scenes_src.contains("public var samples: [Double]"),
        "Expected Vector.<Number> lowered to [Double] on ScoreEngine:\n{scenes_src}"
    );

    // 2. Validate generated project.pbxproj & Info.plist with /usr/bin/plutil -lint
    assert_plutil_lint(&report.pbxproj_path);
    assert_plutil_lint(&report.info_plist_path);

    // 3. Customize developer-owned App/AppSwiftHost.swift, add a new ad-hoc proxy call in ActionScript,
    //    and re-export (`overwrite_app_shell = false`) to verify strict Generated/ vs App/ contract!
    let app_host_path = out_dir.join("App/AppSwiftHost.swift");
    let custom_host_code = r#"import Foundation

// DEVELOPER_OWNED_HOST_IMPLEMENTATION_T472
@MainActor
final class AppSwiftHost: SwiftHostProtocol {
    var actions: [String] = []
    var haptics: [String] = []
    var reportedScores: [(Int, [Double])] = []

    init() {}

    func reportScore(score: Int, samples: [Double]) -> String {
        reportedScores.append((score, samples))
        return "platinum"
    }

    func notifyAction(tag: String) {
        actions.append(tag)
    }

    func pulseHaptic(arg0: String) {
        haptics.append(arg0)
    }
}
"#;
    fs::write(&app_host_path, custom_host_code).expect("write developer AppSwiftHost.swift");

    // Add a new ExternalInterface proxy call on frame 1 and re-export without overwriting App/!
    project.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 1,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "ExternalInterface.call(\"unlockBadge\", \"mask_master\", 2);".into(),
        },
    });
    let reexport = export_xcode_project(&project, &out_dir, &opts, None)
        .expect("incremental non-destructive re-export");
    assert_eq!(reexport.preserved_app_files.len(), 3);
    assert_eq!(reexport.appended_host_methods, vec!["unlockBadge"]);

    let spliced_host = fs::read_to_string(&app_host_path).unwrap();
    assert!(
        spliced_host.contains("DEVELOPER_OWNED_HOST_IMPLEMENTATION_T472")
            && spliced_host.contains("return \"platinum\""),
        "Developer AppSwiftHost.swift implementation MUST never be overwritten!"
    );
    assert!(
        spliced_host.contains("func unlockBadge("),
        "Newly added proxy stub unlockBadge MUST be spliced into AppSwiftHost.swift:\n{spliced_host}"
    );

    assert_plutil_lint(&reexport.pbxproj_path);
    assert_plutil_lint(&reexport.info_plist_path);

    if should_skip_xcodebuild() {
        return;
    }

    // 4. Real `/usr/bin/xcodebuild` compilation (`SWIFT_STRICT_CONCURRENCY=complete`) for macOS arm64!
    let mac_build = Command::new("/usr/bin/xcodebuild")
        .arg("-project")
        .arg(&reexport.xcodeproj_path)
        .arg("-scheme")
        .arg(&reexport.scheme_name)
        .arg("-configuration")
        .arg("Debug")
        .arg("-sdk")
        .arg("macosx")
        .arg("-arch")
        .arg("arm64")
        .arg("-derivedDataPath")
        .arg(&derived_mac)
        .arg("SWIFT_STRICT_CONCURRENCY=complete")
        .arg("CODE_SIGNING_ALLOWED=NO")
        .arg("build")
        .output()
        .expect("run macOS xcodebuild");
    assert!(
        mac_build.status.success(),
        "macOS xcodebuild (SWIFT_STRICT_CONCURRENCY=complete) failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&mac_build.stdout),
        String::from_utf8_lossy(&mac_build.stderr)
    );

    // 5. Real `/usr/bin/xcodebuild` compilation (`SWIFT_STRICT_CONCURRENCY=complete`) AND
    //    `swiftc -typecheck -warnings-as-errors` for iOS Simulator (`iphonesimulator`)!
    if let Some(ios_sdk) = sdk_path("iphonesimulator") {
        let ios_build = Command::new("/usr/bin/xcodebuild")
            .arg("-project")
            .arg(&reexport.xcodeproj_path)
            .arg("-scheme")
            .arg(&reexport.scheme_name)
            .arg("-configuration")
            .arg("Debug")
            .arg("-sdk")
            .arg("iphonesimulator")
            .arg("-destination")
            .arg("generic/platform=iOS Simulator")
            .arg("-derivedDataPath")
            .arg(&derived_ios)
            .arg("SWIFT_STRICT_CONCURRENCY=complete")
            .arg("CODE_SIGNING_ALLOWED=NO")
            .arg("build")
            .output()
            .expect("run iphonesimulator xcodebuild");
        assert!(
            ios_build.status.success(),
            "iOS Simulator xcodebuild (SWIFT_STRICT_CONCURRENCY=complete) failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ios_build.stdout),
            String::from_utf8_lossy(&ios_build.stderr)
        );

        let ios_typecheck = Command::new("/usr/bin/swiftc")
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
            .arg(out_dir.join("App/DeepSwiftShowcaseApp.swift"))
            .arg(out_dir.join("App/ContentView.swift"))
            .arg(&app_host_path)
            .output()
            .expect("run iphonesimulator swiftc -typecheck");
        assert!(
            ios_typecheck.status.success(),
            "iOS Simulator Swift 6 strict typecheck failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ios_typecheck.stdout),
            String::from_utf8_lossy(&ios_typecheck.stderr)
        );
    }

    // 6. Compile & execute live headless Swift 6 CLI runtime verification harness!
    let harness_swift_path = tmp.path().join("T472RuntimeHarness.swift");
    let harness_bin_path = tmp.path().join("T472RuntimeHarnessBin");
    let resources_dir_escaped = out_dir
        .join("Generated/Resources")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    let harness_swift = format!(
        r#"import Foundation
import CoreGraphics

@main
@MainActor
struct T472RuntimeHarness {{
    static func pixelRGBA(_ ptr: UnsafePointer<UInt8>, width: Int, x: Int, y: Int) -> (UInt8, UInt8, UInt8, UInt8) {{
        let idx = (y * width + x) * 4
        return (ptr[idx], ptr[idx + 1], ptr[idx + 2], ptr[idx + 3])
    }}

    static func renderSnapshot(_ stage: NFStage, width: Int = 800, height: Int = 600) -> [UInt8] {{
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        bytes.withUnsafeMutableBytes {{ rawBuf in
            guard let base = rawBuf.baseAddress,
                  let ctx = CGContext(
                      data: base,
                      width: width,
                      height: height,
                      bitsPerComponent: 8,
                      bytesPerRow: width * 4,
                      space: space,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                  )
            else {{
                fatalError("Failed to allocate CGContext")
            }}
            ctx.translateBy(x: 0, y: CGFloat(height))
            ctx.scaleBy(x: 1.0, y: -1.0)
            NFRenderer.render(stage: stage, in: ctx, viewSize: CGSize(width: width, height: height), drawsBackground: true)
        }}
        return bytes
    }}

    static func main() {{
        let resourcesURL = URL(fileURLWithPath: "{resources_dir_escaped}")
        NFResourceCache.shared.resourceDirectory = resourcesURL
        let audioMgr = NFAudioManager(resourceDirectory: resourcesURL)

        let host = AppSwiftHost()
        let scene = MainScene()
        scene.host = host
        let stage = NFStage(scene: scene, audioManager: audioMgr)

        // A. Verify compiled ScoreEngine.as (`Vector.<Number>`, switch fallthrough, for each, for..in, Math.*, try/catch/finally) + SwiftHost proxy
        // firstPts = round(40 * 1.5 * 2.0 * 1.0) + 20 = 140.0
        // secondPts = round(10 * 1.5 * 1.0 * 1.0) + 20 = 35.0
        // sumSamples = (140 + 35) + 2 keys = 177.0
        assert(abs(scene.computedScore - 177.0) < 1e-6, "Expected computedScore == 177.0, got \(scene.computedScore)")
        assert(scene.caughtErrorMsg == "negative_points", "Expected caughtErrorMsg == 'negative_points', got \(scene.caughtErrorMsg)")
        assert(scene.engine?.finallyCount == 1, "Expected finally block to increment finallyCount")
        assert(host.reportedScores.count == 1 && host.reportedScores[0].0 == 177 && host.reportedScores[0].1 == [140.0, 35.0],
               "Expected SwiftHost.reportScore(177, [140.0, 35.0]), got \(host.reportedScores)")

        // B. Verify Dynamic TextField htmlText plain-text strip fallback + alpha
        assert(scene.statusField.text == "Tier: platinum & Score=177\nReady",
               "Unexpected stripped htmlText: \(scene.statusField.text)")
        assert(abs(scene.statusField.alpha - 0.85) < 1e-6, "Expected statusField.alpha == 0.85")

        // C. Verify Nested MovieClip independent timeline clocks & recursive affine + ColorTransform composition
        // `parentPod` called `stop()` on frame 1, while `nestedCounter` inside it is playing independently!
        assert(scene.parentPod.currentFrame == 1 && scene.parentPod.isPlaying == false,
               "parentPod must be stopped on frame 1")
        assert(scene.parentPod.nestedCounter.currentFrame == 1 && scene.parentPod.nestedCounter.visitCount == 1,
               "nestedCounter frame 0 script must run on initial mount! got visitCount=\(scene.parentPod.nestedCounter.visitCount)")

        let worldM = scene.parentPod.nestedCounter.stageCGAffineTransform()
        // parentPod(x:560, y:80, sx:2, sy:2) * nestedCounter(x:20, y:10, sx:1.5, sy:0.5) => a=3.0, d=1.0, tx=600, ty=100
        assert(abs(Double(worldM.a) - 3.0) < 1e-5 && abs(Double(worldM.d) - 1.0) < 1e-5,
               "Expected composed scale (3.0, 1.0), got (\(worldM.a), \(worldM.d))")
        assert(abs(Double(worldM.tx) - 600.0) < 1e-5 && abs(Double(worldM.ty) - 100.0) < 1e-5,
               "Expected composed translation (600, 100), got (\(worldM.tx), \(worldM.ty))")

        let worldCT = scene.parentPod.nestedCounter.concatenatedColorTransform()
        // parent redMult 1.0 * child redMult 0.5 = 0.5; greenMult 0.5 * 1.0 = 0.5; alpha (0.9 * 0.8) = 0.72
        assert(abs(worldCT.redMultiplier - 0.5) < 1e-5 && abs(worldCT.greenMultiplier - 0.5) < 1e-5,
               "Unexpected composed ColorTransform multipliers: \(worldCT)")
        assert(abs(worldCT.alphaMultiplier - 0.72) < 1e-5,
               "Expected composed alphaMultiplier == 0.72, got \(worldCT.alphaMultiplier)")

        // Step 2 frames: MainScene -> frame 3; parentPod stays at frame 1; nestedCounter advances 1 -> 2 -> 3 ("boosted", +10)!
        stage.stepSingleFrame()
        stage.stepSingleFrame()
        assert(scene.currentFrame == 3, "MainScene should be at frame 3")
        assert(scene.parentPod.currentFrame == 1, "Stopped parentPod must remain on frame 1")
        assert(scene.parentPod.nestedCounter.currentFrame == 3,
               "Independent child nestedCounter must advance to frame 3, got \(scene.parentPod.nestedCounter.currentFrame)")
        assert(scene.parentPod.nestedCounter.currentLabel == "boosted" && scene.parentPod.nestedCounter.visitCount == 11,
               "nestedCounter frame 3 script/label must execute! label=\(String(describing: scene.parentPod.nestedCounter.currentLabel)) count=\(scene.parentPod.nestedCounter.visitCount)")

        // Test child MovieClip gotoAndStop / play / stop control from parent Swift code
        scene.parentPod.nestedCounter.gotoAndStop("boot")
        assert(scene.parentPod.nestedCounter.currentFrame == 1 && !scene.parentPod.nestedCounter.isPlaying,
               "gotoAndStop('boot') on nestedCounter failed")

        // D. Verify Synchronized Graphic playback (`Loop`, `PlayOnce`, `SingleFrame(2)`)
        // At MainScene frame 3 (0-based age 2): Loop -> frame 3, PlayOnce -> frame 3, SingleFrame(2) -> frame 3
        assert(scene.gfxLoop.currentFrame == 3 && scene.gfxOnce.currentFrame == 3 && scene.gfxSingle.currentFrame == 3,
               "Unexpected Graphic frames at age 2")
        // Step 2 more frames -> MainScene frame 5 (0-based age 4):
        // Loop (4 % 4 = 0 -> 1-based frame 1), PlayOnce (min(3, 4) = 3 -> 1-based frame 4), SingleFrame(2) -> 1-based frame 3!
        stage.stepSingleFrame()
        stage.stepSingleFrame()
        assert(scene.currentFrame == 5, "MainScene should be at frame 5")
        assert(scene.gfxLoop.currentFrame == 1, "Graphic Loop at age 4 should wrap to frame 1, got \(scene.gfxLoop.currentFrame)")
        assert(scene.gfxOnce.currentFrame == 4, "Graphic PlayOnce at age 4 should clamp at frame 4, got \(scene.gfxOnce.currentFrame)")
        assert(scene.gfxSingle.currentFrame == 3, "Graphic SingleFrame(2) must hold frame 3, got \(scene.gfxSingle.currentFrame)")

        // E. Verify CoreGraphics Mask clipping (EvenOdd ring vs EvenOdd hole vs outside), Filters (DropShadow + Blur), and Screen blend!
        let snapActiveMask = renderSnapshot(stage)
        snapActiveMask.withUnsafeBufferPointer {{ buf in
            guard let ptr = buf.baseAddress else {{ return }}
            // 1. Mask ring at (120, 120): inside outer (100..260), outside hole (150..210) -> bright red MaskedBanner!
            let ringPx = pixelRGBA(ptr, width: 800, x: 120, y: 120)
            assert(ringPx.0 > 200 && ringPx.1 < 60, "Mask ring (120,120) must draw red banner, got \(ringPx)")
            // 2. EvenOdd mask hole at (180, 180): clipped out -> dark stage background (16, 20, 28)!
            let holePx = pixelRGBA(ptr, width: 800, x: 180, y: 180)
            assert(holePx.0 < 40 && holePx.1 < 40, "EvenOdd mask hole (180,180) must clip out banner, got \(holePx)")
            // 3. Outside mask at (90, 90) (inside banner 80..280, outside mask 100..260): clipped out!
            let outPx = pixelRGBA(ptr, width: 800, x: 90, y: 90)
            assert(outPx.0 < 40 && outPx.1 < 40, "Outside mask (90,90) must clip out banner, got \(outPx)")

            // 4. Screen Blend overlap at (360, 120): base (160, 40, 120) SCREEN top (120, 150, 60) -> R > 200, G > 160!
            let screenPx = pixelRGBA(ptr, width: 800, x: 360, y: 120)
            assert(screenPx.0 > 200 && screenPx.1 > 160, "Screen blend overlap must brighten RGB, got \(screenPx)")

            // 5. DropShadow below el-blend-base (310..390, 80..160, distance=10 at 90deg -> shadow at x=325, y=166 on light mat):
            //    darkens light mat (200, 210, 225) while mat above y=80 at (325, 72) stays bright!
            let shadowBelowPx = pixelRGBA(ptr, width: 800, x: 325, y: 166)
            let matAbovePx = pixelRGBA(ptr, width: 800, x: 325, y: 72)
            assert(shadowBelowPx.0 + 40 < matAbovePx.0 && shadowBelowPx.2 + 40 < matAbovePx.2,
                   "DropShadow at (325,166) must darken light mat downward (\(shadowBelowPx) vs \(matAbovePx))")

            // 6. Blur filter on blurTile (470..520, 90..140): bleeds cyan glow outside x=468!
            let cleanBgPx = pixelRGBA(ptr, width: 800, x: 460, y: 230)
            let blurBleedPx = pixelRGBA(ptr, width: 800, x: 467, y: 115)
            assert(blurBleedPx.1 > cleanBgPx.1 + 15 && blurBleedPx.2 > cleanBgPx.2 + 15,
                   "Blur filter must bleed soft cyan pixels outside crisp edge at (467,115), got \(blurBleedPx)")
        }}

        // Also verify mask hit-testing respects EvenOdd hole & outer clip!
        assert(stage.hitTestStageElements(CGPoint(x: 120, y: 120)) != nil, "Mask ring must hit")
        assert(stage.hitTestStageElements(CGPoint(x: 180, y: 180)) == nil, "Mask EvenOdd hole must NOT hit")
        assert(stage.hitTestStageElements(CGPoint(x: 90, y: 90)) == nil, "Masked-out region must NOT hit")

        // Step 1 more frame to frame 6 (0-based 5, blank mask keyframe) -> entire maskedBanner is clipped out!
        stage.stepSingleFrame()
        assert(scene.currentFrame == 6, "Expected MainScene frame 6")
        let snapEmptyMask = renderSnapshot(stage)
        snapEmptyMask.withUnsafeBufferPointer {{ buf in
            guard let ptr = buf.baseAddress else {{ return }}
            let clippedPx = pixelRGBA(ptr, width: 800, x: 120, y: 120)
            assert(clippedPx.0 < 40, "Blank mask keyframe on frame 6 must clip out (120,120), got \(clippedPx)")
        }}

        // F. Verify Liquid `showAll` & `noScale` Button Hit-Testing (`Up`/`Over`/`Down`/`Hit`), `on(press)`/`on(release)`, MouseEvents, and Multi-Touch!
        stage.scaleMode = .showAll
        stage.align = .center
        // Viewport 1600x1200 -> 2.0x uniform scale! actionBtn Stage (60, 480), Hit frame size 140x50 -> Hit extended x=190 (outside visual 120, inside Hit 140!)
        stage.updateViewport(width: 1600, height: 1200)
        let hitOnlyViewPt = CGPoint(x: (60.0 + 130.0) * 2.0, y: (480.0 + 25.0) * 2.0)

        stage.handlePointerMove(viewPoint: hitOnlyViewPt, buttonDown: false)
        assert(scene.actionBtn.buttonState == .over && scene.moveEvents >= 1,
               "Button 4th-frame Hit region in scaled showAll viewport must trigger .over & MOUSE_MOVE")

        stage.handlePointerDown(viewPoint: hitOnlyViewPt)
        assert(scene.actionBtn.buttonState == .down, "Button must enter .down on press")
        assert(scene.pressCount == 1 && scene.releaseCount == 0 && scene.downEvents == 1,
               "on(press) and MOUSE_DOWN must fire immediately on pointer down without firing on(release)")
        assert(host.actions.contains("btn_press"), "on(press) must invoke SwiftHost.notifyAction('btn_press')")

        stage.handlePointerUp(viewPoint: hitOnlyViewPt)
        assert(scene.actionBtn.buttonState == .over, "Button must return to .over on mouse release")
        assert(scene.releaseCount == 1 && scene.clickEvents == 1 && scene.upEvents == 1,
               "on(release), MOUSE_UP, and MouseEvent.CLICK must fire on pointer up")
        assert(host.haptics == ["medium"], "on(release) fscommand('pulseHaptic', 'medium') must invoke host stub")

        // Switch to `noScale` top-left and test multi-touch (`TouchEvent.TOUCH_TAP` with two simultaneous touch IDs!)
        stage.scaleMode = .noScale
        stage.align = .topLeft
        stage.updateViewport(width: 1024, height: 768)
        let btnStagePt = CGPoint(x: 100.0, y: 500.0)
        stage.handleTouchBegin(touchId: 1, viewPoint: btnStagePt)
        stage.handleTouchBegin(touchId: 2, viewPoint: CGPoint(x: 110.0, y: 505.0))
        stage.handleTouchEnd(touchId: 2, viewPoint: CGPoint(x: 110.0, y: 505.0))
        assert(scene.tapEvents == 1 && scene.lastTapId == 2, "Secondary touch end must dispatch TouchEvent.TOUCH_TAP with touchPointID=2")
        stage.handleTouchEnd(touchId: 1, viewPoint: btnStagePt)
        assert(scene.tapEvents == 2 && scene.lastTapId == 1, "Primary touch end must dispatch TouchEvent.TOUCH_TAP with touchPointID=1")

        // G. Verify Input TextField focus, SwiftUI overlay inputState binding, restrict + maxChars filtering, and Event.CHANGE!
        let inputPt = CGPoint(x: 120.0, y: 425.0)
        stage.handlePointerDown(viewPoint: inputPt)
        stage.handlePointerUp(viewPoint: inputPt)
        assert(stage.focus === scene.playerInput, "Clicking playerInput must focus it on NFStage")
        assert(stage.inputState.activeField === scene.playerInput && stage.inputState.text == "Guest",
               "SwiftUI NFStageInputState must activate for focused Input TextField")
        assert(stage.inputState.frameInView.width >= 200.0, "Expected non-empty view rect for SwiftUI TextField overlay")

        stage.commitInputTextEdit("Neo_Dev!@#987654")
        // restrict="A-Za-z0-9_" strips "!@#", maxChars=12 truncates "Neo_Dev987654" (13 chars) -> "Neo_Dev98765" (12 chars)
        assert(scene.playerInput.text == "Neo_Dev98765",
               "Expected restricted + maxChars(12) text 'Neo_Dev98765', got '\(scene.playerInput.text)'")
        assert(scene.inputChangeCount == 1 && scene.statusField.text == "Player:Neo_Dev98765",
               "Committed SwiftUI input edit must dispatch Event.CHANGE to AS3 listener!")

        // H. Verify Polyphonic AVAudioEngine SoundChannels, SoundTransform(volume, pan) mixing, and stopAllSounds()!
        audioMgr.stopAllSounds()
        assert(audioMgr.activeChannelCount == 0, "Expected clean audio slate after stopAllSounds()")

        let chLeft = audioMgr.playSound("chime.wav", loops: 4, soundTransform: NFSoundTransform(0.4, -0.8))
        let chRight = audioMgr.playSound("chime.wav", loops: 4, soundTransform: NFSoundTransform(0.9, 0.75))
        let chLaser = audioMgr.playSound("laser.wav", loops: 4, soundTransform: NFSoundTransform(0.6, 0.0))
        assert(chLeft != nil && chRight != nil && chLaser != nil, "Expected 3 polyphonic SoundChannels from AVAudioEngine")
        assert(audioMgr.activeChannelCount == 3 && audioMgr.activeInstanceCount("chime.wav") == 2,
               "Expected 3 active polyphonic voices (2 chime + 1 laser), got \(audioMgr.activeChannelCount)")
        if let chL = chLeft, let chR = chRight {{
            assert(chL.leftPeak > chL.rightPeak, "Left-panned channel must have leftPeak > rightPeak")
            assert(chR.rightPeak > chR.leftPeak, "Right-panned channel must have rightPeak > leftPeak")
            chR.soundTransform = NFSoundTransform(0.25, -0.5)
            assert(abs(chR.effectiveMix.volume - 0.25) < 1e-5 && abs(chR.effectiveMix.pan - (-0.5)) < 1e-5,
                   "Live SoundTransform update on channel failed: \(chR.effectiveMix)")
            chL.stop()
        }}
        assert(audioMgr.activeChannelCount == 2 && audioMgr.activeInstanceCount("chime.wav") == 1,
               "Stopping chLeft must leave chRight and chLaser playing")
        scene.stopAllSounds()
        // Note: scene.stopAllSounds() stops stage.audioManager (our audioMgr)!
        assert(audioMgr.activeChannelCount == 0 && !audioMgr.isPlaying("chime.wav") && !audioMgr.isPlaying("laser.wav"),
               "stopAllSounds() must stop all active polyphonic AVAudioEngine channels")

        print("T472_XCODE_SWIFT_HARNESS_OK")
    }}
}}
"#
    );
    fs::write(&harness_swift_path, harness_swift).expect("write T472RuntimeHarness.swift");

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
        .arg(&harness_swift_path)
        .arg("-o")
        .arg(&harness_bin_path)
        .output()
        .expect("compile T472RuntimeHarness");
    assert!(
        swiftc_out.status.success(),
        "swiftc failed compiling T472RuntimeHarness:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&swiftc_out.stdout),
        String::from_utf8_lossy(&swiftc_out.stderr)
    );

    let run_out = Command::new(&harness_bin_path)
        .output()
        .expect("execute T472RuntimeHarnessBin");
    assert!(
        run_out.status.success()
            && String::from_utf8_lossy(&run_out.stdout).contains("T472_XCODE_SWIFT_HARNESS_OK"),
        "T472RuntimeHarnessBin execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_out.stdout),
        String::from_utf8_lossy(&run_out.stderr)
    );
}

#[test]
fn t472_as2_helper_class_and_sound_compile_and_execute_in_swift6() {
    let tmp = TempDir::new().expect("tempdir");
    let out_dir = tmp.path().join("AS2DeepSwiftExport");

    let beep_wav = make_pcm_wav_bytes(4_410, 400);
    let beep_res = ResourceUnit::from_bytes("res-beep", "audio/beep.wav", "audio/wav", &beep_wav)
        .expect("beep wav");

    let mut proj = Project::blank();
    proj.name = "Classic AS2 Swift".into();
    proj.script_language = Some(ScriptLanguage::ActionScript2);
    proj.resources.push(beep_res);
    proj.sources.push(SourceUnit {
        id: "src-as2-math".into(),
        path: "ComboHelper.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: r#"class ComboHelper {
    var base:Number;

    function ComboHelper(startBase:Number) {
        this.base = startBase;
    }

    function compute(step:Number):Number {
        return Math.floor(this.base + Math.abs(step) * Math.E / Math.E);
    }
}
"#
        .into(),
    });

    let root_id = proj.root.clone();
    let root = proj.symbol_mut(&root_id).expect("root");
    root.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: r#"
var helper:ComboHelper = new ComboHelper(12);
var total:Number = helper.compute(-8);
var sfx:Sound = new Sound(this);
sfx.attachSound("beep.wav");
sfx.setVolume(75);
sfx.setPan(-25);
sfx.start(0, 2);
SwiftHost.recordCombo(total);
stopAllSounds();
"#
            .into(),
        },
    });

    let report = export_xcode_project(&proj, &out_dir, &XcodeExportOptions::default(), None)
        .expect("export AS2 project to Xcode");
    assert_plutil_lint(&report.pbxproj_path);
    assert_plutil_lint(&report.info_plist_path);

    let contract_src =
        fs::read_to_string(out_dir.join("Generated/Scenes/SwiftHostContract.swift")).unwrap();
    assert!(
        contract_src.contains("func recordCombo("),
        "Expected inferred AS2 SwiftHost.recordCombo in contract:\n{contract_src}"
    );

    if should_skip_xcodebuild() {
        return;
    }

    let runner_swift_path = tmp.path().join("AS2VerifyMain.swift");
    let runner_bin_path = tmp.path().join("AS2VerifyBin");
    let resources_dir_escaped = out_dir
        .join("Generated/Resources")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    let runner_swift = format!(
        r#"import Foundation

@main
@MainActor
struct AS2VerifyMain {{
    static func main() {{
        NFAudioManager.shared.resourceDirectory = URL(fileURLWithPath: "{resources_dir_escaped}")
        let host = AppSwiftHost()
        let scene = MainScene()
        scene.host = host
        let stage = NFStage(scene: scene)
        _ = stage
        assert(abs(scene.total - 20.0) < 1e-6, "Expected AS2 ComboHelper.compute(-8) == 20.0, got \(scene.total)")
        assert(NFAudioManager.shared.activeChannelCount == 0, "Expected stopAllSounds() to clear active channels")
        print("AS2_SWIFT6_OK")
    }}
}}
"#
    );
    fs::write(&runner_swift_path, runner_swift).expect("write AS2VerifyMain.swift");

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
        .arg(out_dir.join("App/AppSwiftHost.swift"))
        .arg(&runner_swift_path)
        .arg("-o")
        .arg(&runner_bin_path)
        .output()
        .expect("compile AS2VerifyMain");
    assert!(
        swiftc_out.status.success(),
        "AS2 Swift 6 compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&swiftc_out.stdout),
        String::from_utf8_lossy(&swiftc_out.stderr)
    );

    let run_out = Command::new(&runner_bin_path)
        .output()
        .expect("run AS2VerifyBin");
    assert!(
        run_out.status.success()
            && String::from_utf8_lossy(&run_out.stdout).contains("AS2_SWIFT6_OK"),
        "AS2VerifyBin failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_out.stdout),
        String::from_utf8_lossy(&run_out.stderr)
    );
}
