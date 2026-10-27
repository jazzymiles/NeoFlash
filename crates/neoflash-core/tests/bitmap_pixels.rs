use image::ImageEncoder;
use neoflash_core::*;
use std::sync::Arc;

#[test]
fn portable_image_shared_runtime_pixels_and_transfer_keep_separate_lifetimes() {
    bulk_image_pipeline_and_detached_capture();
    let original_rgba = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0, 255, 255, 255, 128, 9, 8, 7,
        255,
    ];
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&original_rgba, 3, 2, image::ExtendedColorType::Rgba8)
        .unwrap();
    let resource = ResourceUnit::from_bytes("image", "art/checker.png", "image/png", &png).unwrap();
    let decoded = resource.decode_image().unwrap();
    assert_eq!((decoded.width, decoded.height), (3, 2));
    assert_eq!(&*decoded.rgba, &original_rgba);
    assert!(Arc::ptr_eq(
        &decoded,
        &resource.clone().decode_image().unwrap()
    ));
    let malformed =
        ResourceUnit::from_bytes("bad", "art/bad.png", "image/png", &png[..20]).unwrap();
    assert!(malformed.decode_image().is_err());
    let mut project = Project::blank();
    project.format_version = 3;
    project.resources.push(resource);
    let element = Element {
        id: "image-view".into(),
        name: "image".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Bitmap {
            resource_id: "image".into(),
            width: 3,
            height: 2,
            smoothing: false,
        },
    };
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(element.clone());
    let mut exported = project.symbols[0].clone();
    exported.id = "exported-image".into();
    exported.name = "Picture".into();
    exported.kind = SymbolKind::MovieClip;
    exported.linkage_id = Some("Picture".into());
    exported.timeline.layers[0].id = "picture-layer".into();
    exported.timeline.layers[0].keyframes[0].elements[0].id = "picture-art".into();
    project.symbols.push(exported);
    project.validate().unwrap();
    let original = project.to_json().unwrap();
    assert_eq!(Project::from_json(&original).unwrap(), project);
    let authored = evaluate(&project, &project.root, 0);
    let mut player = Player::new(&project);
    assert_eq!(player.bitmap_error(), None);
    let root = player.object_ref(&[]).unwrap();
    let authored_ref = player
        .object_ref(std::slice::from_ref(&element.id))
        .unwrap();
    let pixels = player
        .object_bitmap(&project, &authored_ref)
        .unwrap()
        .unwrap();
    let linked = player.create_linked(&project, "Picture").unwrap();
    let linked_view = player
        .runtime_snapshot(&project)
        .objects
        .into_iter()
        .find(|n| n.namespace_ref.as_ref() == Some(&linked) && n.display.owner == "bitmap")
        .unwrap();
    assert_eq!(
        linked_view.bitmap.unwrap().pixel_ref,
        pixels,
        "linked namespaces share the resource buffer"
    );
    let dynamic = player.create_display(DynamicKind::Bitmap).unwrap();
    player
        .object_set_bitmap(&project, &dynamic, Some(pixels.clone()))
        .unwrap();
    player
        .object_set_smoothing(&project, &dynamic, true)
        .unwrap();
    player
        .object_set_pixel_snapping(&project, &dynamic, PixelSnapping::Never)
        .unwrap();
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root,
                child: dynamic.clone(),
                index: None,
            },
        )
        .unwrap();
    let revision = player.bitmap_info(&pixels).unwrap().revision;
    player
        .bitmap_set_pixel(&pixels, 0, 0, 0x112233, false)
        .unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&pixels, 0, 0, true).unwrap(),
        0xff112233
    );
    assert_ne!(player.bitmap_info(&pixels).unwrap().revision, revision);
    let snapshot = player.runtime_snapshot(&project);
    let json = serde_json::to_value(&snapshot.draws).unwrap();
    assert_eq!(json[0]["kind"]["type"], "bitmap_pixels");
    assert_eq!(json[1]["kind"]["pixel_snapping"], "never");
    assert_eq!(snapshot.draws.len(), 2);
    assert!(snapshot.draws.iter().all(
        |d| matches!(&d.item.kind,RuntimeDrawKind::BitmapPixels{pixel_ref,..} if pixel_ref==&pixels)
    ));
    assert_eq!(evaluate(&project, &project.root, 0), authored);
    assert_eq!(project.to_json().unwrap(), original);
    let cloned = player.bitmap_clone(&pixels).unwrap();
    player
        .bitmap_fill_rect(&cloned, -1, 0, 3, 1, 0xffabcdef)
        .unwrap();
    player.bitmap_scroll(&cloned, 1, 0).unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&cloned, 0, 0, true).unwrap(),
        0xffabcdef,
        "scroll keeps uncovered pixels"
    );
    assert_eq!(
        player.bitmap_get_pixel(&cloned, 2, 0, true).unwrap(),
        0xffabcdef
    );
    assert_eq!(
        player.bitmap_get_pixel(&pixels, 0, 0, true).unwrap(),
        0xff112233
    );
    let overlay = player.bitmap_create(1, 1, true, 0x80ff0000).unwrap();
    let opaque = player.bitmap_create(2, 1, false, 0xff0000ff).unwrap();
    player
        .bitmap_copy_pixels(&opaque, &overlay, 0, 0, 1, 1, 0, 0, true)
        .unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&opaque, 0, 0, true).unwrap(),
        0xff80007f
    );
    player
        .bitmap_copy_pixels(&opaque, &overlay, 0, 0, 1, 1, 1, 0, false)
        .unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&opaque, 1, 0, true).unwrap(),
        0xff80007f,
        "transparent source composites onto opaque destination even without mergeAlpha"
    );
    player
        .bitmap_set_pixel(&opaque, 1, 0, 0x00010203, true)
        .unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&opaque, 1, 0, true).unwrap(),
        0xff010203
    );
    player
        .bitmap_set_pixel(&overlay, 0, 0, 0x00ffffff, true)
        .unwrap();
    assert_eq!(player.bitmap_get_pixel(&overlay, 0, 0, true).unwrap(), 0);
    player
        .bitmap_copy_pixels(
            &cloned,
            &pixels,
            i32::MIN,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            0,
            0,
            false,
        )
        .unwrap();
    assert_eq!(player.bitmap_get_pixel(&pixels, -1, 0, true).unwrap(), 0);
    assert!(player.bitmap_create(4097, 1, true, 0).is_err());
    player.bitmap_dispose(&pixels).unwrap();
    assert!(player.bitmap_pixels(&pixels).is_err());
    assert!(player.runtime_snapshot(&project).draws.is_empty());
    assert!(player.bitmap_info(&pixels).unwrap().disposed);
    player.reset(&project);
    assert!(player.bitmap_info(&pixels).is_err());
    let fresh = player
        .object_bitmap(
            &project,
            &player
                .object_ref(std::slice::from_ref(&element.id))
                .unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(player.bitmap_pixels(&fresh).unwrap(), original_rgba);
    // Record budget is bounded even after disposal; failures leave existing state live.
    for _ in 0..255 {
        let reference = player.bitmap_create(1, 1, true, 0).unwrap();
        player.bitmap_dispose(&reference).unwrap();
    }
    assert!(player.bitmap_create(1, 1, true, 0).is_err());
    assert_eq!(
        player.bitmap_get_pixel(&fresh, 0, 0, true).unwrap(),
        0xffff0000
    );
    // Copy/paste remaps both direct placements and exported library references.
    let payload = ClipboardPayload::capture(&project, vec![element], "source", [0., 0.]).unwrap();
    let mut destination = Project::blank();
    destination.format_version = 3;
    let mut reused = project.resources[0].clone();
    reused.id = "destination-image".into();
    destination.resources.push(reused);
    let layer = destination.symbols[0].timeline.layers[0].id.clone();
    let root = destination.root.clone();
    let pasted = payload
        .paste_into(&destination, "destination", &root, &layer, 0, [0., 0.])
        .unwrap()
        .project;
    assert_eq!(pasted.resources.len(), 1);
    assert!(pasted.symbols.iter().flat_map(|s|&s.timeline.layers).flat_map(|l|&l.keyframes).flat_map(|k|&k.elements).all(|e|matches!(&e.kind,ElementKind::Bitmap{resource_id,..} if resource_id=="destination-image")));
    let again = payload
        .paste_into(&pasted, "destination", &root, &layer, 0, [0., 0.])
        .unwrap()
        .project;
    assert_eq!(
        again.symbols.len(),
        pasted.symbols.len(),
        "identical export closure survives resource-ID remapping"
    );
    let mut invalid = project.clone();
    if let ElementKind::Bitmap { width, .. } =
        &mut invalid.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind
    {
        *width = 4;
    }
    assert!(invalid.validate().is_err());
    invalid = project.clone();
    invalid.resources.clear();
    assert!(invalid.validate().is_err());
}

