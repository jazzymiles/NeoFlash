//! Motion tweens with independent property tracks, motion guides and tween kinds.
use neoflash_core::{
    Easing, Element, ElementKind, Keyframe, Layer, LayerProperties, LayerRole, MotionPathOverride,
    MotionPropertyKey, MotionTrackOverride, PathCommand, Project, Timeline, Transform, TweenKind,
    TweenProperty, evaluate, sample_layer,
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

fn object_pose(layer: &Layer, frame: u32, element_id: &str) -> Transform {
    sample_layer(layer, frame)
        .into_iter()
        .find(|element| element.id == element_id)
        .unwrap_or_else(|| panic!("missing sampled element {element_id}"))
        .transform
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
fn object_scoped_motion_keys_eases_and_options_do_not_broadcast() {
    let mut start = key(0, vec![rect("a", 0.0, 0.0), rect("b", 0.0, 0.0)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut a_track = MotionTrackOverride::new("a");
    a_track.property_eases = Some(vec![(TweenProperty::X, Easing::EaseIn)]);
    a_track.orient_to_path = Some(true);
    a_track.snap_to_guide = Some(true);
    a_track.sync_symbols = Some(true);
    let mut b_track = MotionTrackOverride::new("b");
    b_track.property_eases = Some(vec![(TweenProperty::Y, Easing::EaseOut)]);
    b_track.orient_to_path = Some(false);
    b_track.snap_to_guide = Some(false);
    b_track.sync_symbols = Some(false);
    start.motion_tracks = vec![a_track, b_track];

    let mut object_key = key(10, vec![rect("a", 100.0, 999.0), rect("b", 999.0, 200.0)]);
    object_key.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    object_key.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "a".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "b".into(),
            properties: vec![TweenProperty::Y],
            roving: false,
        },
    ]);

    let mut motion_layer = layer("motion", vec![start, object_key]);
    motion_layer.properties.end_frame = Some(11);
    let mut project = project(11, vec![motion_layer]);
    project.format_version = 9;
    assert!(project.validate().is_err(), "format 10 data must be gated");
    project.upgrade_format_for_features();
    assert_eq!(project.format_version, 10);
    project.validate().unwrap();

    let motion = &project.symbols[0].timeline.layers[0];
    let a = object_pose(motion, 5, "a");
    let b = object_pose(motion, 5, "b");
    let a_expected = neoflash_core::easing(5.0 / 11.0, Easing::EaseIn) * 11.0 / 10.0 * 100.0;
    let b_expected = neoflash_core::easing(5.0 / 11.0, Easing::EaseOut) * 11.0 / 10.0 * 200.0;
    assert!((a.x - a_expected).abs() < 1e-9, "A X uses its EaseIn track");
    assert_eq!(a.y, 0.0, "B's Y key must not key A's Y track");
    assert_eq!(b.x, 0.0, "A's X key must not key B's X track");
    assert!(
        (b.y - b_expected).abs() < 1e-9,
        "B Y uses its EaseOut track"
    );

    let span_start = &motion.keyframes[0];
    assert!(span_start.motion_orient_to_path_for("a"));
    assert!(!span_start.motion_orient_to_path_for("b"));
    assert!(span_start.motion_snap_to_guide_for("a"));
    assert!(!span_start.motion_snap_to_guide_for("b"));
    assert!(span_start.motion_sync_symbols_for("a"));
    assert!(!span_start.motion_sync_symbols_for("b"));

    let json = project.to_json().unwrap();
    assert!(json.contains("\"motion_tracks\""));
    assert!(json.contains("\"motion_keys\""));
    assert!(json.contains("\"sync_symbols\""));
    assert_eq!(Project::from_json(&json).unwrap(), project);

    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let original = project.clone();
    let mut cleared = project.clone();
    let root = cleared.root.clone();
    edit_frames(
        &mut cleared,
        &root,
        TimelineEdit::Clear {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 4,
                end: 5,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    cleared.validate().unwrap();
    let cleared_layer = &cleared.symbols[0].timeline.layers[0];
    for frame in (0..4).chain(6..11) {
        for id in ["a", "b"] {
            let before = object_pose(&original.symbols[0].timeline.layers[0], frame, id);
            let after = object_pose(cleared_layer, frame, id);
            assert!(
                (before.x - after.x).abs() < 1e-6 && (before.y - after.y).abs() < 1e-6,
                "range subdivision changed {id} at frame {frame}: {before:?} vs {after:?}"
            );
        }
    }

    let legacy = curved_motion_project();
    let legacy_json = legacy.to_json().unwrap();
    assert!(!legacy_json.contains("motion_tracks"));
    assert!(!legacy_json.contains("motion_keys"));
    let reloaded = Project::from_json(&legacy_json).unwrap();
    assert_eq!(reloaded, legacy);
    for frame in 0..legacy.symbols[0].timeline.frame_count {
        assert_eq!(
            sample_layer(&reloaded.symbols[0].timeline.layers[0], frame),
            sample_layer(&legacy.symbols[0].timeline.layers[0], frame),
            "legacy format-8 behavior changed at frame {frame}"
        );
    }
}

#[test]
fn object_scoped_motion_paths_and_orientation_use_each_objects_reference() {
    let mut start = key(
        0,
        vec![rect("vertical", 0.0, 0.0), rect("horizontal", 0.0, 100.0)],
    );
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut vertical = MotionTrackOverride::new("vertical");
    vertical.orient_to_path = Some(true);
    vertical.motion_path = Some(MotionPathOverride::Path {
        commands: vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::CubicTo {
                cx1: 0.0,
                cy1: 100.0,
                cx2: 100.0,
                cy2: 100.0,
                x: 100.0,
                y: 0.0,
            },
        ],
    });
    let mut horizontal = MotionTrackOverride::new("horizontal");
    horizontal.orient_to_path = Some(false);
    horizontal.motion_path = Some(MotionPathOverride::Path {
        commands: vec![
            PathCommand::MoveTo { x: 0.0, y: 100.0 },
            PathCommand::LineTo { x: 100.0, y: 100.0 },
        ],
    });
    start.motion_tracks = vec![vertical, horizontal];

    let mut end = key(
        10,
        vec![
            rect("vertical", 100.0, 0.0),
            rect("horizontal", 100.0, 100.0),
        ],
    );
    end.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    end.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "vertical".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "horizontal".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y],
            roving: false,
        },
    ]);
    let mut project = project(11, vec![layer("motion", vec![start, end])]);
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let motion = &project.symbols[0].timeline.layers[0];
    let vertical = object_pose(motion, 5, "vertical");
    let horizontal = object_pose(motion, 5, "horizontal");
    assert!((vertical.x - 50.0).abs() < 0.05);
    assert!((vertical.y - 75.0).abs() < 0.05);
    assert!(
        vertical.rotation < -80.0 && vertical.rotation > -100.0,
        "unexpected oriented path rotation: {vertical:?}"
    );
    assert!((horizontal.x - 50.0).abs() < 1e-9);
    assert!((horizontal.y - 100.0).abs() < 1e-9);
    assert_eq!(horizontal.rotation, 0.0);

    let original = project.clone();
    let mut reordered = project.clone();
    for key in &mut reordered.symbols[0].timeline.layers[0].keyframes {
        key.elements.reverse();
    }
    for frame in 0..11 {
        for id in ["vertical", "horizontal"] {
            assert_eq!(
                object_pose(&original.symbols[0].timeline.layers[0], frame, id),
                object_pose(&reordered.symbols[0].timeline.layers[0], frame, id),
                "element order changed {id}'s track at frame {frame}"
            );
        }
    }

    let mut deleted = reordered;
    for key in &mut deleted.symbols[0].timeline.layers[0].keyframes {
        key.elements.retain(|element| element.id != "vertical");
    }
    deleted.symbols[0].timeline.layers[0].normalize_motion_keys();
    deleted.validate().unwrap();
    assert_eq!(
        deleted.symbols[0].timeline.layers[0].keyframes[0]
            .motion_tracks
            .iter()
            .map(|track| track.element_id.as_str())
            .collect::<Vec<_>>(),
        ["horizontal"]
    );
    for frame in 0..11 {
        assert_eq!(
            object_pose(&original.symbols[0].timeline.layers[0], frame, "horizontal"),
            object_pose(&deleted.symbols[0].timeline.layers[0], frame, "horizontal"),
            "deleting a sibling changed the surviving track at frame {frame}"
        );
    }

    use neoflash_core::{FrameRange, TimelineEdit, edit_frames};
    let mut reversed = original.clone();
    let root = reversed.root.clone();
    edit_frames(
        &mut reversed,
        &root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 0,
                end: 10,
            },
        },
    )
    .unwrap();
    reversed.validate().unwrap();
    for frame in 0..11 {
        for id in ["vertical", "horizontal"] {
            let before = object_pose(&original.symbols[0].timeline.layers[0], 10 - frame, id);
            let after = object_pose(&reversed.symbols[0].timeline.layers[0], frame, id);
            assert!(
                (before.x - after.x).abs() < 1e-8
                    && (before.y - after.y).abs() < 1e-8
                    && (before.rotation - after.rotation).abs() < 1e-8,
                "reverse changed {id} at frame {frame}: {before:?} vs {after:?}"
            );
        }
    }

    let mut converted = original.clone();
    let root = converted.root.clone();
    edit_frames(
        &mut converted,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 2,
                end: 7,
            },
            blank: false,
        },
    )
    .unwrap();
    converted.validate().unwrap();
    for frame in 0..11 {
        for id in ["vertical", "horizontal"] {
            let before = object_pose(&original.symbols[0].timeline.layers[0], frame, id);
            let after = object_pose(&converted.symbols[0].timeline.layers[0], frame, id);
            assert!(
                (before.x - after.x).abs() < 1e-8
                    && (before.y - after.y).abs() < 1e-8
                    && (before.rotation - after.rotation).abs() < 1e-8,
                "Convert to keyframes changed {id} at frame {frame}: {before:?} vs {after:?}"
            );
        }
    }
}

