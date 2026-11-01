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
    assert!((gx5 - 100.0).abs() < 1e-6 && gy5.abs() < 1e-6, "at corner: ({gx5}, {gy5})");
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
        assert!(matches!(right_start.tween, Some(Easing::CubicBezier { .. })));
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
    let m_right = m_keys.iter().find(|k| k.frame == 13).expect("right motion start");
    assert_eq!(m_right.tween_kind, TweenKind::Motion);
    assert!(
        m_keys.iter().any(|k| {
            k.frame == 16
                && k.keyed_properties == Some(vec![TweenProperty::X, TweenProperty::Y])
        })
    );
    for frame in (0..8).chain(13..=20) {
        let (ox, oy) = position(&m_orig, frame);
        let (ex, ey) = position(&m_edit, frame);
        assert!(
            (ox - ex).abs() < 1e-6 && (oy - ey).abs() < 1e-6,
            "motion frame {frame}: expected ({ox}, {oy}), got ({ex}, {ey})"
        );
    }
}
