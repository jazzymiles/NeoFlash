use neoflash_core::*;

#[test]
fn subtree_filters_copy_state_and_process_rgba_with_bounded_lifetimes() {
    let blur = FilterSpec::Blur {
        blur_x: 2.,
        blur_y: 2.,
        quality: 1,
    };
    let glow = FilterSpec::Glow {
        color: 0x00ff00,
        alpha: 0.75,
        blur_x: 2.,
        blur_y: 2.,
        strength: 2.,
        quality: 1,
        inner: false,
        knockout: false,
    };
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 2;
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "filtered-authored".into(),
        name: "Original".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill: [255, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    }];
    project.symbols[0].timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
        });
    let original = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let authored = player.object_ref(&["filtered-authored".into()]).unwrap();
    let container = player.create_display(DynamicKind::Sprite).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    for (parent, child) in [(&root, &container), (&container, &shape)] {
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: parent.clone(),
                    child: child.clone(),
                    index: None,
                },
            )
            .unwrap();
    }
    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginFill {
                    color: 0xff0000,
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
    player
        .object_set_filters(&authored, std::slice::from_ref(&glow))
        .unwrap();
    player
        .object_set_filters(&container, std::slice::from_ref(&blur))
        .unwrap();
    player
        .object_set_filters(&shape, std::slice::from_ref(&glow))
        .unwrap();
    player
        .object_set_scroll_rect(
            &project,
            &container,
            Some(ScrollRect {
                x: 2.,
                y: 3.,
                width: 15.,
                height: 15.,
            }),
        )
        .unwrap();
    let scene = player.runtime_snapshot(&project);
    let outer = scene
        .filter_groups
        .iter()
        .find(|g| g.owner_ref == container)
        .unwrap();
    let inner = scene
        .filter_groups
        .iter()
        .find(|g| g.owner_ref == shape)
        .unwrap();
    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    assert!(!outer.clip_ids.is_empty());
    assert_eq!(outer.clip_ids, inner.clip_ids);
    assert!(
        scene
            .draws
            .iter()
            .any(|d| d.owner_ref == shape && d.filter_id.as_deref() == Some(inner.id.as_str())),
        "transparent source geometry is retained for alpha-changing filters"
    );
    let mut copied = player.object_filters(&shape).unwrap();
    copied.clear();
    assert_eq!(player.object_filters(&shape).unwrap(), vec![glow.clone()]);
    assert!(
        player
            .object_set_filters(
                &shape,
                &[FilterSpec::Blur {
                    blur_x: f64::NAN,
                    blur_y: 2.,
                    quality: 1
                }]
            )
            .is_err()
    );
    assert_eq!(player.object_filters(&shape).unwrap(), vec![glow]);
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: root,
                child: container.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        player.object_filters(&container).unwrap(),
        vec![blur.clone()]
    );
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|d| d.owner_ref != shape)
    );
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert!(player.object_filters(&authored).is_err());
    assert!(
        player
            .runtime_snapshot(&project)
            .filter_groups
            .iter()
            .all(|g| g.owner_ref != authored)
    );
    player.reset(&project);
    assert!(player.object_filters(&container).is_err());
    assert!(player.runtime_snapshot(&project).filter_groups.is_empty());
    assert_eq!(project.to_json().unwrap(), original);

    // One original raster workflow exercises shared premultiplied blur,
    // sequential matrix/color effects and shadow composition; no renderer math.
    let mut pixels = vec![0u8; 9 * 9 * 4];
    let center = (4 * 9 + 4) * 4;
    pixels[center..center + 4].copy_from_slice(&[255, 0, 0, 255]);
    pixels[..4].copy_from_slice(&[0, 0, 255, 0]);
    let softened = filter_rgba(9, 9, &pixels, std::slice::from_ref(&blur)).unwrap();
    assert_eq!(&softened[center..center + 4], &[255, 0, 0, 28]);
    assert!(
        softened
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 0 || (p[0] == 255 && p[1] == 0 && p[2] == 0)),
        "invisible blue RGB never leaks into premultiplied blur"
    );
    let matrix = FilterSpec::ColorMatrix {
        matrix: [
            0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 1., 0.,
        ],
    };
    let tinted = filter_rgba(9, 9, &pixels, &[blur.clone(), matrix.clone()]).unwrap();
    assert_eq!(&tinted[center..center + 4], &[0, 255, 0, 28]);
    assert_eq!(filter_rgba(9, 9, &softened, &[matrix]).unwrap(), tinted);
    let shadow = FilterSpec::DropShadow {
        distance: 2.,
        angle: 0.,
        color: 0x112233,
        alpha: 1.,
        blur_x: 0.,
        blur_y: 0.,
        strength: 1.,
        quality: 0,
        inner: false,
        knockout: false,
        hide_object: true,
    };
    let projected = filter_rgba(9, 9, &pixels, std::slice::from_ref(&shadow)).unwrap();
    assert_eq!(&projected[center..center + 4], &[0; 4]);
    assert_eq!(&projected[center + 8..center + 12], &[17, 34, 51, 255]);
    assert_eq!(
        filter_padding(&[blur, shadow]).unwrap(),
        FilterPadding {
            left: 1,
            top: 1,
            right: 3,
            bottom: 1
        }
    );
    let inner = FilterSpec::Glow {
        color: 0xffffff,
        alpha: 1.,
        blur_x: 2.,
        blur_y: 2.,
        strength: 1.,
        quality: 1,
        inner: true,
        knockout: true,
    };
    let knocked = filter_rgba(9, 9, &pixels, &[inner]).unwrap();
    assert!(knocked[center + 3] > 0 && knocked[center + 3] < 255);
    assert!(
        knocked
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .all(|(i, p)| i == 4 * 9 + 4 || p[3] == 0)
    );
    assert!(filter_rgba(8192, 8192, &[], &[]).is_err());
    assert!(filter_rgba(9, 9, &pixels[..pixels.len() - 1], &[]).is_err());
    assert!(
        filter_padding(&vec![
            FilterSpec::Blur {
                blur_x: 1.,
                blur_y: 1.,
                quality: 1
            };
            MAX_OBJECT_FILTERS + 1
        ])
        .is_err()
    );
}