#[test]
fn three_object_overlapping_keys_keep_roving_object_scoped() {
    let mut start = key(
        0,
        vec![
            rect("a", 0.0, 0.0),
            rect("b", 0.0, 0.0),
            rect("c", 0.0, 0.0),
        ],
    );
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;

    let mut at_two = key(
        2,
        vec![
            rect("a", 10.0, 999.0),
            rect("b", 10.0, 999.0),
            rect("c", 999.0, 999.0),
        ],
    );
    at_two.keyed_properties = Some(vec![TweenProperty::X]);
    at_two.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "a".into(),
            properties: vec![TweenProperty::X],
            roving: true,
        },
        MotionPropertyKey {
            element_id: "b".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
    ]);

    let mut at_five = key(
        5,
        vec![
            rect("a", 999.0, 999.0),
            rect("b", 999.0, 999.0),
            rect("c", 999.0, 50.0),
        ],
    );
    at_five.keyed_properties = Some(vec![TweenProperty::Y]);
    at_five.motion_keys = Some(vec![MotionPropertyKey {
        element_id: "c".into(),
        properties: vec![TweenProperty::Y],
        roving: false,
    }]);

    let mut at_ten = key(
        10,
        vec![
            rect("a", 100.0, 999.0),
            rect("b", 100.0, 999.0),
            rect("c", 999.0, 100.0),
        ],
    );
    at_ten.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    at_ten.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "a".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "b".into(),
            properties: vec![TweenProperty::X],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "c".into(),
            properties: vec![TweenProperty::Y],
            roving: false,
        },
    ]);

    let mut project = project(
        11,
        vec![layer("motion", vec![start, at_two, at_five, at_ten])],
    );
    project.upgrade_format_for_features();
    project.validate().unwrap();
    let motion = &project.symbols[0].timeline.layers[0];
    let a = object_pose(motion, 2, "a");
    let b = object_pose(motion, 2, "b");
    let c = object_pose(motion, 2, "c");
    assert!((a.x - 20.0).abs() < 1e-9, "A's roving key is retimed");
    assert!((b.x - 10.0).abs() < 1e-9, "B's key keeps authored time");
    assert_eq!(a.y, 0.0);
    assert_eq!(b.y, 0.0);
    assert!((c.y - 20.0).abs() < 1e-9);
    assert_eq!(c.x, 0.0, "unkeyed C X must ignore stored full poses");
    assert!(motion.keyframes[1].motion_key_roving_for("a"));
    assert!(!motion.keyframes[1].motion_key_roving_for("b"));
    assert!(!motion.keyframes[1].motion_key_roving_for("c"));
}

#[test]
fn object_scoped_guide_snap_leaves_sibling_track_unsnapped() {
    let mut start = key(0, vec![rect("snapped", 0.0, 0.0), rect("free", 0.0, 20.0)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut snapped = MotionTrackOverride::new("snapped");
    snapped.snap_to_guide = Some(true);
    let mut free = MotionTrackOverride::new("free");
    free.snap_to_guide = Some(false);
    start.motion_tracks = vec![snapped, free];

    let mut end = key(
        10,
        vec![rect("snapped", 100.0, 100.0), rect("free", 100.0, 120.0)],
    );
    end.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
    end.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "snapped".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "free".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y],
            roving: false,
        },
    ]);
    let mut motion = layer("motion", vec![start, end]);
    motion.properties.motion_guide = Some("guide".into());

    let mut guide = layer(
        "guide",
        vec![key(
            0,
            vec![Element {
                id: "path".into(),
                name: "path".into(),
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
                ..Element::default()
            }],
        )],
    );
    guide.properties.role = LayerRole::Guide;
    let mut project = project(11, vec![motion, guide]);
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let timeline = &project.symbols[0].timeline;
    let sampled = neoflash_core::sample_layer_in(timeline, &timeline.layers[0], 5);
    let snapped = sampled
        .iter()
        .find(|element| element.id == "snapped")
        .unwrap();
    let free = sampled.iter().find(|element| element.id == "free").unwrap();
    assert!((snapped.transform.x - 100.0).abs() < 1e-6);
    assert!(snapped.transform.y.abs() < 1e-6);
    assert!((free.transform.x - 50.0).abs() < 1e-9);
    assert!((free.transform.y - 70.0).abs() < 1e-9);
}

