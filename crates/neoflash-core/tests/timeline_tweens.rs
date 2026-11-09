//! Motion tweens with independent property tracks, motion guides and tween kinds.
use neoflash_core::{
    Easing, Element, ElementKind, Keyframe, Layer, LayerProperties, LayerRole, PathCommand,
    Project, Timeline, Transform, TweenKind, TweenProperty, evaluate, sample_layer,
};

fn rect(id: &str, x: f64, y: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    }
}

fn layer(id: &str, keyframes: Vec<Keyframe>) -> Layer {
    Layer {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes,
        properties: LayerProperties::default(),
    }
}

fn key(frame: u32, elements: Vec<Element>) -> Keyframe {
    Keyframe {
        frame,
        elements,
        ..Default::default()
    }
}

fn project(frame_count: u32, layers: Vec<Layer>) -> Project {
    let mut project = Project::blank();
    project.symbols[0].timeline = Timeline {
        frame_count,
        layers,
        scripts: vec![],
        guides: vec![],
    };
    project.format_version = 6;
    project
}

fn position(project: &Project, frame: u32) -> (f64, f64) {
    let item = evaluate(project, &project.root, frame)
        .into_iter()
        .next()
        .expect("drawn");
    (item.transform[4], item.transform[5])
}

#[test]
fn motion_tween_tracks_interpolate_independently() {
    // X is keyed at 10 and 20; Y only at 20. Y must interpolate over 0..20
    // instead of being pinned by the X key at frame 10.
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut x_key = key(10, vec![rect("box", 100., 999.)]);
    x_key.tween = Some(Easing::Linear);
    x_key.tween_kind = TweenKind::Motion;
    x_key.keyed_properties = Some(vec![TweenProperty::X]);
    let mut both = key(20, vec![rect("box", 100., 200.)]);
    both.tween = Some(Easing::Linear);
    both.tween_kind = TweenKind::Motion;
    both.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let project = project(30, vec![layer("motion", vec![start, x_key, both])]);
    project.validate().unwrap();
    assert_eq!(position(&project, 0), (0., 0.));
    assert_eq!(position(&project, 5), (50., 50.));
    assert_eq!(position(&project, 10), (100., 100.));
    assert_eq!(position(&project, 15), (100., 150.));
    // After the last property key every track holds its value.
    assert_eq!(position(&project, 25), (100., 200.));
    // Round trip keeps the tween kind and the keyed tracks.
    let reloaded = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(reloaded, project);
}

#[test]
fn property_keys_outside_a_motion_tween_are_rejected() {
    let mut stray = key(5, vec![rect("box", 0., 0.)]);
    stray.keyed_properties = Some(vec![TweenProperty::X]);
    let project = project(10, vec![layer("bad", vec![key(0, vec![]), stray])]);
    assert!(project.validate().is_err());
}

#[test]
fn format_6_features_need_version_6() {
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Shape;
    let mut project = project(10, vec![layer("shape", vec![start, key(5, vec![])])]);
    project.format_version = 5;
    assert!(project.validate().is_err());
    project.upgrade_format_for_features();
    assert_eq!(project.format_version, 6);
    project.validate().unwrap();
}

#[test]
fn shape_tweens_apply_expression_vector_effect_and_appearance_stacks() {
    use neoflash_core::{
        ElementAppearance, ElementVectorEffects, FreeformGradient, FreeformGradientPoint,
        PropertyExpression, VectorModifier, sample_layer_at_fps, shape_tween_elements,
    };

    let start_element = rect("shape", 0.0, 0.0);
    let mut end_element = rect("shape", 20.0, 0.0);
    if let ElementKind::Rectangle { width, height, .. } = &mut end_element.kind {
        *width = 20.0;
        *height = 30.0;
    }

    let baseline = shape_tween_elements(
        std::slice::from_ref(&start_element),
        std::slice::from_ref(&end_element),
        0.5,
    );
    let baseline_commands = baseline[0]
        .to_path_commands()
        .expect("shape tween produces a path")
        .0
        .len();

    let mut start = key(0, vec![start_element]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Shape;
    start.property_expressions = vec![PropertyExpression {
        property: TweenProperty::X,
        expression: "value + time".into(),
        ..PropertyExpression::default()
    }];
    start.vector_effects = vec![ElementVectorEffects {
        element_id: "shape".into(),
        freeform_gradient: Some(FreeformGradient {
            points: vec![FreeformGradientPoint {
                id: "green".into(),
                position: [0.0, 0.0],
                color: [10, 200, 30, 255],
                radius: 100.0,
                weight: 1.0,
            }],
            lines: Vec::new(),
        }),
        ..ElementVectorEffects::default()
    }];
    start.appearances = vec![ElementAppearance {
        element_id: "shape".into(),
        modifiers: vec![VectorModifier::Repeater {
            copies: 2,
            offset_x: 25.0,
            offset_y: 0.0,
            scale_step: 1.0,
            rotation_step: 0.0,
        }],
        ..ElementAppearance::default()
    }];
    let tween_layer = layer("shape", vec![start, key(10, vec![end_element])]);

    let sampled = sample_layer_at_fps(&tween_layer, 5, 10.0);
    assert_eq!(sampled.len(), 1);
    assert!((sampled[0].transform.x - (baseline[0].transform.x + 0.5)).abs() < 1e-9);
    let (commands, _) = sampled[0]
        .to_path_commands()
        .expect("effects preserve editable vector output");
    assert_eq!(commands.len(), baseline_commands * 2);
    match &sampled[0].kind {
        ElementKind::Path { fill, .. } | ElementKind::PaintedPath { fill, .. } => {
            assert_eq!(*fill, [10, 200, 30, 255]);
        }
        other => panic!("expected evaluated shape path, got {other:?}"),
    }
}

#[test]
fn project_frame_edits_bake_expressions_at_the_document_frame_rate() {
    use neoflash_core::{FrameRange, PropertyExpression, TimelineEdit, copy_frames, edit_frames};

    let mut start = key(0, vec![rect("clocked", 0.0, 0.0)]);
    start.property_expressions = vec![PropertyExpression {
        property: TweenProperty::X,
        expression: "time".into(),
        ..PropertyExpression::default()
    }];
    let mut project = project(61, vec![layer("animated", vec![start])]);
    project.fps = 60;
    project.upgrade_format_for_features();
    project.validate().unwrap();
    let root = project.root.clone();
    let range = FrameRange {
        layers: vec!["animated".into()],
        start: 30,
        end: 30,
    };

    let copied = copy_frames(&project, &root, &range, false).unwrap();
    assert!((copied.layers[0].keyframes[0].elements[0].transform.x - 0.5).abs() < 1e-9);

    edit_frames(
        &mut project,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range,
            blank: false,
        },
    )
    .unwrap();
    let baked = project.symbol(&root).unwrap().timeline.layers[0]
        .keyframes
        .iter()
        .find(|key| key.frame == 30)
        .unwrap();
    assert!((baked.elements[0].transform.x - 0.5).abs() < 1e-9);
}

#[test]
fn classic_tween_follows_its_motion_guide() {
    // An L-shaped guide: (0,0) → (100,0) → (100,100). The object moves from the
    // start of the path to its end along the corner, not on the straight diagonal.
    let guide_path = Element {
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
                PathCommand::LineTo { x: 100., y: 100. },
            ],
            fill_rule: Default::default(),
            fill: [0, 0, 0, 0],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.,
        },
        ..rect("path", 0., 0.)
    };
    let mut guide = layer("guide", vec![key(0, vec![guide_path])]);
    guide.properties.role = LayerRole::Guide;
    let mut start = key(0, vec![rect("box", 2., -3.)]);
    start.tween = Some(Easing::Linear);
    let mut guided = layer("guided", vec![start, key(10, vec![rect("box", 98., 104.)])]);
    guided.properties.motion_guide = Some("guide".into());
    let project = project(11, vec![guided, guide]);
    project.validate().unwrap();
    // Registration points snap to the path: nearest points are (2,0) and (100,100).
    let (x, y) = position(&project, 0);
    assert!((x - 2.).abs() < 1e-9 && y.abs() < 1e-9, "{x},{y}");
    // Halfway between arc lengths 2 and 200 is s=101: just past the corner.
    let (x, y) = position(&project, 5);
    assert!((x - 100.).abs() < 1e-6 && (y - 1.).abs() < 1e-6, "{x},{y}");
    let (x, y) = position(&project, 10);
    assert!(
        (x - 98.).abs() < 1e-9 && (y - 104.).abs() < 1e-9,
        "end keyframe is its own pose"
    );
    // The guide is authoring-only: published evaluation draws only the box.
    assert_eq!(
        neoflash_core::Player::new(&project)
            .runtime_snapshot(&project)
            .draws
            .len(),
        1
    );
    let mut snapped = project.clone();
    snapped.symbols[0].timeline.layers[0].keyframes[0].snap_to_guide = true;
    snapped.symbols[0].timeline.layers[0].normalize_motion_keys();
    snapped.format_version = 7;
    assert!(snapped.validate().is_err());
    snapped.upgrade_format_for_features();
    assert_eq!(snapped.format_version, 8);
    snapped.validate().unwrap();
    assert!(snapped.symbols[0].timeline.layers[0].keyframes[0].snap_to_guide);
    assert_eq!(position(&snapped, 0), (0., 0.));
    assert_eq!(position(&snapped, 5), (100., 0.));
}

