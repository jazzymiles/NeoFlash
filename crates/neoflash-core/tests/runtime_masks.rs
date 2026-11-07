use image::ImageEncoder;
use neoflash_core::*;

fn rectangle(id: &str, width: f64, height: f64) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind: ElementKind::Rectangle {
            width,
            height,
            fill: [255; 4],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        ..Default::default()
    }
}
fn gradient(id: &str) -> Element {
    Element {
        kind: ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
                PathCommand::LineTo { x: 100., y: 100. },
                PathCommand::LineTo { x: 0., y: 100. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [255; 4],
            stroke: [0; 4],
            stroke_width: 0.,
            stroke_gradient: None,
            fill_gradient: Some(GradientPaint {
                kind: GradientKind::Linear,
                matrix: [100. / 1638.4, 0., 0., 1., 50., 0.],
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: [0; 4],
                    },
                    GradientStop {
                        ratio: 255,
                        color: [255; 4],
                    },
                ],
                focal_point_ratio: 0.,
            }),
        },
        ..rectangle(id, 100., 100.)
    }
}
fn bitmap(id: &str) -> Element {
    Element {
        kind: ElementKind::Bitmap {
            resource_id: "alpha-image".into(),
            width: 3,
            height: 1,
            smoothing: false,
        },
        ..rectangle(id, 3., 1.)
    }
}
fn add_png(project: &mut Project) {
    let mut bytes = vec![];
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[255, 255, 255, 0, 255, 255, 255, 128, 255, 255, 255, 255],
            3,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    project
        .resources
        .push(ResourceUnit::from_bytes("alpha-image", "alpha.png", "image/png", &bytes).unwrap());
}
fn instance(id: &str, symbol: &str) -> Element {
    Element {
        kind: ElementKind::Instance {
            symbol_id: symbol.into(),
            playback: InstancePlayback::Loop,
        },
        ..rectangle(id, 0., 0.)
    }
}
fn symbol(project: &Project, id: &str, frames: u32, elements: Vec<Element>) -> Symbol {
    let mut symbol = project.symbols[0].clone();
    symbol.id = id.into();
    symbol.name = id.into();
    symbol.timeline.frame_count = frames;
    symbol.timeline.layers[0].id = format!("{id}-layer");
    symbol.timeline.layers[0].keyframes = vec![Keyframe {
        elements,
        ..Default::default()
    }];
    symbol
}
fn cache(player: &mut Player, project: &Project, target: &ObjectRef, as3: bool) {
    player
        .as2_render_command(
            project,
            target,
            serde_json::json!({"op":"set", "name":"cacheAsBitmap", "value":true, "as3":as3}),
        )
        .unwrap();
}
fn script_clip<'a>(snapshot: &'a RuntimeSnapshot, source: &ObjectRef) -> &'a RuntimeClip {
    snapshot
        .clips
        .iter()
        .find(|clip| clip.id == format!("mask:{}:{}", source.generation, source.id))
        .unwrap()
}