#[test]
fn object_motion_validation_and_legacy_key_materialization_are_strict() {
    let mut legacy_key = key(5, vec![rect("a", 10.0, 0.0), rect("b", 20.0, 0.0)]);
    legacy_key.keyed_properties = Some(vec![TweenProperty::X]);
    legacy_key.roving = true;
    legacy_key.materialize_motion_keys();
    assert_eq!(legacy_key.motion_keys.as_ref().unwrap().len(), 2);
    assert!(legacy_key.motion_key_roving_for("a"));
    assert!(legacy_key.motion_key_roving_for("b"));
    assert!(!legacy_key.roving);
    assert_eq!(legacy_key.keyed_properties, Some(vec![TweenProperty::X]));

    let mut start = key(0, vec![rect("a", 0.0, 0.0)]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut track = MotionTrackOverride::new("a");
    track.sync_symbols = Some(true);
    start.motion_tracks = vec![track];
    let mut end = key(10, vec![rect("a", 100.0, 0.0)]);
    end.keyed_properties = Some(vec![TweenProperty::X]);
    end.motion_keys = Some(vec![MotionPropertyKey {
        element_id: "a".into(),
        properties: vec![TweenProperty::X],
        roving: false,
    }]);
    let mut valid = project(11, vec![layer("motion", vec![start, end])]);
    valid.upgrade_format_for_features();
    valid.validate().unwrap();

    let mut duplicate_track = valid.clone();
    let duplicate =
        duplicate_track.symbols[0].timeline.layers[0].keyframes[0].motion_tracks[0].clone();
    duplicate_track.symbols[0].timeline.layers[0].keyframes[0]
        .motion_tracks
        .push(duplicate);
    assert!(duplicate_track.validate().is_err());

    let mut missing_track_object = valid.clone();
    missing_track_object.symbols[0].timeline.layers[0].keyframes[0].motion_tracks[0].element_id =
        "missing".into();
    assert!(missing_track_object.validate().is_err());

    let mut wrong_union = valid.clone();
    wrong_union.symbols[0].timeline.layers[0].keyframes[1].keyed_properties =
        Some(vec![TweenProperty::Y]);
    assert!(wrong_union.validate().is_err());

    let mut invalid_roving = valid.clone();
    invalid_roving.symbols[0].timeline.layers[0].keyframes[1].keyed_properties =
        Some(vec![TweenProperty::Alpha]);
    invalid_roving.symbols[0].timeline.layers[0].keyframes[1].motion_keys =
        Some(vec![MotionPropertyKey {
            element_id: "a".into(),
            properties: vec![TweenProperty::Alpha],
            roving: true,
        }]);
    assert!(invalid_roving.validate().is_err());

    let mut invalid_path = valid;
    invalid_path.symbols[0].timeline.layers[0].keyframes[0].motion_tracks[0].motion_path =
        Some(MotionPathOverride::Path {
            commands: vec![PathCommand::LineTo { x: 1.0, y: 1.0 }],
        });
    assert!(invalid_path.validate().is_err());
}

#[test]
fn targeted_symbol_sync_changes_only_the_requested_instance() {
    use neoflash_core::{InstancePlayback, Symbol, SymbolKind, sync_layer_symbol};
    let graphic = Symbol {
        id: "walk".into(),
        name: "walk".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 6,
            layers: vec![layer("content", vec![key(0, vec![])])],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    let instance = |id: &str| Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Instance {
            symbol_id: "walk".into(),
            playback: InstancePlayback::Loop,
        },
        ..Element::default()
    };
    let mut target_layer = layer(
        "instances",
        vec![
            key(0, vec![instance("a"), instance("b")]),
            key(3, vec![instance("a-next"), instance("b-next")]),
        ],
    );
    let mut project = Project::blank();
    project.symbols.push(graphic);

    assert_eq!(
        sync_layer_symbol(&project, &mut target_layer, 3, 3, "a-next"),
        1
    );
    let playback = |id: &str| match &target_layer.keyframes[1]
        .elements
        .iter()
        .find(|element| element.id == id)
        .unwrap()
        .kind
    {
        ElementKind::Instance { playback, .. } => *playback,
        _ => unreachable!(),
    };
    assert_eq!(playback("a-next"), InstancePlayback::LoopFrom(3));
    assert_eq!(playback("b-next"), InstancePlayback::Loop);
}

#[test]
fn object_scoped_motion_sync_continues_only_the_enabled_graphic() {
    use neoflash_core::{InstancePlayback, Symbol, SymbolKind};
    let graphic = Symbol {
        id: "walk".into(),
        name: "walk".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 6,
            layers: vec![layer(
                "content",
                (0..6)
                    .map(|frame| key(frame, vec![rect("dot", frame as f64 * 10.0, 0.0)]))
                    .collect(),
            )],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    let instance = |id: &str| Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Instance {
            symbol_id: "walk".into(),
            playback: InstancePlayback::Loop,
        },
        ..Element::default()
    };
    let mut start = key(0, vec![instance("a"), instance("b")]);
    start.tween = Some(Easing::Linear);
    start.tween_kind = TweenKind::Motion;
    let mut a = MotionTrackOverride::new("a");
    a.sync_symbols = Some(true);
    let mut b = MotionTrackOverride::new("b");
    b.sync_symbols = Some(false);
    start.motion_tracks = vec![a, b];
    let end = key(3, vec![instance("a-next"), instance("b-next")]);
    let mut project = project(4, vec![layer("instances", vec![start, end])]);
    project.symbols.push(graphic);
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let items = evaluate(&project, &project.root, 3);
    let nested_x = |placement: &str| {
        items
            .iter()
            .find(|item| item.instance_path.first().is_some_and(|id| id == placement))
            .unwrap_or_else(|| panic!("missing nested draw for {placement}"))
            .transform[4]
    };
    assert_eq!(nested_x("a-next"), 30.0);
    assert_eq!(nested_x("b-next"), 0.0);
}

#[test]
fn classic_symbol_replacements_restart_pure_child_age_without_resetting_ordinary_keys() {
    use neoflash_core::{InstancePlayback, Symbol, SymbolKind};

    // A classic span keeps one placement ID to interpolate its transform. At the
    // endpoint its new symbol must start at its own first frame during scrubbing,
    // just as an independent Player clock does during playback.
    for (playback, kind, expected_frames) in [
        (
            InstancePlayback::Independent,
            SymbolKind::MovieClip,
            [3, 0, 1, 2, 3],
        ),
        (InstancePlayback::Loop, SymbolKind::Graphic, [3, 0, 1, 2, 3]),
        (
            InstancePlayback::LoopFrom(2),
            SymbolKind::Graphic,
            [5, 2, 3, 4, 5],
        ),
        (
            InstancePlayback::PlayOnceFrom(6),
            SymbolKind::Graphic,
            [7, 6, 7, 7, 7],
        ),
    ] {
        let instance = |symbol: &str, x: f64| Element {
            kind: ElementKind::Instance {
                symbol_id: symbol.into(),
                playback,
            },
            ..rect("placement", x, 0.)
        };
        let keys = [(0, "before"), (2, "before"), (4, "after"), (6, "after")]
            .into_iter()
            .map(|(frame, symbol)| {
                let mut key = key(frame, vec![instance(symbol, f64::from(frame) * 10.)]);
                if frame < 6 {
                    key.tween = Some(Easing::Linear);
                }
                key
            })
            .collect();
        let mut project = project(8, vec![layer("classic", keys)]);
        for (id, base_x) in [("before", 0.), ("after", 1000.)] {
            project.symbols.push(Symbol {
                id: id.into(),
                name: id.into(),
                folder: String::new(),
                kind,
                timeline: Timeline {
                    frame_count: 8,
                    layers: vec![layer(
                        &format!("{id}-layer"),
                        (0..8)
                            .map(|frame| {
                                key(
                                    frame,
                                    vec![rect(
                                        &format!("{id}-dot"),
                                        base_x + f64::from(frame) * 10.,
                                        0.,
                                    )],
                                )
                            })
                            .collect(),
                    )],
                    scripts: vec![],
                    guides: vec![],
                },
                class_name: None,
                linkage_id: None,
            });
        }
        project.validate().unwrap();
        for (frame, child_frame) in (3..8).zip(expected_frames) {
            let items = evaluate(&project, &project.root, frame);
            assert_eq!(items.len(), 1);
            let (id, base_x) = if frame < 4 {
                ("before-dot", 0.)
            } else {
                ("after-dot", 1000.)
            };
            assert_eq!(items[0].element_id, id);
            assert_eq!(
                items[0].transform[4],
                base_x + f64::from(child_frame) * 10. + f64::from(frame.min(6)) * 10.,
                "{playback:?} at parent frame {frame}",
            );
        }
    }
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
    let json = include_str!("../../../examples/animation/timeline-tweens.nflash");
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

#[test]
fn t399_reverse_easing_exact_time_symmetry_and_double_reverse_restores_all_variants() {
    use neoflash_core::motion_tools::reverse_easing;
    use neoflash_core::{
        ClassicRotateDirection, FrameRange, LayerRole, MotionPathOverride, MotionPropertyKey,
        MotionTrackOverride, TimelineEdit, easing, edit_frames, sample_layer_in,
    };

    let eases = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Flash {
            acceleration: -75.0,
        },
        Easing::Flash { acceleration: 65.0 },
        Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        Easing::CubicBezier {
            x1: 0.34,
            y1: 1.56,
            x2: 0.64,
            y2: -0.45,
        },
        Easing::CubicBezier {
            x1: 0.15,
            y1: -12.0,
            x2: 0.85,
            y2: 14.5,
        },
    ];

    // 1. Direct mathematical identity: f_rev(u) == 1.0 - f(1.0 - u) within 1e-6 for all u in [0, 1].
    for ease in eases {
        let rev = reverse_easing(ease);
        let roundtrip = reverse_easing(rev);
        for step in 0..=200 {
            let u = f64::from(step) / 200.0;
            let lhs = easing(u, rev);
            let rhs = 1.0 - easing(1.0 - u, ease);
            assert!(
                (lhs - rhs).abs() < 1e-6,
                "reverse_easing({ease:?}) failed at u={u}: {lhs} vs {rhs}"
            );
            assert!(
                (easing(u, roundtrip) - easing(u, ease)).abs() < 1e-12,
                "double reverse_easing({ease:?}) drifted at u={u}"
            );
        }
    }

    // 2. Classic tween reversal across all invertible eases + CW <-> CCW + classic_rotate_turns + tween_scale.
    for ease in eases {
        let mut start = key(0, vec![rect("spin", 10.0, 20.0)]);
        start.elements[0].transform.rotation = 15.0;
        start.elements[0].transform.scale_x = 2.0;
        start.tween = Some(ease);
        start.classic_rotate_dir = ClassicRotateDirection::Cw;
        start.classic_rotate_turns = 2;
        start.tween_scale = false;
        let mut end = key(12, vec![rect("spin", 210.0, -60.0)]);
        end.elements[0].transform.rotation = 105.0;
        end.elements[0].transform.scale_x = 0.5;
        let mut orig = project(13, vec![layer("classic", vec![start, end])]);
        orig.upgrade_format_for_features();
        orig.validate().unwrap();

        let mut rev = orig.clone();
        let root = rev.root.clone();
        edit_frames(
            &mut rev,
            &root,
            TimelineEdit::Reverse {
                range: FrameRange {
                    layers: vec!["classic".into()],
                    start: 0,
                    end: 12,
                },
            },
        )
        .unwrap();
        rev.validate().unwrap();
        let rev_layer = &rev.symbols[0].timeline.layers[0];
        assert_eq!(
            rev_layer.keyframes.len(),
            2,
            "invertible classic tween ({ease:?}) must not bake on Reverse"
        );
        assert_eq!(
            rev_layer.keyframes[0].classic_rotate_dir,
            ClassicRotateDirection::Ccw
        );
        assert_eq!(rev_layer.keyframes[0].classic_rotate_turns, 2);
        assert!(!rev_layer.keyframes[0].tween_scale);
        assert_eq!(rev_layer.keyframes[1].tween, None);

        for k in 0..=12 {
            let before = sample_layer(&orig.symbols[0].timeline.layers[0], 12 - k)[0].transform;
            let after = sample_layer(rev_layer, k)[0].transform;
            assert!((before.x - after.x).abs() < 1e-6, "{ease:?} x at {k}");
            assert!((before.y - after.y).abs() < 1e-6, "{ease:?} y at {k}");
            let b_rad = before.rotation.to_radians();
            let a_rad = after.rotation.to_radians();
            assert!(
                (b_rad.cos() - a_rad.cos()).abs() < 1e-6
                    && (b_rad.sin() - a_rad.sin()).abs() < 1e-6,
                "{ease:?} rotation angle at {k}: {} vs {}",
                before.rotation,
                after.rotation
            );
        }

        // Reversing a second time restores original poses, CW direction, turns, and ease!
        let mut twice = rev.clone();
        edit_frames(
            &mut twice,
            &root,
            TimelineEdit::Reverse {
                range: FrameRange {
                    layers: vec!["classic".into()],
                    start: 0,
                    end: 12,
                },
            },
        )
        .unwrap();
        twice.validate().unwrap();
        let twice_layer = &twice.symbols[0].timeline.layers[0];
        assert_eq!(
            twice_layer.keyframes[0].classic_rotate_dir,
            ClassicRotateDirection::Cw
        );
        assert_eq!(twice_layer.keyframes[0].classic_rotate_turns, 2);
        for f in 0..=12 {
            let p0 = sample_layer(&orig.symbols[0].timeline.layers[0], f)[0].transform;
            let p2 = sample_layer(twice_layer, f)[0].transform;
            assert!((p0.x - p2.x).abs() < 1e-6);
            assert!((p0.y - p2.y).abs() < 1e-6);
            assert!((p0.rotation - p2.rotation).abs() < 1e-6);
        }
    }

    // 3. Guided Classic tween with QuadraticTo + CubicTo guide stroke and orient_to_path + snap_to_guide.
    let mut guide_layer = layer(
        "guide",
        vec![key(
            0,
            vec![Element {
                id: "gpath".into(),
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::QuadraticTo {
                            cx: 60.0,
                            cy: 120.0,
                            x: 120.0,
                            y: 40.0,
                        },
                        PathCommand::CubicTo {
                            cx1: 160.0,
                            cy1: -20.0,
                            cx2: 200.0,
                            cy2: 80.0,
                            x: 240.0,
                            y: 20.0,
                        },
                    ],
                    fill: [0; 4],
                    stroke: [0, 128, 255, 255],
                    stroke_width: 1.0,
                    fill_rule: neoflash_core::FillRule::NonZero,
                },
                ..Element::default()
            }],
        )],
    );
    guide_layer.properties.role = LayerRole::Guide;
    let mut g0 = key(0, vec![rect("rider", 0.0, 0.0)]);
    g0.tween = Some(Easing::Flash { acceleration: 65.0 });
    g0.orient_to_path = true;
    g0.snap_to_guide = true;
    let g10 = key(10, vec![rect("rider", 240.0, 20.0)]);
    let mut guided = layer("guided", vec![g0, g10]);
    guided.properties.motion_guide = Some("guide".into());
    let mut gproj = project(11, vec![guide_layer, guided]);
    gproj.upgrade_format_for_features();
    gproj.validate().unwrap();
    let mut grev = gproj.clone();
    let groot = grev.root.clone();
    edit_frames(
        &mut grev,
        &groot,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["guide".into(), "guided".into()],
                start: 0,
                end: 10,
            },
        },
    )
    .unwrap();
    grev.validate().unwrap();
    let ElementKind::Path {
        commands: gcmds, ..
    } = &grev.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind
    else {
        panic!("expected guide path");
    };
    assert!(
        gcmds
            .iter()
            .any(|c| matches!(c, PathCommand::QuadraticTo { .. }))
    );
    assert!(
        gcmds
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. }))
    );
    for k in 0..=10 {
        let b = sample_layer_in(
            &gproj.symbols[0].timeline,
            &gproj.symbols[0].timeline.layers[1],
            10 - k,
        )[0]
        .transform;
        let a = sample_layer_in(
            &grev.symbols[0].timeline,
            &grev.symbols[0].timeline.layers[1],
            k,
        )[0]
        .transform;
        assert!((b.x - a.x).abs() < 1e-6 && (b.y - a.y).abs() < 1e-6);
        if (1..10).contains(&k) {
            assert!((b.rotation - a.rotation).abs() < 1e-6);
        }
    }

    // 4. Motion tween with per-property eases, per-object motion_tracks, CubicTo path, and orient_to_path.
    let mut m_start = key(0, vec![rect("lead", 0.0, 0.0), rect("orbiter", 20.0, 50.0)]);
    m_start.tween = Some(Easing::EaseIn);
    m_start.tween_kind = TweenKind::Motion;
    m_start.property_eases = vec![
        (
            TweenProperty::X,
            Easing::Flash {
                acceleration: -75.0,
            },
        ),
        (
            TweenProperty::Y,
            Easing::CubicBezier {
                x1: 0.3,
                y1: 1.4,
                x2: 0.7,
                y2: -0.3,
            },
        ),
        (TweenProperty::ScaleX, Easing::EaseOut),
    ];
    let mut orb_track = MotionTrackOverride::new("orbiter");
    orb_track.easing = Some(Easing::Flash { acceleration: 65.0 });
    orb_track.orient_to_path = Some(true);
    orb_track.property_eases = Some(vec![
        (TweenProperty::X, Easing::EaseInOut),
        (TweenProperty::Y, Easing::EaseInOut),
    ]);
    orb_track.motion_path = Some(MotionPathOverride::Path {
        commands: vec![
            PathCommand::MoveTo { x: 20.0, y: 50.0 },
            PathCommand::CubicTo {
                cx1: 80.0,
                cy1: 190.0,
                cx2: 180.0,
                cy2: -40.0,
                x: 260.0,
                y: 90.0,
            },
        ],
    });
    m_start.motion_tracks = vec![orb_track];

    let mut m_end = key(
        16,
        vec![rect("lead", 180.0, 120.0), rect("orbiter", 260.0, 90.0)],
    );
    m_end.elements[0].transform.scale_x = 2.5;
    m_end.keyed_properties = Some(vec![
        TweenProperty::X,
        TweenProperty::Y,
        TweenProperty::ScaleX,
    ]);
    m_end.motion_keys = Some(vec![
        MotionPropertyKey {
            element_id: "lead".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y, TweenProperty::ScaleX],
            roving: false,
        },
        MotionPropertyKey {
            element_id: "orbiter".into(),
            properties: vec![TweenProperty::X, TweenProperty::Y],
            roving: false,
        },
    ]);

    let mut m_orig = project(17, vec![layer("motion", vec![m_start, m_end])]);
    m_orig.upgrade_format_for_features();
    m_orig.validate().unwrap();

    let mut m_rev = m_orig.clone();
    let m_root = m_rev.root.clone();
    edit_frames(
        &mut m_rev,
        &m_root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 0,
                end: 16,
            },
        },
    )
    .unwrap();
    m_rev.validate().unwrap();

    let m_rev_layer = &m_rev.symbols[0].timeline.layers[0];
    assert_eq!(
        m_rev_layer.keyframes.len(),
        2,
        "invertible Motion span must stay a 2-key Motion span on Reverse instead of baking"
    );
    assert_eq!(m_rev_layer.keyframes[0].tween_kind, TweenKind::Motion);
    assert_eq!(m_rev_layer.keyframes[0].tween, Some(Easing::EaseOut));
    assert!(
        m_rev_layer.keyframes[0]
            .property_eases
            .contains(&(TweenProperty::X, Easing::Flash { acceleration: 75.0 }))
    );
    let rev_orb_path = m_rev_layer.keyframes[0]
        .motion_track("orbiter")
        .and_then(|t| t.motion_path.as_ref());
    assert!(matches!(
        rev_orb_path,
        Some(MotionPathOverride::Path { commands })
            if commands.iter().any(|c| matches!(c, PathCommand::CubicTo { .. }))
    ));

    for k in 0..=16 {
        for id in ["lead", "orbiter"] {
            let b = object_pose(&m_orig.symbols[0].timeline.layers[0], 16 - k, id);
            let a = object_pose(m_rev_layer, k, id);
            assert!(
                (b.x - a.x).abs() < 1e-6
                    && (b.y - a.y).abs() < 1e-6
                    && (b.scale_x - a.scale_x).abs() < 1e-6
                    && (b.rotation - a.rotation).abs() < 1e-6,
                "motion reverse mismatch for {id} at {k}: {b:?} vs {a:?}"
            );
        }
    }

    let mut m_twice = m_rev.clone();
    edit_frames(
        &mut m_twice,
        &m_root,
        TimelineEdit::Reverse {
            range: FrameRange {
                layers: vec!["motion".into()],
                start: 0,
                end: 16,
            },
        },
    )
    .unwrap();
    m_twice.validate().unwrap();
    assert_eq!(
        m_twice.symbols[0].timeline.layers[0].keyframes[0].tween,
        Some(Easing::EaseIn)
    );
    for f in 0..=16 {
        for id in ["lead", "orbiter"] {
            let p0 = object_pose(&m_orig.symbols[0].timeline.layers[0], f, id);
            let p2 = object_pose(&m_twice.symbols[0].timeline.layers[0], f, id);
            assert!(
                (p0.x - p2.x).abs() < 1e-6
                    && (p0.y - p2.y).abs() < 1e-6
                    && (p0.scale_x - p2.scale_x).abs() < 1e-6
                    && (p0.rotation - p2.rotation).abs() < 1e-6
            );
        }
    }
}