#[test]
fn motion_guide_must_reference_a_guide_layer() {
    let mut guided = layer("guided", vec![key(0, vec![])]);
    guided.properties.motion_guide = Some("other".into());
    let project = project(5, vec![guided, layer("other", vec![key(0, vec![])])]);
    assert!(project.validate().is_err());
}

#[test]
fn classic_tween_kind_is_unchanged() {
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    let l = layer("classic", vec![start, key(10, vec![rect("box", 100., 0.)])]);
    assert_eq!(sample_layer(&l, 5)[0].transform.x, 50.);
}

fn xs(project: &Project, frames: std::ops::Range<u32>) -> Vec<f64> {
    frames.map(|frame| position(project, frame).0).collect()
}

#[test]
fn reverse_frames_mirrors_keys_and_keeps_outside_frames() {
    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    let mut middle = key(4, vec![rect("box", 40., 0.)]);
    middle.label = "mid".into();
    let tail = key(6, vec![rect("box", 100., 0.)]);
    let mut project = project(8, vec![layer("row", vec![start, middle, tail])]);
    let before = xs(&project, 0..8);
    assert_eq!(before, [0., 10., 20., 30., 40., 40., 100., 100.]);
    let root = project.root.clone();
    edit_frames(
        &mut project,
        &root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["row".into()],
                start: 0,
                end: 5,
            },
        },
    )
    .unwrap();
    // Frames 0..=5 play backwards; frames 6.. are untouched.
    assert_eq!(
        xs(&project, 0..8),
        [40., 40., 30., 20., 10., 0., 100., 100.]
    );
    let layer = &project.symbols[0].timeline.layers[0];
    assert_eq!(
        layer.key_at(1).unwrap().label,
        "mid",
        "labels travel with their key"
    );
}

#[test]
fn convert_to_keyframes_samples_each_frame_and_blank_clears() {
    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    let mut project = project(
        6,
        vec![layer(
            "row",
            vec![start, key(4, vec![rect("box", 40., 0.)])],
        )],
    );
    let root = project.root.clone();
    let range = FrameRange {
        layers: vec!["row".into()],
        start: 1,
        end: 2,
    };
    edit_frames(
        &mut project,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: range.clone(),
            blank: false,
        },
    )
    .unwrap();
    let frames: Vec<u32> = project.symbols[0].timeline.layers[0]
        .keyframes
        .iter()
        .map(|k| k.frame)
        .collect();
    assert_eq!(frames, [0, 1, 2, 3, 4]);
    // The pose is unchanged on every frame, including the tween after the range.
    assert_eq!(xs(&project, 0..5), [0., 10., 20., 30., 40.]);
    edit_frames(
        &mut project,
        &root,
        TimelineEdit::ConvertToKeyframes { range, blank: true },
    )
    .unwrap();
    assert!(evaluate(&project, &root, 1).is_empty());
    assert!(evaluate(&project, &root, 2).is_empty());
    assert_eq!(position(&project, 3).0, 30.);
}

#[test]
fn range_edits_keep_motion_tweens_valid() {
    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut a = key(4, vec![rect("box", 40., 0.)]);
    a.tween = start.tween;
    a.tween_kind = TweenKind::Motion;
    a.keyed_properties = Some(vec![TweenProperty::X]);
    let mut b = a.clone();
    b.frame = 8;
    b.elements = vec![rect("box", 80., 0.)];
    let mut project = project(10, vec![layer("row", vec![start, a, b])]);
    let root = project.root.clone();
    edit_frames(
        &mut project,
        &root,
        TimelineEdit::Delete {
            range: FrameRange {
                layers: vec!["row".into()],
                start: 4,
                end: 4,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    project.validate().unwrap();
}

#[test]
fn timeline_example_document_is_valid() {
    let json = include_str!("../../../examples/timeline-tweens.nflash");
    let project = Project::from_json(json).unwrap();
    assert_eq!(project.scene_ids().len(), 2);
    // Authoring evaluation draws the title, the morph, the star, the guided dot and the guide.
    assert_eq!(evaluate(&project, &project.root, 15).len(), 5);
}

#[test]
fn imported_fla_corpus_plays_through_the_timeline_engine() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus/imported");
    let mut files = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nflash") {
            continue;
        }
        files += 1;
        let project = Project::from_json(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let frames = project.symbol(&project.root).unwrap().timeline.frame_count;
        for frame in 0..frames {
            evaluate(&project, &project.root, frame);
        }
        let mut player = neoflash_core::Player::new(&project);
        for _ in 0..frames {
            player.tick(&project);
        }
    }
    assert!(files >= 8);
    // The imported MX shape tween morphs: its middle frame matches neither keyframe.
    let json = include_str!("../../../examples/fla-corpus/imported/ruffle-mx-shape-tween.nflash");
    let project = Project::from_json(json).unwrap();
    let paths = |frame| {
        evaluate(&project, &project.root, frame)
            .into_iter()
            .map(|item| format!("{:?}", item.kind))
            .collect::<Vec<_>>()
    };
    let (first, middle, last) = (paths(0), paths(15), paths(29));
    assert_eq!(middle.len(), first.len());
    assert_ne!(middle, first);
    assert_ne!(middle, last);
}

fn l_guide() -> Layer {
    // (0,0) → (100,0) → (100,100): tangent 0° on the first leg, 90° on the second.
    let guide_path = Element {
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
                PathCommand::LineTo { x: 100., y: 100. },
            ],
            fill_rule: Default::default(),
            fill: [0, 0, 0, 0],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.,
        },
        ..rect("path", 0., 0.)
    };
    let mut guide = layer("guide", vec![key(0, vec![guide_path])]);
    guide.properties.role = LayerRole::Guide;
    guide
}

#[test]
fn orient_to_path_turns_the_object_with_the_guide_tangent() {
    use neoflash_core::sample_layer_in;
    let mut box_start = rect("box", 2., 0.);
    box_start.transform.rotation = 10.;
    let mut start = key(0, vec![box_start]);
    start.tween = Some(Easing::Linear);
    start.orient_to_path = true;
    let mut guided = layer(
        "guided",
        vec![start, key(10, vec![rect("box", 100., 100.)])],
    );
    guided.properties.motion_guide = Some("guide".into());
    let mut project = project(11, vec![guided, l_guide()]);
    project.validate().unwrap();
    let rotation = |project: &Project, frame| {
        let timeline = &project.symbols[0].timeline;
        sample_layer_in(timeline, &timeline.layers[0], frame)[0]
            .transform
            .rotation
    };
    // On the first leg the start rotation holds; past the corner the box has turned 90°.
    assert!((rotation(&project, 0) - 10.).abs() < 1e-9);
    assert!((rotation(&project, 3) - 10.).abs() < 1e-9);
    assert!(
        (rotation(&project, 5) - 100.).abs() < 1e-9,
        "{}",
        rotation(&project, 5)
    );
    // The published evaluation (editor, runtime, HTML5) uses the same path.
    let item = evaluate(&project, &project.root, 5)
        .into_iter()
        .find(|item| matches!(item.kind, ElementKind::Rectangle { .. }) && item.transform[4] > 99.)
        .expect("box drawn");
    assert!((item.transform[1].atan2(item.transform[0]).to_degrees() - 100.).abs() < 1e-6);
    // Without the option the rotation interpolates between the keys (10 → 0).
    project.symbols[0].timeline.layers[0].keyframes[0].orient_to_path = false;
    assert!((rotation(&project, 5) - 5.).abs() < 1e-9);
    // The option is a format 6 feature and serialises only when set.
    let unguided = self::project(2, vec![layer("row", vec![key(0, vec![])])]);
    assert!(!unguided.uses_format_6());
    let mut oriented = unguided.clone();
    oriented.symbols[0].timeline.layers[0].keyframes[0].orient_to_path = true;
    assert!(oriented.uses_format_6());
    let plain =
        serde_json::to_string(&unguided.symbols[0].timeline.layers[0].keyframes[0]).unwrap();
    assert!(!plain.contains("orient_to_path"));
}

