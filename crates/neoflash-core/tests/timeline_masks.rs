use image::ImageEncoder;
use neoflash_core::*;

fn rect(id: &str, width: f64, height: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Rectangle {
            width,
            height,
            fill: [230, 30, 10, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ..Element::default()
    }
}
fn contour(x: f64, y: f64, w: f64, h: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x, y },
        PathCommand::LineTo { x: x + w, y },
        PathCommand::LineTo { x: x + w, y: y + h },
        PathCommand::LineTo { x, y: y + h },
        PathCommand::Close,
    ]
}
fn gradient(id: &str) -> Element {
    Element {
        kind: ElementKind::PaintedPath {
            commands: contour(0., 0., 100., 100.),
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
            stroke_gradient: None,
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                matrix: [100. / 1638.4, 0., 0., 1., 50., 0.],
                focal_point_ratio: 0.,
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: [0; 4],
                    },
                    GradientStop {
                        ratio: 255,
                        color: [0, 0, 0, 255],
                    },
                ],
            }),
        },
        ..rect(id, 100., 100.)
    }
}
fn layer(id: &str, elements: Vec<Element>) -> Layer {
    Layer {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            elements,
            ..Default::default()
        }],
        properties: Default::default(),
    }
}
fn masked(sources: Vec<Element>, mode: MaskMode) -> Project {
    let mut project = Project::blank();
    project.format_version = 5;
    project.symbols[0].timeline.frame_count = 11;
    let mut content = layer(
        "content-layer",
        vec![rect("content", 200., 100.), rect("overlap", 200., 100.)],
    );
    content.properties.mask = Some("mask-layer".into());
    let mut mask = layer("mask-layer", sources);
    mask.properties.role = LayerRole::Mask;
    mask.properties.mask_mode = mode;
    project.symbols[0].timeline.layers = vec![content, mask];
    project.upgrade_format_for_features();
    project
}
fn coverage(project: &Project, frame: u32, points: &[[f64; 2]]) -> Vec<f64> {
    let scene = evaluate_scene(project, &project.root, frame, false, true);
    let masks = MaskCoverage::new(project, &scene.clips).unwrap();
    points
        .iter()
        .map(|point| masks.alpha(&scene.draws[0].clip_ids, *point))
        .collect()
}
#[test]
fn explicit_alpha_mode_roundtrips_without_changing_legacy_geometry_and_tweens() {
    let old = masked(vec![gradient("gradient")], MaskMode::Geometry);
    old.validate().unwrap();
    let json = old.to_json().unwrap();
    assert!(!json.contains("mask_mode"));
    assert_eq!(old.format_version, 5);
    assert_eq!(
        coverage(&old, 0, &[[0., 50.], [50., 50.], [99., 50.]]),
        vec![1., 1., 1.]
    );
    let mut project = Project::from_json(&json).unwrap();
    project.symbols[0].timeline.layers[1].properties.mask_mode = MaskMode::Alpha;
    assert!(project.validate().is_err());
    project.upgrade_format_for_features();
    assert_eq!(project.format_version, 9);
    let source = &mut project.symbols[0].timeline.layers[1];
    source.keyframes[0].tween = Some(Easing::Linear);
    source.keyframes[0].elements[0].transform.alpha = 0.5;
    let mut endpoint = source.keyframes[0].clone();
    endpoint.frame = 10;
    endpoint.tween = None;
    endpoint.elements[0].transform.x = 100.;
    source.keyframes.push(endpoint);
    project.validate().unwrap();
    let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert_eq!(reopened, project);
    for frame in [0, 5, 10] {
        let x = f64::from(frame) * 10.;
        let samples = coverage(
            &reopened,
            frame,
            &[[x - 1., 50.], [x + 25., 50.], [x + 75., 50.]],
        );
        assert_eq!(samples[0], 0.);
        assert!((samples[1] - 0.125).abs() < 0.01);
        assert!((samples[2] - 0.375).abs() < 0.01);
        let scene = evaluate_scene(&reopened, &reopened.root, frame, false, true);
        assert_eq!(scene.mask_groups.len(), 1);
        assert_eq!(scene.draws[0].mask_group, scene.draws[1].mask_group);
        let mut player = Player::new(&reopened);
        player.seek(&reopened, frame);
        let snapshot = player.runtime_snapshot(&reopened);
        assert_eq!(snapshot.draws.len(), 2);
        assert_eq!(snapshot.filter_groups.len(), 1);
        assert_eq!(snapshot.draws[0].filter_id, snapshot.draws[1].filter_id);
        let runtime_masks = MaskCoverage::new(&reopened, &snapshot.clips).unwrap();
        assert!(
            (runtime_masks.alpha(&snapshot.draws[0].clip_ids, [x + 75., 50.]) - samples[2]).abs()
                < 0.01
        );
    }
    let mut invalid = project.clone();
    invalid.symbols[0].timeline.layers[0].properties.mask_mode = MaskMode::Alpha;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("Mask role")
    );
}
#[test]
fn png_alpha_mixed_with_compound_holes_is_decoded_and_preserved_in_runtime() {
    let mut bytes = vec![];
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[0, 0, 0, 0, 0, 0, 0, 85, 0, 0, 0, 170, 0, 0, 0, 255],
            4,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let mut png = rect("png", 4., 1.);
    png.kind = ElementKind::Bitmap {
        resource_id: "pixels".into(),
        width: 4,
        height: 1,
        smoothing: false,
    };
    png.transform.scale_x = 10.;
    png.transform.scale_y = 40.;
    let mut commands = contour(0., 0., 40., 40.);
    commands.extend(contour(10., 10., 20., 20.));
    let mut donut = rect("donut", 40., 40.);
    donut.transform.x = 50.;
    donut.kind = ElementKind::Path {
        commands,
        fill_rule: FillRule::EvenOdd,
        fill: [0, 0, 0, 128],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut project = masked(vec![png, donut], MaskMode::Alpha);
    project
        .resources
        .push(ResourceUnit::from_bytes("pixels", "alpha.png", "image/png", &bytes).unwrap());
    project.validate().unwrap();
    let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
    let samples = coverage(
        &reopened,
        0,
        &[
            [5., 20.],
            [15., 20.],
            [25., 20.],
            [35., 20.],
            [55., 20.],
            [70., 20.],
        ],
    );
    for (actual, expected) in samples
        .iter()
        .zip([0., 1. / 3., 2. / 3., 1., 128. / 255., 0.])
    {
        assert!((actual - expected).abs() < 0.002);
    }
    let player = Player::new(&reopened);
    let snapshot = player.runtime_snapshot(&reopened);
    let alpha = snapshot.clips[0].alpha_components.as_ref().unwrap();
    let bitmap = alpha.iter().find_map(|c| c.fill_bitmap.as_ref()).unwrap();
    assert_eq!(
        player
            .bitmap_pixels(&bitmap.pixel_ref)
            .unwrap()
            .iter()
            .skip(3)
            .step_by(4)
            .copied()
            .collect::<Vec<_>>(),
        vec![0, 85, 170, 255]
    );
    assert!(alpha.iter().all(|c| c.resource_bitmap.is_none()));
    let mut geometric = project;
    geometric.symbols[0].timeline.layers[1].properties.mask_mode = MaskMode::Geometry;
    assert!(
        geometric
            .validate()
            .unwrap_err()
            .to_string()
            .contains("Alpha mode")
    );
}
#[test]
fn nested_masks_clip_the_source_before_outer_coverage_and_keep_transforms() {
    let mut project = masked(vec![], MaskMode::Alpha);
    let mut inner = project.symbols[0].clone();
    inner.id = "nested".into();
    inner.name = "nested".into();
    let mut fills = layer(
        "nested-fills",
        vec![rect("nested-a", 60., 40.), rect("nested-b", 60., 40.)],
    );
    fills.properties.mask = Some("inner-mask".into());
    let mut shape = rect("inner-half", 30., 40.);
    shape.transform.alpha = 0.5;
    let mut inner_mask = layer("inner-mask", vec![shape]);
    inner_mask.properties.role = LayerRole::Mask;
    inner_mask.properties.mask_mode = MaskMode::Alpha;
    inner.timeline.layers = vec![fills, inner_mask];
    project.symbols.push(inner);
    let mut instance = rect("nested-source", 0., 0.);
    instance.kind = ElementKind::Instance {
        symbol_id: "nested".into(),
        playback: InstancePlayback::Loop,
    };
    instance.transform.x = 20.;
    instance.transform.scale_x = 2.;
    project.symbols[0].timeline.layers[1].keyframes[0].elements = vec![instance];
    project.validate().unwrap();
    let points = [[30., 20.], [79., 20.], [81., 20.], [0., 20.]];
    let samples = coverage(&project, 0, &points);
    assert!(
        (samples[0] - 0.5).abs() < 0.003,
        "overlap applies inner alpha once: {samples:?}"
    );
    assert!((samples[1] - 0.5).abs() < 0.003);
    assert_eq!(samples[2..], [0., 0.]);
    let player = Player::new(&project);
    let snapshot = player.runtime_snapshot(&project);
    let masks = MaskCoverage::new(&project, &snapshot.clips).unwrap();
    for (point, expected) in points.into_iter().zip(samples) {
        assert!((masks.alpha(&snapshot.draws[0].clip_ids, point) - expected).abs() < 0.003);
    }
    assert_eq!(
        snapshot.draws.len(),
        2,
        "all nested source artwork consumed"
    );
    // Distinct linked layers composite independently; objects on one layer do not.
    let mut second = layer("nested-second", vec![rect("nested-c", 60., 40.)]);
    second.properties.mask = Some("inner-mask".into());
    project.symbols[1].timeline.layers.insert(1, second);
    project.validate().unwrap();
    assert!((coverage(&project, 0, &[[30., 20.]])[0] - 0.75).abs() < 0.004);
    let snapshot = Player::new(&project).runtime_snapshot(&project);
    let masks = MaskCoverage::new(&project, &snapshot.clips).unwrap();
    assert!((masks.alpha(&snapshot.draws[0].clip_ids, [30., 20.]) - 0.75).abs() < 0.004);

    // The same inner mask as visible target content: its source must not inherit
    // the outer target's coverage, which is applied once at the parent composite.
    project.symbols[1].timeline.layers.remove(1);
    let mut target = rect("nested-visible", 0., 0.);
    target.kind = ElementKind::Instance {
        symbol_id: "nested".into(),
        playback: InstancePlayback::Loop,
    };
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![target];
    let mut outer = rect("outer-half", 100., 100.);
    outer.transform.alpha = 0.5;
    project.symbols[0].timeline.layers[1].keyframes[0].elements = vec![outer];
    project.validate().unwrap();
    let scene = evaluate_scene(&project, &project.root, 0, false, true);
    let masks = MaskCoverage::new(&project, &scene.clips).unwrap();
    assert!((masks.alpha(&scene.draws[0].clip_ids, [20., 20.]) - 0.25).abs() < 0.004);
    let snapshot = Player::new(&project).runtime_snapshot(&project);
    let masks = MaskCoverage::new(&project, &snapshot.clips).unwrap();
    assert!((masks.alpha(&snapshot.draws[0].clip_ids, [20., 20.]) - 0.25).abs() < 0.004);
}