#[test]
fn t399_de_casteljau_subdivide_and_reverse_preserve_quadratic_and_cubic_bezier_paths() {
    use neoflash_core::motion_tools::{
        ExactMotionCurve, reverse_path_commands, subdivide_path_commands,
    };
    use neoflash_core::{
        FrameRange, MotionPathOverride, MotionPropertyKey, MotionTrackOverride, TimelineEdit,
        edit_frames,
    };

    // 1. Direct QuadraticTo + CubicTo + LineTo exact subdivision & reversal.
    let mixed = vec![
        PathCommand::MoveTo { x: 10.0, y: 20.0 },
        PathCommand::QuadraticTo {
            cx: 70.0,
            cy: 150.0,
            x: 130.0,
            y: 30.0,
        },
        PathCommand::CubicTo {
            cx1: 170.0,
            cy1: -60.0,
            cx2: 240.0,
            cy2: 140.0,
            x: 300.0,
            y: 50.0,
        },
        PathCommand::LineTo { x: 360.0, y: 80.0 },
    ];
    let rev_mixed = reverse_path_commands(&mixed).unwrap();
    assert_eq!(
        rev_mixed,
        vec![
            PathCommand::MoveTo { x: 360.0, y: 80.0 },
            PathCommand::LineTo { x: 300.0, y: 50.0 },
            PathCommand::CubicTo {
                cx1: 240.0,
                cy1: 140.0,
                cx2: 170.0,
                cy2: -60.0,
                x: 130.0,
                y: 30.0,
            },
            PathCommand::QuadraticTo {
                cx: 70.0,
                cy: 150.0,
                x: 10.0,
                y: 20.0,
            },
        ]
    );
    assert_eq!(reverse_path_commands(&rev_mixed).unwrap(), mixed);

    let full_curve = ExactMotionCurve::from_commands(&mixed).unwrap();
    let sub_mid = subdivide_path_commands(&mixed, 0.15, 0.78).unwrap();
    assert!(
        sub_mid
            .iter()
            .any(|c| matches!(c, PathCommand::QuadraticTo { .. }))
    );
    assert!(
        sub_mid
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. }))
    );
    let sub_curve = ExactMotionCurve::from_commands(&sub_mid).unwrap();
    for step in 0..=50 {
        let u = f64::from(step) / 50.0;
        let orig_pt = full_curve.point_at((0.15 + (0.78 - 0.15) * u) * full_curve.length());
        let sub_pt = sub_curve.point_at(u * sub_curve.length());
        assert!(
            (orig_pt[0] - sub_pt[0]).hypot(orig_pt[1] - sub_pt[1]) < 1e-9,
            "De Casteljau sub-curve drifted at u={u}: {orig_pt:?} vs {sub_pt:?}"
        );
    }

    // 2. Both legacy `Keyframe.motion_path` AND format-10 `MotionPathOverride::Path` across
    //    SplitClip, Delete (head, middle, tail), Clear, Insert, Move, and Reverse!
    for use_object_path in [false, true] {
        let mut start = key(0, vec![rect("actor", 0.0, 0.0)]);
        start.tween = Some(Easing::Linear);
        start.tween_kind = TweenKind::Motion;
        start.orient_to_path = !use_object_path;
        let bezier_cmds = vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::CubicTo {
                cx1: 30.0,
                cy1: 160.0,
                cx2: 170.0,
                cy2: -80.0,
                x: 220.0,
                y: 60.0,
            },
        ];
        if use_object_path {
            let mut tr = MotionTrackOverride::new("actor");
            tr.orient_to_path = Some(true);
            tr.motion_path = Some(MotionPathOverride::Path {
                commands: bezier_cmds,
            });
            start.motion_tracks = vec![tr];
        } else {
            start.motion_path = Some(bezier_cmds);
        }
        let mut end = key(20, vec![rect("actor", 220.0, 60.0)]);
        end.keyed_properties = Some(vec![TweenProperty::X, TweenProperty::Y]);
        if use_object_path {
            end.motion_keys = Some(vec![MotionPropertyKey {
                element_id: "actor".into(),
                properties: vec![TweenProperty::X, TweenProperty::Y],
                roving: false,
            }]);
        }
        let mut base = project(21, vec![layer("curve", vec![start, end])]);
        base.upgrade_format_for_features();
        base.validate().unwrap();
        let root = base.root.clone();

        let assert_has_cubic = |k: &Keyframe| {
            let cmds = if use_object_path {
                match k.motion_track("actor").and_then(|t| t.motion_path.as_ref()) {
                    Some(MotionPathOverride::Path { commands }) => commands.as_slice(),
                    other => panic!(
                        "expected object motion path on frame {}, got {other:?}",
                        k.frame
                    ),
                }
            } else {
                k.motion_path
                    .as_deref()
                    .unwrap_or_else(|| panic!("expected legacy motion_path on frame {}", k.frame))
            };
            assert!(
                cmds.iter()
                    .any(|c| matches!(c, PathCommand::CubicTo { .. })),
                "expected editable CubicTo on frame {}, got {cmds:?}",
                k.frame
            );
        };

        // 2a. SplitClip at frame 9
        let mut split_proj = base.clone();
        edit_frames(
            &mut split_proj,
            &root,
            TimelineEdit::SplitClip {
                layers: vec!["curve".into()],
                at: 9,
            },
        )
        .unwrap();
        split_proj.validate().unwrap();
        let sl = &split_proj.symbols[0].timeline.layers[0];
        let left_k = sl
            .keyframes
            .iter()
            .find(|k| k.frame == 0 && k.keyed_properties.is_none())
            .unwrap();
        let right_k = sl
            .keyframes
            .iter()
            .find(|k| k.frame == 9 && k.keyed_properties.is_none())
            .unwrap();
        assert_has_cubic(left_k);
        assert_has_cubic(right_k);
        for f in 0..=20 {
            let b = object_pose(&base.symbols[0].timeline.layers[0], f, "actor");
            let a = object_pose(sl, f, "actor");
            assert!(
                (b.x - a.x).abs() < 1e-6
                    && (b.y - a.y).abs() < 1e-6
                    && (b.rotation - a.rotation).abs() < 1e-6,
                "SplitClip (obj={use_object_path}) drift at {f}: {b:?} vs {a:?}"
            );
        }

        // 2b. Delete head (0..=4), middle (7..=10), and tail (15..=20)
        for (del_start, del_end) in [(0u32, 4u32), (7, 10), (15, 20)] {
            let mut del_proj = base.clone();
            edit_frames(
                &mut del_proj,
                &root,
                TimelineEdit::Delete {
                    range: FrameRange {
                        layers: vec!["curve".into()],
                        start: del_start,
                        end: del_end,
                    },
                    include_scripts: false,
                },
            )
            .unwrap();
            del_proj.validate().unwrap();
            let dl = &del_proj.symbols[0].timeline.layers[0];
            let ord_starts: Vec<&Keyframe> = dl
                .keyframes
                .iter()
                .filter(|k| k.keyed_properties.is_none() && k.tween.is_some())
                .collect();
            assert!(
                !ord_starts.is_empty(),
                "Delete {del_start}..={del_end} must preserve editable motion span(s)"
            );
            for sk in &ord_starts {
                assert_has_cubic(sk);
            }
            let del_len = del_end - del_start + 1;
            for f in 0..(21 - del_len) {
                let src_f = if f < del_start { f } else { f + del_len };
                let b = object_pose(&base.symbols[0].timeline.layers[0], src_f, "actor");
                let a = object_pose(dl, f, "actor");
                assert!(
                    (b.x - a.x).abs() < 1e-6
                        && (b.y - a.y).abs() < 1e-6
                        && (b.rotation - a.rotation).abs() < 1e-6,
                    "Delete {del_start}..={del_end} (obj={use_object_path}) drift at {f}"
                );
            }
        }

        // 2c. Clear middle (6..=9)
        let mut clr_proj = base.clone();
        edit_frames(
            &mut clr_proj,
            &root,
            TimelineEdit::Clear {
                range: FrameRange {
                    layers: vec!["curve".into()],
                    start: 6,
                    end: 9,
                },
                include_scripts: false,
            },
        )
        .unwrap();
        clr_proj.validate().unwrap();
        let cl = &clr_proj.symbols[0].timeline.layers[0];
        assert_has_cubic(cl.keyframes.iter().find(|k| k.frame == 0).unwrap());
        assert_has_cubic(cl.keyframes.iter().find(|k| k.frame == 10).unwrap());
        for f in (0..6).chain(10..=20) {
            let b = object_pose(&base.symbols[0].timeline.layers[0], f, "actor");
            let a = object_pose(cl, f, "actor");
            assert!(
                (b.x - a.x).abs() < 1e-6
                    && (b.y - a.y).abs() < 1e-6
                    && (b.rotation - a.rotation).abs() < 1e-6
            );
        }

        // 2d. Insert frames (at 8, count 4) & Move sub-range (4..=12 -> at 22)
        let mut ins_proj = base.clone();
        edit_frames(
            &mut ins_proj,
            &root,
            TimelineEdit::Insert {
                layers: vec!["curve".into()],
                at: 8,
                count: 4,
                include_scripts: false,
            },
        )
        .unwrap();
        ins_proj.validate().unwrap();
        assert_has_cubic(&ins_proj.symbols[0].timeline.layers[0].keyframes[0]);

        let mut mov_proj = base.clone();
        edit_frames(
            &mut mov_proj,
            &root,
            TimelineEdit::Move {
                range: FrameRange {
                    layers: vec!["curve".into()],
                    start: 4,
                    end: 12,
                },
                layers: vec!["curve".into()],
                at: 24,
                copy: true,
                include_scripts: false,
            },
        )
        .unwrap();
        mov_proj.validate().unwrap();
        let ml = &mov_proj.symbols[0].timeline.layers[0];
        assert_has_cubic(
            ml.keyframes
                .iter()
                .find(|k| k.frame == 24 && k.keyed_properties.is_none())
                .unwrap(),
        );
        for offset in 0..=8 {
            let b = object_pose(&base.symbols[0].timeline.layers[0], 4 + offset, "actor");
            let a = object_pose(ml, 24 + offset, "actor");
            assert!((b.x - a.x).abs() < 1e-6 && (b.y - a.y).abs() < 1e-6);
        }

        // 2e. Reverse (0..=20)
        let mut rev_proj = base.clone();
        edit_frames(
            &mut rev_proj,
            &root,
            TimelineEdit::Reverse {
                range: FrameRange {
                    layers: vec!["curve".into()],
                    start: 0,
                    end: 20,
                },
            },
        )
        .unwrap();
        rev_proj.validate().unwrap();
        let rl = &rev_proj.symbols[0].timeline.layers[0];
        assert_eq!(rl.keyframes.len(), 2);
        assert_has_cubic(&rl.keyframes[0]);
        for k in 0..=20 {
            let b = object_pose(&base.symbols[0].timeline.layers[0], 20 - k, "actor");
            let a = object_pose(rl, k, "actor");
            assert!(
                (b.x - a.x).abs() < 1e-6
                    && (b.y - a.y).abs() < 1e-6
                    && (b.rotation - a.rotation).abs() < 1e-6
            );
        }
    }
}

