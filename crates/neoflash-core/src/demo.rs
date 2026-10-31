use crate::*;

impl Project {
    pub fn blank() -> Self {
        let root = new_id("symbol");
        Self {
            format_version: 1,
            library_folders: vec![],
            sources: vec![],
            resources: vec![],
            document_class: None,
            name: "Untitled".into(),
            width: 960,
            height: 600,
            fps: 24,
            background: [255, 255, 255, 255],
            root: root.clone(),
            symbols: vec![Symbol {
                folder: String::new(),
                class_name: None,
                linkage_id: None,
                id: root,
                name: "Scene 1".into(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    guides: vec![],
                    frame_count: 120,
                    layers: vec![Layer {
                        properties: Default::default(),
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![],
                            tween: None,
                            label: String::new(),
                        }],
                    }],
                    scripts: vec![],
                },
            }],
        }
    }

    /// A complete editable scene demonstrating nesting, shared symbols and timeline motion.
    pub fn demo() -> Self {
        let mut p = Self::blank();
        p.name = "Ideas in motion".into();
        p.background = [19, 24, 36, 255];
        let white = [241, 244, 250, 255];
        let muted = [139, 154, 177, 255];
        let orange = [255, 142, 79, 255];
        let blue = [102, 160, 255, 255];
        let spark_id = new_id("symbol");
        let dot = ellipse("Spark", 0., 0., 24., 24., orange);
        p.symbols.push(Symbol {
            folder: String::new(),
            class_name: None,
            linkage_id: None,
            id: spark_id.clone(),
            name: "Spark".into(),
            kind: SymbolKind::Graphic,
            timeline: Timeline {
                guides: vec![],
                frame_count: 1,
                layers: vec![layer("Shape", vec![key(0, vec![dot], None)])],
                scripts: vec![],
            },
        });

        let orbit_id = new_id("symbol");
        let ring = Element {
            accessibility: None,
            id: new_id("element"),
            name: "Ring".into(),
            transform: Transform::default(),
            kind: ElementKind::Ellipse {
                width: 216.,
                height: 216.,
                fill: [0, 0, 0, 0],
                stroke: [68, 84, 109, 255],
                stroke_width: 2.,
            },
            scripts: vec![],
        };
        let ring_inner = Element {
            accessibility: None,
            id: new_id("element"),
            name: "Inner ring".into(),
            transform: Transform {
                x: 42.,
                y: 42.,
                ..Default::default()
            },
            kind: ElementKind::Ellipse {
                width: 132.,
                height: 132.,
                fill: [0, 0, 0, 0],
                stroke: [45, 60, 82, 255],
                stroke_width: 1.,
            },
            scripts: vec![],
        };
        let center = ellipse("Core", 78., 78., 60., 60., blue);
        let mut spark = instance(
            "spark",
            &spark_id,
            96.,
            -12.,
            InstancePlayback::SingleFrame(0),
        );
        let positions = [
            (96., -12.),
            (204., 96.),
            (96., 204.),
            (-12., 96.),
            (96., -12.),
        ];
        let keys = positions
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| {
                spark.transform.x = x;
                spark.transform.y = y;
                key(i as u32 * 15, vec![spark.clone()], Some(Easing::EaseInOut))
            })
            .collect();
        p.symbols.push(Symbol {
            folder: String::new(),
            class_name: None,
            linkage_id: None,
            id: orbit_id.clone(),
            name: "Orbit".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                guides: vec![],
                frame_count: 61,
                layers: vec![
                    layer(
                        "Guide rings",
                        vec![key(0, vec![ring, ring_inner, center], None)],
                    ),
                    layer("Satellite", keys),
                ],
                scripts: vec![],
            },
        });

        let mut main_orbit = instance(
            "mainOrbit",
            &orbit_id,
            640.,
            180.,
            InstancePlayback::Independent,
        );
        main_orbit.scripts.push(ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "this.rotation += 15;\ntrace(\"Orbit rotated\");".into(),
            },
        });
        let mut small_orbit = instance(
            "smallOrbit",
            &orbit_id,
            520.,
            390.,
            InstancePlayback::Independent,
        );
        small_orbit.transform.scale_x = 0.3;
        small_orbit.transform.scale_y = 0.3;
        small_orbit.transform.alpha = 0.7;
        let root = p.symbol_mut(&p.root.clone()).unwrap();
        root.timeline.layers = vec![
            layer(
                "Typography",
                vec![key(
                    0,
                    vec![
                        text(
                            "Eyebrow",
                            "NEOFLASH  /  MOTION STUDY 01",
                            64.,
                            58.,
                            13.,
                            muted,
                        ),
                        text("Headline", "Ideas in motion.", 60., 142., 52., white),
                        text(
                            "Description",
                            "One symbol. Many possibilities.",
                            64.,
                            224.,
                            21.,
                            muted,
                        ),
                        text(
                            "Description 2",
                            "A familiar timeline. A modern canvas.",
                            64.,
                            258.,
                            21.,
                            muted,
                        ),
                        text(
                            "Footer",
                            "EDIT A SYMBOL  ·  SCRUB THE TIMELINE  ·  EXPORT HTML5",
                            64.,
                            534.,
                            12.,
                            muted,
                        ),
                        text(
                            "Interaction hint",
                            "CLICK THE ORBIT IN WEB PREVIEW",
                            618.,
                            460.,
                            11.,
                            muted,
                        ),
                    ],
                    None,
                )],
            ),
            layer(
                "Accents",
                vec![key(
                    0,
                    vec![
                        rect("Accent", 64., 110., 40., 4., orange),
                        rect("Badge", 64., 337., 220., 48., [35, 48, 67, 255]),
                        text(
                            "Badge label",
                            "BUILT TO BE INTERACTIVE",
                            82.,
                            351.,
                            12.,
                            white,
                        ),
                        rect("Divider", 64., 510., 832., 1., [48, 60, 79, 255]),
                    ],
                    None,
                )],
            ),
            layer(
                "Orbit instances",
                vec![key(0, vec![main_orbit, small_orbit], None)],
            ),
        ];
        root.timeline.scripts = vec![FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "trace(\"NeoFlash: the stage is ready\");".into(),
            },
        }];
        p
    }
}