#[test]
fn cached_authored_mixed_masks_share_transformed_gradient_and_decoded_png_alpha() {
    let mut project = Project::blank();
    project.format_version = 8;
    add_png(&mut project);
    let mut ramp = gradient("ramp");
    ramp.transform.alpha = 0.5;
    let mut image = bitmap("image");
    image.transform.x = 110.;
    image.transform.alpha = 0.5;
    let inner = symbol(&project, "inner", 1, vec![ramp, image]);
    let mut inner_placement = instance("nested", "inner");
    inner_placement.transform.x = 8.;
    inner_placement.transform.alpha = 0.5;
    let outer = symbol(&project, "outer", 1, vec![inner_placement]);
    let target_symbol = symbol(
        &project,
        "target-symbol",
        1,
        vec![rectangle("content", 200., 100.)],
    );
    project.symbols.extend([inner, outer, target_symbol]);
    let mut mask = instance("mask", "outer");
    mask.transform.x = 20.;
    mask.transform.scale_x = 2.;
    project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![instance("target", "target-symbol"), mask];
    project.validate().unwrap();
    let original = project.to_json().unwrap();
    // The same Rust path is called by AS2 setMask and AS3 DisplayObject.mask.
    for as3 in [false, true] {
        let mut player = Player::new(&project);
        let target = player.object_ref(&["target".into()]).unwrap();
        let mask = player.object_ref(&["mask".into()]).unwrap();
        assert!(
            player
                .object_set_mask(&project, &target, Some(mask.clone()))
                .is_err()
        );
        assert_eq!(player.object_mask(&target).unwrap(), None);
        cache(&mut player, &project, &target, as3);
        cache(&mut player, &project, &mask, as3);
        player
            .object_set_mask(&project, &target, Some(mask.clone()))
            .unwrap();
        let snapshot = player.runtime_snapshot(&project);
        let components = script_clip(&snapshot, &mask)
            .alpha_components
            .as_ref()
            .unwrap();
        assert_eq!(components.len(), 2);
        let ramp = components
            .iter()
            .find(|c| c.fill_gradient.is_some())
            .unwrap();
        assert_eq!(ramp.matrix, [2., 0., 0., 1., 36., 0.]);
        assert_eq!(ramp.alpha, 1.);
        let paint = ramp.fill_gradient.as_ref().unwrap();
        assert_eq!(
            [
                paint.sample(0., 0.)[3],
                paint.sample(50., 0.)[3],
                paint.sample(100., 0.)[3]
            ],
            [0, 32, 64]
        );
        let image = components.iter().find(|c| c.fill_bitmap.is_some()).unwrap();
        assert_eq!(image.matrix, [2., 0., 0., 1., 256., 0.]);
        assert_eq!(
            image.alpha, 1.,
            "placement alpha is already in the bitmap color transform"
        );
        let paint = image.fill_bitmap.as_ref().unwrap();
        let rgba = player.bitmap_pixels(&paint.pixel_ref).unwrap();
        assert_eq!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .map(|p| paint.color_transform.apply(*p)[3])
                .collect::<Vec<_>>(),
            [0, 32, 64]
        );
        assert!(
            snapshot
                .draws
                .iter()
                .all(|draw| !draw.item.instance_path.starts_with(&["mask".into()]))
        );
        assert!(snapshot.filter_groups.iter().any(|group| {
            group
                .alpha_masks
                .iter()
                .any(|alpha| alpha.clip_id == script_clip(&snapshot, &mask).id)
        }));
        // Removing either cache flag would select unsupported geometric PNG
        // coverage. Both prepare and set must refuse without changing the mask.
        let before = serde_json::to_value(&snapshot).unwrap();
        for node in [&target, &mask] {
            for op in ["prepare", "set"] {
                assert!(
                    player
                        .as2_render_command(
                            &project,
                            node,
                            serde_json::json!({
                                "op":op, "name":"cacheAsBitmap", "value":false, "as3":as3
                            })
                        )
                        .is_err()
                );
                assert_eq!(
                    serde_json::to_value(player.runtime_snapshot(&project)).unwrap(),
                    before
                );
            }
        }
        player.object_set_mask(&project, &target, None).unwrap();
        assert!(
            player
                .runtime_snapshot(&project)
                .draws
                .iter()
                .any(|draw| draw.item.instance_path.starts_with(&["mask".into()]))
        );
    }
    assert_eq!(project.to_json().unwrap(), original);
    Project::from_json(&original).unwrap().validate().unwrap();
}