#[test]
fn multi_scene_flattened_graphic_playback_matches_scene_local_authoring_1_to_1() {
    use neoflash_core::{InstancePlayback, Player, Symbol, SymbolKind, flatten_scenes_with_ranges};

    let graphic = Symbol {
        id: "walk-gfx".into(),
        name: "WalkGfx".into(),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![layer(
                "gfx-art",
                (0..4)
                    .map(|f| key(f, vec![rect("dot", f64::from(f) * 10.0, 0.0)]))
                    .collect(),
            )],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };
    let clip_sym = Symbol {
        id: "badge-mc".into(),
        name: "BadgeMc".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 5,
            layers: vec![layer(
                "mc-art",
                (0..5)
                    .map(|f| key(f, vec![rect("pip", f64::from(f) * 100.0, 0.0)]))
                    .collect(),
            )],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };

    let gfx_inst = |id: &str, playback: InstancePlayback, y: f64| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            y,
            ..Default::default()
        },
        kind: ElementKind::Instance {
            symbol_id: "walk-gfx".into(),
            playback,
        },
        ..Element::default()
    };

    // Scene 1 (5 frames): PlayOnce graphic that clamps before the scene boundary, sharing ID
    // "shared-once" with Scene 2 to verify scene-boundary lifetime isolation on flatten.
    let mut proj = Project::blank();
    proj.symbols[0].name = "Scene 1".into();
    proj.symbols[0].timeline = Timeline {
        frame_count: 5,
        layers: vec![layer(
            "s1-art",
            vec![key(
                0,
                vec![gfx_inst("shared-once", InstancePlayback::PlayOnce, 0.0)],
            )],
        )],
        scripts: vec![],
        guides: vec![],
    };

    // Scene 2 (8 frames): Loop, LoopFrom, PlayOnce, PlayOnceFrom, SingleFrame, held delayed span,
    // synced layer (including MovieClip-behavior instance), and Motion tween ◆ playback switch.
    let mut synced_mc = Element {
        id: "s2-synced-mc".into(),
        name: "s2-synced-mc".into(),
        transform: Transform {
            y: 300.0,
            ..Default::default()
        },
        kind: ElementKind::Instance {
            symbol_id: "badge-mc".into(),
            playback: InstancePlayback::Loop,
        },
        instance_behavior: Some(SymbolKind::MovieClip),
        ..Element::default()
    };
    let mut synced_layer = layer(
        "s2-synced",
        vec![
            key(0, vec![synced_mc.clone()]),
            key(
                2,
                vec![{
                    synced_mc.id = "s2-synced-mc-late".into();
                    synced_mc
                }],
            ),
        ],
    );
    synced_layer.properties.sync_timeline = true;

    let mut motion_start = key(
        0,
        vec![gfx_inst("s2-motion-gfx", InstancePlayback::Loop, 400.0)],
    );
    motion_start.tween = Some(Easing::Linear);
    motion_start.tween_kind = TweenKind::Motion;
    let mut motion_mid = key(
        4,
        vec![gfx_inst(
            "s2-motion-gfx",
            InstancePlayback::SingleFrame(2),
            420.0,
        )],
    );
    motion_mid.tween = Some(Easing::Linear);
    motion_mid.tween_kind = TweenKind::Motion;
    motion_mid.keyed_properties = Some(vec![TweenProperty::Y]);

    let scene2 = Symbol {
        id: "scene-2".into(),
        name: "Scene 2".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 8,
            layers: vec![
                layer(
                    "s2-standard",
                    vec![key(
                        0,
                        vec![
                            gfx_inst("shared-once", InstancePlayback::PlayOnce, 0.0),
                            gfx_inst("s2-loop", InstancePlayback::Loop, 50.0),
                            gfx_inst("s2-loop-from", InstancePlayback::LoopFrom(2), 100.0),
                            gfx_inst("s2-once-from", InstancePlayback::PlayOnceFrom(1), 150.0),
                            gfx_inst("s2-single", InstancePlayback::SingleFrame(3), 200.0),
                        ],
                    )],
                ),
                layer(
                    "s2-delayed",
                    vec![
                        key(0, vec![]),
                        key(
                            3,
                            vec![gfx_inst("s2-held", InstancePlayback::PlayOnce, 250.0)],
                        ),
                    ],
                ),
                synced_layer,
                layer("s2-motion", vec![motion_start, motion_mid]),
            ],
            scripts: vec![],
            guides: vec![],
        },
        class_name: None,
        linkage_id: None,
    };

    proj.scenes = vec![proj.root.clone(), scene2.id.clone()];
    proj.symbols.push(scene2);
    proj.symbols.push(graphic);
    proj.symbols.push(clip_sym);
    proj.upgrade_format_for_features();

    let (flat, ranges) = flatten_scenes_with_ranges(&proj);
    flat.validate().unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!((ranges[0].start, ranges[0].frames), (0, 5));
    assert_eq!((ranges[1].start, ranges[1].frames), (5, 8));

    let mut player = Player::new(&flat);
    player.set_scenes(&flat, ranges.clone()).unwrap();

    for (scene_idx, scene_id) in [(0usize, proj.root.as_str()), (1usize, "scene-2")] {
        let range = &ranges[scene_idx];
        for local_f in 0..range.frames {
            let flat_f = range.start + local_f;
            assert_eq!(player.frame(), flat_f);
            let authored = evaluate(&proj, scene_id, local_f);
            let flattened = evaluate(&flat, &flat.root, flat_f);
            let runtime_items = player.evaluate(&flat);
            assert_eq!(
                authored.len(),
                flattened.len(),
                "scene {scene_id} local frame {local_f} draw count"
            );
            assert_eq!(
                authored.len(),
                runtime_items.len(),
                "scene {scene_id} local frame {local_f} runtime draw count"
            );
            for (idx, ((a, b), r)) in authored
                .iter()
                .zip(&flattened)
                .zip(&runtime_items)
                .enumerate()
            {
                assert_eq!(
                    a.transform, b.transform,
                    "scene {scene_id} frame {local_f} draw {idx} flattened transform"
                );
                assert_eq!(
                    a.transform, r.transform,
                    "scene {scene_id} frame {local_f} draw {idx} Player transform"
                );
            }
            if flat_f + 1 < flat.symbol(&flat.root).unwrap().timeline.frame_count {
                player.tick(&flat);
            }
        }
    }
}