#[test]
fn synchronise_symbols_keeps_nested_graphics_in_step() {
    use neoflash_core::{
        FrameRange, InstancePlayback, Symbol, SymbolKind, TimelineEdit, edit_frames,
    };
    // A 10-frame graphic whose art moves 10 px per nested frame.
    let art = layer(
        "art",
        (0..10)
            .map(|f| key(f, vec![rect("dot", f as f64 * 10., 0.)]))
            .collect(),
    );
    let graphic = Symbol {
        id: "anim".into(),
        name: "anim".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 10,
            layers: vec![art],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    let instance = |id: &str, playback| Element {
        kind: ElementKind::Instance {
            symbol_id: "anim".into(),
            playback,
        },
        ..rect(id, 0., 0.)
    };
    // Frame 4 places another instance (fresh id) that restarts at nested frame 0; frame 6 keeps
    // that instance but was given another first frame.
    let row = layer(
        "row",
        vec![
            key(0, vec![instance("a", InstancePlayback::Loop)]),
            key(4, vec![instance("b", InstancePlayback::Loop)]),
            key(6, vec![instance("b", InstancePlayback::LoopFrom(2))]),
        ],
    );
    let mut project = project(9, vec![row]);
    project.symbols.push(graphic);
    project.validate().unwrap();
    assert_eq!(
        xs(&project, 0..9),
        [0., 10., 20., 30., 0., 10., 40., 50., 60.]
    );
    let root = project.root.clone();
    let range = FrameRange {
        layers: vec!["row".into()],
        start: 0,
        end: 8,
    };
    edit_frames(
        &mut project,
        &root,
        TimelineEdit::SyncSymbols {
            range: range.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        xs(&project, 0..9),
        [0., 10., 20., 30., 40., 50., 60., 70., 80.]
    );
    let keys = &project.symbols[0].timeline.layers[0].keyframes;
    assert!(matches!(
        keys[1].elements[0].kind,
        ElementKind::Instance {
            playback: InstancePlayback::LoopFrom(4),
            ..
        }
    ));
    // Running it again changes nothing and says so; the document is untouched.
    let before = project.clone();
    assert!(edit_frames(&mut project, &root, TimelineEdit::SyncSymbols { range }).is_err());
    assert_eq!(project, before);
}

#[test]
fn motion_tween_orient_to_path_turns_with_the_direction_of_travel() {
    // Move right to frame 10, then down to frame 20: oriented, the object turns 90°.
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    start.orient_to_path = true;
    let mut right = key(10, vec![rect("box", 100., 0.)]);
    right.tween = start.tween;
    right.tween_kind = TweenKind::Motion;
    right.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut down = right.clone();
    down.frame = 20;
    down.elements = vec![rect("box", 100., 100.)];
    let project = project(21, vec![layer("motion", vec![start, right, down])]);
    project.validate().unwrap();
    let rotation = |frame| {
        sample_layer(&project.symbols[0].timeline.layers[0], frame)[0]
            .transform
            .rotation
    };
    assert!(rotation(5).abs() < 1e-9);
    assert!((rotation(15) - 90.).abs() < 1e-9, "{}", rotation(15));
}

#[test]
fn bounce_and_elastic_eases_evaluate_exact_curves_and_require_format_7() {
    use neoflash_core::{as_easing, easing};
    for (variant, class) in [(Easing::Bounce, "Bounce"), (Easing::Elastic, "Elastic")] {
        assert_eq!(easing(0.0, variant), 0.0);
        assert_eq!(easing(1.0, variant), 1.0);
        for step in 1..20 {
            let t = step as f64 / 20.0;
            let expected =
                as_easing::ease(class, "easeOut", t, 0.0, 1.0, 1.0, f64::NAN, f64::NAN).unwrap();
            assert!(
                (easing(t, variant) - expected).abs() < 1e-12,
                "{class} mismatch at t={t}"
            );
        }
    }
    assert!(
        (1..20).any(|i| easing(i as f64 / 20.0, Easing::Elastic) > 1.0),
        "Elastic overshoots 1.0"
    );
    assert!(
        (0..=20).all(|i| {
            let v = easing(i as f64 / 20.0, Easing::Bounce);
            (0.0..=1.0).contains(&v)
        }),
        "Bounce stays within 0..=1"
    );

    // Format-6 project without format-7 features stays at format_version 6.
    let mut v6 = project(
        11,
        vec![layer(
            "classic",
            vec![
                Keyframe {
                    tween: Some(Easing::Linear),
                    ..key(0, vec![rect("box", 0., 0.)])
                },
                key(10, vec![rect("box", 100., 0.)]),
            ],
        )],
    );
    v6.validate().unwrap();
    let reloaded = Project::from_json(&v6.to_json().unwrap()).unwrap();
    assert_eq!(reloaded.format_version, 6);

    // Using Bounce requires format 7 and upgrades cleanly.
    v6.symbols[0].timeline.layers[0].keyframes[0].tween = Some(Easing::Bounce);
    assert!(v6.uses_format_7());
    assert!(v6.validate().is_err());
    v6.upgrade_format_for_features();
    assert_eq!(v6.format_version, 7);
    v6.validate().unwrap();
    let expected_x = easing(0.5, Easing::Bounce) * 100.0;
    assert!((position(&v6, 5).0 - expected_x).abs() < 1e-9);
}

#[test]
fn per_property_eases_evaluate_per_track_and_validate_invariants() {
    use neoflash_core::easing;
    let mut start = key(0, vec![rect("box", 0., 0.)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    start.property_eases = vec![
        (TweenProperty::X, Easing::EaseIn),
        (TweenProperty::Y, Easing::Bounce),
    ];
    let mut end = key(10, vec![rect("box", 100., 200.)]);
    end.tween = Some(Easing::Linear);
    end.tween_kind = TweenKind::Motion;
    end.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut proj = project(11, vec![layer("motion", vec![start, end])]);
    proj.upgrade_format_for_features();
    assert_eq!(proj.format_version, 7);
    proj.validate().unwrap();

    let (x5, y5) = position(&proj, 5);
    assert!((x5 - 25.0).abs() < 1e-9, "X uses EaseIn: {x5}");
    assert!(
        (y5 - easing(0.5, Easing::Bounce) * 200.0).abs() < 1e-9,
        "Y uses Bounce: {y5}"
    );
    let round_trip = Project::from_json(&proj.to_json().unwrap()).unwrap();
    assert_eq!(round_trip, proj);

    // Reject duplicate properties in property_eases.
    let mut dup = proj.clone();
    dup.symbols[0].timeline.layers[0].keyframes[0]
        .property_eases
        .push((TweenProperty::X, Easing::EaseOut));
    assert!(dup.validate().is_err());

    // Reject property_eases on a ◆ property key.
    let mut on_diamond = proj.clone();
    on_diamond.symbols[0].timeline.layers[0].keyframes[1].property_eases =
        vec![(TweenProperty::X, Easing::EaseOut)];
    assert!(on_diamond.validate().is_err());

    // Reject property_eases on a classic tween.
    let mut on_classic = proj;
    on_classic.symbols[0].timeline.layers[0].keyframes[1].keyed_properties = None;
    on_classic.symbols[0].timeline.layers[0].keyframes[0].tween_kind = TweenKind::Classic;
    assert!(on_classic.validate().is_err());
}

#[test]
fn motion_tween_sync_symbols_and_snap_to_guide() {
    use neoflash_core::{InstancePlayback, Symbol, SymbolKind};
    let graphic = Symbol {
        id: "walk".into(),
        name: "walk".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 6,
            layers: vec![layer(
                "g",
                (0..6)
                    .map(|f| key(f, vec![rect("dot", f as f64 * 10.0, 0.0)]))
                    .collect(),
            )],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    let inst = |id: &str| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Instance {
            symbol_id: "walk".into(),
            playback: InstancePlayback::Loop,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    };
    let mut k0 = key(0, vec![inst("a")]);
    k0.tween = Some(Easing::Linear);
    k0.tween_kind = TweenKind::Motion;
    k0.sync_symbols = true;
    let k3 = key(3, vec![inst("b")]);
    let mut sync_proj = project(6, vec![layer("row", vec![k0, k3])]);
    sync_proj.symbols.push(graphic);
    sync_proj.upgrade_format_for_features();
    sync_proj.validate().unwrap();
    // With sync_symbols on key 0, key 3 continues at graphic frame 3 (x = 30) instead of restarting at 0.
    assert_eq!(position(&sync_proj, 3).0, 30.0);

    // Motion tween with snap_to_guide follows an L-shaped guide path (0,0)->(100,0)->(100,100).
    let mut guide = layer(
        "guide",
        vec![key(
            0,
            vec![Element {
                id: "path".into(),
                name: "path".into(),
                transform: Transform::default(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 100.0, y: 0.0 },
                        PathCommand::LineTo { x: 100.0, y: 100.0 },
                    ],
                    fill_rule: Default::default(),
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 255],
                    stroke_width: 1.0,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            }],
        )],
    );
    guide.properties.role = LayerRole::Guide;

    let mut m0 = key(0, vec![rect("ship", 0.0, 0.0)]);
    m0.tween = Some(Easing::Linear);
    m0.tween_kind = TweenKind::Motion;
    m0.snap_to_guide = true;
    m0.orient_to_path = true;
    let mut m10 = key(10, vec![rect("ship", 100.0, 100.0)]);
    m10.tween = Some(Easing::Linear);
    m10.tween_kind = TweenKind::Motion;
    m10.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut guided = layer("guided", vec![m0, m10]);
    guided.properties.motion_guide = Some("guide".into());

    let mut guided_proj = project(11, vec![guided, guide]);
    guided_proj.upgrade_format_for_features();
    guided_proj.validate().unwrap();
    let (gx5, gy5) = position(&guided_proj, 5);
    assert!(
        (gx5 - 100.0).abs() < 1e-6 && gy5.abs() < 1e-6,
        "at corner: ({gx5}, {gy5})"
    );
    let rot2 = neoflash_core::sample_layer_in(
        &guided_proj.symbols[0].timeline,
        &guided_proj.symbols[0].timeline.layers[0],
        2,
    )[0]
    .transform
    .rotation;
    let rot8 = neoflash_core::sample_layer_in(
        &guided_proj.symbols[0].timeline,
        &guided_proj.symbols[0].timeline.layers[0],
        8,
    )[0]
    .transform
    .rotation;
    assert!(rot2.abs() < 1e-6);
    assert!((rot8 - 90.0).abs() < 1e-6);
}

#[test]
fn range_edits_preserve_editable_curves_and_motion_spans() {
    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    for ease in [
        Easing::CubicBezier {
            x1: 0.2,
            y1: 0.9,
            x2: 0.4,
            y2: 1.25,
        },
        Easing::EaseInOut,
    ] {
        let mut start = key(0, vec![rect("box", 0.0, 0.0)]);
        start.tween = Some(ease);
        let end = key(20, vec![rect("box", 200.0, 100.0)]);
        let original = project(21, vec![layer("classic", vec![start, end])]);
        let mut edited = original.clone();
        let root = edited.root.clone();
        edit_frames(
            &mut edited,
            &root,
            TimelineEdit::Clear {
                range: FrameRange {
                    layers: vec!["classic".into()],
                    start: 8,
                    end: 12,
                },
                include_scripts: false,
            },
        )
        .unwrap();
        edited.validate().unwrap();
        let keys = &edited.symbols[0].timeline.layers[0].keyframes;
        assert!(
            keys.len() < 10,
            "curve was subdivided cleanly, not baked frame-by-frame: {} keys",
            keys.len()
        );
        assert!(matches!(keys[0].tween, Some(Easing::CubicBezier { .. })));
        let right_start = keys.iter().find(|k| k.frame == 13).expect("right sub-span");
        assert!(matches!(
            right_start.tween,
            Some(Easing::CubicBezier { .. })
        ));
        for frame in (0..8).chain(13..=20) {
            let (ox, oy) = position(&original, frame);
            let (ex, ey) = position(&edited, frame);
            assert!(
                (ox - ex).abs() < 1e-4 && (oy - ey).abs() < 1e-4,
                "{ease:?} frame {frame}: expected ({ox}, {oy}), got ({ex}, {ey})"
            );
        }
    }

    // Motion span (0..=20) with ◆ keys at 5, 10, 16: clearing 8..=12 keeps both surviving
    // sub-spans as TweenKind::Motion with their surviving ◆ keys!
    let mut m0 = key(0, vec![rect("box", 0.0, 0.0)]);
    m0.tween = Some(Easing::Linear);
    m0.tween_kind = TweenKind::Motion;
    let mut m5 = key(5, vec![rect("box", 50.0, 0.0)]);
    m5.tween = Some(Easing::Linear);
    m5.tween_kind = TweenKind::Motion;
    m5.keyed_properties = Some(vec![TweenProperty::X]);
    let mut m10 = key(10, vec![rect("box", 100.0, 50.0)]);
    m10.tween = Some(Easing::Linear);
    m10.tween_kind = TweenKind::Motion;
    m10.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut m16 = key(16, vec![rect("box", 160.0, 80.0)]);
    m16.tween = Some(Easing::Linear);
    m16.tween_kind = TweenKind::Motion;
    m16.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let m_orig = project(21, vec![layer("motion", vec![m0, m5, m10, m16])]);
    let mut m_edit = m_orig.clone();
    let root = m_edit.root.clone();
    edit_frames(
        &mut m_edit,
        &root,
        TimelineEdit::Clear {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 8,
                end: 12,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    m_edit.validate().unwrap();
    let m_keys = &m_edit.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(m_keys[0].tween_kind, TweenKind::Motion);
    assert!(
        m_keys
            .iter()
            .any(|k| k.frame == 5 && k.keyed_properties == Some(vec![TweenProperty::X]))
    );
    let m_right = m_keys
        .iter()
        .find(|k| k.frame == 13)
        .expect("right motion start");
    assert_eq!(m_right.tween_kind, TweenKind::Motion);
    assert!(m_keys.iter().any(|k| {
        k.frame == 16 && k.keyed_properties == Some(vec![TweenProperty::X, TweenProperty::Y])
    }));
    for frame in (0..8).chain(13..=20) {
        let (ox, oy) = position(&m_orig, frame);
        let (ex, ey) = position(&m_edit, frame);
        assert!(
            (ox - ex).abs() < 1e-6 && (oy - ey).abs() < 1e-6,
            "motion frame {frame}: expected ({ox}, {oy}), got ({ex}, {ey})"
        );
    }
}

#[test]
fn overshooting_tweens_clamp_alpha_dimensions_and_filters_when_converted_to_keyframes() {
    use neoflash_core::{
        FilterSpec, FrameRange, GradientKind, GradientPaint, GradientStop, OvalOptions,
        PrimitiveOptions, RectangleOptions, TimelineEdit, edit_frames,
    };

    let overshoot = Easing::CubicBezier {
        x1: 0.2,
        y1: -50.0,
        x2: 0.8,
        y2: 50.0,
    };

    // 1. Classic tween with overshooting easing on alpha, Rectangle/Ellipse dimensions,
    // parametric primitive options, and filter blur/alpha/strength.
    let mut start_box = rect("box", 0.0, 0.0);
    start_box.transform.alpha = 0.0;
    start_box.transform.primitive = Some(PrimitiveOptions::Rectangle(RectangleOptions {
        corner_radii: [0.0, 0.0, 0.0, 0.0],
        corners_locked: true,
    }));
    start_box.filters = vec![FilterSpec::Glow {
        color: 0xff0000,
        alpha: 0.0,
        blur_x: 0.0,
        blur_y: 0.0,
        strength: 0.0,
        quality: 1,
        inner: false,
        knockout: false,
    }];
    let mut end_box = rect("box", 100.0, 50.0);
    end_box.transform.alpha = 1.0;
    end_box.transform.primitive = Some(PrimitiveOptions::Rectangle(RectangleOptions {
        corner_radii: [12.0, 12.0, 12.0, 12.0],
        corners_locked: true,
    }));
    if let ElementKind::Rectangle {
        width,
        height,
        stroke_width,
        ..
    } = &mut end_box.kind
    {
        *width = 1.0;
        *height = 1.0;
        *stroke_width = 0.0;
    }
    end_box.filters = vec![FilterSpec::Glow {
        color: 0x00ff00,
        alpha: 1.0,
        blur_x: 200.0,
        blur_y: 200.0,
        strength: 200.0,
        quality: 1,
        inner: false,
        knockout: false,
    }];

    let mut start_oval = Element {
        id: "oval".into(),
        name: "oval".into(),
        transform: Transform {
            alpha: 0.0,
            primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                start_angle: 0.0,
                end_angle: 90.0,
                inner_radius: 0.0,
                close_path: true,
            })),
            ..Default::default()
        },
        kind: ElementKind::Ellipse {
            width: 50.0,
            height: 50.0,
            fill: [10, 20, 30, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        ..Default::default()
    };
    let mut end_oval = start_oval.clone();
    end_oval.transform.alpha = 1.0;
    end_oval.transform.primitive = Some(PrimitiveOptions::Oval(OvalOptions {
        start_angle: 360.0,
        end_angle: 360.0,
        inner_radius: 99.0,
        close_path: true,
    }));
    start_oval.filters = vec![FilterSpec::Blur {
        blur_x: 0.0,
        blur_y: 0.0,
        quality: 1,
    }];
    end_oval.filters = vec![FilterSpec::Blur {
        blur_x: 255.0,
        blur_y: 255.0,
        quality: 1,
    }];

    let mut c0 = key(0, vec![start_box, start_oval]);
    c0.tween = Some(overshoot);
    let c10 = key(10, vec![end_box, end_oval]);

    // 2. Motion tween with overshooting Alpha property track.
    let mut m_start_el = rect("m-box", 0.0, 0.0);
    m_start_el.transform.alpha = 0.0;
    let mut m_end_el = rect("m-box", 80.0, 40.0);
    m_end_el.transform.alpha = 1.0;
    let mut m0 = key(0, vec![m_start_el]);
    m0.tween = Some(overshoot);
    m0.tween_kind = TweenKind::Motion;
    let mut m10 = key(10, vec![m_end_el]);
    m10.tween = Some(overshoot);
    m10.tween_kind = TweenKind::Motion;
    m10.keyed_properties = Some(vec![
        TweenProperty::X,
        TweenProperty::Y,
        TweenProperty::Alpha,
    ]);

    // 3. Shape tween with overshooting alpha and radial focal_point_ratio.
    let grad = |focal: f64| GradientPaint {
        kind: GradientKind::Radial,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [255, 0, 0, 255],
            },
            GradientStop {
                ratio: 255,
                color: [0, 0, 255, 255],
            },
        ],
        focal_point_ratio: focal,
        ..Default::default()
    };
    let painted_box = |id: &str, alpha: f64, focal: f64| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            alpha,
            ..Default::default()
        },
        kind: ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 40.0, y: 0.0 },
                PathCommand::LineTo { x: 40.0, y: 20.0 },
                PathCommand::Close,
            ],
            fill_rule: Default::default(),
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            fill_gradient: Some(grad(focal)),
            stroke_gradient: None,
            stroke_width: 1.0,
        },
        ..Default::default()
    };
    let s_start = painted_box("s-box", 0.0, -1.0);
    let s_end = painted_box("s-box", 1.0, 1.0);
    let mut s0 = key(0, vec![s_start]);
    s0.tween = Some(overshoot);
    s0.tween_kind = TweenKind::Shape;
    let s10 = key(10, vec![s_end]);

    let mut proj = project(
        11,
        vec![
            layer("classic", vec![c0, c10]),
            layer("motion", vec![m0, m10]),
            layer("shape", vec![s0, s10]),
        ],
    );
    proj.upgrade_format_for_features();
    proj.validate().unwrap();

    let root = proj.root.clone();
    edit_frames(
        &mut proj,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: FrameRange {
                layers: vec!["classic".into(), "motion".into(), "shape".into()],
                start: 0,
                end: 10,
            },
            blank: false,
        },
    )
    .expect("converting overshooting tweens to keyframes must produce valid clamped frames");
    proj.validate().unwrap();
}

