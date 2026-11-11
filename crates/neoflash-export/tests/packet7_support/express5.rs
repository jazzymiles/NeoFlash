use super::*;
use neoflash_core::{
    BlendMode, Easing, Element, ElementKind, FilterSpec, FrameSound, InstancePlayback, Keyframe,
    Layer, LayerProperties, LayerRole, MaskMode, PathCommand, Project, ResourceUnit, SoundSync,
    Symbol, SymbolKind, Timeline, Transform, TweenKind,
};

fn build_wheel_symbol() -> Symbol {
    let mut kf0_spoke = rect_el(
        "wheel_spoke_a",
        "",
        -11.0,
        -2.0,
        22.0,
        4.0,
        [250, 204, 21, 255],
        [0, 0, 0, 0],
        0.0,
    );
    kf0_spoke.transform.rotation = 0.0;
    let mut kf11_spoke = kf0_spoke.clone();
    kf11_spoke.transform.rotation = 180.0;

    Symbol {
        id: "sym-wheel".into(),
        name: "ExpressDriveWheel".into(),
        folder: "Locomotive".into(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![
                single_key_layer(
                    "wheel_rim_l",
                    "Rim",
                    vec![ellipse_el(
                        "wheel_rim",
                        "",
                        -13.0,
                        -13.0,
                        26.0,
                        26.0,
                        [30, 41, 59, 255],
                        [248, 250, 252, 255],
                        2.5,
                    )],
                ),
                Layer {
                    id: "wheel_spoke_l".into(),
                    name: "SpokeSpin".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Linear),
                            elements: vec![kf0_spoke],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![kf11_spoke],
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

fn build_locomotive_symbol() -> Symbol {
    let mut headlight = path_el(
        "loco_beam",
        "HeadlightBeam",
        136.0,
        -24.0,
        vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: 165.0, y: -38.0 },
            PathCommand::LineTo { x: 165.0, y: 42.0 },
            PathCommand::Close,
        ],
        [254, 240, 138, 120],
        [0, 0, 0, 0],
        0.0,
    );
    headlight.blend_mode = BlendMode::Add;
    headlight.filters = vec![FilterSpec::Glow {
        color: 0x38BDF8,
        alpha: 0.75,
        blur_x: 12.0,
        blur_y: 12.0,
        strength: 1.6,
        quality: 1,
        inner: false,
        knockout: false,
    }];

    let mut body_elements = vec![
        // Passenger coach
        rect_el(
            "coach_body",
            "",
            -178.0,
            -46.0,
            142.0,
            44.0,
            [23, 37, 84, 255],
            [250, 204, 21, 255],
            2.0,
        ),
        rect_el(
            "coach_roof",
            "",
            -182.0,
            -54.0,
            150.0,
            9.0,
            [51, 65, 85, 255],
            [148, 163, 184, 255],
            1.2,
        ),
        // Glowing coach windows
        rect_el(
            "win_1",
            "",
            -164.0,
            -36.0,
            24.0,
            16.0,
            [254, 240, 138, 255],
            [245, 158, 11, 255],
            1.0,
        ),
        rect_el(
            "win_2",
            "",
            -130.0,
            -36.0,
            24.0,
            16.0,
            [254, 240, 138, 255],
            [245, 158, 11, 255],
            1.0,
        ),
        rect_el(
            "win_3",
            "",
            -96.0,
            -36.0,
            24.0,
            16.0,
            [254, 240, 138, 255],
            [245, 158, 11, 255],
            1.0,
        ),
        rect_el(
            "win_4",
            "",
            -64.0,
            -36.0,
            20.0,
            16.0,
            [125, 211, 252, 255],
            [56, 189, 248, 255],
            1.0,
        ),
        // Coupler
        rect_el(
            "coupler",
            "",
            -36.0,
            -14.0,
            16.0,
            6.0,
            [148, 163, 184, 255],
            [0, 0, 0, 0],
            0.0,
        ),
        // Cab & Boiler
        rect_el(
            "loco_cab",
            "",
            -20.0,
            -58.0,
            58.0,
            56.0,
            [30, 27, 75, 255],
            [250, 204, 21, 255],
            2.0,
        ),
        rect_el(
            "cab_win",
            "",
            -6.0,
            -46.0,
            30.0,
            20.0,
            [254, 243, 199, 255],
            [245, 158, 11, 255],
            1.2,
        ),
        rect_el(
            "loco_boiler",
            "",
            38.0,
            -44.0,
            94.0,
            42.0,
            [15, 23, 42, 255],
            [56, 189, 248, 255],
            2.0,
        ),
        // Smokestack & cowcatcher
        rect_el(
            "loco_stack",
            "",
            102.0,
            -66.0,
            16.0,
            22.0,
            [51, 65, 85, 255],
            [250, 204, 21, 255],
            1.5,
        ),
        path_el(
            "loco_nose",
            "",
            132.0,
            -44.0,
            vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 20.0, y: 42.0 },
                PathCommand::LineTo { x: 0.0, y: 42.0 },
                PathCommand::Close,
            ],
            [250, 204, 21, 255],
            [254, 249, 195, 255],
            1.2,
        ),
        ellipse_el(
            "loco_lamp",
            "",
            126.0,
            -32.0,
            12.0,
            12.0,
            [254, 249, 195, 255],
            [56, 189, 248, 255],
            1.5,
        ),
        headlight,
    ];

    for (idx, wx) in [-148.0, -72.0, 8.0, 62.0, 108.0].into_iter().enumerate() {
        body_elements.push(instance_el(
            &format!("loco_w_{idx}"),
            "",
            "sym-wheel",
            wx,
            0.0,
            InstancePlayback::Loop,
        ));
    }

    let puff_a = ellipse_el(
        "steam_puff",
        "",
        96.0,
        -82.0,
        24.0,
        16.0,
        [224, 242, 254, 190],
        [0, 0, 0, 0],
        0.0,
    );
    let mut puff_b = ellipse_el(
        "steam_puff",
        "",
        68.0,
        -112.0,
        44.0,
        26.0,
        [186, 230, 253, 70],
        [0, 0, 0, 0],
        0.0,
    );
    puff_b.transform.alpha = 0.45;

    Symbol {
        id: "sym-locomotive".into(),
        name: "Midnight Locomotive Mk.V".into(),
        folder: "Locomotive".into(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 12,
            layers: vec![
                Layer {
                    id: "loco_steam_l".into(),
                    name: "SteamPlume".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            elements: vec![puff_a],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 11,
                            elements: vec![puff_b],
                            ..Keyframe::default()
                        },
                    ],
                    properties: LayerProperties::default(),
                },
                single_key_layer("loco_body_l", "ChassisAndCoach", body_elements),
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn build_lantern_symbol() -> Symbol {
    let mut core = radial_glow_el(
        "lantern_glow",
        "",
        0.0,
        0.0,
        28.0,
        [254, 249, 195, 255],
        [56, 189, 248, 190],
        [56, 189, 248, 0],
    );
    core.filters = vec![FilterSpec::Glow {
        color: 0x7DD3FC,
        alpha: 0.85,
        blur_x: 10.0,
        blur_y: 10.0,
        strength: 1.8,
        quality: 1,
        inner: false,
        knockout: false,
    }];

    Symbol {
        id: "sym-lantern".into(),
        name: "Comet Escort Lantern".into(),
        folder: "Celestial".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![single_key_layer(
                "lantern_l",
                "Core",
                vec![
                    core,
                    path_el(
                        "lantern_tail",
                        "",
                        0.0,
                        0.0,
                        vec![
                            PathCommand::MoveTo { x: -8.0, y: -5.0 },
                            PathCommand::LineTo { x: -68.0, y: 0.0 },
                            PathCommand::LineTo { x: -8.0, y: 5.0 },
                            PathCommand::Close,
                        ],
                        [125, 211, 252, 165],
                        [0, 0, 0, 0],
                        0.0,
                    ),
                ],
            )],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    }
}

fn starfield_elements(prefix: &str, count: usize) -> Vec<Element> {
    let mut stars = Vec::with_capacity(count);
    for i in 0..count {
        let x = 28.0 + ((i * 137) % 904) as f64;
        let y = 18.0 + ((i * 79) % 270) as f64;
        let sz = if i % 4 == 0 { 4.5 } else { 2.5 };
        let col = if i % 3 == 0 {
            [254, 249, 195, 235]
        } else {
            [186, 230, 253, 210]
        };
        stars.push(ellipse_el(
            &format!("{prefix}_star_{i}"),
            "",
            x,
            y,
            sz,
            sz,
            col,
            [0, 0, 0, 0],
            0.0,
        ));
    }
    stars
}

fn build_scene_1() -> Symbol {
    let mut backdrop = vec![gradient_rect_el(
        "s1_sky",
        "EclipseSky",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [7, 11, 30, 255]),
            (145, [34, 24, 78, 255]),
            (255, [118, 52, 92, 255]),
        ],
        true,
    )];
    backdrop.extend(starfield_elements("s1", 24));
    // Eclipse sun & corona
    backdrop.push(radial_glow_el(
        "s1_corona",
        "EclipseCorona",
        720.0,
        132.0,
        96.0,
        [254, 240, 138, 240],
        [244, 114, 182, 130],
        [34, 24, 78, 0],
    ));
    backdrop.push(ellipse_el(
        "s1_moon_shadow",
        "EclipseDisc",
        684.0,
        96.0,
        72.0,
        72.0,
        [10, 14, 34, 255],
        [250, 204, 21, 255],
        2.5,
    ));
    // Distant ridge + Celestial Observatory Dome + Platform
    backdrop.push(path_el(
        "s1_ridge",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 0.0, y: 420.0 },
            PathCommand::LineTo { x: 220.0, y: 318.0 },
            PathCommand::LineTo { x: 470.0, y: 372.0 },
            PathCommand::LineTo { x: 740.0, y: 304.0 },
            PathCommand::LineTo { x: 960.0, y: 368.0 },
            PathCommand::LineTo { x: 960.0, y: 540.0 },
            PathCommand::LineTo { x: 0.0, y: 540.0 },
            PathCommand::Close,
        ],
        [17, 24, 48, 255],
        [0, 0, 0, 0],
        0.0,
    ));
    backdrop.push(ellipse_el(
        "s1_obs_dome",
        "ObservatoryDome",
        132.0,
        236.0,
        176.0,
        140.0,
        [30, 41, 72, 255],
        [250, 204, 21, 255],
        2.5,
    ));
    backdrop.push(rect_el(
        "s1_obs_base",
        "ObservatoryHall",
        112.0,
        312.0,
        216.0,
        124.0,
        [23, 31, 58, 255],
        [148, 163, 184, 255],
        2.0,
    ));
    backdrop.push(rect_el(
        "s1_tracks_bed",
        "PlatformBallast",
        0.0,
        436.0,
        960.0,
        104.0,
        [12, 18, 34, 255],
        [250, 204, 21, 255],
        2.0,
    ));

    // Mask layer + Masked nebula beam
    let mask_layer = Layer {
        id: "s1_beam_mask".into(),
        name: "TelescopeApertureMask".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![path_el(
                "s1_mask_cone",
                "",
                0.0,
                0.0,
                vec![
                    PathCommand::MoveTo { x: 248.0, y: 262.0 },
                    PathCommand::LineTo { x: 690.0, y: 64.0 },
                    PathCommand::LineTo { x: 760.0, y: 196.0 },
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

    let masked_beam_layer = Layer {
        id: "s1_beam_fill".into(),
        name: "NebulaLightBeam".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![gradient_rect_el(
                "s1_beam_grad",
                "",
                220.0,
                50.0,
                560.0,
                240.0,
                vec![
                    (0, [56, 189, 248, 165]),
                    (140, [250, 204, 21, 120]),
                    (255, [244, 114, 182, 40]),
                ],
                false,
            )],
            ..Keyframe::default()
        }],
        properties: LayerProperties {
            mask: Some("s1_beam_mask".into()),
            ..LayerProperties::default()
        },
    };

    // Shape-tweened departure mist cloud
    let mist_a = path_el(
        "s1_mist",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 60.0, y: 430.0 },
            PathCommand::LineTo { x: 260.0, y: 408.0 },
            PathCommand::LineTo { x: 320.0, y: 438.0 },
            PathCommand::LineTo { x: 60.0, y: 438.0 },
            PathCommand::Close,
        ],
        [186, 230, 253, 110],
        [0, 0, 0, 0],
        0.0,
    );
    let mist_b = path_el(
        "s1_mist",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: 180.0, y: 424.0 },
            PathCommand::LineTo { x: 620.0, y: 396.0 },
            PathCommand::LineTo { x: 720.0, y: 438.0 },
            PathCommand::LineTo { x: 180.0, y: 438.0 },
            PathCommand::Close,
        ],
        [254, 240, 138, 70],
        [0, 0, 0, 0],
        0.0,
    );
    let mist_layer = Layer {
        id: "s1_mist_l".into(),
        name: "DepartureMistShapeTween".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Shape,
                elements: vec![mist_a],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![mist_b],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    // Motion-tweened Comet Escort Lantern
    let lantern_start = instance_el(
        "s1_lantern",
        "escortLantern",
        "sym-lantern",
        140.0,
        180.0,
        InstancePlayback::Independent,
    );
    let mut lantern_end = lantern_start.clone();
    lantern_end.transform = Transform {
        x: 840.0,
        y: 118.0,
        scale_x: 1.15,
        scale_y: 1.15,
        ..Transform::default()
    };
    let lantern_layer = Layer {
        id: "s1_lantern_l".into(),
        name: "CometLanternMotion".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Motion,
                motion_path: Some(vec![
                    PathCommand::MoveTo { x: 140.0, y: 180.0 },
                    PathCommand::CubicTo {
                        cx1: 360.0,
                        cy1: 72.0,
                        cx2: 610.0,
                        cy2: 210.0,
                        x: 840.0,
                        y: 118.0,
                    },
                ]),
                elements: vec![lantern_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![lantern_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    // Classic-tweened Midnight Locomotive Mk.V departing Observatory
    let train_start = instance_el(
        "s1_train",
        "expressTrain",
        "sym-locomotive",
        -120.0,
        432.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(760.0, 432.0);
    let train_layer = Layer {
        id: "s1_train_l".into(),
        name: "MidnightExpressDeparture".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "departure".into(),
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

    // Title card + Bitmap crest
    let mut title_card = vec![
        Element {
            id: "s1_crest_bmp".into(),
            name: "observatoryCrest".into(),
            transform: tf(36.0, 26.0),
            kind: ElementKind::Bitmap {
                resource_id: "res-crest".into(),
                width: 48,
                height: 48,
                smoothing: true,
            },
            ..Element::default()
        },
        text_el(
            "s1_eyebrow",
            "",
            96.0,
            30.0,
            "MIDNIGHT EXPRESS · EPISODE V",
            13.0,
            [250, 204, 21, 255],
        ),
        text_el(
            "s1_title",
            "",
            96.0,
            50.0,
            "The Celestial Observatory & Eclipse Crossing",
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

    // Stereo stream audio layer
    let mut snd = FrameSound::new("res-audio-s1");
    snd.sync = SoundSync::Stream;
    let sound_layer = Layer {
        id: "s1_audio_l".into(),
        name: "ObservatoryChimeStream".into(),
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
        name: "Scene 1 · Celestial Observatory Departure".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            // Note: Layer 0 is bottom-most or top-most? Let's check existing midnight-express-4!
            // In NeoFlash, layers[0] is drawn first (background) and later layers are drawn on top!
            // Wait: mask layer links via `properties.mask = Some("s1_beam_mask")`.
            layers: vec![
                single_key_layer("s1_bg_l", "EclipseSkyAndObservatory", backdrop),
                mask_layer,
                masked_beam_layer,
                mist_layer,
                lantern_layer,
                train_layer,
                single_key_layer("s1_hud_l", "EpisodeTitleAndCrest", title_card),
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
        "s2_sky",
        "AstralCosmos",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [5, 9, 26, 255]),
            (135, [18, 42, 86, 255]),
            (255, [28, 88, 112, 255]),
        ],
        true,
    )];
    backdrop.extend(starfield_elements("s2", 28));
    // Sweeping planetary rings & viaduct arch visual
    backdrop.push(path_el(
        "s2_planet_ring",
        "",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: -40.0, y: 460.0 },
            PathCommand::CubicTo {
                cx1: 260.0,
                cy1: 180.0,
                cx2: 640.0,
                cy2: 140.0,
                x: 1000.0,
                y: 220.0,
            },
            PathCommand::LineTo {
                x: 1000.0,
                y: 256.0,
            },
            PathCommand::CubicTo {
                cx1: 640.0,
                cy1: 182.0,
                cx2: 260.0,
                cy2: 222.0,
                x: -40.0,
                y: 496.0,
            },
            PathCommand::Close,
        ],
        [56, 189, 248, 75],
        [186, 230, 253, 180],
        1.5,
    ));
    let mut viaduct_arch = path_el(
        "s2_viaduct_arch",
        "AstralViaduct",
        0.0,
        0.0,
        vec![
            PathCommand::MoveTo { x: -60.0, y: 426.0 },
            PathCommand::CubicTo {
                cx1: 280.0,
                cy1: 268.0,
                cx2: 640.0,
                cy2: 248.0,
                x: 1020.0,
                y: 336.0,
            },
        ],
        [0, 0, 0, 0],
        [250, 204, 21, 235],
        5.0,
    );
    viaduct_arch.filters = vec![FilterSpec::Glow {
        color: 0x38BDF8,
        alpha: 0.7,
        blur_x: 10.0,
        blur_y: 10.0,
        strength: 1.5,
        quality: 1,
        inner: false,
        knockout: false,
    }];
    backdrop.push(viaduct_arch);

    // Motion Guide Layer for guided Classic tween with orient_to_path
    let guide_curve = vec![
        PathCommand::MoveTo { x: -80.0, y: 422.0 },
        PathCommand::CubicTo {
            cx1: 280.0,
            cy1: 264.0,
            cx2: 640.0,
            cy2: 244.0,
            x: 1020.0,
            y: 332.0,
        },
    ];
    let guide_layer = Layer {
        id: "s2_viaduct_guide".into(),
        name: "ViaductMotionGuide".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![path_el(
                "s2_guide_stroke",
                "",
                0.0,
                0.0,
                guide_curve,
                [0, 0, 0, 0],
                [56, 189, 248, 255],
                1.5,
            )],
            ..Keyframe::default()
        }],
        properties: LayerProperties {
            role: LayerRole::Guide,
            ..LayerProperties::default()
        },
    };

    // Guided locomotive layer (orient_to_path: true)
    let train_start = instance_el(
        "s2_train",
        "viaductTrain",
        "sym-locomotive",
        -80.0,
        422.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(1020.0, 332.0);

    let guided_train_layer = Layer {
        id: "s2_train_l".into(),
        name: "GuidedViaductExpress".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "viaduct_crossing".into(),
                tween: Some(Easing::CubicBezier {
                    x1: 0.32,
                    y1: 0.08,
                    x2: 0.68,
                    y2: 0.94,
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

    // Escort comet racing overhead with BlurFilter
    let mut comet_start = instance_el(
        "s2_comet",
        "cometEscort",
        "sym-lantern",
        40.0,
        160.0,
        InstancePlayback::Independent,
    );
    comet_start.filters = vec![FilterSpec::Blur {
        blur_x: 4.0,
        blur_y: 1.5,
        quality: 1,
    }];
    let mut comet_end = comet_start.clone();
    comet_end.transform = tf(900.0, 110.0);

    let comet_layer = Layer {
        id: "s2_comet_l".into(),
        name: "CometEscortSweep".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseOut),
                elements: vec![comet_start],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![comet_end],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    let mut snd = FrameSound::new("res-audio-s2");
    snd.sync = SoundSync::Stream;
    let sound_layer = Layer {
        id: "s2_audio_l".into(),
        name: "AstralViaductStream".into(),
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
        name: "Scene 2 · Astral Ring Viaduct & Comet Escort".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            layers: vec![
                single_key_layer("s2_bg_l", "AstralViaductBackdrop", backdrop),
                guide_layer,
                guided_train_layer,
                comet_layer,
                single_key_layer(
                    "s2_caption_l",
                    "SceneCaption",
                    vec![text_el(
                        "s2_cap",
                        "",
                        36.0,
                        32.0,
                        "SCENE II · ASTRAL RING VIADUCT",
                        15.0,
                        [186, 230, 253, 255],
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
    // Shape-tweened pre-dawn -> golden sunrise sky!
    let dawn_a = gradient_rect_el(
        "s3_sky_morph",
        "",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [14, 24, 58, 255]),
            (150, [68, 52, 116, 255]),
            (255, [196, 98, 104, 255]),
        ],
        true,
    );
    let dawn_b = gradient_rect_el(
        "s3_sky_morph",
        "",
        0.0,
        0.0,
        960.0,
        540.0,
        vec![
            (0, [24, 48, 96, 255]),
            (140, [238, 138, 76, 255]),
            (255, [253, 224, 114, 255]),
        ],
        true,
    );
    let sky_layer = Layer {
        id: "s3_sky_l".into(),
        name: "SunriseSkyMorph".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Shape,
                elements: vec![dawn_a],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 59,
                elements: vec![dawn_b],
                ..Keyframe::default()
            },
        ],
        properties: LayerProperties::default(),
    };

    // Grand Horizon Station architecture + lake reflection
    let station_elements = vec![
        radial_glow_el(
            "s3_sun",
            "RisingSun",
            740.0,
            290.0,
            120.0,
            [254, 249, 195, 245],
            [251, 146, 60, 145],
            [251, 146, 60, 0],
        ),
        rect_el(
            "s3_lake",
            "HorizonLake",
            0.0,
            378.0,
            960.0,
            64.0,
            [30, 58, 95, 220],
            [253, 224, 114, 160],
            1.0,
        ),
        // Clocktower & Grand Canopy
        rect_el(
            "s3_tower",
            "ClockTower",
            596.0,
            168.0,
            68.0,
            268.0,
            [30, 41, 68, 255],
            [250, 204, 21, 255],
            2.0,
        ),
        ellipse_el(
            "s3_clock_face",
            "StationClock",
            612.0,
            194.0,
            36.0,
            36.0,
            [254, 249, 195, 255],
            [245, 158, 11, 255],
            2.0,
        ),
        rect_el(
            "s3_canopy",
            "GlassCanopy",
            310.0,
            296.0,
            390.0,
            22.0,
            [56, 189, 248, 150],
            [250, 204, 21, 255],
            2.0,
        ),
        rect_el(
            "s3_platform",
            "StationPlatform",
            0.0,
            436.0,
            960.0,
            104.0,
            [15, 23, 42, 255],
            [250, 204, 21, 255],
            2.0,
        ),
    ];

    // Smooth EaseOut station arrival
    let train_start = instance_el(
        "s3_train",
        "arrivalTrain",
        "sym-locomotive",
        -110.0,
        432.0,
        InstancePlayback::Loop,
    );
    let mut train_end = train_start.clone();
    train_end.transform = tf(390.0, 432.0);
    let train_layer = Layer {
        id: "s3_train_l".into(),
        name: "GrandHorizonArrival".into(),
        visible: true,
        locked: false,
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "grand_horizon_arrival".into(),
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

    // Finale banner fade-in
    let mut banner_start = text_el(
        "s3_finale_txt",
        "",
        36.0,
        34.0,
        "FINALE · DAWN AT GRAND HORIZON STATION",
        20.0,
        [254, 249, 195, 255],
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
        name: "DawnFinaleChimeStream".into(),
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
        name: "Scene 3 · Grand Horizon Station & Dawn Chime".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 60,
            layers: vec![
                sky_layer,
                single_key_layer("s3_arch_l", "StationArchitecture", station_elements),
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

pub fn build_midnight_express_5() -> Project {
    let wav_s1 = make_harmonic_stereo_wav(2500, &[261.63, 329.63, 392.00, 587.33], true);
    let wav_s2 = make_harmonic_stereo_wav(2500, &[293.66, 369.99, 440.00, 659.25], true);
    let wav_s3 = make_harmonic_stereo_wav(2500, &[329.63, 415.30, 493.88, 783.99], false);

    let resources = vec![
        make_crest_resource("res-crest", "images/observatory-crest.png"),
        ResourceUnit::from_bytes(
            "res-audio-s1",
            "audio/scene1-departure.wav",
            "audio/wav",
            &wav_s1,
        )
        .expect("s1 wav"),
        ResourceUnit::from_bytes(
            "res-audio-s2",
            "audio/scene2-viaduct.wav",
            "audio/wav",
            &wav_s2,
        )
        .expect("s2 wav"),
        ResourceUnit::from_bytes(
            "res-audio-s3",
            "audio/scene3-dawn.wav",
            "audio/wav",
            &wav_s3,
        )
        .expect("s3 wav"),
    ];

    let mut project = Project {
        format_version: 9,
        name: "Midnight Express 5 — The Celestial Observatory & Eclipse Crossing".into(),
        width: 960,
        height: 540,
        fps: 24,
        background: [8, 12, 28, 255],
        root: "scene-1".into(),
        symbols: vec![
            build_wheel_symbol(),
            build_locomotive_symbol(),
            build_lantern_symbol(),
            build_scene_1(),
            build_scene_2(),
            build_scene_3(),
        ],
        library_folders: vec!["Locomotive".into(), "Celestial".into()],
        sources: Vec::new(),
        resources,
        document_class: None,
        scenes: vec!["scene-1".into(), "scene-2".into(), "scene-3".into()],
    };
    project.upgrade_format_for_features();
    project
}
