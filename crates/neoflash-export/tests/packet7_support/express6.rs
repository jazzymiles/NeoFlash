use super::*;
use image::{ImageFormat, Rgba, RgbaImage};
use neoflash_core::{
    BitmapCompression, BitmapExportSettings, BlendMode, Easing, Element, ElementKind, FilterSpec,
    FrameSound, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode,
    PathCommand, Project, ResourceUnit, SoundSync, Symbol, SymbolKind, Timeline, Transform,
    TweenKind, TweenProperty,
};
use std::io::Cursor;

fn make_polar_crest_png(width: u32, height: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(width, height);
    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    let max_r = cx.min(cy) - 2.0;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let star_axis = dx.abs().min(dy.abs()) < 1.6 && r < max_r * 0.72;
            let px = if r > max_r {
                [0, 0, 0, 0]
            } else if r > max_r - 2.8 {
                [52, 211, 153, 255]
            } else if star_axis {
                [254, 249, 195, 255]
            } else if (r - max_r * 0.58).abs() < 2.0 {
                [56, 189, 248, 255]
            } else {
                let glow = (1.0 - r / max_r).clamp(0.0, 1.0);
                [
                    (8.0 + glow * 34.0) as u8,
                    (36.0 + glow * 132.0) as u8,
                    (68.0 + glow * 142.0) as u8,
                    248,
                ]
            };
            img.put_pixel(x, y, Rgba(px));
        }
    }
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)
        .expect("encode polar crest PNG");
    buf.into_inner()
}

fn make_polar_crest_resource(id: &str, path: &str) -> ResourceUnit {
    let bytes = make_polar_crest_png(48, 48);
    let mut unit =
        ResourceUnit::from_bytes(id, path, "image/png", &bytes).expect("polar crest resource");
    unit.bitmap_export = Some(BitmapExportSettings {
        compression: BitmapCompression::LosslessPng,
        use_imported_jpeg_data: true,
        jpeg_quality: 92,
        allow_smoothing: true,
    });
    unit
}