#[test]
fn timeline_edits_sample_guided_classic_tween_along_motion_guide_and_charge_budget_once() {
    use neoflash_core::{FrameRange, TimelineEdit, copy_frames, edit_frames};

    let make_guided_project = |frame_count: u32| {
        let mut guide = layer(
            "guide",
            vec![key(
                0,
                vec![Element {
                    id: "guide-path".into(),
                    name: "guide-path".into(),
                    transform: Transform::default(),
                    kind: ElementKind::Path {
                        commands: vec![
                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                            PathCommand::LineTo { x: 100.0, y: 0.0 },
                            PathCommand::LineTo { x: 100.0, y: 100.0 },
                        ],
                        fill_rule: Default::default(),
                        fill: [0, 0, 0, 0],
                        stroke: [0, 0, 0, 255],
                        stroke_width: 1.0,
                    },
                    ..Default::default()
                }],
            )],
        );
        guide.properties.role = LayerRole::Guide;

        let mut start = key(0, vec![rect("car", 0.0, 0.0)]);
        start.tween = Some(Easing::Linear);
        start.orient_to_path = true;
        let end = key(frame_count - 1, vec![rect("car", 100.0, 100.0)]);
        let mut guided = layer("guided", vec![start, end]);
        guided.properties.motion_guide = Some("guide".into());
        let mut p = project(frame_count, vec![guided, guide]);
        p.upgrade_format_for_features();
        p.validate().unwrap();
        p
    };

    // 1. ConvertToKeyframes and copy_frames on a guided classic tween sample along the guide path
    // (corner at (100, 0) on frame 5, not the unguided chord midpoint (50, 50)).
    let mut guided_proj = make_guided_project(11);
    let root = guided_proj.root.clone();
    let clip = copy_frames(
        &guided_proj,
        &root,
        &FrameRange {
            layers: vec!["guided".into()],
            start: 5,
            end: 5,
        },
        false,
    )
    .unwrap();
    let copied_pos = &clip.layers[0].keyframes[0].elements[0].transform;
    assert!(
        (copied_pos.x - 100.0).abs() < 1e-6 && copied_pos.y.abs() < 1e-6,
        "copy_frames must sample along motion guide at frame 5: ({}, {})",
        copied_pos.x,
        copied_pos.y
    );

    edit_frames(
        &mut guided_proj,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: FrameRange {
                layers: vec!["guided".into()],
                start: 0,
                end: 10,
            },
            blank: false,
        },
    )
    .unwrap();
    let baked_pos = &guided_proj.symbols[0].timeline.layers[0].keyframes[5].elements[0].transform;
    assert!(
        (baked_pos.x - 100.0).abs() < 1e-6 && baked_pos.y.abs() < 1e-6,
        "ConvertToKeyframes must sample along motion guide at frame 5: ({}, {})",
        baked_pos.x,
        baked_pos.y
    );

    // 2. A 60,000-frame guided classic tween converted to keyframes charges the bake budget once
    // (60,000 <= 100,000 MAX_BAKED_RECORDS) instead of double-charging (120,000 > 100,000).
    let mut large_guided = make_guided_project(60_000);
    let root = large_guided.root.clone();
    edit_frames(
        &mut large_guided,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: FrameRange {
                layers: vec!["guided".into()],
                start: 0,
                end: 59_999,
            },
            blank: false,
        },
    )
    .expect(
        "60,000-frame guided tween ConvertToKeyframes must not double-charge MAX_BAKED_RECORDS",
    );
}

