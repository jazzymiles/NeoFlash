use neoflash_core::{
    Easing, Element, ElementKind, InstancePlayback, InstancePoseError, Keyframe, Layer,
    LayerProperties, LayerRole, PathCommand, Project, Symbol, SymbolKind, Timeline, Transform,
    evaluate_instance_pose,
};

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
        ..Keyframe::default()
    }
}

fn symbol(id: &str, frame_count: u32, layers: Vec<Layer>) -> Symbol {
    Symbol {
        id: id.into(),
        name: id.into(),
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count,
            layers,
            scripts: vec![],
            guides: vec![],
        },
    }
}

fn instance(id: &str, symbol_id: &str, transform: Transform) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform,
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback: InstancePlayback::SingleFrame(2),
        },
        ..Element::default()
    }
}

fn shape(id: &str) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ..Element::default()
    }
}

fn project_with(root: Symbol, child: Symbol) -> Project {
    let mut project = Project::blank();
    project.root = root.id.clone();
    project.symbols = vec![root, child];
    project.format_version = 9;
    project
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

#[test]
fn exact_instance_pose_uses_the_sampled_target_not_a_same_symbol_sibling() {
    let first = instance(
        "first",
        "child",
        Transform {
            x: 10.,
            ..Transform::default()
        },
    );
    let second_start = instance(
        "second",
        "child",
        Transform {
            x: 100.,
            y: 20.,
            rotation: 10.,
            scale_x: 2.,
            scale_y: 3.,
            skew: 5.,
            ..Transform::default()
        },
    );
    let mut second_end = second_start.clone();
    second_end.transform.x = 200.;
    second_end.transform.y = 60.;
    second_end.transform.rotation = 50.;
    let mut start = key(0, vec![first.clone(), second_start]);
    start.tween = Some(Easing::Linear);
    let end = key(10, vec![first, second_end]);
    let project = project_with(
        symbol("root", 11, vec![layer("placements", vec![start, end])]),
        symbol("child", 4, vec![layer("child-layer", vec![key(0, vec![])])]),
    );

    let pose = evaluate_instance_pose(&project, "root", 5, "placements", "second").unwrap();
    assert_eq!(pose.symbol_id, "child");
    assert_eq!(pose.frame, 2);
    near(pose.matrix[4], 150.);
    near(pose.matrix[5], 40.);
    // The sibling placement of the same symbol is at x=10 and must not be selected.
    assert_ne!(pose.matrix[4], 10.);
    assert!(!pose.nonlinear_scale9);
}

#[test]
fn exact_instance_pose_includes_motion_guide_position_and_orientation() {
    let guide_path = Element {
        id: "guide-path".into(),
        name: String::new(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 50., y: 0. },
                PathCommand::LineTo { x: 50., y: 100. },
            ],
            fill_rule: Default::default(),
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.,
        },
        ..Element::default()
    };
    let mut guide = layer("motion-guide", vec![key(0, vec![guide_path])]);
    guide.properties.role = LayerRole::Guide;

    let mut start = key(0, vec![instance("guided", "child", Transform::default())]);
    start.tween = Some(Easing::Linear);
    start.snap_to_guide = true;
    start.orient_to_path = true;
    let end = key(
        10,
        vec![instance(
            "guided",
            "child",
            Transform {
                x: 100.,
                y: 100.,
                ..Transform::default()
            },
        )],
    );
    let mut placements = layer("placements", vec![start, end]);
    placements.properties.motion_guide = Some("motion-guide".into());
    let project = project_with(
        symbol("root", 11, vec![guide, placements]),
        symbol("child", 4, vec![layer("child-layer", vec![key(0, vec![])])]),
    );

    let pose = evaluate_instance_pose(&project, "root", 5, "placements", "guided").unwrap();
    near(pose.matrix[4], 50.);
    near(pose.matrix[5], 25.);
    let angle = pose.matrix[1].atan2(pose.matrix[0]).to_degrees();
    near(angle, 90.);
}

#[test]
fn exact_instance_pose_reports_invalid_identity_scale9_and_singular_matrices() {
    let placement = instance(
        "placed",
        "child",
        Transform {
            scale_x: 2.,
            ..Transform::default()
        },
    );
    let mut child = symbol("child", 4, vec![layer("child-layer", vec![key(0, vec![])])]);
    child.set_scale9_grid(Some([1., 1., 8., 8.]));
    let mut project = project_with(
        symbol(
            "root",
            1,
            vec![layer("placements", vec![key(0, vec![placement])])],
        ),
        child,
    );

    let pose = evaluate_instance_pose(&project, "root", 0, "placements", "placed").unwrap();
    assert!(pose.nonlinear_scale9);
    assert_eq!(
        evaluate_instance_pose(&project, "root", 1, "placements", "placed"),
        Err(InstancePoseError::FrameOutOfRange)
    );
    assert_eq!(
        evaluate_instance_pose(&project, "root", 0, "missing", "placed"),
        Err(InstancePoseError::LayerMissing)
    );
    assert_eq!(
        evaluate_instance_pose(&project, "root", 0, "placements", "missing"),
        Err(InstancePoseError::PlacementMissing)
    );

    project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .scale_x = 0.;
    assert_eq!(
        evaluate_instance_pose(&project, "root", 0, "placements", "placed"),
        Err(InstancePoseError::SingularMatrix)
    );

    project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .scale_x = 1.;
    project.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .x = f64::NAN;
    assert_eq!(
        evaluate_instance_pose(&project, "root", 0, "placements", "placed"),
        Err(InstancePoseError::NonFiniteMatrix)
    );

    project.symbols[0].timeline.layers[0].keyframes[0].elements[0] = shape("placed");
    assert_eq!(
        evaluate_instance_pose(&project, "root", 0, "placements", "placed"),
        Err(InstancePoseError::PlacementIsNotInstance)
    );
}
