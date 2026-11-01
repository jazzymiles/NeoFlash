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
