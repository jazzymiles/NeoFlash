#[path = "tests_episode3.rs"]
mod episode3;
#[path = "tests_episode4.rs"]
mod episode4;
#[path = "tests_t414.rs"]
mod t414;

use super::{shapes::sample_shape_tween_frame, *};
use base64::Engine as _;
use image::{ExtendedColorType, ImageEncoder as _};
use neoflash_core::{
    BlendMode, Easing, Element, ElementKind, FillRule, FrameScript, GradientKind, GradientPaint,
    GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties, MotionPathOverride,
    MotionPropertyKey, MotionTrackOverride, OvalOptions, PathCommand, PrimitiveOptions, Project,
    PropertyExpression, RectangleOptions, Script, ScriptLanguage, Symbol, SymbolKind, Timeline,
    Transform, TweenKind, TweenProperty,
};
use serde_json::Value;
use std::{fs, path::PathBuf};

pub(super) struct TestOutputDir(pub(super) PathBuf);

impl TestOutputDir {
    pub(super) fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "neoflash-lottie-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test output {}: {error}", path.display()),
            }
        }
    }
}

impl Drop for TestOutputDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn object_scoped_motion_tracks_export_independent_paths_eases_and_orientation() {
    let rect = |id: &str, x: f64, y: f64, alpha: f64| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            alpha,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let path = |start_x, control_y, end_x| MotionPathOverride::Path {
        commands: vec![
            PathCommand::MoveTo { x: start_x, y: 0.0 },
            PathCommand::CubicTo {
                cx1: start_x,
                cy1: control_y,
                cx2: end_x,
                cy2: control_y,
                x: end_x,
                y: 0.0,
            },
        ],
    };
    let mut a_track = MotionTrackOverride::new("a");
    a_track.easing = Some(Easing::EaseIn);
    a_track.property_eases = Some(vec![
        (TweenProperty::X, Easing::Linear),
        (TweenProperty::Y, Easing::Linear),
    ]);
    a_track.orient_to_path = Some(true);
    a_track.motion_path = Some(path(0.0, 100.0, 100.0));
    let mut b_track = MotionTrackOverride::new("b");
    b_track.easing = Some(Easing::EaseOut);
    b_track.property_eases = Some(vec![
        (TweenProperty::X, Easing::Linear),
        (TweenProperty::Y, Easing::Linear),
    ]);
    b_track.orient_to_path = Some(false);
    b_track.motion_path = Some(path(200.0, -100.0, 300.0));

    let mut project = Project::blank();
    project.format_version = 10;
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 21;
    root.timeline.layers[0].name = "Motion".into();
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            elements: vec![rect("a", 0.0, 0.0, 1.0), rect("b", 200.0, 0.0, 1.0)],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Motion,
            motion_tracks: vec![a_track, b_track],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 20,
            elements: vec![rect("a", 100.0, 0.0, 0.0), rect("b", 300.0, 0.0, 0.0)],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Motion,
            keyed_properties: Some(vec![
                TweenProperty::X,
                TweenProperty::Y,
                TweenProperty::Alpha,
            ]),
            motion_keys: Some(vec![
                MotionPropertyKey {
                    element_id: "a".into(),
                    properties: vec![TweenProperty::X, TweenProperty::Y, TweenProperty::Alpha],
                    roving: false,
                },
                MotionPropertyKey {
                    element_id: "b".into(),
                    properties: vec![TweenProperty::X, TweenProperty::Y, TweenProperty::Alpha],
                    roving: false,
                },
            ]),
            ..Keyframe::default()
        },
    ];
    project
        .validate()
        .expect("format-10 motion project validates");

    let (value, _) = export_lottie_value(
        &project,
        &LottieOptions {
            include_background: false,
            ..LottieOptions::default()
        },
    )
    .expect("export object-scoped motion");
    let layers = value["layers"].as_array().unwrap();
    let layer = |name: &str| {
        layers
            .iter()
            .find(|layer| layer["nm"] == format!("Motion · {name}"))
            .unwrap_or_else(|| panic!("missing Lottie track for {name}"))
    };
    let a = layer("a");
    let b = layer("b");
    fn key_at<'a>(property: &str, layer: &'a Value, frame: u32) -> &'a Value {
        layer["ks"][property]["k"]
            .as_array()
            .unwrap()
            .iter()
            .find(|key| key["t"] == frame)
            .unwrap_or_else(|| panic!("missing {property} key at frame {frame}"))
    }
    let a_position = &key_at("p", a, 10)["s"];
    let b_position = &key_at("p", b, 10)["s"];
    assert!((a_position[0].as_f64().unwrap() - 50.0).abs() < 0.05);
    assert!((a_position[1].as_f64().unwrap() - 75.0).abs() < 0.05);
    assert!((b_position[0].as_f64().unwrap() - 250.0).abs() < 0.05);
    assert!((b_position[1].as_f64().unwrap() + 75.0).abs() < 0.05);
    assert!((key_at("o", a, 10)["s"][0].as_f64().unwrap() - 75.0).abs() < 0.05);
    assert!((key_at("o", b, 10)["s"][0].as_f64().unwrap() - 25.0).abs() < 0.05);
    assert_eq!(a["ao"], 1);
    assert_eq!(b["ao"], 0);
}

fn heart_puff_commands(cx: f64, cy: f64, size: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo {
            x: cx,
            y: cy + size * 0.85,
        },
        PathCommand::CubicTo {
            cx1: cx - size * 1.15,
            cy1: cy + size * 0.2,
            cx2: cx - size * 0.95,
            cy2: cy - size * 0.85,
            x: cx,
            y: cy - size * 0.3,
        },
        PathCommand::CubicTo {
            cx1: cx + size * 0.95,
            cy1: cy - size * 0.85,
            cx2: cx + size * 1.15,
            cy2: cy + size * 0.2,
            x: cx,
            y: cy + size * 0.85,
        },
        PathCommand::Close,
    ]
}