fn layer(name: &str, keyframes: Vec<Keyframe>) -> Layer {
    Layer {
        properties: Default::default(),
        id: new_id("layer"),
        name: name.into(),
        visible: true,
        locked: false,
        keyframes,
    }
}
fn key(frame: u32, elements: Vec<Element>, tween: Option<Easing>) -> Keyframe {
    Keyframe {
        frame,
        elements,
        tween,
        label: String::new(),
    }
}
fn rect(name: &str, x: f64, y: f64, width: f64, height: f64, fill: Color) -> Element {
    Element {
        accessibility: None,
        id: new_id("element"),
        name: name.into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
    }
}
fn ellipse(name: &str, x: f64, y: f64, width: f64, height: f64, fill: Color) -> Element {
    Element {
        kind: ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        ..rect(name, x, y, width, height, fill)
    }
}
fn text(name: &str, value: &str, x: f64, y: f64, font_size: f64, color: Color) -> Element {
    Element {
        accessibility: None,
        id: new_id("element"),
        name: name.into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Text {
            text: value.into(),
            font_family: "Helvetica Neue".into(),
            font_size,
            color,
        },
        scripts: vec![],
    }
}
fn instance(name: &str, symbol_id: &str, x: f64, y: f64, playback: InstancePlayback) -> Element {
    Element {
        accessibility: None,
        id: new_id("element"),
        name: name.into(),
        transform: Transform {
            x,
            y,
            ..Default::default()
        },
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback,
        },
        scripts: vec![],
    }
}