#[test]
fn nonlinear_motion_tween_subdivision_and_shape_hints_and_swatches_roundtrip() {
    use neoflash_core::{FrameRange, ShapeHint, TimelineEdit, edit_frames};

    // 1. Non-linear motion tween preserves TweenKind::Motion and subdivides eases on Clear.
    let mut m0 = key(0, vec![rect("box", 0.0, 0.0)]);
    m0.tween = Some(Easing::EaseInOut);
    m0.tween_kind = TweenKind::Motion;
    m0.property_eases = vec![(TweenProperty::X, Easing::EaseIn)];
    let mut m20 = key(20, vec![rect("box", 200.0, 100.0)]);
    m20.tween = Some(Easing::EaseInOut);
    m20.tween_kind = TweenKind::Motion;
    m20.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut proj = project(21, vec![layer("motion", vec![m0, m20])]);
    proj.upgrade_format_for_features();
    proj.validate().unwrap();
    let root = proj.root.clone();
    edit_frames(
        &mut proj,
        &root,
        TimelineEdit::Clear {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 8,
                end: 12,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    proj.validate().unwrap();
    let keys = &proj.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(keys[0].tween_kind, TweenKind::Motion);
    assert!(
        keys.iter()
            .any(|k| k.frame == 13 && k.tween_kind == TweenKind::Motion)
    );

    // 2. Shape hints on a shape tween round-trip and require format 7.
    let mut s0 = key(0, vec![rect("s", 0.0, 0.0)]);
    s0.tween = Some(Easing::Linear);
    s0.tween_kind = TweenKind::Shape;
    s0.shape_hints = vec![ShapeHint {
        label: 'a',
        start_uv: [0.0, 0.0],
        end_uv: [1.0, 1.0],
    }];
    let s10 = key(10, vec![rect("s", 50.0, 50.0)]);
    let mut shape_proj = project(11, vec![layer("shape", vec![s0, s10])]);
    shape_proj.upgrade_format_for_features();
    assert_eq!(shape_proj.format_version, 7);
    shape_proj.validate().unwrap();
    let reloaded = Project::from_json(&shape_proj.to_json().unwrap()).unwrap();
    assert_eq!(reloaded, shape_proj);

    // 3. Document-level custom swatches round-trip cleanly.
    let mut swatch_proj = Project::blank();
    let palette = vec![[12, 34, 56, 255], [200, 100, 50, 128]];
    swatch_proj.set_custom_swatches(&palette);
    swatch_proj.validate().unwrap();
    assert_eq!(swatch_proj.custom_swatches(), palette);
    swatch_proj.set_custom_swatches(&[]);
    assert!(swatch_proj.custom_swatches().is_empty());
}

fn curved_motion_project() -> Project {
    let mut start = key(
        0,
        vec![rect("curve-box", 0., 0.), rect("follower", 30., 15.)],
    );
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    start.motion_path = Some(vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::CubicTo {
            cx1: 0.,
            cy1: 100.,
            cx2: 100.,
            cy2: 100.,
            x: 100.,
            y: 0.,
        },
    ]);
    let mut end = key(
        20,
        vec![rect("curve-box", 100., 0.), rect("follower", 130., 15.)],
    );
    end.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    let mut result = project(21, vec![layer("curve-motion", vec![start, end])]);
    result.upgrade_format_for_features();
    result
}

#[test]
fn editable_motion_curve_arc_length_tracks_orientation_and_old_document_loading() {
    let mut project = curved_motion_project();
    project.validate().unwrap();
    assert_eq!(project.format_version, 8);
    let timeline = &project.symbols[0].timeline;
    let middle = sample_layer(&timeline.layers[0], 10);
    assert!((middle[0].transform.x - 50.).abs() < 0.05);
    assert!((middle[0].transform.y - 75.).abs() < 0.05);
    assert!((middle[1].transform.x - middle[0].transform.x - 30.).abs() < 1e-9);
    assert!((middle[1].transform.y - middle[0].transform.y - 15.).abs() < 1e-9);
    // Equal time portions cover equal distances, rather than equal cubic parameters.
    let commands = timeline.layers[0].keyframes[0]
        .motion_path
        .as_ref()
        .unwrap();
    let path = neoflash_core::motion_path_polyline(commands).unwrap();
    for frame in [5, 10, 15] {
        let pose = sample_layer(&timeline.layers[0], frame)[0].transform;
        let distance = path.nearest([pose.x, pose.y]);
        assert!((distance / path.length() - f64::from(frame) / 20.).abs() < 0.001);
    }
    assert_eq!(position(&project, 20), (100., 0.));
    let reloaded = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(reloaded, project);

    project.symbols[0].timeline.layers[0].keyframes[0].orient_to_path = true;
    let oriented = sample_layer(&project.symbols[0].timeline.layers[0], 10)[0].transform;
    assert!(
        oriented.rotation < -80. && oriented.rotation > -100.,
        "{oriented:?}"
    );
    // Different temporal X/Y eases retain their independent track clocks.
    project.symbols[0].timeline.layers[0].keyframes[0].property_eases =
        vec![(TweenProperty::X, Easing::EaseIn)];
    let independently_eased = sample_layer(&project.symbols[0].timeline.layers[0], 10)[0].transform;
    assert!(independently_eased.x < 25.);
    assert!((independently_eased.y - 75.).abs() < 0.05);

    let old = Project::blank();
    let old_json = old.to_json().unwrap();
    assert!(!old_json.contains("motion_path"));
    assert_eq!(Project::from_json(&old_json).unwrap(), old);
    assert!(!old.uses_format_8());
}

#[test]
fn editable_motion_curve_validation_keys_and_range_edits() {
    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let project = curved_motion_project();
    let mut invalid = project.clone();
    invalid.format_version = 7;
    assert!(invalid.validate().is_err());
    for commands in [
        vec![],
        vec![PathCommand::MoveTo { x: 0., y: 0. }, PathCommand::Close],
        vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: f64::NAN, y: 0. },
        ],
    ] {
        let mut invalid = project.clone();
        invalid.symbols[0].timeline.layers[0].keyframes[0].motion_path = Some(commands);
        assert!(invalid.validate().is_err());
    }
    let mut invalid = project.clone();
    invalid.symbols[0].timeline.layers[0].keyframes[0].tween_kind = TweenKind::Classic;
    assert!(invalid.validate().is_err());
    invalid.symbols[0].timeline.layers[0].normalize_motion_keys();
    assert!(
        invalid.symbols[0].timeline.layers[0].keyframes[0]
            .motion_path
            .is_none()
    );

    // Inserting a position key sampled from the curve preserves its geometry and timing.
    let mut keyed = project.clone();
    let elements = sample_layer(&keyed.symbols[0].timeline.layers[0], 10);
    let mut midpoint = key(10, elements);
    midpoint.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    keyed.symbols[0].timeline.layers[0].keyframes.push(midpoint);
    keyed.symbols[0].timeline.layers[0].normalize_motion_keys();
    keyed.validate().unwrap();
    for frame in 0..21 {
        let before = position(&project, frame);
        let after = position(&keyed, frame);
        assert!((before.0 - after.0).abs() < 0.05 && (before.1 - after.1).abs() < 0.05);
    }

    // Range cuts use the existing exact-frame bake fallback for spatial curves.
    let mut cut = project.clone();
    let root = cut.root.clone();
    edit_frames(
        &mut cut,
        &root,
        TimelineEdit::Delete {
            range: FrameRange {
                layers: vec!["curve-motion".into()],
                start: 7,
                end: 9,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    cut.validate().unwrap();
    for frame in 0..18 {
        let before = position(&project, if frame >= 7 { frame + 3 } else { frame });
        let after = position(&cut, frame);
        assert!((before.0 - after.0).abs() < 1e-8 && (before.1 - after.1).abs() < 1e-8);
    }
}

#[test]
fn editable_motion_curve_reverse_and_cross_document_clipboard_preserve_poses() {
    use neoflash_core::{
        ClipboardPayload, FrameRange, InstancePlayback, TimelineEdit, edit_frames,
    };
    let project = curved_motion_project();
    let mut reversed = project.clone();
    let root = reversed.root.clone();
    edit_frames(
        &mut reversed,
        &root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["curve-motion".into()],
                start: 0,
                end: 20,
            },
        },
    )
    .unwrap();
    reversed.validate().unwrap();
    for frame in 0..21 {
        let before = position(&project, 20 - frame);
        let after = position(&reversed, frame);
        assert!((before.0 - after.0).abs() < 1e-8 && (before.1 - after.1).abs() < 1e-8);
    }

    let mut source = Project::blank();
    let mut symbol = project.symbols[0].clone();
    symbol.id = "curved-symbol".into();
    let placement = Element {
        kind: ElementKind::Instance {
            symbol_id: symbol.id.clone(),
            playback: InstancePlayback::Independent,
        },
        ..rect("curved-instance", 0., 0.)
    };
    source.symbols.push(symbol);
    source.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(placement.clone());
    source.upgrade_format_for_features();
    let clip =
        ClipboardPayload::capture(&source, vec![placement], "curve-source", [0., 0.]).unwrap();
    let clip = ClipboardPayload::from_text(&clip.to_text().unwrap())
        .unwrap()
        .unwrap();
    let destination = Project::blank();
    let result = clip
        .paste_into(
            &destination,
            "different-session",
            &destination.root,
            &destination.symbols[0].timeline.layers[0].id,
            0,
            [0., 0.],
        )
        .unwrap();
    result.project.validate().unwrap();
    assert_eq!(result.project.format_version, 8);
    let imported = result
        .project
        .symbols
        .iter()
        .find(|symbol| {
            symbol
                .timeline
                .layers
                .iter()
                .any(|layer| layer.keyframes.iter().any(|key| key.motion_path.is_some()))
        })
        .unwrap();
    let pose = sample_layer(&imported.timeline.layers[0], 10)[0].transform;
    assert!((pose.x - 50.).abs() < 0.05 && (pose.y - 75.).abs() < 0.05);
}