#[test]
fn property_expression_sampling_uses_source_document_fps() {
    let mut project = Project::blank();
    project.fps = 30;
    project.format_version = 9;
    let root_id = project.root.clone();
    let element = Element {
        id: "expression-shape".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.0,
            height: 10.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 61;
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![element.clone()],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Motion,
            property_expressions: vec![PropertyExpression {
                property: TweenProperty::X,
                expression: "value + time".into(),
                ..PropertyExpression::default()
            }],
            ..Keyframe::default()
        },
        Keyframe {
            frame: 60,
            elements: vec![element],
            ..Keyframe::default()
        },
    ];
    project.validate().expect("expression project validates");

    let position_at_30 = |fps| {
        let mut project = project.clone();
        project.fps = fps;
        let (value, _) = export_lottie_value(
            &project,
            &LottieOptions {
                include_background: false,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        value["layers"][0]["ks"]["p"]["k"]
            .as_array()
            .unwrap()
            .iter()
            .find(|key| key["t"] == 30)
            .unwrap()["s"][0]
            .as_f64()
            .unwrap()
    };
    assert!((position_at_30(30) - 1.0).abs() < 1e-9);
    assert!((position_at_30(60) - 0.5).abs() < 1e-9);
}

#[test]
fn exact_shape_keys_use_the_shared_effect_aware_sampler() {
    use neoflash_core::{
        ElementAppearance, ElementVectorEffects, FreeformGradient, FreeformGradientPoint,
        VectorModifier, sample_layer_in_at_fps,
    };

    let rectangle = |width| Element {
        id: "shape".into(),
        kind: ElementKind::Rectangle {
            width,
            height: 10.0,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let start = Keyframe {
        frame: 0,
        elements: vec![rectangle(10.0)],
        tween: Some(Easing::Linear),
        tween_kind: TweenKind::Shape,
        property_expressions: vec![PropertyExpression {
            property: TweenProperty::X,
            expression: "value + 7".into(),
            ..PropertyExpression::default()
        }],
        vector_effects: vec![ElementVectorEffects {
            element_id: "shape".into(),
            freeform_gradient: Some(FreeformGradient {
                points: vec![FreeformGradientPoint {
                    id: "green".into(),
                    position: [0.0, 0.0],
                    color: [0, 255, 0, 255],
                    radius: 10.0,
                    weight: 1.0,
                }],
                lines: Vec::new(),
            }),
            ..ElementVectorEffects::default()
        }],
        appearances: vec![ElementAppearance {
            element_id: "shape".into(),
            modifiers: vec![VectorModifier::Repeater {
                copies: 2,
                offset_x: 20.0,
                offset_y: 0.0,
                scale_step: 1.0,
                rotation_step: 0.0,
            }],
            ..ElementAppearance::default()
        }],
        ..Keyframe::default()
    };
    let end = Keyframe {
        frame: 10,
        elements: vec![rectangle(20.0)],
        property_expressions: vec![PropertyExpression {
            property: TweenProperty::Y,
            expression: "value + 3".into(),
            ..PropertyExpression::default()
        }],
        ..Keyframe::default()
    };
    let layer = Layer {
        id: "shape-layer".into(),
        name: "Shape".into(),
        visible: true,
        locked: false,
        keyframes: vec![start, end],
        properties: LayerProperties::default(),
    };
    let timeline = Timeline {
        frame_count: 11,
        layers: vec![layer],
        scripts: Vec::new(),
        guides: Vec::new(),
    };

    for frame in [0, 10] {
        let exported = sample_shape_tween_frame(&timeline, &timeline.layers[0], frame, 30.0);
        let shared = sample_layer_in_at_fps(&timeline, &timeline.layers[0], frame, 30.0);
        assert_eq!(exported, shared);
    }
    assert_eq!(
        sample_shape_tween_frame(&timeline, &timeline.layers[0], 0, 30.0)[0]
            .transform
            .x,
        7.0
    );
    assert_eq!(
        sample_shape_tween_frame(&timeline, &timeline.layers[0], 10, 30.0)[0]
            .transform
            .y,
        3.0
    );
}

#[test]
fn simple_shape_tween_keeps_one_animated_shape_track() {
    let mut project = Project::blank();
    project.fps = 30;
    project.format_version = 6;
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 11;
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "morph".into(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 20.0,
                    fill: [255, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            }],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Shape,
            ..Keyframe::default()
        },
        Keyframe {
            frame: 10,
            elements: vec![Element {
                id: "morph".into(),
                kind: ElementKind::Ellipse {
                    width: 50.0,
                    height: 30.0,
                    fill: [0, 0, 255, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        },
    ];

    let (value, _) = export_lottie_value(
        &project,
        &LottieOptions {
            include_background: false,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    let layers = value["layers"].as_array().unwrap();
    assert_eq!(
        layers.len(),
        1,
        "simple morph should not become frame slices"
    );
    assert_eq!(layers[0]["ty"], 4);
    let items = layers[0]["shapes"][0]["it"].as_array().unwrap();
    let path = items.iter().find(|item| item["ty"] == "sh").unwrap();
    assert_eq!(path["ks"]["a"], 1, "morph path should remain animated");
}

#[test]
fn demo_and_tween_features_export_valid_bodymovin_lottie_json() {
    let mut project = Project::blank();
    project.name = "Hero Banner".into();
    project.width = 640;
    project.height = 360;
    project.fps = 30;
    project.background = [24, 28, 36, 255];
    project.format_version = 8;

    // Add a nested Graphic symbol ("Star") with 4 frames so we can test Precomps + T-038
    // `sync_timeline` and `LoopFrom` time remapping!
    let star_id = "symbol-star".to_owned();
    project.symbols.push(Symbol {
        id: star_id.clone(),
        name: "Star".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            guides: vec![],
            frame_count: 4,
            scripts: vec![],
            layers: vec![Layer {
                id: "star-layer".into(),
                name: "Star Shape".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: (0..4)
                    .map(|f| Keyframe {
                        frame: f,
                        elements: vec![Element {
                            id: "star-rect".into(),
                            name: "Box".into(),
                            transform: Transform {
                                x: f64::from(f * 10),
                                y: 0.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 40.0,
                                height: 40.0,
                                fill: [255, 200, 40, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    })
                    .collect(),
            }],
        },
    });

    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 10;
    // Layer 0 (bottom): Classic-tweened rounded rectangle with Screen blend mode + label
    root.timeline.layers[0] = Layer {
        id: "layer-bg-card".into(),
        name: "Card".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            blend_mode: BlendMode::Screen,
            ..LayerProperties::default()
        },
        keyframes: vec![
            Keyframe {
                frame: 0,
                label: "intro".into(),
                tween: Some(Easing::EaseInOut),
                tween_kind: TweenKind::Classic,
                elements: vec![Element {
                    id: "card-el".into(),
                    name: "Card".into(),
                    transform: Transform {
                        x: 40.0,
                        y: 50.0,
                        primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                            corner_radii: [8.0; 4],
                            corners_locked: true,
                        })),
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 120.0,
                        height: 80.0,
                        fill: [60, 140, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 9,
                elements: vec![Element {
                    id: "card-el".into(),
                    name: "Card".into(),
                    transform: Transform {
                        x: 240.0,
                        y: 110.0,
                        alpha: 0.5,
                        primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                            corner_radii: [8.0; 4],
                            corners_locked: true,
                        })),
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 120.0,
                        height: 80.0,
                        fill: [60, 140, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
        ],
    };

    // Layer 1 (top): Timeline-synced layer (`sync_timeline = true`) with frames 0..6 empty
    // and the `Star` instance appearing at frame 6 (1-based frame 7).
    root.timeline.layers.push(Layer {
        id: "layer-synced-star".into(),
        name: "Synced Star".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            sync_timeline: true,
            ..LayerProperties::default()
        },
        keyframes: vec![
            Keyframe {
                frame: 0,
                elements: vec![],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 6,
                elements: vec![Element {
                    id: "star-inst".into(),
                    name: "StarInstance".into(),
                    transform: Transform {
                        x: 320.0,
                        y: 180.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: star_id.clone(),
                        playback: InstancePlayback::Loop,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
        ],
    });

    let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
    assert_eq!(lottie["v"], LOTTIE_VERSION);
    assert_eq!(lottie["w"], 640);
    assert_eq!(lottie["h"], 360);
    assert_eq!(lottie["fr"], 30.0);
    assert_eq!(lottie["ip"], 0);
    assert_eq!(lottie["op"], 10);
    assert_eq!(report.asset_count, 1, "Star precomp asset");
    assert_eq!(
        report.layer_count, 3,
        "Synced Star precomp + Card shape + Stage Background solid"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Top layer in Lottie (`layers[0]`) is "Synced Star · StarInstance" (`ty: 0`, `ip: 6`, `op: 10`)
    // with time remapping `tm` starting at frame 6 (`6 % 4 == 2`, so `2.0 / 30.0` seconds).
    let top = &lottie["layers"][0];
    assert_eq!(top["ty"], 0);
    assert_eq!(top["ip"], 6);
    assert_eq!(top["op"], 10);
    assert_eq!(top["tm"]["a"], 1);
    assert_eq!(top["tm"]["k"][0]["t"], 6);
    assert_eq!(top["tm"]["k"][0]["s"][0], round4(2.0 / 30.0));

    // Second layer (`layers[1]`) is "Card · Card" (`ty: 4`, Screen blend mode `bm: 2`, animated `p` and `o`).
    let card = &lottie["layers"][1];
    assert_eq!(card["ty"], 4);
    assert_eq!(card["bm"], 2);
    assert_eq!(card["ks"]["p"]["a"], 1);
    assert_eq!(card["ks"]["o"]["a"], 1);

    // Marker "intro" exported at frame 0.
    assert_eq!(lottie["markers"][0]["cm"], "intro");
    assert_eq!(lottie["markers"][0]["tm"], 0);

    // Also test symbol-only export (`options.symbol = Some("Star")`).
    let (star_json, star_report) = export_lottie_json(
        &project,
        &LottieOptions {
            symbol: Some("Star".into()),
            include_background: false,
            include_hidden_layers: false,
            pretty: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    assert_eq!(star_report.name, "Star");
    assert_eq!(star_report.frame_count, 4);
    assert!(star_json.contains("\"nm\": \"Star\""));

    // Test progress callback + cancellation (`export_lottie_with_progress`), frame_range, scale,
    // dotLottie `.lottie` ZIP packaging, and HTML preview output.
    let mut seen_steps = Vec::new();
    let err = export_lottie_with_progress(&project, &LottieOptions::default(), |p| {
        seen_steps.push(p.completed_steps);
        p.completed_steps < 1
    })
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    assert_eq!(seen_steps, vec![0, 1]);

    let output = TestOutputDir::new("dotlottie");
    let dir = &output.0;
    let dot_path = dir.join("banner.lottie");
    let dot_report = export_project_lottie_with_progress(
        &project,
        &dot_path,
        &LottieOptions {
            format: LottieFormat::DotLottie,
            frame_range: Some((2, 6)),
            scale: 2.0,
            precision: 2,
            html_preview: true,
            ..LottieOptions::default()
        },
        |_| true,
    )
    .unwrap();
    assert_eq!(dot_report.width, 1280);
    assert_eq!(dot_report.height, 720);
    assert_eq!(dot_report.in_point, 1);
    assert_eq!(dot_report.frame_count, 5);
    assert!(dot_report.bytes > 64);
    assert!(dot_report.html_preview_path.as_ref().unwrap().exists());
    let zip_bytes = fs::read(&dot_path).unwrap();
    assert_eq!(&zip_bytes[0..4], b"PK\x03\x04");
}

#[test]
fn shape_tween_gradients_oval_and_script_warnings_export_cleanly() {
    let mut project = Project::blank();
    project.format_version = 8;
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 5;
    root.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "stop();".into(),
        },
    });
    root.timeline.layers[0] = Layer {
        id: "morph-layer".into(),
        name: "Morph".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            blend_mode: BlendMode::Invert,
            ..LayerProperties::default()
        },
        keyframes: vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Shape,
                elements: vec![Element {
                    id: "shape-a".into(),
                    name: "Oval".into(),
                    transform: Transform {
                        primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                            start_angle: 0.0,
                            end_angle: 270.0,
                            inner_radius: 25.0,
                            close_path: true,
                        })),
                        ..Transform::default()
                    },
                    kind: ElementKind::Ellipse {
                        width: 100.0,
                        height: 100.0,
                        fill: [255, 80, 80, 255],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 3.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 4,
                elements: vec![Element {
                    id: "shape-b".into(),
                    name: "GradientPath".into(),
                    transform: Transform::default(),
                    kind: ElementKind::PaintedPath {
                        commands: vec![
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::LineTo { x: 120.0, y: 0.0 },
                            PathCommand::QuadraticTo {
                                cx: 120.0,
                                cy: 120.0,
                                x: 0.0,
                                y: 120.0,
                            },
                            PathCommand::Close,
                        ],
                        fill_rule: FillRule::EvenOdd,
                        fill: [0, 200, 120, 255],
                        stroke: [0; 4],
                        fill_gradient: Some(GradientPaint {
                            kind: GradientKind::Linear,
                            matrix: [0.1, 0.0, 0.0, 0.1, 60.0, 60.0],
                            stops: vec![
                                GradientStop {
                                    ratio: 0,
                                    color: [255, 0, 128, 255],
                                },
                                GradientStop {
                                    ratio: 255,
                                    color: [0, 200, 255, 180],
                                },
                            ],
                            focal_point_ratio: 0.0,
                            ..Default::default()
                        }),
                        stroke_gradient: None,
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
        ],
    };

    let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
    assert_eq!(lottie["v"], LOTTIE_VERSION);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("frame script(s) are not executed by Lottie players")),
        "expected script diagnostic: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Blend mode 'invert' is not supported by Lottie")),
        "expected blend mode diagnostic: {:?}",
        report.warnings
    );
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.kind == LottieNoteKind::NotExported
                && n.message.contains("Blend mode 'invert'")),
        "expected structured NotExported note: {:?}",
        report.notes
    );
    assert!(report.not_exported_count >= 2);
}

#[test]
fn html_preview_escapes_script_tags_and_image_formats_transcode_and_dotlottie_preview_writes_folder_assets()
 {
    let mut project = Project::blank();
    project.format_version = 8;
    project.name = "Literal </script><script>globalThis.__t051_marker=true</script> name".into();

    // Create a valid 2x2 translucent PNG resource
    let mut png_bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png_bytes)
        .write_image(
            &[
                255, 0, 0, 255, // opaque red
                0, 255, 0, 128, // translucent green
                0, 0, 255, 255, // opaque blue
                255, 255, 0, 255,
            ],
            2,
            2,
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    project.resources.push(
        neoflash_core::ResourceUnit::from_bytes("red", "red.png", "image/png", &png_bytes).unwrap(),
    );
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "bitmap".into(),
            name: "red".into(),
            kind: ElementKind::Bitmap {
                resource_id: "red".into(),
                width: 2,
                height: 2,
                smoothing: false,
            },
            ..Element::default()
        });
    project.validate().unwrap();

    let output = TestOutputDir::new("images");
    let dir = &output.0;

    // T-057: HTML preview escapes `</script>` and title HTML entities
    let title_out = dir.join("title.json");
    let title_report = export_project_lottie(
        &project,
        &title_out,
        &LottieOptions {
            html_preview: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    let html = fs::read_to_string(title_report.html_preview_path.unwrap()).unwrap();
    assert!(
        !html.contains("</script><script>"),
        "HTML preview must not contain unescaped closing </script> inside title or script data"
    );
    assert!(
        html.contains("&lt;/script&gt;&lt;script&gt;globalThis.__t051_marker=true&lt;/script&gt;"),
        "Title and meta should HTML-escape angle brackets"
    );
    assert!(
        html.contains("\\u003c/script\\u003e\\u003cscript\\u003e"),
        "Inline script JSON should escape < and > as \\u003c and \\u003e"
    );

    // T-058: PNG -> JPEG in Folder and Embed modes produces real JPEG bytes (\xFF\xD8\xFF)
    let folder_jpeg_out = dir.join("folder-jpeg.json");
    let folder_jpeg_report = export_project_lottie(
        &project,
        &folder_jpeg_out,
        &LottieOptions {
            image_mode: LottieImageMode::Folder,
            image_format: LottieImageFormat::Jpeg,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    let written_jpg = fs::read(dir.join("images/img_red.jpg")).unwrap();
    assert!(
        written_jpg.starts_with(b"\xFF\xD8\xFF"),
        "Folder JPEG output must have JPEG magic bytes, got: {:?}",
        &written_jpg[..written_jpg.len().min(8)]
    );
    assert!(
        folder_jpeg_report
            .notes
            .iter()
            .any(|n| n.kind == LottieNoteKind::Approximated
                && n.message.contains("JPEG does not support")),
        "Translucent PNG exported as JPEG should record an Approximated note"
    );

    let (embed_jpeg_val, _) = export_lottie_value(
        &project,
        &LottieOptions {
            image_mode: LottieImageMode::Embed,
            image_format: LottieImageFormat::Jpeg,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    let data_uri = embed_jpeg_val["assets"][0]["p"].as_str().unwrap();
    assert!(
        data_uri.starts_with("data:image/jpeg;base64,"),
        "Embed JPEG must use data:image/jpeg;base64, prefix, got: {data_uri}"
    );
    let decoded_embed = base64::engine::general_purpose::STANDARD
        .decode(data_uri.trim_start_matches("data:image/jpeg;base64,"))
        .unwrap();
    assert!(
        decoded_embed.starts_with(b"\xFF\xD8\xFF"),
        "Embed JPEG base64 payload must decode to JPEG magic bytes"
    );

    // T-059: dotLottie + Images Folder + HTML preview writes external images alongside .preview.html
    let dot_dir = dir.join("dotlottie-sub");
    let dot_out = dot_dir.join("folder.lottie");
    let dot_report = export_project_lottie(
        &project,
        &dot_out,
        &LottieOptions {
            format: LottieFormat::DotLottie,
            image_mode: LottieImageMode::Folder,
            image_format: LottieImageFormat::Original,
            html_preview: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    assert!(dot_report.html_preview_path.unwrap().exists());
    let loose_png = dot_dir.join("images/img_red.png");
    assert!(
        loose_png.exists(),
        "dotLottie with Folder images + HTML preview must also write images/img_red.png next to .preview.html"
    );
    assert!(
        fs::read(&loose_png)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

#[test]
fn large_static_timeline_estimate_is_fast_and_cancellable() {
    let mut p = Project::blank();
    p.name = "Lottie UI estimate 50s 150 shapes".into();
    p.format_version = 8;
    let id = p.root.clone();
    let t = &mut p.symbol_mut(&id).unwrap().timeline;
    t.frame_count = 1200;
    t.layers[0].keyframes[0].elements = (0..150)
        .map(|i| Element {
            id: format!("shape-{i}"),
            name: format!("Rectangle {i}"),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        })
        .collect();
    p.validate().unwrap();

    let start = std::time::Instant::now();
    let (json, report) = export_lottie_json(&p, &LottieOptions::default()).unwrap();
    let elapsed_ms = start.elapsed().as_millis();
    assert_eq!(report.frame_count, 1200);
    assert_eq!(report.layer_count, 151); // 150 shapes + 1 background
    assert!(json.len() > 100_000);
    assert!(
        elapsed_ms < 250,
        "1200 frames x 150 static rectangles should export in <250ms even in debug build, took {elapsed_ms}ms"
    );

    let err = export_lottie_json_with_progress(&p, &LottieOptions::default(), |prog| {
        prog.completed_steps < 1
    })
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"));
}

#[test]
fn midnight_express_multi_scene_cartoon_stress_test() {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use neoflash_core::{
        AuthoredPlayback, BlendMode, Easing, Element, ElementKind, FillRule, FilterSpec,
        FrameRange, FrameSound, GradientKind, GradientPaint, GradientStop, InstancePlayback,
        Keyframe, Layer, LayerProperties, LayerRole, PathCommand, PrimitiveOptions, Project,
        RectangleOptions, ResourceUnit, ShapeHint, SoundEffect, SoundSync, Symbol, SymbolKind,
        Timeline, TimelineEdit, Transform, TweenKind, TweenProperty, edit_frames, evaluate,
        flatten_scenes_with_ranges,
    };

    // 1. Build a valid 8 kHz mono 16-bit PCM WAV whistle/chug audio resource (0.5s = 4000 samples).
    let sample_rate = 8000u32;
    let num_samples = 4000u32;
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
        let chord = ((t * 440.0 * std::f64::consts::TAU).sin()
            + 0.6 * (t * 554.37 * std::f64::consts::TAU).sin())
            * 0.4;
        let s = (chord * 32767.0).clamp(-32768.0, 32767.0) as i16;
        wav.extend_from_slice(&s.to_le_bytes());
    }

    // 2. Build a 16x16 neon billboard PNG resource.
    let mut png_bytes = Vec::new();
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u8 {
        for x in 0..16u8 {
            let border = !(2..14).contains(&x) || !(2..14).contains(&y);
            if border {
                rgba.extend_from_slice(&[0, 240, 255, 255]);
            } else {
                rgba.extend_from_slice(&[24, 18, 56, 220]);
            }
        }
    }
    PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
        .unwrap();

    let mut p = Project::blank();
    p.format_version = 9;
    p.name = "Midnight Express — Classic Flash Cartoon".into();
    p.width = 960;
    p.height = 540;
    p.fps = 24;
    p.background = [12, 16, 34, 255];
    p.resources.push(
        ResourceUnit::from_bytes("res-whistle", "audio/whistle.wav", "audio/wav", &wav).unwrap(),
    );
    p.resources.push(
        ResourceUnit::from_bytes(
            "res-billboard",
            "images/billboard.png",
            "image/png",
            &png_bytes,
        )
        .unwrap(),
    );

    // 3. Nested Graphic symbol: `sym-train` (6 frames, looping locomotive with pulsing headlight).
    let train_symbol = Symbol {
        id: "sym-train".into(),
        name: "Locomotive".into(),
        folder: "Vehicles".into(),
        kind: SymbolKind::Graphic,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 6,
            scripts: vec![],
            guides: vec![],
            layers: vec![Layer {
                id: "train-body-layer".into(),
                name: "Train Body".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: (0..6)
                    .map(|f| Keyframe {
                        frame: f,
                        elements: vec![
                            Element {
                                id: "cabin".into(),
                                name: "Cabin".into(),
                                transform: Transform {
                                    x: -60.0,
                                    y: -24.0,
                                    primitive: Some(PrimitiveOptions::Rectangle(
                                        RectangleOptions {
                                            corner_radii: [8.0, 8.0, 4.0, 4.0],
                                            corners_locked: false,
                                        },
                                    )),
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 120.0,
                                    height: 44.0,
                                    fill: [220, 52, 74, 255],
                                    stroke: [255, 215, 110, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "headlight".into(),
                                name: "Headlight".into(),
                                transform: Transform {
                                    x: 52.0 + f as f64 * 4.0,
                                    y: -14.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 18.0 + f as f64 * 2.0,
                                    height: 18.0 + f as f64 * 2.0,
                                    fill: [255, 244, 160, 255],
                                    stroke: [255, 180, 40, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    })
                    .collect(),
            }],
        },
    };

    // 4. Nested MovieClip symbol: `sym-drone` (4 frames, glowing escort drone).
    let drone_symbol = Symbol {
        id: "sym-drone".into(),
        name: "Escort Drone".into(),
        folder: "Vehicles".into(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
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
                                color: 0x00F0FF,
                                alpha: 0.85,
                                blur_x: 12.0,
                                blur_y: 12.0,
                                strength: 2.0,
                                quality: 2,
                                inner: false,
                                knockout: false,
                            }],
                            kind: ElementKind::Ellipse {
                                width: 48.0,
                                height: 22.0,
                                fill: [40, 225, 255, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 3,
                        elements: vec![Element {
                            id: "drone-hull".into(),
                            name: "Hull".into(),
                            transform: Transform {
                                y: -8.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 48.0,
                                height: 22.0,
                                fill: [40, 225, 255, 255],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.0,
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

    // 5. Scene 1 (`p.root`): "Night Station" (24 frames).
    let scene1_id = p.root.clone();
    {
        let scene1 = p.symbol_mut(&scene1_id).unwrap();
        scene1.name = "Night Station".into();
        scene1.timeline.frame_count = 24;
        scene1.timeline.layers = vec![
            // Layer 0: Night Sky Gradient Backdrop + Moon + Platform
            Layer {
                id: "s1-backdrop".into(),
                name: "Night Sky Backdrop".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    label: "station_start".into(),
                    elements: vec![
                        Element {
                            id: "sky-grad".into(),
                            name: "Sky".into(),
                            kind: ElementKind::PaintedPath {
                                commands: vec![
                                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 0.0 },
                                    PathCommand::LineTo { x: 960.0, y: 540.0 },
                                    PathCommand::LineTo { x: 0.0, y: 540.0 },
                                    PathCommand::Close,
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [16, 22, 48, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                                fill_gradient: Some(GradientPaint {
                                    kind: GradientKind::Linear,
                                    matrix: [0.3, 0.0, 0.0, 0.3, 480.0, 270.0],
                                    stops: vec![
                                        GradientStop {
                                            ratio: 0,
                                            color: [10, 14, 36, 255],
                                        },
                                        GradientStop {
                                            ratio: 165,
                                            color: [28, 42, 88, 255],
                                        },
                                        GradientStop {
                                            ratio: 255,
                                            color: [62, 38, 82, 255],
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
                            id: "full-moon".into(),
                            name: "Moon".into(),
                            transform: Transform {
                                x: 760.0,
                                y: 64.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 84.0,
                                height: 84.0,
                                fill: [255, 246, 200, 255],
                                stroke: [255, 225, 130, 180],
                                stroke_width: 4.0,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "platform-deck".into(),
                            name: "Platform".into(),
                            transform: Transform {
                                x: 0.0,
                                y: 440.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 960.0,
                                height: 100.0,
                                fill: [24, 28, 44, 255],
                                stroke: [90, 110, 160, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
            },
            // Layer 1: Masked Clouds (revealed by the animated Searchlight Mask above it)
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
                            x: 120.0,
                            y: 90.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 680.0,
                            height: 140.0,
                            fill: [110, 220, 255, 220],
                            stroke: [255, 255, 255, 255],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 2: Animated Searchlight Mask (sweeps across the sky)
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
                                x: 120.0,
                                y: 80.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 180.0,
                                height: 160.0,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 23,
                        elements: vec![Element {
                            id: "beam-cone".into(),
                            name: "Beam".into(),
                            transform: Transform {
                                x: 560.0,
                                y: 80.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 180.0,
                                height: 160.0,
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
            // Layer 3: Curved Railway Motion Guide
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
                                PathCommand::MoveTo { x: 80.0, y: 410.0 },
                                PathCommand::QuadraticTo {
                                    cx: 360.0,
                                    cy: 320.0,
                                    x: 540.0,
                                    y: 380.0,
                                },
                                PathCommand::QuadraticTo {
                                    cx: 720.0,
                                    cy: 430.0,
                                    x: 880.0,
                                    y: 360.0,
                                },
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [0, 0, 0, 0],
                            stroke: [120, 200, 255, 255],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            },
            // Layer 4: Guided Express Train (`orient_to_path` + `snap_to_guide` + Graphic `Loop`)
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
                        tween: Some(Easing::EaseIn),
                        orient_to_path: true,
                        snap_to_guide: true,
                        elements: vec![Element {
                            id: "train-inst".into(),
                            name: "Train".into(),
                            transform: Transform {
                                x: 80.0,
                                y: 410.0,
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
                        frame: 23,
                        elements: vec![Element {
                            id: "train-inst".into(),
                            name: "Train".into(),
                            transform: Transform {
                                x: 880.0,
                                y: 360.0,
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
            // Layer 5: Shape Tween Steam Puff with ShapeHints + Color/Alpha Morph
            Layer {
                id: "s1-steam-morph".into(),
                name: "Steam Puff Morph".into(),
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
                            start_uv: [0.0, 0.5],
                            end_uv: [0.0, 0.5],
                        }],
                        elements: vec![Element {
                            id: "steam-puff".into(),
                            name: "Steam".into(),
                            transform: Transform {
                                x: 160.0,
                                y: 330.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 36.0,
                                height: 28.0,
                                fill: [240, 248, 255, 230],
                                stroke: [180, 220, 255, 200],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 23,
                        elements: vec![Element {
                            id: "steam-puff".into(),
                            name: "Steam".into(),
                            transform: Transform {
                                x: 520.0,
                                y: 190.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 140.0,
                                height: 54.0,
                                fill: [130, 170, 230, 90],
                                stroke: [120, 180, 255, 60],
                                stroke_width: 5.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 6: Motion Tween Neon Station Sign with ◆ property keyframe + Screen Blend Mode
            Layer {
                id: "s1-neon-sign".into(),
                name: "Neon Sign Glow".into(),
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
                                x: 240.0,
                                y: 56.0,
                                alpha: 0.75,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "SCENE 1 · MIDNIGHT STATION".into(),
                                font_family: "Helvetica".into(),
                                font_size: 26.0,
                                color: [255, 90, 200, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 12,
                        keyed_properties: Some(vec![
                            TweenProperty::X,
                            TweenProperty::Y,
                            TweenProperty::Alpha,
                        ]),
                        elements: vec![Element {
                            id: "neon-banner".into(),
                            name: "Neon Banner".into(),
                            transform: Transform {
                                x: 280.0,
                                y: 48.0,
                                alpha: 1.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "SCENE 1 · MIDNIGHT STATION".into(),
                                font_family: "Helvetica".into(),
                                font_size: 26.0,
                                color: [255, 90, 200, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
            },
            // Layer 7: Sound Layer (Stream whistle + stereo envelope)
            Layer {
                id: "s1-audio".into(),
                name: "Whistle Audio".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    sound: Some(FrameSound {
                        resource: "res-whistle".into(),
                        sync: SoundSync::Stream,
                        loops: 1,
                        loop_forever: false,
                        effect: SoundEffect::Custom,
                        in_point: Some(0.0),
                        out_point: None,
                        envelope: Some(vec![[0.0, 0.3, 0.9], [0.4, 0.9, 0.4]]),
                    }),
                    ..Keyframe::default()
                }],
            },
        ];
    }

    // 6. Scene 2: "Neon Bridge Chase" (24 frames), exercising T-055 (synced layer in Scene 2)
    // and T-056 (reversed Flash easing curve).
    let scene2 = Symbol {
        id: "scene-bridge".into(),
        name: "Neon Bridge Chase".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: None,
        linkage_id: None,
        timeline: Timeline {
            frame_count: 24,
            scripts: vec![],
            guides: vec![],
            layers: vec![
                Layer {
                    id: "s2-bridge".into(),
                    name: "Bridge Horizon".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "bridge_chase".into(),
                        elements: vec![
                            Element {
                                id: "bridge-deck".into(),
                                name: "Bridge Deck".into(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 400.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 960.0,
                                    height: 36.0,
                                    fill: [50, 62, 96, 255],
                                    stroke: [0, 230, 255, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "neon-billboard".into(),
                                name: "Billboard".into(),
                                transform: Transform {
                                    x: 740.0,
                                    y: 180.0,
                                    scale_x: 6.0,
                                    scale_y: 6.0,
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
                // Synced layer in Scene 2 with delayed placement at scene-local frame 4 (T-055!)
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
                            elements: vec![],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 4,
                            tween: Some(Easing::Linear),
                            elements: vec![Element {
                                id: "s2-train-inst".into(),
                                name: "Bridge Train".into(),
                                transform: Transform {
                                    x: 140.0,
                                    y: 388.0,
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
                            frame: 23,
                            elements: vec![Element {
                                id: "s2-train-inst".into(),
                                name: "Bridge Train".into(),
                                transform: Transform {
                                    x: 820.0,
                                    y: 388.0,
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
                // Drone Escort layer whose tween we reverse via TimelineEdit::Reverse (T-056!)
                Layer {
                    id: "s2-drone-escort".into(),
                    name: "Drone Escort".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::Flash { acceleration: 70.0 }),
                            elements: vec![Element {
                                id: "s2-drone".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 120.0,
                                    y: 160.0,
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
                            frame: 23,
                            elements: vec![Element {
                                id: "s2-drone".into(),
                                name: "Drone".into(),
                                transform: Transform {
                                    x: 780.0,
                                    y: 140.0,
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
    p.symbols.push(scene2);
    p.scenes = vec![scene1_id.clone(), "scene-bridge".into()];
    p.upgrade_format_for_features();
    p.validate().unwrap();

    // Exercise TimelineEdit::Reverse on Scene 2's Drone Escort layer and verify T-056 easing inversion!
    edit_frames(
        &mut p,
        "scene-bridge",
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["s2-drone-escort".into()],
                start: 0,
                end: 23,
            },
        },
    )
    .unwrap();
    assert_eq!(
        p.symbol("scene-bridge").unwrap().timeline.layers[2].keyframes[0].tween,
        Some(Easing::Flash {
            acceleration: -70.0
        })
    );

    // Verify Scene 1 and Scene 2 flattened evaluation matches authored scene-local evaluation on every frame (T-055).
    let (flat, ranges) = flatten_scenes_with_ranges(&p);
    flat.validate().unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].start, 0);
    assert_eq!(ranges[1].start, 24);
    for f in 0..24 {
        let s1_authored = evaluate(&p, &scene1_id, f);
        let s1_flat = evaluate(&flat, &flat.root, ranges[0].start + f);
        assert_eq!(s1_authored.len(), s1_flat.len(), "Scene 1 frame {f}");
        let s2_authored = evaluate(&p, "scene-bridge", f);
        let s2_flat = evaluate(&flat, &flat.root, ranges[1].start + f);
        assert_eq!(s2_authored.len(), s2_flat.len(), "Scene 2 frame {f}");
        for (a, b) in s2_authored.iter().zip(s2_flat.iter()) {
            assert_eq!(
                a.transform, b.transform,
                "Scene 2 frame {f} item {} transform mismatch",
                a.element_id
            );
        }
    }

    // Verify sound playback emits the Stream whistle command on frame 0.
    let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
    let initial_sounds = playback.drain_sound_commands();
    assert_eq!(initial_sounds.len(), 1);

    // Normal regressions own temporary outputs and never rewrite authored examples.
    // Set NEOFLASH_EXPORT_CARTOON_HTML5=1 for real-runtime publication artifacts.
    let output = TestOutputDir::new("midnight-express");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
    let qa_dir = if export_html5 {
        repo_root.join("target/artifacts/generated-examples/midnight-express")
    } else {
        output.0.clone()
    };
    fs::create_dir_all(&qa_dir).unwrap();
    let lottie_json_path = qa_dir.join("midnight-express.lottie.json");
    let json_report = export_project_lottie(
        &p,
        &lottie_json_path,
        &LottieOptions {
            format: LottieFormat::Json,
            image_mode: LottieImageMode::Folder,
            image_format: LottieImageFormat::Jpeg,
            html_preview: true,
            scale: 1.5,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    assert_eq!(json_report.width, 1440);
    assert_eq!(json_report.height, 810);
    assert_eq!(json_report.frame_count, 48);
    assert!(lottie_json_path.exists());
    assert!(json_report.html_preview_path.unwrap().exists());
    assert!(
        qa_dir.join("images/img_res-billboard.jpg").exists(),
        "Billboard PNG should be transcoded to JPEG in Folder mode"
    );
    let dotlottie_path = qa_dir.join("midnight-express.lottie");
    let dot_report = export_project_lottie(
        &p,
        &dotlottie_path,
        &LottieOptions {
            format: LottieFormat::DotLottie,
            image_mode: LottieImageMode::Embed,
            image_format: LottieImageFormat::Png,
            html_preview: true,
            ..LottieOptions::default()
        },
    )
    .unwrap();
    assert!(dotlottie_path.exists());
    assert!(dot_report.html_preview_path.unwrap().exists());
    let web_runtime = repo_root.join("target/web-runtime");
    if export_html5 {
        assert!(
            web_runtime.join("neoflash_web_bg.wasm").is_file(),
            "NEOFLASH_EXPORT_CARTOON_HTML5 requires scripts/build-web.sh first"
        );
        let html5_dir = qa_dir.join("html5");
        crate::export_project_with_options(
            &p,
            &html5_dir,
            &web_runtime,
            &crate::PublishOptions::default(),
        )
        .unwrap();
        assert!(html5_dir.join("index.html").exists());
    }
    let parsed_lottie: Value =
        serde_json::from_str(&fs::read_to_string(&lottie_json_path).unwrap()).unwrap();
    let bg_layer = parsed_lottie["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "Stage Background")
        .unwrap();
    assert_eq!(
        bg_layer["sw"], 960,
        "Stage Background must use unscaled stage width because it is parented to Stage Scale (Bug C)"
    );
    let masked_clouds = parsed_lottie["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "Searchlight Clouds · Illuminated Clouds")
        .unwrap();
    assert_eq!(
        masked_clouds["masksProperties"][0]["pt"]["a"], 1,
        "Animated Searchlight Mask must export animated mask path keyframes (Bug B)"
    );
    let steam_layer = parsed_lottie["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["nm"] == "Steam Puff Morph · Steam")
        .unwrap();
    let steam_items = steam_layer["shapes"][0]["it"].as_array().unwrap();
    let steam_fill = steam_items.iter().find(|it| it["ty"] == "fl").unwrap();
    assert_eq!(
        steam_fill["c"]["a"], 1,
        "Shape tween with changing fill color must animate Lottie fill color (Bug D)"
    );
    assert_eq!(
        steam_fill["o"]["a"], 1,
        "Shape tween with changing fill alpha must animate Lottie fill opacity (Bug D)"
    );
}

#[test]
fn lottie_raster_transcoding_downscaling_and_dotlottie_manifest_metadata() {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder as _};
    use neoflash_core::{Element, ElementKind, Project, ResourceUnit, Transform};
    use std::io::Read as _;

    let mut project = Project::blank();
    project.background = [18, 24, 38, 255];

    let orig_w = 160u32;
    let orig_h = 80u32;
    let mut rgba = Vec::with_capacity((orig_w * orig_h * 4) as usize);
    for y in 0..orig_h {
        for x in 0..orig_w {
            rgba.push(((x * 255) / orig_w) as u8);
            rgba.push(((y * 255) / orig_h) as u8);
            rgba.push(180);
            rgba.push(if x < 40 { 128 } else { 255 });
        }
    }
    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, orig_w, orig_h, ExtendedColorType::Rgba8)
        .unwrap();

    project
        .resources
        .push(ResourceUnit::from_bytes("hero_bmp", "hero.png", "image/png", &png_bytes).unwrap());
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "hero_el".into(),
            name: "Hero".into(),
            transform: Transform::default(),
            kind: ElementKind::Bitmap {
                resource_id: "hero_bmp".into(),
                width: orig_w,
                height: orig_h,
                smoothing: true,
            },
            ..Element::default()
        });
    project.format_version = 3;
    project.validate().unwrap();

    // 1. Folder mode + WebP transcoding + max_image_dimension = 64 -> 64x32 asset, compensated 250% scale,
    // and report.bytes accurately includes both JSON bytes and external WebP file bytes.
    let out_dir = TestOutputDir::new("lottie-transcode-meta");
    let json_out = out_dir.0.join("hero.json");
    let folder_opts = LottieOptions {
        format: LottieFormat::Json,
        image_mode: LottieImageMode::Folder,
        image_format: LottieImageFormat::Webp,
        max_image_dimension: Some(64),
        ..LottieOptions::default()
    };
    let (est_json, est_report) = export_lottie_json(&project, &folder_opts).unwrap();
    let disk_report = export_project_lottie(&project, &json_out, &folder_opts).unwrap();
    let webp_path = out_dir.0.join("images/img_hero_bmp.webp");
    let webp_bytes = fs::read(&webp_path).expect("transcoded WebP written");
    assert!(webp_bytes.starts_with(b"RIFF") && &webp_bytes[8..12] == b"WEBP");
    let decoded_webp = image::load_from_memory(&webp_bytes).unwrap();
    assert_eq!((decoded_webp.width(), decoded_webp.height()), (64, 32));
    assert_eq!(est_report.bytes, est_json.len() + webp_bytes.len());
    assert_eq!(disk_report.bytes, est_report.bytes);
    assert!(
        est_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_BITMAP_DOWNSCALED"))
    );

    let parsed: Value = serde_json::from_str(&est_json).unwrap();
    let img_asset = &parsed["assets"][0];
    assert_eq!(
        (img_asset["w"].as_u64(), img_asset["h"].as_u64()),
        (Some(64), Some(32))
    );
    let img_layer = parsed["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["ty"] == 2)
        .expect("image layer");
    assert_eq!(
        img_layer["ks"]["s"]["k"],
        serde_json::json!([250.0, 250.0, 100.0])
    );

    // 2. dotLottie (.lottie) with JPEG alpha-composited transcoding + complete manifest.json metadata.
    let dot_opts = LottieOptions {
        format: LottieFormat::DotLottie,
        image_format: LottieImageFormat::Jpeg,
        max_image_dimension: Some(80),
        ..LottieOptions::default()
    };
    let (zip_bytes, dot_report) = export_dotlottie_bytes(&project, &dot_opts).unwrap();
    assert_eq!(dot_report.bytes, zip_bytes.len());
    assert!(
        dot_report
            .notes
            .iter()
            .any(|n| n.code.as_deref() == Some("LOTTIE_JPEG_ALPHA_COMPOSITED"))
    );
    validate_dotlottie_bytes(&zip_bytes).expect("valid dotLottie container");

    let mut za = zip::ZipArchive::new(std::io::Cursor::new(&zip_bytes)).unwrap();
    let mut manifest_str = String::new();
    za.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest_str)
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest_str).unwrap();
    assert_eq!(manifest["version"], "1.0");
    assert_eq!(manifest["generator"], "NeoFlash");
    let anim = &manifest["animations"][0];
    assert_eq!(anim["id"], "animation");
    assert_eq!(anim["theme"], "dark");
    assert_eq!(anim["themeColor"], "#121826");
    assert_eq!(anim["background"], "#121826");
    assert_eq!(anim["loop"], true);
    assert_eq!(anim["speed"], 1);
    assert_eq!(manifest["resources"][0]["path"], "images/img_hero_bmp.jpg");

    let mut jpg_bytes = Vec::new();
    za.by_name("images/img_hero_bmp.jpg")
        .unwrap()
        .read_to_end(&mut jpg_bytes)
        .unwrap();
    assert!(jpg_bytes.starts_with(b"\xFF\xD8\xFF"));
    let decoded_jpg = image::load_from_memory(&jpg_bytes).unwrap();
    assert_eq!((decoded_jpg.width(), decoded_jpg.height()), (80, 40));
}

#[test]
fn lottie_cooperative_cancellation_aborts_heavy_multi_hundred_frame_export_promptly() {
    use neoflash_core::{Easing, Element, ElementKind, FillRule, Keyframe, PathCommand, Project};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let mut project = Project::blank();
    let root_id = project.root.clone();
    {
        let timeline = &mut project.symbol_mut(&root_id).unwrap().timeline;
        timeline.frame_count = 600;
        let layer = &mut timeline.layers[0];
        layer.keyframes = vec![
            Keyframe {
                frame: 0,
                tween: Some(Easing::EaseInOut),
                tween_kind: neoflash_core::TweenKind::Shape,
                elements: (0..12)
                    .map(|i| Element {
                        id: format!("morph-{i}"),
                        name: format!("Morph {i}"),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                PathCommand::LineTo {
                                    x: 40.0 + f64::from(i),
                                    y: 10.0,
                                },
                                PathCommand::LineTo { x: 20.0, y: 50.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [240, 80, 60, 255],
                            stroke: [20, 20, 20, 255],
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    })
                    .collect(),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 599,
                elements: (0..12)
                    .map(|i| Element {
                        id: format!("morph-{i}"),
                        name: format!("Morph {i}"),
                        kind: ElementKind::Path {
                            commands: vec![
                                PathCommand::MoveTo { x: 100.0, y: 80.0 },
                                PathCommand::LineTo {
                                    x: 220.0 + f64::from(i),
                                    y: 90.0,
                                },
                                PathCommand::LineTo { x: 160.0, y: 210.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [60, 180, 240, 255],
                            stroke: [255, 255, 255, 255],
                            stroke_width: 4.0,
                        },
                        ..Element::default()
                    })
                    .collect(),
                ..Keyframe::default()
            },
        ];
    }
    project.format_version = 6;
    project.validate().unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_cb = Arc::clone(&cancel);
    let mut step1_seen = false;
    let err = export_lottie_json_with_cancel(
        &project,
        &LottieOptions {
            precision: 2,
            ..LottieOptions::default()
        },
        &cancel,
        |prog| {
            if prog.completed_steps == 1 {
                step1_seen = true;
                // Flip the cooperative cancel token right as the 600-frame track/shape emission begins.
                cancel_for_cb.store(true, Ordering::Relaxed);
            }
            assert!(
                prog.completed_steps < 2,
                "600-frame shape emission must abort inside frame loop before reaching step 2"
            );
            true
        },
    )
    .unwrap_err();
    assert!(step1_seen);
    assert!(err.to_string().contains("cancelled"), "{err}");
}
