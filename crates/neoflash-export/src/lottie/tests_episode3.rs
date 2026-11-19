use super::*;

#[test]
fn midnight_express_episode_3_starlight_morph_cartoon() {
    use crate::video::{VideoCodec, VideoFormat, VideoOptions, export_project_video};
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use neoflash_core::{
        AuthoredPlayback, BlendMode, Easing, Element, ElementKind, FillRule, FilterSpec,
        FrameSound, GradientKind, GradientPaint, GradientStop, InstancePlayback, Keyframe, Layer,
        LayerProperties, LayerRole, PathCommand, PrimitiveOptions, Project, RectangleOptions,
        ResourceUnit, ShapeHint, SoundEffect, SoundSync, Symbol, SymbolKind, Timeline, Transform,
        TweenKind, TweenProperty, evaluate, flatten_scenes_with_ranges,
    };
    use std::f64::consts::TAU;

    // Helper 1: Crisp Crescent Moon contour around (cx, cy) with radius r.
    fn crescent_moon_commands(cx: f64, cy: f64, r: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                x: cx - r * 0.15,
                y: cy - r,
            },
            PathCommand::QuadraticTo {
                cx: cx + r * 0.95,
                cy: cy - r * 0.88,
                x: cx + r * 0.95,
                y: cy,
            },
            PathCommand::QuadraticTo {
                cx: cx + r * 0.95,
                cy: cy + r * 0.88,
                x: cx - r * 0.15,
                y: cy + r,
            },
            PathCommand::QuadraticTo {
                cx: cx + r * 0.42,
                cy: cy + r * 0.52,
                x: cx + r * 0.42,
                y: cy,
            },
            PathCommand::QuadraticTo {
                cx: cx + r * 0.42,
                cy: cy - r * 0.52,
                x: cx - r * 0.15,
                y: cy - r,
            },
            PathCommand::Close,
        ]
    }

    // Helper 2: N-pointed Star Beacon contour around (cx, cy).
    fn star_beacon_commands(
        cx: f64,
        cy: f64,
        outer_r: f64,
        inner_r: f64,
        points: usize,
    ) -> Vec<PathCommand> {
        let mut cmds = Vec::with_capacity(points * 2 + 2);
        let total = points * 2;
        for i in 0..total {
            let angle = -std::f64::consts::FRAC_PI_2 + (i as f64) * TAU / (total as f64);
            let rad = if i % 2 == 0 { outer_r } else { inner_r };
            let x = cx + rad * angle.cos();
            let y = cy + rad * angle.sin();
            if i == 0 {
                cmds.push(PathCommand::MoveTo { x, y });
            } else {
                cmds.push(PathCommand::LineTo { x, y });
            }
        }
        cmds.push(PathCommand::Close);
        cmds
    }

    // Helper 3: Winged Comet Crest contour around (cx, cy).
    fn winged_comet_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                x: cx + 78.0 * s,
                y: cy,
            },
            PathCommand::QuadraticTo {
                cx: cx + 44.0 * s,
                cy: cy - 38.0 * s,
                x: cx - 12.0 * s,
                y: cy - 52.0 * s,
            },
            PathCommand::QuadraticTo {
                cx: cx + 6.0 * s,
                cy: cy - 18.0 * s,
                x: cx - 88.0 * s,
                y: cy - 22.0 * s,
            },
            PathCommand::QuadraticTo {
                cx: cx - 42.0 * s,
                cy,
                x: cx - 96.0 * s,
                y: cy + 8.0 * s,
            },
            PathCommand::QuadraticTo {
                cx: cx + 4.0 * s,
                cy: cy + 16.0 * s,
                x: cx - 14.0 * s,
                y: cy + 50.0 * s,
            },
            PathCommand::QuadraticTo {
                cx: cx + 44.0 * s,
                cy: cy + 36.0 * s,
                x: cx + 78.0 * s,
                y: cy,
            },
            PathCommand::Close,
        ]
    }

    // Helper 4: Fluffy Cloud Puff contour around (cx, cy).
    fn cloud_puff_commands(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                x: cx - rx,
                y: cy + ry * 0.2,
            },
            PathCommand::QuadraticTo {
                cx: cx - rx * 0.95,
                cy: cy - ry * 0.65,
                x: cx - rx * 0.35,
                y: cy - ry * 0.6,
            },
            PathCommand::QuadraticTo {
                cx,
                cy: cy - ry * 1.25,
                x: cx + rx * 0.4,
                y: cy - ry * 0.6,
            },
            PathCommand::QuadraticTo {
                cx: cx + rx * 1.0,
                cy: cy - ry * 0.55,
                x: cx + rx,
                y: cy + ry * 0.15,
            },
            PathCommand::QuadraticTo {
                cx: cx + rx * 0.9,
                cy: cy + ry * 0.85,
                x: cx + rx * 0.25,
                y: cy + ry * 0.75,
            },
            PathCommand::QuadraticTo {
                cx: cx - rx * 0.3,
                cy: cy + ry * 0.95,
                x: cx - rx,
                y: cy + ry * 0.2,
            },
            PathCommand::Close,
        ]
    }

    // Helper 5: Celestial Sky-Whale Spirit contour around (cx, cy).
    fn sky_whale_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
        vec![
            // Snout tip (right)
            PathCommand::MoveTo {
                x: cx + 115.0 * s,
                y: cy + 6.0 * s,
            },
            // Forehead & arched dorsal back
            PathCommand::QuadraticTo {
                cx: cx + 105.0 * s,
                cy: cy - 46.0 * s,
                x: cx + 22.0 * s,
                y: cy - 38.0 * s,
            },
            // Dorsal fin
            PathCommand::QuadraticTo {
                cx: cx - 6.0 * s,
                cy: cy - 68.0 * s,
                x: cx - 22.0 * s,
                y: cy - 30.0 * s,
            },
            // Tail stock to upper fluke
            PathCommand::QuadraticTo {
                cx: cx - 68.0 * s,
                cy: cy - 16.0 * s,
                x: cx - 118.0 * s,
                y: cy - 48.0 * s,
            },
            // Tail notch
            PathCommand::QuadraticTo {
                cx: cx - 94.0 * s,
                cy: cy - 6.0 * s,
                x: cx - 122.0 * s,
                y: cy + 32.0 * s,
            },
            // Lower tail stock & belly
            PathCommand::QuadraticTo {
                cx: cx - 64.0 * s,
                cy: cy + 8.0 * s,
                x: cx - 10.0 * s,
                y: cy + 28.0 * s,
            },
            // Flipper fin
            PathCommand::QuadraticTo {
                cx: cx + 14.0 * s,
                cy: cy + 64.0 * s,
                x: cx + 38.0 * s,
                y: cy + 28.0 * s,
            },
            // Chin back to snout
            PathCommand::QuadraticTo {
                cx: cx + 88.0 * s,
                cy: cy + 30.0 * s,
                x: cx + 115.0 * s,
                y: cy + 6.0 * s,
            },
            PathCommand::Close,
        ]
    }

    // Helper 6: Starlight Portal Arch contour around (cx, cy).
    fn portal_arch_commands(cx: f64, cy: f64, w: f64, h: f64) -> Vec<PathCommand> {
        let hw = w * 0.5;
        let hh = h * 0.5;
        let inner_w = hw * 0.58;
        let inner_h = hh * 0.72;
        vec![
            PathCommand::MoveTo {
                x: cx - hw,
                y: cy + hh,
            },
            PathCommand::LineTo {
                x: cx - hw,
                y: cy - hh * 0.15,
            },
            PathCommand::QuadraticTo {
                cx: cx - hw,
                cy: cy - hh * 1.05,
                x: cx,
                y: cy - hh,
            },
            PathCommand::QuadraticTo {
                cx: cx + hw,
                cy: cy - hh * 1.05,
                x: cx + hw,
                y: cy - hh * 0.15,
            },
            PathCommand::LineTo {
                x: cx + hw,
                y: cy + hh,
            },
            PathCommand::LineTo {
                x: cx + inner_w,
                y: cy + hh,
            },
            PathCommand::LineTo {
                x: cx + inner_w,
                y: cy - hh * 0.1,
            },
            PathCommand::QuadraticTo {
                cx,
                cy: cy - inner_h * 1.15,
                x: cx - inner_w,
                y: cy - hh * 0.1,
            },
            PathCommand::LineTo {
                x: cx - inner_w,
                y: cy + hh,
            },
            PathCommand::Close,
        ]
    }

    // Helper 7: Aurora Ribbon Wave across the sky.
    fn aurora_ribbon_commands(
        y_left: f64,
        y_mid: f64,
        y_right: f64,
        crest: f64,
    ) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x: 40.0, y: y_left },
            PathCommand::CubicTo {
                cx1: 260.0,
                cy1: y_mid - crest,
                cx2: 420.0,
                cy2: y_mid + crest * 0.8,
                x: 480.0,
                y: y_mid,
            },
            PathCommand::CubicTo {
                cx1: 560.0,
                cy1: y_mid - crest * 0.9,
                cx2: 740.0,
                cy2: y_right - crest * 0.5,
                x: 920.0,
                y: y_right,
            },
            PathCommand::LineTo {
                x: 920.0,
                y: y_right + 46.0,
            },
            PathCommand::CubicTo {
                cx1: 740.0,
                cy1: y_right + 46.0 + crest * 0.4,
                cx2: 560.0,
                cy2: y_mid + 52.0 - crest * 0.6,
                x: 480.0,
                y: y_mid + 52.0,
            },
            PathCommand::CubicTo {
                cx1: 420.0,
                cy1: y_mid + 52.0 + crest * 0.7,
                cx2: 260.0,
                cy2: y_left + 44.0 - crest * 0.8,
                x: 40.0,
                y: y_left + 44.0,
            },
            PathCommand::Close,
        ]
    }

    // Helper 8: Paper Lantern Diamond -> Origami Bird -> Star.
    fn lantern_diamond_commands(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x: cx, y: cy - ry },
            PathCommand::QuadraticTo {
                cx: cx + rx * 0.65,
                cy: cy - ry * 0.35,
                x: cx + rx,
                y: cy,
            },
            PathCommand::QuadraticTo {
                cx: cx + rx * 0.65,
                cy: cy + ry * 0.35,
                x: cx,
                y: cy + ry,
            },
            PathCommand::QuadraticTo {
                cx: cx - rx * 0.65,
                cy: cy + ry * 0.35,
                x: cx - rx,
                y: cy,
            },
            PathCommand::QuadraticTo {
                cx: cx - rx * 0.65,
                cy: cy - ry * 0.35,
                x: cx,
                y: cy - ry,
            },
            PathCommand::Close,
        ]
    }

    fn origami_bird_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
        vec![
            // Beak (right)
            PathCommand::MoveTo {
                x: cx + 48.0 * s,
                y: cy - 4.0 * s,
            },
            // Head to upper wing tip
            PathCommand::LineTo {
                x: cx + 14.0 * s,
                y: cy - 14.0 * s,
            },
            PathCommand::LineTo {
                x: cx - 12.0 * s,
                y: cy - 52.0 * s,
            },
            // Wing trailing edge to tail tip
            PathCommand::LineTo {
                x: cx - 18.0 * s,
                y: cy - 10.0 * s,
            },
            PathCommand::LineTo {
                x: cx - 48.0 * s,
                y: cy - 22.0 * s,
            },
            // Lower tail & lower wing
            PathCommand::LineTo {
                x: cx - 24.0 * s,
                y: cy + 8.0 * s,
            },
            PathCommand::LineTo {
                x: cx - 6.0 * s,
                y: cy + 44.0 * s,
            },
            PathCommand::LineTo {
                x: cx + 18.0 * s,
                y: cy + 10.0 * s,
            },
            PathCommand::Close,
        ]
    }

    // 1. Build a rich 8 kHz mono 16-bit PCM WAV nocturnal chime + train whistle (1.5s = 12000 samples).
    let sample_rate = 8000u32;
    let num_samples = 12000u32;
    let data_bytes = num_samples * 2;
    let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let env = (1.0 - (t / 1.5) * 0.45).clamp(0.0, 1.0);
        // Dreamy major-9th nocturnal bell + train whistle harmonic chord
        let f1 = (t * 440.0 * TAU).sin();
        let f2 = (t * 554.37 * TAU).sin() * 0.65;
        let f3 = (t * 659.25 * TAU).sin() * 0.55;
        let f4 = (t * 987.77 * TAU).sin() * 0.35 * (t * 6.0 * TAU).sin().abs();
        let sample = ((f1 + f2 + f3 + f4) * 0.24 * env * 32767.0).clamp(-32768.0, 32767.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    // 2. Build a 16x16 Starlight Station neon emblem PNG resource.
    let mut png_bytes = Vec::new();
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u8 {
        for x in 0..16u8 {
            let border = !(2..14).contains(&x) || !(2..14).contains(&y);
            let center_diamond = (x as i32 - 8).abs() + (y as i32 - 8).abs() <= 4;
            if border {
                rgba.extend_from_slice(&[90, 245, 255, 255]);
            } else if center_diamond {
                rgba.extend_from_slice(&[255, 220, 110, 255]);
            } else {
                rgba.extend_from_slice(&[24, 18, 56, 230]);
            }
        }
    }
    PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
        .unwrap();

    let mut p = Project::blank();
    p.format_version = 9;
    p.name = "Midnight Express 3 — The Starlight Morph".into();
    p.width = 960;
    p.height = 540;
    p.fps = 24;
    p.background = [12, 16, 36, 255];
    p.resources.push(
        ResourceUnit::from_bytes(
            "res-starlight-whistle",
            "audio/starlight_whistle.wav",
            "audio/wav",
            &wav,
        )
        .unwrap(),
    );
    p.resources.push(
        ResourceUnit::from_bytes(
            "res-billboard",
            "images/starlight_sign.png",
            "image/png",
            &png_bytes,
        )
        .unwrap(),
    );

    // 3. Nested Graphic symbol: `sym-train` (8 frames, detailed Midnight Express locomotive
    //    with cabin, boiler, smokestack, warm passenger windows, wheels, and pulsing headlight).
    let train_symbol = Symbol {
        id: "sym-train".into(),
        name: "Midnight Locomotive".into(),
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
                                // Chassis undercarriage
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
                                        fill: [38, 48, 78, 255],
                                        stroke: [255, 210, 105, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                // Main Cabin & Boiler Body
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
                                        fill: [215, 50, 78, 255],
                                        stroke: [255, 218, 115, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                // Smokestack Chimney
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
                                        fill: [52, 74, 122, 255],
                                        stroke: [255, 218, 115, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                // Cozy Warm Lit Passenger Window #1
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
                                        fill: [255, 238, 155, 255],
                                        stroke: [90, 50, 40, 255],
                                        stroke_width: 1.5,
                                    },
                                    ..Element::default()
                                },
                                // Cozy Warm Lit Passenger Window #2
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
                                        fill: [255, 238, 155, 255],
                                        stroke: [90, 50, 40, 255],
                                        stroke_width: 1.5,
                                    },
                                    ..Element::default()
                                },
                                // Rear Wheel
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
                                        fill: [24, 32, 58, 255],
                                        stroke: [255, 215, 110, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                                // Front Wheel
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
                                        fill: [24, 32, 58, 255],
                                        stroke: [255, 215, 110, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                                // Pulsing Golden Headlight
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
                                        fill: [255, 248, 175, 245],
                                        stroke: [255, 195, 55, 255],
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

    // 4. Nested MovieClip symbol: `sym-drone` (6 frames, glowing starlight escort drone).
    let drone_symbol = Symbol {
        id: "sym-drone".into(),
        name: "Starlight Escort Drone".into(),
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
                                color: 0x5AF5FF,
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
                                fill: [65, 235, 255, 255],
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
                                y: -10.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 52.0,
                                height: 24.0,
                                fill: [65, 235, 255, 255],
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
    // 5. SCENE 1 (`p.root`): "Night Station · Starlight Morph" (48 frames)
    // =========================================================================
    let scene1_id = p.root.clone();
    {
        let scene1 = p.symbol_mut(&scene1_id).unwrap();
        scene1.name = "Night Station · Starlight Morph".into();
        scene1.timeline.frame_count = 48;
        scene1.timeline.layers = vec![
            // Layer 0: Night Sky Gradient Backdrop + Viaduct Deck
            Layer {
                id: "s1-backdrop".into(),
                name: "Night Sky & Viaduct".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "ep3_scene1_station".into(),
                    elements: vec![
                        Element {
                            id: "sky-grad".into(),
                            name: "Night Sky".into(),
                            kind: ElementKind::PaintedPath {
                                commands: vec![
                                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 540.0 },
                                    PathCommand::LineTo { x: 0.0, y: 540.0 },
                                    PathCommand::Close,
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [14, 20, 46, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                                fill_gradient: Some(GradientPaint {
                                    kind: GradientKind::Linear,
                                    matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                    stops: vec![
                                        GradientStop {
                                            ratio: 0,
                                            color: [8, 12, 34, 255],
                                        },
                                        GradientStop {
                                            ratio: 155,
                                            color: [26, 38, 86, 255],
                                        },
                                        GradientStop {
                                            ratio: 255,
                                            color: [72, 42, 98, 255],
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
                            id: "platform-deck".into(),
                            name: "Starlight Viaduct".into(),
                            transform: Transform {
                                x: 0.0,
                                y: 438.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 960.0,
                                height: 102.0,
                                fill: [20, 26, 46, 255],
                                stroke: [95, 130, 195, 255],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
            },
            // Layer 1: Masked Illuminated Clouds
            Layer {
                id: "s1-lit-clouds".into(),
                name: "Searchlight Clouds".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    mask: Some("s1-searchlight-mask".into()),
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "cloud-band".into(),
                        name: "Illuminated Clouds".into(),
                        transform: Transform {
                            x: 60.0,
                            y: 40.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 820.0,
                            height: 220.0,
                            fill: [68, 165, 235, 130],
                            stroke: [160, 235, 255, 190],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 2: Animated Searchlight Mask
            Layer {
                id: "s1-searchlight-mask".into(),
                name: "Searchlight Mask".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        elements: vec![Element {
                            id: "beam-cone".into(),
                            name: "Beam".into(),
                            transform: Transform {
                                x: 110.0,
                                y: 72.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 190.0,
                                height: 165.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "beam-cone".into(),
                            name: "Beam".into(),
                            transform: Transform {
                                x: 610.0,
                                y: 72.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 190.0,
                                height: 165.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 3: SHAPE TWEEN MORPH #1 — Crescent Moon -> 8-Point Star Beacon -> Winged Comet
            Layer {
                id: "s1-celestial-morph".into(),
                name: "Moon to Star-Comet Morph".into(),
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
                            id: "celestial-morph".into(),
                            name: "Crescent Moon".into(),
                            kind: ElementKind::Path {
                                commands: crescent_moon_commands(760.0, 125.0, 62.0),
                                fill_rule: FillRule::NonZero,
                                fill: [255, 240, 165, 255],
                                stroke: [255, 195, 80, 240],
                                stroke_width: 3.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 24,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        shape_hints: vec![ShapeHint {
                            label: 'a',
                            start_uv: [1.0, 0.5],
                            end_uv: [1.0, 0.5],
                        }],
                        elements: vec![Element {
                            id: "celestial-morph".into(),
                            name: "Starlight Beacon".into(),
                            kind: ElementKind::Path {
                                commands: star_beacon_commands(650.0, 135.0, 74.0, 30.0, 8),
                                fill_rule: FillRule::NonZero,
                                fill: [95, 245, 255, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 4.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "celestial-morph".into(),
                            name: "Winged Comet".into(),
                            kind: ElementKind::Path {
                                commands: winged_comet_commands(790.0, 155.0, 0.95),
                                fill_rule: FillRule::NonZero,
                                fill: [255, 130, 225, 255],
                                stroke: [255, 235, 135, 255],
                                stroke_width: 3.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 4: Curved Railway Motion Guide
            Layer {
                id: "s1-track-guide".into(),
                name: "Track Guide".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    role: LayerRole::Guide,
                    ..LayerProperties::default()
                },
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "guide-rail".into(),
                        name: "Rail".into(),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 90.0, y: 415.0 },
                                PathCommand::QuadraticTo {
                                    cx: 360.0,
                                    cy: 330.0,
                                    x: 540.0,
                                    y: 382.0,
                                },
                                PathCommand::QuadraticTo {
                                    cx: 710.0,
                                    cy: 425.0,
                                    x: 865.0,
                                    y: 355.0,
                                },
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [0, 0, 0, 0],
                            stroke: [120, 205, 255, 255],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 5: Guided Express Train (`orient_to_path` + `snap_to_guide` + Graphic `Loop`)
            Layer {
                id: "s1-express-train".into(),
                name: "Express Train".into(),
                visible: true,
                locked: false,
                properties: LayerProperties {
                    motion_guide: Some("s1-track-guide".into()),
                    ..LayerProperties::default()
                },
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        orient_to_path: true,
                        snap_to_guide: true,
                        elements: vec![Element {
                            id: "train-inst".into(),
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
                            id: "train-inst".into(),
                            name: "Train".into(),
                            transform: Transform {
                                x: 865.0,
                                y: 355.0,
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
            // Layer 6: SHAPE TWEEN MORPH #2 — Steam Cloud Puff -> Celestial Sky-Whale -> Starlight Portal Arch
            Layer {
                id: "s1-steam-morph".into(),
                name: "Steam to Sky-Whale Morph".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseOut),
                        tween_kind: TweenKind::Shape,
                        shape_hints: vec![
                            ShapeHint {
                                label: 'a',
                                start_uv: [1.0, 0.5],
                                end_uv: [1.0, 0.5],
                            },
                            ShapeHint {
                                label: 'b',
                                start_uv: [0.0, 0.5],
                                end_uv: [0.0, 0.5],
                            },
                        ],
                        elements: vec![Element {
                            id: "steam-spirit".into(),
                            name: "Steam Spirit".into(),
                            kind: ElementKind::Path {
                                commands: cloud_puff_commands(185.0, 315.0, 52.0, 34.0),
                                fill_rule: FillRule::NonZero,
                                fill: [235, 248, 255, 235],
                                stroke: [150, 220, 255, 215],
                                stroke_width: 2.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 24,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        shape_hints: vec![ShapeHint {
                            label: 'a',
                            start_uv: [0.5, 0.0],
                            end_uv: [0.5, 0.0],
                        }],
                        elements: vec![Element {
                            id: "steam-spirit".into(),
                            name: "Celestial Sky-Whale".into(),
                            kind: ElementKind::Path {
                                commands: sky_whale_commands(465.0, 225.0, 1.05),
                                fill_rule: FillRule::NonZero,
                                fill: [110, 222, 255, 230],
                                stroke: [225, 252, 255, 245],
                                stroke_width: 3.2,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 47,
                        elements: vec![Element {
                            id: "steam-spirit".into(),
                            name: "Starlight Portal Arch".into(),
                            kind: ElementKind::Path {
                                commands: portal_arch_commands(835.0, 325.0, 135.0, 190.0),
                                fill_rule: FillRule::NonZero,
                                fill: [195, 125, 255, 235],
                                stroke: [95, 245, 255, 255],
                                stroke_width: 4.5,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 7: Motion Tween Neon Episode Title with ◆ property keyframe + Screen Blend Mode
            Layer {
                id: "s1-neon-sign".into(),
                name: "Neon Episode Title".into(),
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
                        motion_path: None,
                        elements: vec![Element {
                            id: "neon-banner".into(),
                            name: "Neon Banner".into(),
                            transform: Transform {
                                x: 72.0,
                                y: 44.0,
                                alpha: 0.8,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "MIDNIGHT EXPRESS 3 · THE STARLIGHT MORPH".into(),
                                font_family: "Helvetica".into(),
                                font_size: 24.0,
                                color: [255, 115, 220, 255],
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
                            id: "neon-banner".into(),
                            name: "Neon Banner".into(),
                            transform: Transform {
                                x: 96.0,
                                y: 38.0,
                                alpha: 1.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "MIDNIGHT EXPRESS 3 · THE STARLIGHT MORPH".into(),
                                font_family: "Helvetica".into(),
                                font_size: 24.0,
                                color: [255, 115, 220, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 8: Sound Layer (Stream whistle + stereo envelope)
            Layer {
                id: "s1-audio".into(),
                name: "Starlight Whistle Audio".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound {
                        resource: "res-starlight-whistle".into(),
                        sync: SoundSync::Stream,
                        loops: 1,
                        loop_forever: false,
                        effect: SoundEffect::Custom,
                        in_point: Some(0.0),
                        out_point: None,
                        envelope: Some(vec![[0.0, 0.35, 0.9], [0.7, 0.95, 0.45]]),
                    }),
                    ..Keyframe::default()
                }],
            },
        ];
    }

    // =========================================================================
    // 6. SCENE 2: "Nebula Bridge & Shape Symphony" (48 frames)
    // =========================================================================
    let scene2 = Symbol {
        id: "scene-nebula-bridge".into(),
        name: "Nebula Bridge & Shape Symphony".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 48,
            scripts: vec![],
            guides: vec![],
            layers: vec![
                // Layer 0: Cosmic Backdrop & Bridge Deck
                Layer {
                    id: "s2-bridge".into(),
                    name: "Nebula Sky & Bridge".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "ep3_scene2_nebula".into(),
                        elements: vec![
                            Element {
                                id: "s2-sky-grad".into(),
                                name: "Nebula Sky".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 540.0 },
                                        PathCommand::LineTo { x: 0.0, y: 540.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [18, 16, 48, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [12, 10, 42, 255],
                                            },
                                            GradientStop {
                                                ratio: 150,
                                                color: [42, 28, 92, 255],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [22, 68, 112, 255],
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
                                id: "bridge-deck".into(),
                                name: "Bridge Deck".into(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 410.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 960.0,
                                    height: 38.0,
                                    fill: [42, 56, 94, 255],
                                    stroke: [0, 235, 255, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "neon-billboard".into(),
                                name: "Starlight Sign".into(),
                                transform: Transform {
                                    x: 780.0,
                                    y: 250.0,
                                    scale_x: 5.5,
                                    scale_y: 5.5,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Bitmap {
                                    resource_id: "res-billboard".into(),
                                    width: 16,
                                    height: 16,
                                    smoothing: true,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    }],
                },
                // Layer 1: SHAPE TWEEN MORPH #3 — Aurora Ribbon Wave Morph (`PaintedPath` gradient morph)
                Layer {
                    id: "s2-aurora-morph".into(),
                    name: "Aurora Ribbon Shape Morph".into(),
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
                            elements: vec![Element {
                                id: "aurora-ribbon".into(),
                                name: "Emerald Aurora".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: aurora_ribbon_commands(115.0, 90.0, 120.0, 55.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 250, 195, 195],
                                    stroke: [140, 255, 230, 210],
                                    stroke_width: 2.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [0, 245, 185, 195],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [80, 175, 255, 210],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![Element {
                                id: "aurora-ribbon".into(),
                                name: "Magenta Crown Aurora".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: aurora_ribbon_commands(85.0, 130.0, 80.0, -65.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 110, 225, 205],
                                    stroke: [255, 225, 130, 225],
                                    stroke_width: 3.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [255, 105, 220, 210],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [255, 215, 100, 215],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "aurora-ribbon".into(),
                                name: "Starlight Halo Aurora".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: aurora_ribbon_commands(105.0, 75.0, 110.0, 48.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [110, 220, 255, 200],
                                    stroke: [255, 255, 255, 220],
                                    stroke_width: 2.5,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [110, 235, 255, 200],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [195, 130, 255, 210],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 2: SHAPE TWEEN MORPH #4 — Paper Lanterns -> Origami Star-Birds -> 4-Point Stars
                Layer {
                    id: "s2-lantern-birds-morph".into(),
                    name: "Lantern to Star-Bird Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [1.0, 0.5],
                                end_uv: [1.0, 0.5],
                            }],
                            elements: vec![
                                Element {
                                    id: "lantern-bird-1".into(),
                                    name: "Sky Lantern 1".into(),
                                    kind: ElementKind::Path {
                                        commands: lantern_diamond_commands(
                                            210.0, 240.0, 32.0, 44.0,
                                        ),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 185, 85, 245],
                                        stroke: [255, 240, 175, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "lantern-bird-2".into(),
                                    name: "Sky Lantern 2".into(),
                                    kind: ElementKind::Path {
                                        commands: lantern_diamond_commands(
                                            350.0, 195.0, 26.0, 36.0,
                                        ),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 150, 195, 245],
                                        stroke: [255, 235, 245, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            }],
                            elements: vec![
                                Element {
                                    id: "lantern-bird-1".into(),
                                    name: "Origami Star-Bird 1".into(),
                                    kind: ElementKind::Path {
                                        commands: origami_bird_commands(470.0, 215.0, 1.15),
                                        fill_rule: FillRule::NonZero,
                                        fill: [110, 250, 220, 245],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 2.8,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "lantern-bird-2".into(),
                                    name: "Origami Star-Bird 2".into(),
                                    kind: ElementKind::Path {
                                        commands: origami_bird_commands(610.0, 175.0, 0.95),
                                        fill_rule: FillRule::NonZero,
                                        fill: [155, 225, 255, 245],
                                        stroke: [255, 255, 255, 255],
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
                                    id: "lantern-bird-1".into(),
                                    name: "Guiding Star 1".into(),
                                    kind: ElementKind::Path {
                                        commands: star_beacon_commands(690.0, 195.0, 44.0, 16.0, 4),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 242, 155, 255],
                                        stroke: [255, 195, 85, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "lantern-bird-2".into(),
                                    name: "Guiding Star 2".into(),
                                    kind: ElementKind::Path {
                                        commands: star_beacon_commands(810.0, 155.0, 36.0, 13.0, 4),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 210, 245, 255],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 3: Synced Bridge Express Train
                Layer {
                    id: "s2-synced-train".into(),
                    name: "Synced Bridge Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        sync_timeline: true,
                        ..LayerProperties::default()
                    },
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
                                    x: 840.0,
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
                // Layer 4: Escort Drone
                Layer {
                    id: "s2-drone-escort".into(),
                    name: "Drone Escort".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Flash { acceleration: 55.0 }),
                            elements: vec![Element {
                                id: "s2-drone".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 130.0,
                                    y: 165.0,
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
                                id: "s2-drone".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 810.0,
                                    y: 145.0,
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
    // 7. SCENE 3: "Dream Harbour Dawn" (48 frames)
    // =========================================================================
    let scene3 = Symbol {
        id: "scene-harbour-dawn".into(),
        name: "Dream Harbour Dawn".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 48,
            scripts: vec![],
            guides: vec![],
            layers: vec![
                // Layer 0: SHAPE TWEEN MORPH #5 — Night-to-Dawn Gradient Sky Morph + Harbour Platform
                Layer {
                    id: "s3-dawn-sky-morph".into(),
                    name: "Night-to-Dawn Sky Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            label: "ep3_scene3_dawn".into(),
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            elements: vec![
                                Element {
                                    id: "s3-sky".into(),
                                    name: "Dawn Sky".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 540.0 },
                                            PathCommand::LineTo { x: 0.0, y: 540.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [24, 30, 68, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [18, 24, 58, 255],
                                                },
                                                GradientStop {
                                                    ratio: 155,
                                                    color: [68, 48, 108, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [155, 85, 125, 255],
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
                                    name: "Harbour Platform".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 432.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 108.0,
                                        fill: [38, 40, 66, 255],
                                        stroke: [255, 195, 120, 255],
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
                                    name: "Dawn Sky".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 540.0 },
                                            PathCommand::LineTo { x: 0.0, y: 540.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [245, 175, 130, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [48, 72, 138, 255],
                                                },
                                                GradientStop {
                                                    ratio: 155,
                                                    color: [228, 122, 148, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [255, 212, 132, 255],
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
                                    name: "Harbour Platform".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 432.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 108.0,
                                        fill: [64, 54, 78, 255],
                                        stroke: [255, 220, 145, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 1: SHAPE TWEEN MORPH #6 — Sleeping Crescent -> Dawn Star Crown -> Golden Rising Sun
                Layer {
                    id: "s3-horizon-sun-morph".into(),
                    name: "Crescent to Sunrise Sun Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            }],
                            elements: vec![Element {
                                id: "horizon-celestial".into(),
                                name: "Sleeping Crescent".into(),
                                kind: ElementKind::Path {
                                    commands: crescent_moon_commands(735.0, 255.0, 56.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [215, 232, 255, 245],
                                    stroke: [140, 195, 255, 230],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "horizon-celestial".into(),
                                name: "Rising Dawn Sun".into(),
                                kind: ElementKind::Path {
                                    commands: star_beacon_commands(735.0, 195.0, 88.0, 66.0, 12),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 222, 110, 255],
                                    stroke: [255, 145, 65, 255],
                                    stroke_width: 4.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 2: Arriving Express Train (glides to a gentle stop at Dream Harbour)
                Layer {
                    id: "s3-arrival-train".into(),
                    name: "Harbour Arrival Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            elements: vec![Element {
                                id: "s3-train-inst".into(),
                                name: "Harbour Train".into(),
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
                            frame: 26,
                            elements: vec![Element {
                                id: "s3-train-inst".into(),
                                name: "Harbour Train".into(),
                                transform: Transform {
                                    x: 410.0,
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
                // Layer 3: SHAPE TWEEN MORPH #7 — Chimney Steam Puff -> Glowing Rose Heart -> Morning Star
                Layer {
                    id: "s3-heart-steam-morph".into(),
                    name: "Chimney Heart-Steam Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            }],
                            elements: vec![Element {
                                id: "heart-puff".into(),
                                name: "Chimney Puff".into(),
                                kind: ElementKind::Path {
                                    commands: cloud_puff_commands(260.0, 325.0, 32.0, 22.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [240, 248, 255, 225],
                                    stroke: [185, 225, 255, 210],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 26,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            }],
                            elements: vec![Element {
                                id: "heart-puff".into(),
                                name: "Glowing Heart Puff".into(),
                                kind: ElementKind::Path {
                                    commands: heart_puff_commands(445.0, 255.0, 56.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 125, 175, 245],
                                    stroke: [255, 235, 245, 255],
                                    stroke_width: 3.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "heart-puff".into(),
                                name: "Morning Star Emblem".into(),
                                kind: ElementKind::Path {
                                    commands: star_beacon_commands(455.0, 175.0, 58.0, 22.0, 6),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 242, 150, 250],
                                    stroke: [255, 185, 95, 255],
                                    stroke_width: 3.2,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 4: Closing Nightly Series Title
                Layer {
                    id: "s3-closing-title".into(),
                    name: "Closing Series Title".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            tween_kind: TweenKind::Motion,
                            elements: vec![Element {
                                id: "s3-title".into(),
                                name: "Goodnight Title".into(),
                                transform: Transform {
                                    x: 72.0,
                                    y: 52.0,
                                    alpha: 0.65,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "DREAM HARBOUR DAWN · SEE YOU TOMORROW NIGHT".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 24.0,
                                    color: [255, 244, 195, 255],
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
                                id: "s3-title".into(),
                                name: "Goodnight Title".into(),
                                transform: Transform {
                                    x: 84.0,
                                    y: 44.0,
                                    alpha: 1.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "DREAM HARBOUR DAWN · SEE YOU TOMORROW NIGHT".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 24.0,
                                    color: [255, 244, 195, 255],
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

    p.symbols.push(scene2);
    p.symbols.push(scene3);
    p.scenes = vec![
        scene1_id.clone(),
        "scene-nebula-bridge".into(),
        "scene-harbour-dawn".into(),
    ];
    p.upgrade_format_for_features();
    p.validate().unwrap();

    // Verify all 3 scenes flatten cleanly to 144 frames and evaluate every morphed frame!
    let (flat, ranges) = flatten_scenes_with_ranges(&p);
    flat.validate().unwrap();
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0].start, 0);
    assert_eq!(ranges[1].start, 48);
    assert_eq!(ranges[2].start, 96);
    assert_eq!(flat.symbol(&flat.root).unwrap().timeline.frame_count, 144);

    for f in 0..144 {
        let items = evaluate(&flat, &flat.root, f);
        assert!(
            !items.is_empty(),
            "Flattened frame {f} must produce non-empty draw items"
        );
    }

    // Verify sound playback emits the Stream whistle command on frame 0.
    let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
    let initial_sounds = playback.drain_sound_commands();
    assert_eq!(initial_sounds.len(), 1);

    // Keep Lottie coverage in the ordinary suite. Full media generation is explicit:
    // NEOFLASH_EXPORT_EPISODE3_MEDIA=1 writes MP4/GIF, and
    // NEOFLASH_EXPORT_CARTOON_HTML5=1 uses the built HTML5 runtime.
    // Neither mode rewrites examples/; requested artifacts go to target/artifacts/generated-examples/.
    let output = TestOutputDir::new("midnight-express-3");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let export_media = std::env::var_os("NEOFLASH_EXPORT_EPISODE3_MEDIA").is_some();
    let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
    let out_dir = if export_media || export_html5 {
        repo_root.join("target/artifacts/generated-examples")
    } else {
        output.0.clone()
    };
    fs::create_dir_all(&out_dir).unwrap();

    let lottie_json_path = out_dir.join("midnight-express-3.lottie.json");
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
    assert!(lottie_report.html_preview_path.unwrap().exists());

    // Keep multi-scene video preparation in the normal suite without encoding frames.
    let video_estimate = crate::video::estimate_video(&p, &VideoOptions::default()).unwrap();
    assert_eq!(video_estimate.frame_count, 144);
    assert_eq!(video_estimate.duration_secs, 6.0);

    let mp4_path = out_dir.join("midnight-express-3.mp4");
    let gif_path = out_dir.join("midnight-express-3.gif");
    if export_media {
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
        assert!(mp4_path.exists());
    }
    if export_media {
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
        assert!(gif_path.exists());
    }

    let web_runtime = repo_root.join("target/web-runtime");
    if export_html5 {
        assert!(
            web_runtime.join("neoflash_web_bg.wasm").is_file(),
            "NEOFLASH_EXPORT_CARTOON_HTML5 requires scripts/build-web.sh first"
        );
        let html5_dir = out_dir.join("midnight-express-3-html5");
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