#[test]
fn layer_blend_mode_and_timeline_sync_crop_evaluate_and_roundtrip() {
    use neoflash_core::{
        BlendMode, InstancePlayback, Player, Symbol, SymbolKind, evaluate,
        layer_effective_blend_mode, layer_effectively_synced,
    };

    // 1. Layer blend mode inheritance (element Normal -> layer -> folder) and element override.
    let mut folder = layer("folder", vec![]);
    folder.properties.role = LayerRole::Folder;
    folder.properties.blend_mode = BlendMode::Overlay;

    let mut inherited_child = layer(
        "child-normal",
        vec![key(0, vec![rect("in-folder", 10.0, 0.0)])],
    );
    inherited_child.properties.parent = Some("folder".into());

    let mut explicit_elem = rect("explicit-screen", 20.0, 0.0);
    explicit_elem.blend_mode = BlendMode::Screen;
    let mut multiply_layer = layer(
        "multiply-layer",
        vec![key(0, vec![rect("inherit-mul", 0.0, 0.0), explicit_elem])],
    );
    multiply_layer.properties.blend_mode = BlendMode::Multiply;

    let mut proj = project(10, vec![folder, inherited_child, multiply_layer]);
    proj.upgrade_format_for_features();
    assert_eq!(proj.format_version, 8);
    proj.validate().unwrap();

    let tl = &proj.symbols[0].timeline;
    assert_eq!(
        layer_effective_blend_mode(tl, &tl.layers[1]),
        BlendMode::Overlay
    );
    assert_eq!(
        layer_effective_blend_mode(tl, &tl.layers[2]),
        BlendMode::Multiply
    );

    let player = Player::new(&proj);
    let nodes = player.display_nodes(&proj);
    let by_id = |id: &str| {
        nodes
            .iter()
            .find(|node| node.path.last().map(|s| s.as_str()) == Some(id))
            .expect("evaluated display node must exist")
    };
    assert_eq!(by_id("in-folder").blend_mode, BlendMode::Overlay);
    assert_eq!(by_id("inherit-mul").blend_mode, BlendMode::Multiply);
    assert_eq!(by_id("explicit-screen").blend_mode, BlendMode::Screen);

    // 2. Timeline-synchronized layer crop (`sync_timeline`): frames 0..6 (1..6) are empty and the
    // MovieClip appears at frame 6 (1-based frame 7) -> evaluates at its own frame 6 (1-based 7).
    let child_keyframes = (0..10)
        .map(|f| key(f, vec![rect("child-dot", f64::from(f) * 10.0, 0.0)]))
        .collect();
    let child_sym = Symbol {
        id: "child-mc".into(),
        name: "ChildMC".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 10,
            layers: vec![layer("child-layer", child_keyframes)],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    proj.symbols.push(child_sym);

    let mc_instance = Element {
        kind: ElementKind::Instance {
            symbol_id: "child-mc".into(),
            playback: InstancePlayback::Independent,
        },
        ..rect("cropped-mc", 0.0, 0.0)
    };
    let mut cropped_layer = layer("cropped", vec![key(0, vec![]), key(6, vec![mc_instance])]);
    proj.symbols[0].timeline.layers.push(cropped_layer.clone());

    // Without sync_timeline, at parent frame 6 (1-based frame 7) the MovieClip starts at frame 0 (x = 0).
    let unsynced_draws = evaluate(&proj, &proj.root, 6);
    let unsynced_dot = unsynced_draws
        .iter()
        .find(|d| d.element_id == "child-dot")
        .unwrap();
    assert!((unsynced_dot.transform[4] - 0.0).abs() < 1e-6);

    // With sync_timeline = true, at parent frame 6 (1-based frame 7) the MovieClip evaluates at frame 6 (x = 60).
    cropped_layer.properties.sync_timeline = true;
    *proj.symbols[0].timeline.layers.last_mut().unwrap() = cropped_layer;
    assert!(layer_effectively_synced(
        &proj.symbols[0].timeline,
        proj.symbols[0].timeline.layers.last().unwrap()
    ));
    let synced_draws_f6 = evaluate(&proj, &proj.root, 6);
    let synced_dot_f6 = synced_draws_f6
        .iter()
        .find(|d| d.element_id == "child-dot")
        .unwrap();
    assert!((synced_dot_f6.transform[4] - 60.0).abs() < 1e-6);

    let synced_draws_f8 = evaluate(&proj, &proj.root, 8);
    let synced_dot_f8 = synced_draws_f8
        .iter()
        .find(|d| d.element_id == "child-dot")
        .unwrap();
    assert!((synced_dot_f8.transform[4] - 80.0).abs() < 1e-6);

    proj.upgrade_format_for_features();
    proj.validate().unwrap();
    let reloaded = Project::from_json(&proj.to_json().unwrap()).unwrap();
    assert_eq!(reloaded, proj);
}

#[test]
fn shape_hints_ignore_unplaced_partition_contours_weld_merge_strokes_and_survive_reverse() {
    use neoflash_core::{
        FillRule, FrameRange, ShapeHint, ShapeHintGeometry, TimelineEdit, active_shape_hint_labels,
        default_shape_hint_uv, edit_frames, shape_tween_elements, shape_tween_elements_with_hints,
        weld_merge_strokes_for_tween,
    };

    let square_path = |x0: f64, y0: f64, size: f64| Element {
        id: "sq".into(),
        name: "sq".into(),
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: x0, y: y0 },
                PathCommand::LineTo {
                    x: x0 + size,
                    y: y0,
                },
                PathCommand::LineTo {
                    x: x0 + size,
                    y: y0 + size,
                },
                PathCommand::LineTo {
                    x: x0,
                    y: y0 + size,
                },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [200, 40, 40, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Default::default()
    };
    let on_curve = |el: &Element| -> Vec<[f64; 2]> {
        let (cmds, _) = el.to_path_commands().unwrap();
        cmds.into_iter()
            .filter_map(|c| match c {
                PathCommand::MoveTo { x, y }
                | PathCommand::LineTo { x, y }
                | PathCommand::QuadraticTo { x, y, .. }
                | PathCommand::CubicTo { x, y, .. } => Some([x, y]),
                PathCommand::Close => None,
            })
            .collect()
    };

    let from = [square_path(0.0, 0.0, 100.0)];
    let to = [square_path(0.0, 0.0, 100.0)];

    // SH-01: Unplaced (Red) hints or half-placed hints must not alter the morphed output.
    let unplaced = [
        ShapeHint {
            label: 'a',
            start_uv: default_shape_hint_uv('a'),
            end_uv: default_shape_hint_uv('a'),
        },
        ShapeHint {
            label: 'b',
            start_uv: [0.0, 0.0],
            end_uv: default_shape_hint_uv('b'),
        },
    ];
    assert!(active_shape_hint_labels(&from, &to, &unplaced).is_empty());
    assert_eq!(
        shape_tween_elements_with_hints(&from, &to, &unplaced, 0.5),
        shape_tween_elements(&from, &to, 0.5),
        "unplaced Red hints must not distort shape tween"
    );

    // SH-02: Piecewise contour partitioning between 2+ hints at asymmetric perimeter fractions.
    // Pin 'a' at (0,0)->(0,0) (0% -> 0%) and 'b' at (100,0)->(100,100) (25% -> 50%).
    // At t = 0.5, 'a' must be at (0,0) and 'b' must be at (100,50) simultaneously with zero slip.
    let two_hints = [
        ShapeHint {
            label: 'a',
            start_uv: [0.0, 0.0],
            end_uv: [0.0, 0.0],
        },
        ShapeHint {
            label: 'b',
            start_uv: [1.0, 0.0],
            end_uv: [1.0, 1.0],
        },
    ];
    assert_eq!(
        active_shape_hint_labels(&from, &to, &two_hints),
        vec!['a', 'b']
    );
    let mid_two = &shape_tween_elements_with_hints(&from, &to, &two_hints, 0.5)[0];
    let pts_two = on_curve(mid_two);
    assert!(
        pts_two
            .iter()
            .any(|p| (p[0] - 0.0).hypot(p[1] - 0.0) < 1e-5),
        "expected hint 'a' pinned at (0, 0), got {pts_two:?}"
    );
    assert!(
        pts_two
            .iter()
            .any(|p| (p[0] - 100.0).hypot(p[1] - 50.0) < 1e-5),
        "expected hint 'b' pinned at (100, 50), got {pts_two:?}"
    );

    // Out-of-order 3rd hint 'c' (placed at (100,100) on start [50%] but at (50,0) on end [12.5% < 50%])
    // is rejected as inactive while 'a' and 'b' stay active.
    let crossed_hints = [
        two_hints[0],
        two_hints[1],
        ShapeHint {
            label: 'c',
            start_uv: [1.0, 1.0],
            end_uv: [0.5, 0.0],
        },
    ];
    assert_eq!(
        active_shape_hint_labels(&from, &to, &crossed_hints),
        vec!['a', 'b']
    );

    // Open contour with an interior hint mapping x=20 (0.2) to x=80 (0.8) on a horizontal stroke.
    let open_line = |id: &str| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 50.0 },
                PathCommand::LineTo { x: 100.0, y: 50.0 },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [20, 20, 20, 255],
            stroke_width: 2.0,
        },
        ..Default::default()
    };
    let open_from = [open_line("line")];
    let open_to = [open_line("line")];
    let open_hint = [ShapeHint {
        label: 'a',
        start_uv: [0.2, 0.0],
        end_uv: [0.8, 0.0],
    }];
    assert_eq!(
        active_shape_hint_labels(&open_from, &open_to, &open_hint),
        vec!['a']
    );
    let open_mid = &shape_tween_elements_with_hints(&open_from, &open_to, &open_hint, 0.5)[0];
    assert!(
        on_curve(open_mid)
            .iter()
            .any(|p| (p[0] - 50.0).abs() < 1e-4 && (p[1] - 50.0).abs() < 1e-4),
        "open contour interior hint should interpolate 20 -> 80 through 50"
    );

    // SH-03: Merge Drawing stroke segments weld into a unified closed contour and rotate with fill.
    let merge_edge = |id: &str, a: [f64; 2], b: [f64; 2]| Element {
        id: id.into(),
        // Raw Merge fragments have document IDs but no user-addressable object identity.
        name: String::new(),
        merge: true,
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: a[0], y: a[1] },
                PathCommand::LineTo { x: b[0], y: b[1] },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        ..Default::default()
    };
    let mut merge_fill = square_path(0.0, 0.0, 100.0);
    merge_fill.merge = true;
    let merge_rect = vec![
        merge_fill,
        merge_edge("s0", [0.0, 0.0], [100.0, 0.0]),
        merge_edge("s1", [100.0, 0.0], [100.0, 100.0]),
        merge_edge("s2", [100.0, 100.0], [0.0, 100.0]),
        merge_edge("s3", [0.0, 100.0], [0.0, 0.0]),
    ];
    let rot_hint = [ShapeHint {
        label: 'a',
        start_uv: [0.0, 0.0],
        end_uv: [1.0, 1.0],
    }];
    let morphed_merge = shape_tween_elements_with_hints(&merge_rect, &merge_rect, &rot_hint, 0.5);
    assert_eq!(
        morphed_merge.len(),
        2,
        "4 merge stroke edges must weld into 1 closed stroke alongside the fill"
    );
    for elem in &morphed_merge {
        let pts = on_curve(elem);
        assert!(
            pts.iter().any(|p| (p[0] - 50.0).hypot(p[1] - 50.0) < 1e-4),
            "both fill and welded merge stroke must rotate corner (0,0)->(100,100) through (50,50)"
        );
    }
    let mut scripted_edges = vec![
        merge_edge("scripted-0", [0.0, 0.0], [100.0, 0.0]),
        merge_edge("scripted-1", [100.0, 0.0], [100.0, 100.0]),
    ];
    scripted_edges[1].scripts.push(neoflash_core::ObjectScript {
        event: neoflash_core::ObjectEvent::Click,
        script: neoflash_core::Script {
            language: neoflash_core::ScriptLanguage::JavaScript,
            source: "trace('keep me')".into(),
        },
    });
    let original_ids = |elements: &[Element]| {
        elements
            .iter()
            .map(|element| element.id.clone())
            .collect::<Vec<_>>()
    };
    let assert_ids_preserved = |elements: Vec<Element>, reason: &str| {
        let expected = original_ids(&elements);
        let actual = original_ids(&weld_merge_strokes_for_tween(&elements));
        assert_eq!(actual, expected, "{reason}");
    };
    assert_ids_preserved(
        scripted_edges,
        "welding must not discard authored object-handler IDs",
    );

    let mut named_edges = vec![
        merge_edge("named-0", [0.0, 0.0], [100.0, 0.0]),
        merge_edge("named-1", [100.0, 0.0], [100.0, 100.0]),
    ];
    named_edges[0].name = "firstEdge".into();
    named_edges[1].name = "secondEdge".into();
    assert_ids_preserved(
        named_edges,
        "user-addressable Merge strokes keep each authored ID",
    );

    let effect_edges = || {
        vec![
            merge_edge("effect-0", [0.0, 0.0], [100.0, 0.0]),
            merge_edge("effect-1", [100.0, 0.0], [100.0, 100.0]),
        ]
    };
    let mut cached = effect_edges();
    cached
        .iter_mut()
        .for_each(|edge| edge.cache_as_bitmap = true);
    assert_ids_preserved(cached, "cached Merge strokes keep their object boundaries");

    let mut blended = effect_edges();
    blended
        .iter_mut()
        .for_each(|edge| edge.blend_mode = neoflash_core::BlendMode::Multiply);
    assert_ids_preserved(blended, "blend groups must not be collapsed by welding");

    let mut filtered = effect_edges();
    filtered.iter_mut().for_each(|edge| {
        edge.filters = vec![neoflash_core::FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 4.0,
            quality: 1,
        }]
    });
    assert_ids_preserved(filtered, "per-object filters must remain per object");

    let mut translucent = effect_edges();
    translucent
        .iter_mut()
        .for_each(|edge| edge.transform.alpha = 0.5);
    assert_ids_preserved(
        translucent,
        "per-object alpha compositing must not be replaced by one welded alpha",
    );

    let mut metadata = effect_edges();
    metadata
        .iter_mut()
        .for_each(|edge| edge.transform.pivot = Some([12.0, 8.0]));
    assert_ids_preserved(
        metadata,
        "authoring transform metadata must retain the IDs it addresses",
    );

    // SH-04: Multi-contour compound shape (outer ring + inner hole) applies hints per contour.
    let compound = Element {
        id: "donut".into(),
        name: "donut".into(),
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 100.0 },
                PathCommand::LineTo { x: 0.0, y: 100.0 },
                PathCommand::Close,
                PathCommand::MoveTo { x: 30.0, y: 30.0 },
                PathCommand::LineTo { x: 70.0, y: 30.0 },
                PathCommand::LineTo { x: 70.0, y: 70.0 },
                PathCommand::LineTo { x: 30.0, y: 70.0 },
                PathCommand::Close,
            ],
            fill_rule: FillRule::EvenOdd,
            fill: [50, 120, 200, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Default::default()
    };
    let geom = ShapeHintGeometry::from_elements(std::slice::from_ref(&compound)).unwrap();
    assert_eq!(geom.contours.len(), 2);
    assert_eq!((geom.low, geom.high), ([0.0, 0.0], [100.0, 100.0]));
    let hole_hint = [ShapeHint {
        label: 'a',
        start_uv: [0.3, 0.3],
        end_uv: [0.7, 0.7],
    }];
    let compound_mid = &shape_tween_elements_with_hints(
        std::slice::from_ref(&compound),
        std::slice::from_ref(&compound),
        &hole_hint,
        0.5,
    )[0];
    let compound_pts = on_curve(compound_mid);
    assert!(
        compound_pts
            .iter()
            .any(|p| (p[0] - 0.0).hypot(p[1] - 0.0) < 1e-5),
        "outer contour corner (0,0) stays stationary when hint is on inner hole"
    );
    assert!(
        compound_pts
            .iter()
            .any(|p| (p[0] - 50.0).hypot(p[1] - 50.0) < 1e-5),
        "inner hole corner (30,30)->(70,70) passes through (50,50)"
    );

    // SH-11: TimelineEdit::Reverse preserves TweenKind::Shape and swaps start_uv <-> end_uv.
    let mut k0 = key(0, vec![square_path(0.0, 0.0, 100.0)]);
    k0.tween = Some(Easing::Linear);
    k0.tween_kind = TweenKind::Shape;
    k0.shape_hints = vec![ShapeHint {
        label: 'a',
        start_uv: [0.0, 0.0],
        end_uv: [1.0, 1.0],
    }];
    let k10 = key(10, vec![square_path(20.0, 10.0, 80.0)]);
    let mut proj = project(11, vec![layer("morph", vec![k0, k10])]);
    proj.upgrade_format_for_features();
    proj.validate().unwrap();
    let root = proj.root.clone();
    edit_frames(
        &mut proj,
        &root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["morph".into()],
                start: 0,
                end: 10,
            },
        },
    )
    .unwrap();
    proj.validate().unwrap();
    let reversed_keys = &proj.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(
        reversed_keys.len(),
        2,
        "reversing a shape tween must keep 2 keyframes instead of baking frame-by-frame"
    );
    assert_eq!(reversed_keys[0].tween_kind, TweenKind::Shape);
    assert_eq!(
        reversed_keys[0].shape_hints,
        vec![ShapeHint {
            label: 'a',
            start_uv: [1.0, 1.0],
            end_uv: [0.0, 0.0],
        }]
    );
    assert!(reversed_keys[1].shape_hints.is_empty());
}
