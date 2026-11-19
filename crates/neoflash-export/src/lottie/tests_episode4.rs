use super::*;

#[test]
fn midnight_express_episode_4_aurora_odyssey_cartoon() {
    use crate::video::{VideoCodec, VideoFormat, VideoOptions, export_project_video};
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    use neoflash_core::{
        AssetWarpRig, AuthoredPlayback, BlendMode, DecoEffectKind, Display3dPose, Easing, Element,
        ElementKind, FillRule, FilterSpec, FrameScript, FrameSound, GradientKind, GradientPaint,
        GradientStop, IkArmature, IkBone, InstancePlayback, Keyframe, Layer, LayerProperties,
        LayerRole, PathCommand, PrimitiveOptions, Project, RectangleOptions, ResourceUnit, Script,
        ScriptLanguage, ShapeHint, SoundEffect, SoundSync, SprayBrushConfig, Symbol, SymbolKind,
        Timeline, Transform, TweenKind, TweenProperty, WarpPin, WarpPinKind, evaluate,
        flatten_scenes_with_ranges, generate_deco_elements, generate_spray_brush_along_points,
        project_element_3d,
    };
    use std::f64::consts::TAU;

    // High-vertex (64-point) Ice Crystal Rose contour
    fn ice_crystal_rose_64(cx: f64, cy: f64, base_r: f64) -> Vec<PathCommand> {
        let n = 64usize;
        let mut cmds = Vec::with_capacity(n + 2);
        for i in 0..n {
            let a = (i as f64 / n as f64) * TAU - std::f64::consts::FRAC_PI_2;
            let r = base_r + 22.0 * (a * 8.0).cos() + 9.0 * (a * 16.0).sin();
            let x = cx + r * a.cos();
            let y = cy + r * a.sin();
            if i == 0 {
                cmds.push(PathCommand::MoveTo { x, y });
            } else {
                cmds.push(PathCommand::LineTo { x, y });
            }
        }
        cmds.push(PathCommand::Close);
        cmds
    }

    // High-vertex (64-point) Northern Crown Star contour
    fn northern_crown_64(cx: f64, cy: f64, outer_r: f64, inner_r: f64) -> Vec<PathCommand> {
        let n = 64usize;
        let mut cmds = Vec::with_capacity(n + 2);
        for i in 0..n {
            let a = (i as f64 / n as f64) * TAU - std::f64::consts::FRAC_PI_2;
            let spire = if i % 4 == 0 {
                outer_r
            } else if i % 2 == 0 {
                (outer_r + inner_r) * 0.55
            } else {
                inner_r
            };
            let x = cx + spire * a.cos();
            let y = cy + spire * a.sin();
            if i == 0 {
                cmds.push(PathCommand::MoveTo { x, y });
            } else {
                cmds.push(PathCommand::LineTo { x, y });
            }
        }
        cmds.push(PathCommand::Close);
        cmds
    }

    // Aurora Ribbon wave contour
    fn aurora_ribbon_commands(cx: f64, cy: f64, width: f64, height: f64) -> Vec<PathCommand> {
        let hw = width * 0.5;
        let hh = height * 0.5;
        vec![
            PathCommand::MoveTo {
                x: cx - hw,
                y: cy - hh,
            },
            PathCommand::CubicTo {
                cx1: cx - hw * 0.35,
                cy1: cy - hh * 2.2,
                cx2: cx + hw * 0.35,
                cy2: cy + hh * 1.4,
                x: cx + hw,
                y: cy - hh * 0.6,
            },
            PathCommand::LineTo {
                x: cx + hw,
                y: cy + hh,
            },
            PathCommand::CubicTo {
                cx1: cx + hw * 0.35,
                cy1: cy + hh * 2.4,
                cx2: cx - hw * 0.35,
                cy2: cy - hh * 1.2,
                x: cx - hw,
                y: cy + hh * 0.8,
            },
            PathCommand::Close,
        ]
    }

    // 1. Synthesize 8 kHz mono 16-bit PCM WAV Aurora Choir + Train Whistle (1.5s = 12000 samples)
    let sample_rate = 8000u32;
    let num_samples = 12000u32;
    let data_bytes = num_samples * 2;
    let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let env = (1.0 - (t / 1.5) * 0.4).clamp(0.0, 1.0);
        // Lydian Aurora shimmer chord (F4 + A4 + C5 + E5 + B5)
        let f1 = (t * 349.23 * TAU).sin();
        let f2 = (t * 440.00 * TAU).sin() * 0.7;
        let f3 = (t * 523.25 * TAU).sin() * 0.6;
        let f4 = (t * 659.25 * TAU).sin() * 0.5;
        let f5 = (t * 987.77 * TAU).sin() * 0.35 * (t * 5.0 * TAU).sin().abs();
        let sample =
            ((f1 + f2 + f3 + f4 + f5) * 0.21 * env * 32767.0).clamp(-32768.0, 32767.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    // 2. Build a 16x16 Aurora Emblem PNG resource
    let mut png_bytes = Vec::new();
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u8 {
        for x in 0..16u8 {
            let border = !(2..14).contains(&x) || !(2..14).contains(&y);
            let star = (x as i32 - 8).abs() + (y as i32 - 8).abs() <= 4;
            if border {
                rgba.extend_from_slice(&[52, 211, 153, 255]);
            } else if star {
                rgba.extend_from_slice(&[250, 204, 21, 255]);
            } else {
                rgba.extend_from_slice(&[15, 23, 42, 235]);
            }
        }
    }
    PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
        .unwrap();

    let mut p = Project::blank();
    p.format_version = 9;
    p.name = "Midnight Express 4 — The Aurora Puppet & IK Odyssey".into();
    p.width = 960;
    p.height = 540;
    p.fps = 24;
    p.background = [10, 15, 32, 255];
    p.resources.push(
        ResourceUnit::from_bytes(
            "res-aurora-whistle",
            "audio/aurora_whistle.wav",
            "audio/wav",
            &wav,
        )
        .unwrap(),
    );
    p.resources.push(
        ResourceUnit::from_bytes(
            "res-aurora-crest",
            "images/aurora_crest.png",
            "image/png",
            &png_bytes,
        )
        .unwrap(),
    );

    // 3. Nested Graphic symbol: `sym-train` (8 frames, Aurora Express locomotive)
    let train_symbol = Symbol {
        id: "sym-train".into(),
        name: "Aurora Express Locomotive".into(),
        folder: "Vehicles".into(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 8,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "train-body-layer".into(),
                name: "Locomotive Hull".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: (0..8)
                    .map(|f| {
                        let pulse = (f as f64 * TAU / 8.0).sin() * 0.5 + 0.5;
                        let bounce = if f % 2 == 0 { 0.0 } else { -1.5 };
                        Keyframe {
                            frame: f,
                            elements: vec![
                                Element {
                                    id: "chassis".into(),
                                    name: "Chassis".into(),
                                    transform: Transform {
                                        x: -74.0,
                                        y: 12.0 + bounce,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 148.0,
                                        height: 10.0,
                                        fill: [30, 58, 95, 255],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "cabin".into(),
                                    name: "Cabin".into(),
                                    transform: Transform {
                                        x: -68.0,
                                        y: -28.0 + bounce,
                                        primitive: Some(PrimitiveOptions::Rectangle(
                                            RectangleOptions {
                                                corner_radii: [10.0, 10.0, 4.0, 4.0],
                                                corners_locked: false,
                                            },
                                        )),
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 132.0,
                                        height: 44.0,
                                        fill: [16, 185, 129, 255],
                                        stroke: [250, 204, 21, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "chimney".into(),
                                    name: "Chimney".into(),
                                    transform: Transform {
                                        x: 34.0,
                                        y: -48.0 + bounce,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 18.0,
                                        height: 22.0,
                                        fill: [14, 116, 144, 255],
                                        stroke: [250, 204, 21, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "win-1".into(),
                                    name: "Window 1".into(),
                                    transform: Transform {
                                        x: -52.0,
                                        y: -18.0 + bounce,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 26.0,
                                        height: 18.0,
                                        fill: [254, 240, 138, 255],
                                        stroke: [15, 23, 42, 255],
                                        stroke_width: 1.5,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "win-2".into(),
                                    name: "Window 2".into(),
                                    transform: Transform {
                                        x: -16.0,
                                        y: -18.0 + bounce,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 26.0,
                                        height: 18.0,
                                        fill: [254, 240, 138, 255],
                                        stroke: [15, 23, 42, 255],
                                        stroke_width: 1.5,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "wheel-rear".into(),
                                    name: "Rear Wheel".into(),
                                    transform: Transform {
                                        x: -48.0,
                                        y: 14.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 22.0,
                                        height: 22.0,
                                        fill: [15, 23, 42, 255],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "wheel-front".into(),
                                    name: "Front Wheel".into(),
                                    transform: Transform {
                                        x: 22.0,
                                        y: 14.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 22.0,
                                        height: 22.0,
                                        fill: [15, 23, 42, 255],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "headlight".into(),
                                    name: "Headlight".into(),
                                    transform: Transform {
                                        x: 56.0 + pulse * 6.0,
                                        y: -16.0 + bounce,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 20.0 + pulse * 8.0,
                                        height: 20.0 + pulse * 8.0,
                                        fill: [167, 243, 208, 245],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        }
                    })
                    .collect(),
            }],
        },
    };

    // 4. Nested MovieClip symbol: `sym-drone` (6 frames, Aurora Escort Drone)
    let drone_symbol = Symbol {
        id: "sym-drone".into(),
        name: "Aurora Escort Drone".into(),
        folder: "Vehicles".into(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "drone-layer".into(),
                name: "Drone".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        elements: vec![Element {
                            id: "drone-hull".into(),
                            name: "Hull".into(),
                            filters: vec![FilterSpec::Glow {
                                color: 0x34D399,
                                alpha: 0.9,
                                blur_x: 14.0,
                                blur_y: 14.0,
                                strength: 2.2,
                                quality: 2,
                                inner: false,
                                knockout: false,
                            }],
                            kind: ElementKind::Ellipse {
                                width: 52.0,
                                height: 24.0,
                                fill: [52, 211, 153, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 5,
                        elements: vec![Element {
                            id: "drone-hull".into(),
                            name: "Hull".into(),
                            transform: Transform {
                                y: -12.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 52.0,
                                height: 24.0,
                                fill: [52, 211, 153, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            }],
        },
    };

    p.library_folders = vec!["Vehicles".into()];
    p.symbols.push(train_symbol);
    p.symbols.push(drone_symbol);

    // =========================================================================
    // 5. SCENE 1 (`p.root`): "Aurora Glacier Depot · IK & Puppet Rig" (48 frames)
    // =========================================================================
    let scene1_id = p.root.clone();
    {
        let scene1 = p.symbol_mut(&scene1_id).unwrap();
        scene1.name = "Aurora Glacier Depot · IK & Puppet Rig".into();
        scene1.timeline.frame_count = 48;
        // Live ActionScript HUD measuring real FPS during HTML5 playback
        scene1.timeline.scripts = vec![FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: r#"
if (this.ep4Init == undefined) {
this.ep4Init = true;
this.targetFps = 24;
this.measuredFps = 24.0;
this.lastStamp = getTimer();
this.onEnterFrame = function() {
    var now = getTimer();
    var dt = now - this.lastStamp;
    if (dt <= 0) { dt = 1; }
    this.lastStamp = now;
    var inst = 1000 / dt;
    this.measuredFps = Math.round((this.measuredFps * 0.85 + inst * 0.15) * 10) / 10;
};
}
"#
                .into(),
            },
        }];
        scene1.timeline.layers = vec![
            // Layer 0: Polar Night Sky + Glacier Depot Platform
            Layer {
                id: "ep4-s1-backdrop".into(),
                name: "Polar Sky & Glacier Depot".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "ep4_scene1_aurora_depot".into(),
                    elements: vec![
                        Element {
                            id: "ep4-sky-grad".into(),
                            name: "Polar Sky".into(),
                            kind: ElementKind::PaintedPath {
                                commands: vec![
                                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 540.0 },
                                    PathCommand::LineTo { x: 0.0, y: 540.0 },
                                    PathCommand::Close,
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [10, 18, 42, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                                fill_gradient: Some(GradientPaint {
                                    kind: GradientKind::Linear,
                                    matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                    stops: vec![
                                        GradientStop {
                                            ratio: 0,
                                            color: [6, 12, 30, 255],
                                        },
                                        GradientStop {
                                            ratio: 145,
                                            color: [16, 68, 92, 255],
                                        },
                                        GradientStop {
                                            ratio: 255,
                                            color: [42, 34, 88, 255],
                                        },
                                    ],
                                    focal_point_ratio: 0.0,
                                    ..Default::default()
                                }),
                                stroke_gradient: None,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "ep4-glacier-deck".into(),
                            name: "Glacier Viaduct".into(),
                            transform: Transform {
                                x: 0.0,
                                y: 438.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 960.0,
                                height: 102.0,
                                fill: [18, 32, 56, 255],
                                stroke: [52, 211, 153, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
            },
            // Layer 1: PUPPET TOOL (Asset Warp Rig) — Waving Aurora Borealis Curtain
            Layer {
                id: "ep4-s1-puppet-aurora".into(),
                name: "Puppet Warped Aurora Curtain".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    blend_mode: BlendMode::Screen,
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        asset_warps: vec![AssetWarpRig {
                            element_id: "aurora-curtain".into(),
                            envelop_subdivisions: 4,
                            pins: vec![
                                WarpPin {
                                    id: "pin-west".into(),
                                    kind: WarpPinKind::Open,
                                    rest_pos: [180.0, 130.0],
                                    pos: [180.0, 118.0],
                                    rotation: 8.0,
                                    stiffness: 0.5,
                                },
                                WarpPin {
                                    id: "pin-center".into(),
                                    kind: WarpPinKind::Open,
                                    rest_pos: [480.0, 130.0],
                                    pos: [480.0, 152.0],
                                    rotation: -12.0,
                                    stiffness: 0.5,
                                },
                                WarpPin {
                                    id: "pin-east".into(),
                                    kind: WarpPinKind::Open,
                                    rest_pos: [780.0, 130.0],
                                    pos: [780.0, 112.0],
                                    rotation: 10.0,
                                    stiffness: 0.5,
                                },
                            ],
                        }],
                        elements: vec![Element {
                            id: "aurora-curtain".into(),
                            name: "Aurora Borealis Curtain".into(),
                            kind: ElementKind::Path {
                                commands: aurora_ribbon_commands(480.0, 130.0, 680.0, 56.0),
                                fill_rule: FillRule::NonZero,
                                fill: [52, 211, 153, 185],
                                stroke: [167, 243, 208, 220],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "aurora-curtain".into(),
                            name: "Aurora Borealis Curtain".into(),
                            kind: ElementKind::Path {
                                commands: aurora_ribbon_commands(480.0, 116.0, 700.0, 64.0),
                                fill_rule: FillRule::NonZero,
                                fill: [56, 189, 248, 195],
                                stroke: [192, 132, 252, 235],
                                stroke_width: 3.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 2: INVERSE KINEMATICS (Bone/Bind Armature) — Articulated Signal Crane Arm
            Layer {
                id: "ep4-s1-ik-crane".into(),
                name: "IK Articulated Signal Crane".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Classic,
                        ik_armatures: vec![IkArmature {
                            id: "ik-signal-crane".into(),
                            name: "Signal Crane Armature".into(),
                            effector_bone_id: "bone-upper".into(),
                            target_pos: [215.0, 295.0],
                            pole_pos: Some([120.0, 320.0]),
                            bones: vec![
                                IkBone {
                                    id: "bone-lower".into(),
                                    parent_bone_id: None,
                                    element_id: "crane-lower".into(),
                                    length: 78.0,
                                    min_angle: Some((-120.0_f64).to_radians()),
                                    max_angle: Some((-15.0_f64).to_radians()),
                                    stiffness: 0.25,
                                },
                                IkBone {
                                    id: "bone-upper".into(),
                                    parent_bone_id: Some("bone-lower".into()),
                                    element_id: "crane-upper".into(),
                                    length: 68.0,
                                    min_angle: Some((-90.0_f64).to_radians()),
                                    max_angle: Some((45.0_f64).to_radians()),
                                    stiffness: 0.15,
                                },
                            ],
                        }],
                        elements: vec![
                            Element {
                                id: "crane-lower".into(),
                                name: "Crane Lower Boom".into(),
                                transform: Transform {
                                    x: 140.0,
                                    y: 360.0,
                                    rotation: -25.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 14.0,
                                    height: 78.0,
                                    fill: [56, 189, 248, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "crane-upper".into(),
                                name: "Crane Signal Lantern".into(),
                                transform: Transform {
                                    x: 175.0,
                                    y: 300.0,
                                    rotation: 15.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 28.0,
                                    height: 28.0,
                                    fill: [52, 211, 153, 255],
                                    stroke: [254, 240, 138, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![
                            Element {
                                id: "crane-lower".into(),
                                name: "Crane Lower Boom".into(),
                                transform: Transform {
                                    x: 140.0,
                                    y: 350.0,
                                    rotation: 12.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 14.0,
                                    height: 78.0,
                                    fill: [56, 189, 248, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "crane-upper".into(),
                                name: "Crane Signal Lantern".into(),
                                transform: Transform {
                                    x: 205.0,
                                    y: 270.0,
                                    rotation: 45.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 28.0,
                                    height: 28.0,
                                    fill: [52, 211, 153, 255],
                                    stroke: [254, 240, 138, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 3: HIGH-VERTEX SHAPE TWEEN #1 (64-vertex Ice Crystal Rose -> 64-vertex Northern Crown Star)
            Layer {
                id: "ep4-s1-crystal-morph".into(),
                name: "64-Vertex Ice Crystal to Northern Crown Morph".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        shape_hints: vec![
                            ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            },
                            ShapeHint {
                                label: 'b',
                                start_uv: [0.5, 1.0],
                                end_uv: [0.5, 1.0],
                            },
                        ],
                        elements: vec![Element {
                            id: "ice-crystal-64".into(),
                            name: "Ice Crystal Rose 64".into(),
                            kind: ElementKind::Path {
                                commands: ice_crystal_rose_64(760.0, 135.0, 58.0),
                                fill_rule: FillRule::NonZero,
                                fill: [125, 211, 252, 240],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 3.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "ice-crystal-64".into(),
                            name: "Northern Crown 64".into(),
                            kind: ElementKind::Path {
                                commands: northern_crown_64(740.0, 125.0, 76.0, 32.0),
                                fill_rule: FillRule::NonZero,
                                fill: [250, 204, 21, 250],
                                stroke: [52, 211, 153, 255],
                                stroke_width: 3.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 4: Curved Glacier Track Motion Guide
            Layer {
                id: "ep4-s1-track-guide".into(),
                name: "Glacier Track Guide".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Guide,
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "ep4-guide-rail".into(),
                        name: "Rail".into(),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 90.0, y: 415.0 },
                                PathCommand::QuadraticTo {
                                    cx: 380.0,
                                    cy: 325.0,
                                    x: 560.0,
                                    y: 380.0,
                                },
                                PathCommand::QuadraticTo {
                                    cx: 720.0,
                                    cy: 420.0,
                                    x: 870.0,
                                    y: 350.0,
                                },
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [0, 0, 0, 0],
                            stroke: [52, 211, 153, 255],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 5: Guided Aurora Express Train (`orient_to_path` + `snap_to_guide`)
            Layer {
                id: "ep4-s1-express-train".into(),
                name: "Aurora Express Train".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    motion_guide: Some("ep4-s1-track-guide".into()),
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        orient_to_path: true,
                        snap_to_guide: true,
                        elements: vec![Element {
                            id: "ep4-train-inst".into(),
                            name: "Train".into(),
                            transform: Transform {
                                x: 90.0,
                                y: 415.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-train".into(),
                                playback: InstancePlayback::Loop,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "ep4-train-inst".into(),
                            name: "Train".into(),
                            transform: Transform {
                                x: 870.0,
                                y: 350.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Instance {
                                symbol_id: "sym-train".into(),
                                playback: InstancePlayback::Loop,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 6: Episode 4 Neon Banner
            Layer {
                id: "ep4-s1-title".into(),
                name: "Episode 4 Banner".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    blend_mode: BlendMode::Screen,
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Motion,
                        elements: vec![Element {
                            id: "ep4-banner".into(),
                            name: "Banner".into(),
                            transform: Transform {
                                x: 68.0,
                                y: 42.0,
                                alpha: 0.85,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "MIDNIGHT EXPRESS 4 · THE AURORA PUPPET & IK ODYSSEY".into(),
                                font_family: "Helvetica".into(),
                                font_size: 23.0,
                                color: [52, 211, 153, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 24,
                        keyed_properties: Some(vec![
                            TweenProperty::X,
                            TweenProperty::Y,
                            TweenProperty::Alpha,
                        ]),
                        elements: vec![Element {
                            id: "ep4-banner".into(),
                            name: "Banner".into(),
                            transform: Transform {
                                x: 88.0,
                                y: 36.0,
                                alpha: 1.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "MIDNIGHT EXPRESS 4 · THE AURORA PUPPET & IK ODYSSEY".into(),
                                font_family: "Helvetica".into(),
                                font_size: 23.0,
                                color: [52, 211, 153, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 7: Sound Layer
            Layer {
                id: "ep4-s1-audio".into(),
                name: "Aurora Choir & Whistle Audio".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound {
                        resource: "res-aurora-whistle".into(),
                        sync: SoundSync::Stream,
                        loops: 1,
                        loop_forever: false,
                        effect: SoundEffect::Custom,
                        in_point: Some(0.0),
                        out_point: None,
                        envelope: Some(vec![[0.0, 0.4, 0.9], [0.75, 0.95, 0.5]]),
                    }),
                    ..Keyframe::default()
                }],
            },
        ];
    }

    // =========================================================================
    // 6. SCENE 2: "Clockwork Aurora Viaduct · Deco, Spray & 3D" (48 frames)
    // =========================================================================
    let deco_elements = generate_deco_elements(
        DecoEffectKind::SymmetryBrush,
        [480.0, 185.0],
        115.0,
        28.0,
        [56, 189, 248, 220],
        [250, 204, 21, 255],
    );
    let spray_elements = generate_spray_brush_along_points(
        &[[120.0, 95.0], [310.0, 75.0], [640.0, 88.0], [830.0, 105.0]],
        &SprayBrushConfig {
            symbol_id: None,
            fill: [254, 240, 138, 235],
            scale_w_pct: 90.0,
            scale_h_pct: 90.0,
            random_scaling: true,
            rotate_symbol: false,
            random_rotation: true,
            brush_width: 48.0,
            brush_height: 48.0,
            brush_angle_deg: 0.0,
        },
    );
    let arch_3d_base = Element {
        id: "s2-3d-arch".into(),
        name: "3D Clockwork Arch".into(),
        transform: Transform {
            x: 420.0,
            y: 260.0,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 120.0,
            height: 150.0,
            fill: [30, 58, 138, 180],
            stroke: [52, 211, 153, 255],
            stroke_width: 3.0,
        },
        ..Element::default()
    };
    let arch_3d = project_element_3d(
        &arch_3d_base,
        &Display3dPose {
            x: 420.0,
            y: 260.0,
            z: -40.0,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
            rotation_x: 18.0,
            rotation_y: -24.0,
            rotation_z: 4.0,
        },
        480.0,
        [480.0, 270.0],
    );

    let mut s2_backdrop_elements = vec![
        Element {
            id: "s2-sky".into(),
            name: "Viaduct Sky".into(),
            kind: ElementKind::Rectangle {
                width: 960.0,
                height: 540.0,
                fill: [12, 20, 48, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        },
        Element {
            id: "s2-bridge-deck".into(),
            name: "Bridge Deck".into(),
            transform: Transform {
                x: 0.0,
                y: 412.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 960.0,
                height: 24.0,
                fill: [30, 58, 95, 255],
                stroke: [250, 204, 21, 255],
                stroke_width: 2.5,
            },
            ..Element::default()
        },
        arch_3d,
    ];
    s2_backdrop_elements.extend(deco_elements);
    s2_backdrop_elements.extend(spray_elements);

    let scene2 = Symbol {
        id: "scene-clockwork-viaduct".into(),
        name: "Clockwork Aurora Viaduct · Deco & 3D".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 48,
            scripts: vec![],
            guides: vec![],
            layers: vec![
                Layer {
                    id: "ep4-s2-backdrop".into(),
                    name: "Deco, Spray & 3D Viaduct".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "ep4_scene2_viaduct".into(),
                        elements: s2_backdrop_elements,
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "ep4-s2-morph".into(),
                    name: "Crown to Crystal Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![Element {
                                id: "s2-crown-64".into(),
                                name: "Crown 64".into(),
                                kind: ElementKind::Path {
                                    commands: northern_crown_64(240.0, 185.0, 68.0, 28.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [52, 211, 153, 240],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "s2-crown-64".into(),
                                name: "Crystal 64".into(),
                                kind: ElementKind::Path {
                                    commands: ice_crystal_rose_64(720.0, 185.0, 64.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [244, 114, 182, 240],
                                    stroke: [254, 240, 138, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s2-train".into(),
                    name: "Viaduct Express Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Linear),
                            elements: vec![Element {
                                id: "s2-train-inst".into(),
                                name: "Bridge Train".into(),
                                transform: Transform {
                                    x: 110.0,
                                    y: 382.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "s2-train-inst".into(),
                                name: "Bridge Train".into(),
                                transform: Transform {
                                    x: 850.0,
                                    y: 382.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s2-drone".into(),
                    name: "Aurora Drone Escort".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Flash { acceleration: 50.0 }),
                            elements: vec![Element {
                                id: "s2-drone-inst".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 130.0,
                                    y: 150.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-drone".into(),
                                    playback: InstancePlayback::Independent,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "s2-drone-inst".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 820.0,
                                    y: 135.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-drone".into(),
                                    playback: InstancePlayback::Independent,
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

    // =========================================================================
    // 7. SCENE 3: "Solstice Citadel Sunrise" (48 frames)
    // =========================================================================
    let scene3 = Symbol {
        id: "scene-solstice-sunrise".into(),
        name: "Solstice Citadel Sunrise".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 48,
            scripts: vec![],
            guides: vec![],
            layers: vec![
                Layer {
                    id: "ep4-s3-sky-morph".into(),
                    name: "Polar-to-Solstice Sky Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            label: "ep4_scene3_sunrise".into(),
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![
                                Element {
                                    id: "s3-sky".into(),
                                    name: "Solstice Sky".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 540.0 },
                                            PathCommand::LineTo { x: 0.0, y: 540.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [18, 28, 64, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [14, 24, 58, 255],
                                                },
                                                GradientStop {
                                                    ratio: 155,
                                                    color: [45, 85, 125, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [135, 95, 145, 255],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "s3-platform".into(),
                                    name: "Citadel Platform".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 432.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 108.0,
                                        fill: [30, 42, 68, 255],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![
                                Element {
                                    id: "s3-sky".into(),
                                    name: "Solstice Sky".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 540.0 },
                                            PathCommand::LineTo { x: 0.0, y: 540.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [250, 185, 125, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [38, 92, 148, 255],
                                                },
                                                GradientStop {
                                                    ratio: 155,
                                                    color: [232, 128, 152, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [254, 218, 128, 255],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "s3-platform".into(),
                                    name: "Citadel Platform".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 432.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 108.0,
                                        fill: [58, 52, 78, 255],
                                        stroke: [250, 204, 21, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s3-sun-morph".into(),
                    name: "64-Vertex Frost Rose to Solstice Sun Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![Element {
                                id: "solstice-sun".into(),
                                name: "Frost Rose".into(),
                                kind: ElementKind::Path {
                                    commands: ice_crystal_rose_64(735.0, 250.0, 56.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [167, 243, 208, 245],
                                    stroke: [52, 211, 153, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "solstice-sun".into(),
                                name: "Solstice Sun".into(),
                                kind: ElementKind::Path {
                                    commands: northern_crown_64(735.0, 190.0, 90.0, 68.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [250, 204, 21, 255],
                                    stroke: [249, 115, 22, 255],
                                    stroke_width: 4.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s3-arrival-train".into(),
                    name: "Citadel Arrival Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            elements: vec![Element {
                                id: "s3-train-inst".into(),
                                name: "Citadel Train".into(),
                                transform: Transform {
                                    x: 110.0,
                                    y: 404.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 28,
                            elements: vec![Element {
                                id: "s3-train-inst".into(),
                                name: "Citadel Train".into(),
                                transform: Transform {
                                    x: 420.0,
                                    y: 404.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s3-heart-morph".into(),
                    name: "Aurora Heart Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![Element {
                                id: "s3-heart".into(),
                                name: "Glowing Aurora Heart".into(),
                                kind: ElementKind::Path {
                                    commands: heart_puff_commands(280.0, 310.0, 38.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [52, 211, 153, 235],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "s3-heart".into(),
                                name: "Golden Heart".into(),
                                kind: ElementKind::Path {
                                    commands: heart_puff_commands(455.0, 215.0, 58.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [244, 114, 182, 250],
                                    stroke: [254, 240, 138, 255],
                                    stroke_width: 3.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                Layer {
                    id: "ep4-s3-closing-title".into(),
                    name: "Closing Morning Title".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "s3-morning-title".into(),
                            name: "Morning Title".into(),
                            transform: Transform {
                                x: 72.0,
                                y: 46.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "SOLSTICE CITADEL SUNRISE · GOOD MORNING!".into(),
                                font_family: "Helvetica".into(),
                                font_size: 24.0,
                                color: [254, 240, 138, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
            ],
        },
    };

    p.symbols.push(scene2);
    p.symbols.push(scene3);
    p.scenes = vec![
        scene1_id.clone(),
        "scene-clockwork-viaduct".into(),
        "scene-solstice-sunrise".into(),
    ];
    p.upgrade_format_for_features();
    p.validate().unwrap();

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if std::env::var_os("NEOFLASH_WRITE_EPISODE4_EXAMPLE").is_some() {
        let example_path = repo_root.join("examples/midnight-express-4.nflash");
        fs::write(&example_path, p.to_json().unwrap()).unwrap();
    }

    let (flat, ranges) = flatten_scenes_with_ranges(&p);
    flat.validate().unwrap();
    assert_eq!(ranges.len(), 3);
    assert_eq!(flat.symbol(&flat.root).unwrap().timeline.frame_count, 144);
    for f in 0..144 {
        let items = evaluate(&flat, &flat.root, f);
        assert!(!items.is_empty(), "frame {f} must produce draw items");
    }

    let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
    assert_eq!(playback.drain_sound_commands().len(), 1);

    let output = TestOutputDir::new("midnight-express-4");
    let export_media = std::env::var_os("NEOFLASH_EXPORT_EPISODE4_MEDIA").is_some();
    let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
    let out_dir = if export_media || export_html5 {
        repo_root.join("target/artifacts/generated-examples")
    } else {
        output.0.clone()
    };
    fs::create_dir_all(&out_dir).unwrap();

    let lottie_json_path = out_dir.join("midnight-express-4.lottie.json");
    let lottie_report = export_project_lottie(
        &p,
        &lottie_json_path,
        &LottieOptions {
            format: LottieFormat::Json,
            image_mode: LottieImageMode::Embed,
            image_format: LottieImageFormat::Png,
            html_preview: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    assert_eq!(lottie_report.frame_count, 144);
    assert!(lottie_json_path.exists());

    if export_media {
        let mp4_path = out_dir.join("midnight-express-4.mp4");
        let mp4_report = export_project_video(
            &p,
            &mp4_path,
            &VideoOptions {
                format: VideoFormat::Mp4,
                codec: VideoCodec::H264,
                include_audio: true,
                ..VideoOptions::default()
            },
        )
        .unwrap();
        assert_eq!(mp4_report.frame_count, 144);
        let gif_path = out_dir.join("midnight-express-4.gif");
        let gif_report = export_project_video(
            &p,
            &gif_path,
            &VideoOptions {
                format: VideoFormat::Gif,
                codec: VideoCodec::GifLzw,
                include_audio: false,
                ..VideoOptions::default()
            },
        )
        .unwrap();
        assert_eq!(gif_report.frame_count, 144);
    }

    let web_runtime = repo_root.join("target/web-runtime");
    if export_html5 && web_runtime.join("neoflash_web_bg.wasm").is_file() {
        let html5_dir = out_dir.join("midnight-express-4-html5");
        crate::export_project_with_options(
            &p,
            &html5_dir,
            &web_runtime,
            &crate::PublishOptions::default(),
        )
        .unwrap();
        assert!(html5_dir.join("index.html").exists());
    }
}
