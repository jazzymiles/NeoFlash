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
                ..Default::default()
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
    // Both visible shapes inherit the same ordered outer/inner clip stack.
    let expected = [
        r#"layer:["mask-layer"]"#,
        r#"layer:["nested-visible","inner-mask"]"#,
    ];
    for draw in &scene.draws {
        assert_eq!(draw.clip_ids, expected);
    }
    for draw in &snapshot.draws {
        assert_eq!(draw.clip_ids.len(), expected.len());
        for (actual, suffix) in draw.clip_ids.iter().zip(expected) {
            assert!(actual.ends_with(suffix), "{actual} must end with {suffix}");
        }
    }
}

#[test]
fn luminance_and_inverted_masks_evaluate_coverage_hit_test_and_runtime_snapshot() {
    // 1. Solid green vector fill (0, 255, 0, 255): BT.709 luminance = 0.7152.
    let mut green = rect("green-luma", 100., 100.);
    green.kind = ElementKind::Rectangle {
        width: 100.,
        height: 100.,
        fill: [0, 255, 0, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    let mut project = masked(vec![green], MaskMode::Luminance);
    assert_eq!(project.format_version, 9);
    project.validate().unwrap();
    let samples = coverage(&project, 0, &[[50., 50.], [150., 50.]]);
    assert!(
        (samples[0] - 0.7152).abs() < 0.002,
        "BT.709 green luminance: {samples:?}"
    );
    assert_eq!(samples[1], 0.0);

    // 2. Invert the Luminance mask: inside becomes 1.0 - 0.7152 = 0.2848; outside becomes 1.0.
    project.symbols[0].timeline.layers[1].properties.mask_invert = true;
    project.validate().unwrap();
    let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
    assert!(
        reopened.symbols[0].timeline.layers[1]
            .properties
            .mask_invert
    );
    let inv_samples = coverage(&reopened, 0, &[[50., 50.], [150., 50.]]);
    assert!(
        (inv_samples[0] - (1.0 - 0.7152)).abs() < 0.002,
        "inverted green luminance: {inv_samples:?}"
    );
    assert!((inv_samples[1] - 1.0).abs() < 0.001);

    // Runtime snapshot preserves `invert` and `luminance` on RuntimeClip.
    let snapshot = Player::new(&reopened).runtime_snapshot(&reopened);
    assert!(snapshot.clips[0].invert);
    let runtime_masks = MaskCoverage::new(&reopened, &snapshot.clips).unwrap();
    assert!(
        (runtime_masks.alpha(&snapshot.draws[0].clip_ids, [50., 50.]) - inv_samples[0]).abs()
            < 0.002
    );
    assert!((runtime_masks.alpha(&snapshot.draws[0].clip_ids, [150., 50.]) - 1.0).abs() < 0.001);

    // 3. Inverted Geometry mask: hit-testing and coverage are 0 inside the mask shape and 1 outside.
    let mut geo_inv = masked(vec![rect("hole", 80., 80.)], MaskMode::Geometry);
    geo_inv.symbols[0].timeline.layers[1].properties.mask_invert = true;
    geo_inv.upgrade_format_for_features();
    assert_eq!(geo_inv.format_version, 9);
    geo_inv.validate().unwrap();
    let geo_cov = coverage(&geo_inv, 0, &[[40., 40.], [120., 40.]]);
    assert_eq!(geo_cov, vec![0.0, 1.0]);
    let scene = evaluate_scene(&geo_inv, &geo_inv.root, 0, false, true);
    assert!(
        !scene.contains_point(&scene.draws[0].item.instance_path, [40., 40.]),
        "point inside inverted geometry mask hole should be clipped out"
    );
    assert!(
        scene.contains_point(&scene.draws[0].item.instance_path, [120., 40.]),
        "point outside inverted geometry mask hole should hit target"
    );
}

#[test]
fn nested_authored_bitmap_fills_follow_the_active_key_and_supply_mask_pixels() {
    let mut bytes = vec![];
    let rgba = [255, 255, 255, 255, 0, 0, 0, 0];
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&rgba, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut project = masked(vec![], MaskMode::Luminance);
    project
        .resources
        .push(ResourceUnit::from_bytes("photo", "photo.png", "image/png", &bytes).unwrap());
    let mut photo = rect("image shape", 20., 8.);
    let mut shape_layer = layer("image layer", vec![photo.clone()]);
    let fill = AuthoredBitmapFill {
        element_id: photo.id.clone(),
        resource_id: "photo".into(),
        matrix: [2., 0., 0., 1., 2., 0.],
        repeat: true,
        smoothing: false,
    };
    shape_layer.keyframes[0].set_bitmap_fill(fill.clone());
    let mut solid = shape_layer.keyframes[0].clone();
    solid.frame = 2;
    solid.bitmap_fills.clear();
    shape_layer.keyframes.push(solid);
    photo.visible = false;
    shape_layer.keyframes.push(Keyframe {
        frame: 3,
        elements: vec![photo],
        ..Default::default()
    });

    let mut inner = project.symbols[0].clone();
    inner.id = "photo group".into();
    inner.timeline.frame_count = 4;
    inner.timeline.layers = vec![shape_layer];
    project.symbols.push(inner);
    let mut instance = Element {
        id: "visible instance".into(),
        kind: ElementKind::Instance {
            symbol_id: "photo group".into(),
            playback: InstancePlayback::Loop,
        },
        ..Element::default()
    };
    instance.transform.x = 10.;
    instance.transform.alpha = 0.5;
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![instance.clone()];
    instance.id = "mask instance".into();
    project.symbols[0].timeline.layers[1].keyframes[0].elements = vec![instance];
    project.upgrade_format_for_features();
    let project = Project::from_json(&project.to_json().unwrap()).unwrap();
    let mut player = Player::new(&project);
    player.seek(&project, 1);
    let scene = evaluate_scene(&project, &project.root, 1, false, true);
    assert_eq!(scene.draws.len(), 1);
    assert_eq!(scene.draws[0].item.bitmap_fill.as_ref(), Some(&fill));
    assert_eq!(scene.draws[0].item.transform[4], 10.);
    let mask = MaskCoverage::new(&project, &scene.clips).unwrap();
    for (x, expected) in [(10.5, 0.), (12.5, 0.5), (14.5, 0.), (16.5, 0.5)] {
        assert!((mask.alpha(&scene.draws[0].clip_ids, [x, 2.]) - expected).abs() < 0.001);
    }
    let snapshot = player.runtime_snapshot(&project);
    let draw = &snapshot.draws[0].item;
    assert_eq!(draw.bitmap_fill.as_ref(), Some(&fill));
    let RuntimeDrawKind::GradientPath {
        fill_bitmap: Some(paint),
        ..
    } = &draw.kind
    else {
        panic!("runtime did not resolve the image paint");
    };
    assert_eq!(paint.matrix, fill.matrix);
    assert_eq!(player.bitmap_pixels(&paint.pixel_ref).unwrap(), rgba);
    assert_eq!(paint.color_transform.multipliers[3], 0.5);
    let mask_pixels = snapshot.clips[0].alpha_components.as_ref().unwrap()[0]
        .fill_bitmap
        .as_ref()
        .unwrap();
    assert_eq!(mask_pixels, paint);

    player.seek(&project, 2);
    assert!(
        player.runtime_snapshot(&project).draws[0]
            .item
            .bitmap_fill
            .is_none()
    );
    assert!(
        evaluate(&project, &project.root, 2)
            .iter()
            .all(|d| d.bitmap_fill.is_none())
    );
    player.seek(&project, 3);
    assert!(player.runtime_snapshot(&project).draws.is_empty());
}