#[test]
fn classic_and_motion_tween_dummy_filters_fade_in_out_and_pair_mismatched_stacks() {
    use neoflash_core::{FilterSpec, IDENTITY_COLOR_MATRIX};

    let classic_filters = |a: &[FilterSpec], b: &[FilterSpec], frame: u32| -> Vec<FilterSpec> {
        let mut el_a = rect("box", 0.0, 0.0);
        el_a.filters = a.to_vec();
        let mut el_b = rect("box", 20.0, 0.0);
        el_b.filters = b.to_vec();
        let mut k0 = key(0, vec![el_a]);
        k0.tween = Some(Easing::Linear);
        let k2 = key(2, vec![el_b]);
        let lyr = layer("classic-f", vec![k0, k2]);
        sample_layer(&lyr, frame)[0].filters.clone()
    };

    let samples: Vec<FilterSpec> = vec![
        FilterSpec::Blur {
            blur_x: 12.0,
            blur_y: 8.0,
            quality: 2,
        },
        FilterSpec::Glow {
            color: 0xff3366,
            alpha: 0.8,
            blur_x: 10.0,
            blur_y: 10.0,
            strength: 2.0,
            quality: 2,
            inner: true,
            knockout: false,
        },
        FilterSpec::DropShadow {
            distance: 8.0,
            angle: 60.0,
            color: 0x112233,
            alpha: 0.9,
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 1.5,
            quality: 3,
            inner: false,
            knockout: true,
            hide_object: false,
        },
        FilterSpec::Bevel {
            distance: 6.0,
            angle: 45.0,
            highlight_color: 0xffffff,
            highlight_alpha: 0.8,
            shadow_color: 0x000000,
            shadow_alpha: 0.6,
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 2.0,
            quality: 2,
            placement: "outer".into(),
            knockout: false,
        },
        FilterSpec::GradientGlow {
            distance: 4.0,
            angle: 30.0,
            colors: vec![0xff0000, 0x00ff00],
            alphas: vec![0.4, 1.0],
            ratios: vec![0, 255],
            blur_x: 8.0,
            blur_y: 8.0,
            strength: 2.0,
            quality: 2,
            placement: "inner".into(),
            knockout: false,
        },
        FilterSpec::GradientBevel {
            distance: 4.0,
            angle: 120.0,
            colors: vec![0x0000ff, 0xffffff],
            alphas: vec![0.6, 0.8],
            ratios: vec![0, 255],
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 1.5,
            quality: 2,
            placement: "full".into(),
            knockout: false,
        },
        FilterSpec::ColorMatrix {
            matrix: [
                0.5, 0.0, 0.0, 0.0, 20.0, 0.0, 0.5, 0.0, 0.0, 20.0, 0.0, 0.0, 0.5, 0.0, 20.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
            ],
        },
        FilterSpec::Convolution {
            matrix_x: 3,
            matrix_y: 3,
            matrix: vec![0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0],
            divisor: 1.0,
            bias: 10.0,
            preserve_alpha: true,
            clamp: true,
            color: 0x000000,
            alpha: 0.5,
        },
    ];

    for spec in &samples {
        let fade_in_mid = classic_filters(&[], std::slice::from_ref(spec), 1);
        let fade_out_mid = classic_filters(std::slice::from_ref(spec), &[], 1);
        assert_eq!(fade_in_mid.len(), 1, "fade-in for {spec:?}");
        assert_eq!(fade_out_mid.len(), 1, "fade-out for {spec:?}");
        fade_in_mid[0].validate().unwrap();
        fade_out_mid[0].validate().unwrap();
        assert_eq!(
            fade_in_mid[0], fade_out_mid[0],
            "symmetric midpoint {spec:?}"
        );

        let fade_in_zero = classic_filters(&[], std::slice::from_ref(spec), 0);
        if let FilterSpec::ColorMatrix { matrix } = &fade_in_zero[0] {
            assert_eq!(*matrix, IDENTITY_COLOR_MATRIX);
        }
        if let FilterSpec::Convolution { matrix, bias, .. } = &fade_in_zero[0] {
            assert_eq!(
                matrix.as_slice(),
                &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]
            );
            assert!(bias.abs() < 1e-9);
        }
    }

    // Mismatched multi-filter stacks pair matching variants in order via LCS rather than greedy pop.
    let a_stack = vec![
        samples[1].clone(), // Glow (fades out)
        samples[0].clone(), // Blur(12, 8) -> pairs with Blur(4, 2)
        samples[2].clone(), // DropShadow(8) -> pairs with DropShadow(2)
    ];
    let b_stack = vec![
        FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 2.0,
            quality: 2,
        },
        FilterSpec::DropShadow {
            distance: 2.0,
            angle: 60.0,
            color: 0x112233,
            alpha: 0.5,
            blur_x: 2.0,
            blur_y: 2.0,
            strength: 0.5,
            quality: 3,
            inner: false,
            knockout: true,
            hide_object: false,
        },
        samples[3].clone(), // Bevel (fades in)
    ];
    let mid_stack = classic_filters(&a_stack, &b_stack, 1);
    assert_eq!(mid_stack.len(), 4);
    assert!(matches!(mid_stack[0], FilterSpec::Glow { alpha, .. } if (alpha - 0.4).abs() < 1e-6));
    assert!(
        matches!(mid_stack[1], FilterSpec::Blur { blur_x, blur_y, .. } if (blur_x - 8.0).abs() < 1e-6 && (blur_y - 5.0).abs() < 1e-6)
    );
    assert!(
        matches!(mid_stack[2], FilterSpec::DropShadow { distance, .. } if (distance - 5.0).abs() < 1e-6)
    );
    assert!(
        matches!(mid_stack[3], FilterSpec::Bevel { distance, .. } if (distance - 3.0).abs() < 1e-6)
    );

    // Motion tween hold-then-fade: Blur(12, 12) at frame 0, held Blur(12, 12) at ◆ frame 5, [] at ◆ frame 10.
    let mut el0 = rect("box", 0.0, 0.0);
    el0.filters = vec![FilterSpec::Blur {
        blur_x: 12.0,
        blur_y: 12.0,
        quality: 1,
    }];
    let mut el5 = rect("box", 50.0, 0.0);
    el5.filters = el0.filters.clone();
    let el10 = rect("box", 100.0, 0.0);

    let mut k0 = key(0, vec![el0]);
    k0.tween = Some(Easing::Linear);
    k0.tween_kind = TweenKind::Motion;
    let mut k5 = key(5, vec![el5]);
    k5.tween = Some(Easing::Linear);
    k5.tween_kind = TweenKind::Motion;
    k5.keyed_properties = Some(vec![TweenProperty::X]);
    let mut k10 = key(10, vec![el10]);
    k10.tween = Some(Easing::Linear);
    k10.tween_kind = TweenKind::Motion;
    k10.keyed_properties = Some(vec![TweenProperty::X]);

    let motion_layer = layer("motion-filters", vec![k0, k5, k10]);
    let proj = {
        let mut p = project(11, vec![motion_layer]);
        p.upgrade_format_for_features();
        p.validate().unwrap();
        p
    };
    let blur_at = |f: u32| match sample_layer(&proj.symbols[0].timeline.layers[0], f)[0]
        .filters
        .first()
    {
        Some(FilterSpec::Blur { blur_x, .. }) => *blur_x,
        None => 0.0,
        other => panic!("unexpected filter {other:?}"),
    };
    assert!((blur_at(3) - 12.0).abs() < 1e-6, "held across 0..=5");
    assert!((blur_at(5) - 12.0).abs() < 1e-6, "exact at ◆ frame 5");
    assert!((blur_at(8) - 4.8).abs() < 1e-6, "fades across 5..10");
    assert!(blur_at(10).abs() < 1e-6, "cleared at ◆ frame 10");
}