// One end-to-end family extension: byte transfer, CPU processing, encoding and
// capture share the same buffers/graph without advancing authored state.
fn bulk_image_pipeline_and_detached_capture() {
    let project = Project::blank();
    let mut p = Player::new(&project);
    let image = p.bitmap_create(2, 2, true, 0).unwrap();
    let rect = BitmapRect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    let bytes = [255, 10, 20, 30, 128, 255, 0, 0, 99];
    assert_eq!(
        p.bitmap_set_pixels(&image, rect, &bytes, false).unwrap(),
        PixelWrite {
            consumed: 8,
            complete: false
        }
    );
    assert_eq!(
        p.bitmap_get_pixels(&image, rect, false).unwrap(),
        [255, 10, 20, 30, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        p.bitmap_get_pixels(
            &image,
            BitmapRect {
                x: -1,
                y: 0,
                width: 2,
                height: 1
            },
            true
        )
        .unwrap(),
        [30, 20, 10, 255]
    );
    let single = BitmapRect {
        x: 0,
        y: 1,
        width: 1,
        height: 1,
    };
    p.bitmap_set_pixels(&image, single, &[7, 8, 9, 255], true)
        .unwrap();
    p.bitmap_color_transform(
        &image,
        single,
        ColorTransform {
            multipliers: [0., 1., 1., 1.],
            offsets: [40., 0., 0., 0.],
        },
    )
    .unwrap();
    assert_eq!(p.bitmap_get_pixel(&image, 0, 1, true).unwrap(), 0xff280807);
    let png = p.bitmap_encode_png(&image, rect, false).unwrap();
    let resource =
        ResourceUnit::from_bytes("roundtrip", "roundtrip.png", "image/png", &png).unwrap();
    let decoded = resource.decode_image().unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 2));
    assert_eq!(&*decoded.rgba, p.bitmap_pixels(&image).unwrap());
    let revision = p.bitmap_info(&image).unwrap().revision;
    let raster = [0, 0, 255, 128].repeat(4);
    p.bitmap_draw_rgba(&image, 2, 2, &raster, Some(single), &revision)
        .unwrap();
    assert_eq!(p.bitmap_get_pixel(&image, 0, 0, true).unwrap(), 0xff0a141e);
    assert_eq!(p.bitmap_get_pixel(&image, 0, 1, true).unwrap(), 0xff140483);
    let unchanged = p.bitmap_pixels(&image).unwrap();
    assert!(
        p.bitmap_draw_rgba(&image, 2, 2, &raster, None, &revision)
            .is_err()
    );
    assert_eq!(p.bitmap_pixels(&image).unwrap(), unchanged);

    let source = p.bitmap_create(5, 1, true, 0).unwrap();
    p.bitmap_set_pixel(&source, 0, 0, 0xffffffff, true).unwrap();
    let dest = p.bitmap_create(8, 3, true, 0xff123456).unwrap();
    let blur = FilterSpec::Blur {
        blur_x: 2.,
        blur_y: 0.,
        quality: 1,
    };
    let region = BitmapRect {
        x: 1,
        y: 0,
        width: 1,
        height: 1,
    };
    assert_eq!(
        p.bitmap_generate_filter_rect(&source, region, &blur)
            .unwrap(),
        BitmapRect {
            x: 0,
            y: 0,
            width: 3,
            height: 1
        }
    );
    p.bitmap_apply_filter(&dest, &source, region, 3, 1, &blur)
        .unwrap();
    assert!(
        p.bitmap_get_pixel(&dest, 3, 1, true).unwrap() >> 24 > 0,
        "filter uses source neighbour outside sourceRect"
    );
    assert_eq!(p.bitmap_get_pixel(&dest, 0, 0, true).unwrap(), 0xff123456);
    // Self-filtering snapshots before writing and preserves the same result.
    let copy = p.bitmap_clone(&source).unwrap();
    p.bitmap_apply_filter(
        &copy,
        &source,
        BitmapRect {
            x: 0,
            y: 0,
            width: 5,
            height: 1,
        },
        0,
        0,
        &blur,
    )
    .unwrap();
    p.bitmap_apply_filter(
        &source,
        &source,
        BitmapRect {
            x: 0,
            y: 0,
            width: 5,
            height: 1,
        },
        0,
        0,
        &blur,
    )
    .unwrap();
    assert_eq!(
        p.bitmap_pixels(&source).unwrap(),
        p.bitmap_pixels(&copy).unwrap()
    );

    let holder = p.create_display(DynamicKind::Sprite).unwrap();
    let child = p.create_display(DynamicKind::Shape).unwrap();
    p.graphics_apply(
        &child,
        &[
            GraphicsOp::BeginFill {
                color: 0xff0000,
                alpha: 1.,
            },
            GraphicsOp::DrawRect {
                x: 0.,
                y: 0.,
                width: 10.,
                height: 10.,
            },
            GraphicsOp::EndFill,
        ],
    )
    .unwrap();
    p.display_mutate(
        &project,
        DisplayMutation::Add {
            parent: holder.clone(),
            child: child.clone(),
            index: None,
        },
    )
    .unwrap();
    p.object_set_property(&holder, "x", 100.);
    p.object_set_property(&holder, "alpha", 0.);
    p.object_set_visible(&holder, false);
    p.object_set_property(&child, "x", 5.);
    p.object_set_filters(&holder, std::slice::from_ref(&blur))
        .unwrap();
    p.object_set_filters(&child, std::slice::from_ref(&blur))
        .unwrap();
    p.object_set_scroll_rect(
        &project,
        &holder,
        Some(ScrollRect {
            x: 1.,
            y: 0.,
            width: 8.,
            height: 10.,
        }),
    )
    .unwrap();
    let before = serde_json::to_string(&p.runtime_snapshot(&project)).unwrap();
    let capture = p
        .bitmap_draw_snapshot(
            &project,
            &holder,
            [1., 0., 0., 1., 2., 0.],
            ColorTransform::default(),
        )
        .unwrap();
    assert_eq!(capture.draws.len(), 1);
    assert_eq!(capture.draws[0].item.transform, [1., 0., 0., 1., 6., 0.]);
    assert!(!capture.draws[0].clip_ids.is_empty());
    assert!(capture.filter_groups.is_empty());
    assert_eq!(capture.objects[0].parent_ref, None);
    assert!(capture.objects[0].on_stage);
    assert!(matches!(
        capture.draws[0].item.kind,
        RuntimeDrawKind::Element(ElementKind::Path {
            fill: [255, 0, 0, 255],
            ..
        })
    ));
    assert_eq!(
        serde_json::to_string(&p.runtime_snapshot(&project)).unwrap(),
        before
    );
    let mask = p.create_display(DynamicKind::Shape).unwrap();
    p.graphics_apply(
        &mask,
        &[
            GraphicsOp::BeginFill {
                color: 0,
                alpha: 0.,
            },
            GraphicsOp::DrawRect {
                x: 0.,
                y: 0.,
                width: 20.,
                height: 20.,
            },
            GraphicsOp::EndFill,
        ],
    )
    .unwrap();
    p.object_set_mask(&project, &child, Some(mask)).unwrap();
    assert!(
        p.bitmap_draw_snapshot(
            &project,
            &holder,
            [1., 0., 0., 1., 0., 0.],
            ColorTransform::default()
        )
        .is_err()
    );
}