fn build_polar_wheel_symbol() -> Symbol {
    let mut spoke_0 = rect_el(
        "polar_spoke_a",
        "",
        -11.0,
        -2.0,
        22.0,
        4.0,
        [110, 231, 183, 255],
        [0, 0, 0, 0],
        0.0,
    );
    spoke_0.transform.rotation = 0.0;
    let mut spoke_11 = spoke_0.clone();
    spoke_11.transform.rotation = 180.0;

    let mut cross_0 = rect_el(
        "polar_spoke_b",
        "",
        -2.0,
        -11.0,
        4.0,
        22.0,
        [250, 204, 21, 255],
        [0, 0, 0, 0],
        0.0,
    );
    cross_0.transform.rotation = 0.0;
    let mut cross_11 = cross_0.clone();
    cross_11.transform.rotation = 180.0;

    Symbol {
        id: "sym-polar-wheel".into(),
        name: "PolarTurbineWheel".into(),
        folder: "Locomotive".into(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![
                single_key_layer(
                    "pwheel_rim_l",
                    "TurbineRim",
                    vec![ellipse_el(
                        "pwheel_rim",
                        "",
                        -13.0,
                        -13.0,
                        26.0,
                        26.0,
                        [15, 23, 42, 255],
                        [125, 211, 252, 255],
                        2.5,
                    )],
                ),
                Layer {
                    id: "pwheel_spin_l".into(),
                    name: "TurbineSpin".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Linear),
                            elements: vec![spoke_0, cross_0],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![spoke_11, cross_11],
                            ..Keyframe::default()
                        },
                    ],
                    properties: LayerProperties::default(),
                },
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_station_plaque_symbol() -> Symbol {
    let mut sym = Symbol {
        id: "sym-station-plaque".into(),
        name: "PolarStationPlaque".into(),
        folder: "PolarArchitecture".into(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "plaque_frame_l",
                "NineSlicePlaque",
                vec![
                    rect_el(
                        "plaque_outer",
                        "",
                        0.0,
                        0.0,
                        120.0,
                        32.0,
                        [12, 34, 56, 235],
                        [52, 211, 153, 255],
                        2.0,
                    ),
                    rect_el(
                        "plaque_inner",
                        "",
                        6.0,
                        5.0,
                        108.0,
                        22.0,
                        [18, 52, 78, 210],
                        [125, 211, 252, 190],
                        1.0,
                    ),
                ],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    };
    sym.set_scale9_grid(Some([12.0, 6.0, 96.0, 20.0]));
    sym
}

fn build_aurora_locomotive_symbol() -> Symbol {
    let mut searchlight = path_el(
        "aurora_beam",
        "AuroraSearchlight",
        138.0,
        -24.0,
        vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: 175.0, y: -42.0 },
            PathCommand::LineTo { x: 175.0, y: 44.0 },
            PathCommand::Close,
        ],
        [110, 231, 183, 125],
        [0, 0, 0, 0],
        0.0,
    );
    searchlight.blend_mode = BlendMode::Add;
    searchlight.filters = vec![FilterSpec::Glow {
        color: 0x34D399,
        alpha: 0.8,
        blur_x: 14.0,
        blur_y: 14.0,
        strength: 1.7,
        quality: 1,
        inner: false,
        knockout: false,
    }];

    let mut body_elements = vec![
        // Polar Observation Dome Coach
        rect_el(
            "obs_coach_hull",
            "",
            -184.0,
            -44.0,
            148.0,
            42.0,
            [15, 35, 66, 255],
            [52, 211, 153, 255],
            2.0,
        ),
        path_el(
            "obs_glass_dome",
            "",
            -170.0,
            -44.0,
            vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::CubicTo {
                    cx1: 18.0,
                    cy1: -22.0,
                    cx2: 102.0,
                    cy2: -22.0,
                    x: 120.0,
                    y: 0.0,
                },
                PathCommand::Close,
            ],
            [125, 211, 252, 145],
            [186, 230, 253, 235],
            1.5,
        ),
        rect_el(
            "obs_win_1",
            "",
            -168.0,
            -34.0,
            26.0,
            16.0,
            [254, 240, 138, 255],
            [245, 158, 11, 255],
            1.0,
        ),
        rect_el(
            "obs_win_2",
            "",
            -132.0,
            -34.0,
            26.0,
            16.0,
            [254, 240, 138, 255],
            [245, 158, 11, 255],
            1.0,
        ),
        rect_el(
            "obs_win_3",
            "",
            -96.0,
            -34.0,
            26.0,
            16.0,
            [167, 243, 208, 255],
            [52, 211, 153, 255],
            1.0,
        ),
        rect_el(
            "obs_win_4",
            "",
            -62.0,
            -34.0,
            18.0,
            16.0,
            [125, 211, 252, 255],
            [56, 189, 248, 255],
            1.0,
        ),
        rect_el(
            "polar_coupler",
            "",
            -36.0,
            -14.0,
            16.0,
            6.0,
            [148, 163, 184, 255],
            [0, 0, 0, 0],
            0.0,
        ),
        // Art-Deco Glacier Cab & Streamlined Boiler
        rect_el(
            "loco6_cab",
            "",
            -20.0,
            -58.0,
            58.0,
            56.0,
            [17, 24, 62, 255],
            [52, 211, 153, 255],
            2.0,
        ),
        rect_el(
            "loco6_cab_win",
            "",
            -6.0,
            -46.0,
            30.0,
            20.0,
            [254, 249, 195, 255],
            [56, 189, 248, 255],
            1.2,
        ),
        rect_el(
            "loco6_boiler",
            "",
            38.0,
            -46.0,
            96.0,
            44.0,
            [11, 27, 48, 255],
            [125, 211, 252, 255],
            2.0,
        ),
        rect_el(
            "loco6_stripe",
            "",
            42.0,
            -26.0,
            88.0,
            5.0,
            [52, 211, 153, 255],
            [0, 0, 0, 0],
            0.0,
        ),
        rect_el(
            "loco6_stack",
            "",
            104.0,
            -66.0,
            16.0,
            20.0,
            [30, 58, 95, 255],
            [110, 231, 183, 255],
            1.5,
        ),
        path_el(
            "loco6_prow",
            "",
            134.0,
            -46.0,
            vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 24.0, y: 44.0 },
                PathCommand::LineTo { x: 0.0, y: 44.0 },
                PathCommand::Close,
            ],
            [52, 211, 153, 255],
            [236, 253, 245, 255],
            1.2,
        ),
        ellipse_el(
            "loco6_lamp",
            "",
            128.0,
            -32.0,
            12.0,
            12.0,
            [236, 253, 245, 255],
            [52, 211, 153, 255],
            1.5,
        ),
        searchlight,
    ];

    for (idx, wx) in [-152.0, -74.0, 8.0, 64.0, 110.0].into_iter().enumerate() {
        body_elements.push(instance_el(
            &format!("loco6_w_{idx}"),
            "",
            "sym-polar-wheel",
            wx,
            0.0,
            InstancePlayback::Loop,
        ));
    }

    let frost_a = ellipse_el(
        "frost_plume",
        "",
        98.0,
        -82.0,
        24.0,
        15.0,
        [209, 250, 229, 200],
        [0, 0, 0, 0],
        0.0,
    );
    let mut frost_b = ellipse_el(
        "frost_plume",
        "",
        64.0,
        -114.0,
        46.0,
        26.0,
        [125, 211, 252, 75],
        [0, 0, 0, 0],
        0.0,
    );
    frost_b.transform.alpha = 0.4;

    Symbol {
        id: "sym-aurora-locomotive".into(),
        name: "Midnight Aurora Locomotive Mk.VI".into(),
        folder: "Locomotive".into(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![
                Layer {
                    id: "loco6_frost_l".into(),
                    name: "FrostCrystalPlume".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            elements: vec![frost_a],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![frost_b],
                            ..Keyframe::default()
                        },
                    ],
                    properties: LayerProperties::default(),
                },
                single_key_layer("loco6_body_l", "PolarChassisAndDomeCoach", body_elements),
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_aurora_beacon_symbol() -> Symbol {
    let mut core_0 = radial_glow_el(
        "beacon_orb",
        "",
        0.0,
        0.0,
        28.0,
        [236, 253, 245, 255],
        [52, 211, 153, 195],
        [56, 189, 248, 0],
    );
    core_0.filters = vec![FilterSpec::Glow {
        color: 0x6EE7B7,
        alpha: 0.85,
        blur_x: 11.0,
        blur_y: 11.0,
        strength: 1.8,
        quality: 1,
        inner: false,
        knockout: false,
    }];
    let mut core_mid = core_0.clone();
    core_mid.transform.scale_x = 1.28;
    core_mid.transform.scale_y = 1.28;
    let core_end = core_0.clone();

    Symbol {
        id: "sym-aurora-beacon".into(),
        name: "Polaris Starlight Beacon".into(),
        folder: "PolarSky".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 24,
            layers: vec![Layer {
                id: "beacon_pulse_l".into(),
                name: "AuroraPulse".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        elements: vec![core_0],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 11,
                        tween: Some(Easing::EaseInOut),
                        elements: vec![core_mid],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 23,
                        elements: vec![core_end],
                        ..Keyframe::default()
                    },
                ],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn polar_starfield_elements(prefix: &str, count: usize) -> Vec<Element> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let fx = ((i * 197 + 43) % 930) as f64 + 15.0;
        let fy = ((i * 113 + 29) % 270) as f64 + 14.0;
        let sz = if i % 5 == 0 {
            3.8
        } else if i % 2 == 0 {
            2.6
        } else {
            1.8
        };
        let col = match i % 4 {
            0 => [236, 253, 245, 245],
            1 => [167, 243, 208, 220],
            2 => [186, 230, 253, 225],
            _ => [254, 249, 195, 210],
        };
        out.push(ellipse_el(
            &format!("{prefix}_star_{i}"),
            "",
            fx,
            fy,
            sz,
            sz,
            col,
            [0, 0, 0, 0],
            0.0,
        ));
    }
    out
}

fn build_scene_1() -> Symbol {
    let mut backdrop = vec![gradient_rect_el(
        "s1_polar_sky",
        "PolarNightSky",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [4, 12, 28, 255]),
            (135, [9, 36, 60, 255]),
            (255, [15, 70, 86, 255]),
        ],
        true,
    )];
    backdrop.extend(polar_starfield_elements("s1", 34));

    // Crystalline Glacier Range + Polar Terminal
    backdrop.push(path_el(
        "s1_glacier_far",
        "GlacierRange",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 0.0, y: 438.0 },
            PathCommand::LineTo { x: 118.0, y: 274.0 },
            PathCommand::LineTo { x: 236.0, y: 356.0 },
            PathCommand::LineTo { x: 392.0, y: 222.0 },
            PathCommand::LineTo { x: 548.0, y: 344.0 },
            PathCommand::LineTo { x: 716.0, y: 238.0 },
            PathCommand::LineTo { x: 868.0, y: 328.0 },
            PathCommand::LineTo { x: 960.0, y: 268.0 },
            PathCommand::LineTo { x: 960.0, y: 438.0 },
            PathCommand::Close,
        ],
        [14, 48, 76, 230],
        [125, 211, 252, 140],
        1.5,
    ));
    backdrop.push(rect_el(
        "s1_terminal_hall",
        "GlacierTerminal",
        56.0,
        286.0,
        264.0,
        150.0,
        [12, 31, 56, 245],
        [52, 211, 153, 235],
        2.0,
    ));
    let mut plaque_inst = instance_el(
        "s1_terminal_plaque",
        "terminalPlaque",
        "sym-station-plaque",
        80.0,
        298.0,
        InstancePlayback::Loop,
    );
    plaque_inst.transform.scale_x = 1.8;
    backdrop.push(plaque_inst);
    backdrop.push(text_el(
        "s1_terminal_label",
        "",
        96.0,
        306.0,
        "POLAR GLACIER TERMINAL",
        12.0,
        [209, 250, 229, 255],
    ));
    backdrop.push(rect_el(
        "s1_snow_deck",
        "SnowBallast",
        0.0,
        436.0,
        960.0,
        104.0,
        [9, 22, 42, 255],
        [56, 189, 248, 220],
        2.0,
    ));
    backdrop.push(rect_el(
        "s1_polar_rail",
        "EmeraldRail",
        0.0,
        433.0,
        960.0,
        4.0,
        [110, 231, 183, 255],
        [0, 0, 0, 0],
        0.0,
    ));

    // Geometry Mask Layer clipping the shimmering Aurora Borealis curtain
    let aurora_mask_layer = Layer {
        id: "s1_aurora_mask".into(),
        name: "AuroraCurtainMask".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![path_el(
                "s1_mask_wave",
                "",
                0.0,
                0.0,
                vec![
                    PathCommand::MoveTo { x: 80.0, y: 196.0 },
                    PathCommand::CubicTo {
                        cx1: 280.0,
                        cy1: 52.0,
                        cx2: 580.0,
                        cy2: 248.0,
                        x: 910.0,
                        y: 78.0,
                    },
                    PathCommand::LineTo { x: 910.0, y: 238.0 },
                    PathCommand::CubicTo {
                        cx1: 590.0,
                        cy1: 318.0,
                        cx2: 290.0,
                        cy2: 142.0,
                        x: 80.0,
                        y: 266.0,
                    },
                    PathCommand::Close,
                ],
                [255, 255, 255, 255],
                [0, 0, 0, 0],
                0.0,
            )],
            ..Keyframe::default()
        }],
        properties: LayerProperties {
            role: LayerRole::Mask,
            mask_mode: MaskMode::Geometry,
            ..LayerProperties::default()
        },
    };

    // Shape-tweened Aurora Ribbon underneath the geometry mask
    let ribbon_a = gradient_rect_el(
        "s1_aurora_grad",
        "",
        60.0,
        40.0,
        860.0,
        260.0,
        vec![
            (0, [52, 211, 153, 185]),
            (130, [56, 189, 248, 155]),
            (255, [192, 132, 252, 90]),
        ],
        false,
    );
    let ribbon_b = gradient_rect_el(
        "s1_aurora_grad",
        "",
        60.0,
        56.0,
        860.0,
        260.0,
        vec![
            (0, [110, 231, 183, 195]),
            (120, [45, 212, 191, 165]),
            (255, [244, 114, 182, 105]),
        ],
        false,
    );
    let masked_aurora_layer = Layer {
        id: "s1_aurora_fill".into(),
        name: "AuroraCurtainRibbon".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Shape,
                elements: vec![ribbon_a],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![ribbon_b],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties {
            mask: Some("s1_aurora_mask".into()),
            ..LayerProperties::default()
        },
    };

    // Motion-tweened Polaris Starlight Beacon with mid-span ◆ property keyframe
    let beacon_start = instance_el(
        "s1_beacon",
        "polarisBeacon",
        "sym-aurora-beacon",
        130.0,
        186.0,
        InstancePlayback::Independent,
    );
    let mut beacon_mid = beacon_start.clone();
    beacon_mid.transform = Transform {
        x: 490.0,
        y: 96.0,
        scale_x: 1.25,
        scale_y: 1.25,
        ..Transform::default()
    };
    let mut beacon_end = beacon_start.clone();
    beacon_end.transform = Transform {
        x: 860.0,
        y: 124.0,
        scale_x: 1.05,
        scale_y: 1.05,
        ..Transform::default()
    };
    let mut beacon_layer = Layer {
        id: "s1_beacon_l".into(),
        name: "PolarisBeaconMotion".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Motion,
                motion_path: Some(vec![
                    PathCommand::MoveTo { x: 130.0, y: 186.0 },
                    PathCommand::CubicTo {
                        cx1: 320.0,
                        cy1: 68.0,
                        cx2: 610.0,
                        cy2: 196.0,
                        x: 860.0,
                        y: 124.0,
                    },
                ]),
                elements: vec![beacon_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 29,
                keyed_properties: Some(vec![
                    TweenProperty::X,
                    TweenProperty::Y,
                    TweenProperty::ScaleX,
                    TweenProperty::ScaleY,
                ]),
                elements: vec![beacon_mid],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                keyed_properties: Some(vec![
                    TweenProperty::X,
                    TweenProperty::Y,
                    TweenProperty::ScaleX,
                    TweenProperty::ScaleY,
                ]),
                elements: vec![beacon_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };
    beacon_layer.normalize_motion_keys();

    // Classic-tweened Midnight Aurora Locomotive Mk.VI departure
    let train_start = instance_el(
        "s1_train6",
        "auroraTrain",
        "sym-aurora-locomotive",
        -120.0,
        432.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(775.0, 432.0);
    let train_layer = Layer {
        id: "s1_train_l".into(),
        name: "AuroraExpressDeparture".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "polar_departure".into(),
                tween: Some(Easing::EaseInOut),
                elements: vec![train_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![train_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let mut title_card = vec![
        Element {
            id: "s1_polar_crest".into(),
            name: "polarCrest".into(),
            transform: tf(36.0, 26.0),
            kind: ElementKind::Bitmap {
                resource_id: "res-polar-crest".into(),
                width: 48,
                height: 48,
                smoothing: true,
            },
            ..Element::default()
        },
        text_el(
            "s1_eyebrow6",
            "",
            96.0,
            30.0,
            "MIDNIGHT EXPRESS · EPISODE VI",
            13.0,
            [110, 231, 183, 255],
        ),
        text_el(
            "s1_title6",
            "",
            96.0,
            50.0,
            "The Aurora Borealis & Polar Starlight Viaduct",
            22.0,
            [248, 250, 252, 255],
        ),
    ];
    title_card[2].filters = vec![FilterSpec::DropShadow {
        distance: 3.0,
        angle: 45.0,
        color: 0x020617,
        alpha: 0.85,
        blur_x: 6.0,
        blur_y: 6.0,
        strength: 1.2,
        quality: 1,
        inner: false,
        knockout: false,
        hide_object: false,
    }];

    let mut snd = FrameSound::new("res-audio-s1");
    snd.sync = SoundSync::Stream;
    let sound_layer = Layer {
        id: "s1_audio_l".into(),
        name: "PolarDepartureStream".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            sound: Some(snd),
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    };

    Symbol {
        id: "scene-1".into(),
        name: "Scene 1 · Polar Glacier Terminal & Aurora Curtain".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            layers: vec![
                single_key_layer("s1_bg_l", "PolarGlacierBackdrop", backdrop),
                aurora_mask_layer,
                masked_aurora_layer,
                beacon_layer,
                train_layer,
                single_key_layer("s1_hud_l", "EpisodeTitleAndPolarCrest", title_card),
                sound_layer,
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_scene_2() -> Symbol {
    let mut backdrop = vec![gradient_rect_el(
        "s2_fjord_sky",
        "StarlightFjordSky",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [3, 10, 26, 255]),
            (145, [12, 46, 78, 255]),
            (255, [20, 96, 108, 255]),
        ],
        true,
    )];
    backdrop.extend(polar_starfield_elements("s2", 30));
    backdrop.push(radial_glow_el(
        "s2_polaris_star",
        "PolarisCorona",
        760.0,
        108.0,
        92.0,
        [236, 253, 245, 240],
        [52, 211, 153, 130],
        [56, 189, 248, 0],
    ));
    // Frozen Fjord water mirror at bottom
    backdrop.push(rect_el(
        "s2_fjord_water",
        "FrozenFjordMirror",
        0.0,
        444.0,
        960.0,
        96.0,
        [8, 33, 58, 235],
        [52, 211, 153, 165],
        1.5,
    ));

    // Twin Ice-Arch Viaduct deck with emerald glow
    let mut ice_arch = path_el(
        "s2_ice_viaduct",
        "StarlightIceViaduct",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: -60.0, y: 420.0 },
            PathCommand::CubicTo {
                cx1: 270.0,
                cy1: 236.0,
                cx2: 650.0,
                cy2: 222.0,
                x: 1020.0,
                y: 328.0,
            },
        ],
        [0, 0, 0, 0],
        [110, 231, 183, 240],
        5.5,
    );
    ice_arch.filters = vec![FilterSpec::Glow {
        color: 0x38BDF8,
        alpha: 0.75,
        blur_x: 10.0,
        blur_y: 10.0,
        strength: 1.6,
        quality: 1,
        inner: false,
        knockout: false,
    }];
    backdrop.push(ice_arch);

    // Shape-tweened aurora reflection shimmer on the fjord
    let shimmer_a = path_el(
        "s2_fjord_shimmer",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 120.0, y: 468.0 },
            PathCommand::LineTo { x: 480.0, y: 458.0 },
            PathCommand::LineTo { x: 540.0, y: 482.0 },
            PathCommand::LineTo { x: 90.0, y: 486.0 },
            PathCommand::Close,
        ],
        [52, 211, 153, 95],
        [0, 0, 0, 0],
        0.0,
    );
    let shimmer_b = path_el(
        "s2_fjord_shimmer",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 340.0, y: 462.0 },
            PathCommand::LineTo { x: 820.0, y: 454.0 },
            PathCommand::LineTo { x: 870.0, y: 480.0 },
            PathCommand::LineTo { x: 310.0, y: 484.0 },
            PathCommand::Close,
        ],
        [56, 189, 248, 85],
        [0, 0, 0, 0],
        0.0,
    );
    let shimmer_layer = Layer {
        id: "s2_shimmer_l".into(),
        name: "FjordAuroraReflection".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Shape,
                elements: vec![shimmer_a],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![shimmer_b],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    // Motion Guide Layer for guided Classic tween with orient_to_path
    let guide_curve = vec![
        PathCommand::MoveTo { x: -80.0, y: 416.0 },
        PathCommand::CubicTo {
            cx1: 270.0,
            cy1: 232.0,
            cx2: 650.0,
            cy2: 218.0,
            x: 1020.0,
            y: 324.0,
        },
    ];
    let guide_layer = Layer {
        id: "s2_viaduct_guide".into(),
        name: "ViaductAuroraGuide".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![path_el(
                "s2_guide_stroke6",
                "",
                0.0,
                0.0,
                guide_curve,
                [0, 0, 0, 0],
                [52, 211, 153, 255],
                1.5,
            )],
            ..Keyframe::default()
        }],
        properties: LayerProperties {
            role: LayerRole::Guide,
            ..LayerProperties::default()
        },
    };

    let train_start = instance_el(
        "s2_train6",
        "viaductAuroraTrain",
        "sym-aurora-locomotive",
        -80.0,
        416.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(1020.0, 324.0);

    let guided_train_layer = Layer {
        id: "s2_train_l".into(),
        name: "GuidedAuroraExpress".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "ice_arch_viaduct".into(),
                tween: Some(Easing::CubicBezier {
                    x1: 0.30,
                    y1: 0.06,
                    x2: 0.68,
                    y2: 0.95,
                }),
                orient_to_path: true,
                snap_to_guide: true,
                elements: vec![train_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![train_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties {
            motion_guide: Some("s2_viaduct_guide".into()),
            ..LayerProperties::default()
        },
    };

    let mut escort_start = instance_el(
        "s2_escort",
        "polarisEscort",
        "sym-aurora-beacon",
        50.0,
        148.0,
        InstancePlayback::Independent,
    );
    escort_start.filters = vec![FilterSpec::Blur {
        blur_x: 4.0,
        blur_y: 1.5,
        quality: 1,
    }];
    let mut escort_end = escort_start.clone();
    escort_end.transform = tf(910.0, 96.0);
    let escort_layer = Layer {
        id: "s2_escort_l".into(),
        name: "PolarisEscortSweep".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseOut),
                elements: vec![escort_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![escort_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let mut snd = FrameSound::new("res-audio-s2");
    snd.sync = SoundSync::Stream;
    let sound_layer = Layer {
        id: "s2_audio_l".into(),
        name: "StarlightViaductStream".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            sound: Some(snd),
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    };

    Symbol {
        id: "scene-2".into(),
        name: "Scene 2 · Starlight Fjord Ice-Arch Viaduct".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            layers: vec![
                single_key_layer("s2_bg_l", "FjordViaductBackdrop", backdrop),
                shimmer_layer,
                guide_layer,
                guided_train_layer,
                escort_layer,
                single_key_layer(
                    "s2_caption_l",
                    "SceneCaption",
                    vec![text_el(
                        "s2_cap6",
                        "",
                        36.0,
                        32.0,
                        "SCENE II · STARLIGHT FJORD ICE-ARCH VIADUCT",
                        15.0,
                        [167, 243, 208, 255],
                    )],
                ),
                sound_layer,
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_scene_3() -> Symbol {
    let sky_a = gradient_rect_el(
        "s3_polar_dawn",
        "",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [6, 22, 48, 255]),
            (145, [16, 94, 102, 255]),
            (255, [88, 62, 132, 255]),
        ],
        true,
    );
    let sky_b = gradient_rect_el(
        "s3_polar_dawn",
        "",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [18, 44, 88, 255]),
            (145, [236, 122, 146, 255]),
            (255, [253, 224, 132, 255]),
        ],
        true,
    );
    let sky_layer = Layer {
        id: "s3_sky_l".into(),
        name: "BorealisDawnSkyMorph".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Shape,
                elements: vec![sky_a],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![sky_b],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let observatory_elements = vec![
        radial_glow_el(
            "s3_polar_sun",
            "PolarDawnSun",
            748.0,
            286.0,
            128.0,
            [254, 249, 195, 245],
            [244, 114, 182, 145],
            [56, 189, 248, 0],
        ),
        rect_el(
            "s3_mirror_ice",
            "MirrorIceBasin",
            0.0,
            382.0,
            960.0,
            54.0,
            [22, 56, 88, 215],
            [167, 243, 208, 165],
            1.0,
        ),
        // Geodesic Borealis Crown Observatory Dome
        path_el(
            "s3_crown_dome",
            "BorealisCrownDome",
            450.0,
            436.0,
            vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::CubicTo {
                    cx1: 16.0,
                    cy1: -186.0,
                    cx2: 264.0,
                    cy2: -186.0,
                    x: 280.0,
                    y: 0.0,
                },
                PathCommand::Close,
            ],
            [18, 48, 78, 225],
            [110, 231, 183, 245],
            2.5,
        ),
        rect_el(
            "s3_spire_left",
            "NorthSpire",
            422.0,
            214.0,
            22.0,
            222.0,
            [15, 35, 66, 255],
            [125, 211, 252, 245],
            2.0,
        ),
        rect_el(
            "s3_spire_right",
            "SouthSpire",
            736.0,
            214.0,
            22.0,
            222.0,
            [15, 35, 66, 255],
            [125, 211, 252, 245],
            2.0,
        ),
        rect_el(
            "s3_canopy6",
            "AuroraGlassCanopy",
            280.0,
            304.0,
            480.0,
            20.0,
            [52, 211, 153, 155],
            [254, 249, 195, 245],
            2.0,
        ),
        rect_el(
            "s3_platform6",
            "ObservatoryPlatform",
            0.0,
            436.0,
            960.0,
            104.0,
            [11, 24, 44, 255],
            [110, 231, 183, 255],
            2.0,
        ),
    ];

    let train_start = instance_el(
        "s3_train6",
        "arrivalAuroraTrain",
        "sym-aurora-locomotive",
        -110.0,
        432.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(405.0, 432.0);
    let train_layer = Layer {
        id: "s3_train_l".into(),
        name: "BorealisCrownArrival".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "borealis_crown_arrival".into(),
                tween: Some(Easing::EaseOut),
                elements: vec![train_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![train_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let mut banner_start = text_el(
        "s3_finale_txt6",
        "",
        36.0,
        34.0,
        "FINALE · POLAR DAWN AT BOREALIS CROWN OBSERVATORY",
        20.0,
        [236, 253, 245, 255],
    );
    banner_start.transform.alpha = 0.15;
    let mut banner_end = banner_start.clone();
    banner_end.transform.alpha = 1.0;

    let finale_layer = Layer {
        id: "s3_finale_l".into(),
        name: "FinaleBanner".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                elements: vec![banner_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![banner_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let mut snd = FrameSound::new("res-audio-s3");
    snd.sync = SoundSync::Stream;
    let sound_layer = Layer {
        id: "s3_audio_l".into(),
        name: "PolarDawnChimeStream".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            sound: Some(snd),
            ..Keyframe::default()
        }],
        properties: LayerProperties::default(),
    };

    Symbol {
        id: "scene-3".into(),
        name: "Scene 3 · Borealis Crown Observatory & Polar Dawn".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            layers: vec![
                sky_layer,
                single_key_layer(
                    "s3_arch_l",
                    "BorealisCrownArchitecture",
                    observatory_elements,
                ),
                train_layer,
                finale_layer,
                sound_layer,
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

pub fn build_midnight_express_6() -> Project {
    let wav_s1 = make_harmonic_stereo_wav(2500, &[293.66, 349.23, 440.00, 659.25], true);
    let wav_s2 = make_harmonic_stereo_wav(2500, &[329.63, 392.00, 493.88, 739.99], true);
    let wav_s3 = make_harmonic_stereo_wav(2500, &[349.23, 440.00, 523.25, 880.00], false);

    let resources = vec![
        make_polar_crest_resource("res-polar-crest", "images/polar-aurora-crest.png"),
        ResourceUnit::from_bytes(
            "res-audio-s1",
            "audio/scene1-polar-departure.wav",
            "audio/wav",
            &wav_s1,
        )
        .expect("s1 wav"),
        ResourceUnit::from_bytes(
            "res-audio-s2",
            "audio/scene2-ice-viaduct.wav",
            "audio/wav",
            &wav_s2,
        )
        .expect("s2 wav"),
        ResourceUnit::from_bytes(
            "res-audio-s3",
            "audio/scene3-polar-dawn.wav",
            "audio/wav",
            &wav_s3,
        )
        .expect("s3 wav"),
    ];

    let mut project = Project {
        format_version: 9,
        name: "Midnight Express 6 — The Aurora Borealis & Polar Starlight Viaduct".into(),
        width: 960,
        height: 540,
        fps: 24,
        background: [4, 12, 28, 255],
        root: "scene-1".into(),
        symbols: vec![
            build_polar_wheel_symbol(),
            build_station_plaque_symbol(),
            build_aurora_locomotive_symbol(),
            build_aurora_beacon_symbol(),
            build_scene_1(),
            build_scene_2(),
            build_scene_3(),
        ],
        library_folders: vec![
            "Locomotive".into(),
            "PolarArchitecture".into(),
            "PolarSky".into(),
        ],
        sources: Vec::new(),
        resources,
        document_class: None,
        scenes: vec!["scene-1".into(), "scene-2".into(), "scene-3".into()],
    };
    project.upgrade_format_for_features();
    project
}