#[test]
fn animated_script_mask_follows_tween_frames_and_validates_future_source_kinds_atomically() {
    let mut project = Project::blank();
    project.format_version = 8;
    project.symbols[0].timeline.frame_count = 3;
    let mut moving = symbol(&project, "moving", 3, vec![gradient("ramp")]);
    moving.timeline.layers[0].keyframes[0].tween = Some(Easing::Linear);
    let mut end = gradient("ramp");
    end.transform.x = 40.;
    moving.timeline.layers[0].keyframes.push(Keyframe {
        frame: 2,
        elements: vec![end],
        ..Default::default()
    });
    project.symbols.push(moving);
    project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![rectangle("target", 200., 100.), instance("mask", "moving")];
    project.validate().unwrap();
    for cached in [false, true] {
        let mut player = Player::new(&project);
        let target = player.object_ref(&["target".into()]).unwrap();
        let mask = player.object_ref(&["mask".into()]).unwrap();
        if cached {
            cache(&mut player, &project, &target, true);
            cache(&mut player, &project, &mask, true);
        }
        player
            .object_set_mask(&project, &target, Some(mask.clone()))
            .unwrap();
        for (frame, x) in [(0, 0.), (1, 20.), (2, 40.), (0, 0.)] {
            player.control(&project, &[], PlaybackCommand::GotoAndStop(frame));
            let snapshot = player.runtime_snapshot(&project);
            let clip = script_clip(&snapshot, &mask);
            assert_eq!(clip.components[0].matrix[4], x);
            assert_eq!(clip.alpha_components.is_some(), cached);
            if cached {
                assert!(
                    clip.alpha_components.as_ref().unwrap()[0]
                        .fill_gradient
                        .is_some()
                );
            }
            assert!(
                snapshot
                    .draws
                    .iter()
                    .all(|draw| !draw.item.instance_path.starts_with(&["mask".into()]))
            );
        }
    }
    // The current frame can be vector while a later source frame contains PNG.
    add_png(&mut project);
    project.symbols[1].timeline.layers[0].keyframes[1].elements = vec![bitmap("ramp")];
    project.symbols[1].timeline.layers[0].keyframes[0].tween = None;
    project.validate().unwrap();
    let mut player = Player::new(&project);
    let target = player.object_ref(&["target".into()]).unwrap();
    let mask = player.object_ref(&["mask".into()]).unwrap();
    assert!(
        player
            .object_set_mask(&project, &target, Some(mask.clone()))
            .is_err()
    );
    assert_eq!(player.object_mask(&target).unwrap(), None);
    cache(&mut player, &project, &target, true);
    cache(&mut player, &project, &mask, true);
    player
        .object_set_mask(&project, &target, Some(mask.clone()))
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    assert!(
        script_clip(&player.runtime_snapshot(&project), &mask)
            .alpha_components
            .as_ref()
            .unwrap()[0]
            .fill_bitmap
            .is_some()
    );
}

#[test]
fn scripted_mask_preserves_nested_authored_clips_and_does_not_paint_their_sources() {
    let mut project = Project::blank();
    project.format_version = 8;
    let mut source = symbol(&project, "source", 1, vec![gradient("ramp")]);
    source.timeline.layers[0].properties.mask = Some("inner-mask-layer".into());
    let mut inner_mask = source.timeline.layers[0].clone();
    inner_mask.id = "inner-mask-layer".into();
    inner_mask.properties = LayerProperties {
        role: LayerRole::Mask,
        ..Default::default()
    };
    inner_mask.keyframes[0].elements = vec![rectangle("inner-mask", 30., 100.)];
    source.timeline.layers.push(inner_mask);
    project.symbols.push(source);
    project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![rectangle("target", 100., 100.), instance("mask", "source")];
    project.validate().unwrap();
    let mut player = Player::new(&project);
    let target = player.object_ref(&["target".into()]).unwrap();
    let mask = player.object_ref(&["mask".into()]).unwrap();
    cache(&mut player, &project, &target, true);
    cache(&mut player, &project, &mask, true);
    player
        .object_set_mask(&project, &target, Some(mask.clone()))
        .unwrap();
    let snapshot = player.runtime_snapshot(&project);
    let clip = script_clip(&snapshot, &mask);
    assert_eq!(clip.components.len(), 1);
    let components = clip.alpha_components.as_ref().unwrap();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].clip_ids.len(), 1);
    let inner = snapshot
        .clips
        .iter()
        .find(|c| c.id == components[0].clip_ids[0])
        .unwrap();
    assert_eq!(inner.components.len(), 1);
    assert!(components[0].fill_gradient.is_some());
    let coverage = MaskCoverage::new(&project, &snapshot.clips).unwrap();
    assert!(coverage.alpha(std::slice::from_ref(&clip.id), [20., 50.]) > 0.);
    assert_eq!(
        coverage.alpha(std::slice::from_ref(&clip.id), [50., 50.]),
        0.
    );
    assert_eq!(snapshot.draws.len(), 1);
}
