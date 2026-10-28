use neoflash_core::*;

fn add(
    player: &mut Player,
    project: &Project,
    parent: &ObjectRef,
    child: &ObjectRef,
) -> MutationResult {
    player
        .display_mutate(
            project,
            DisplayMutation::Add {
                parent: parent.clone(),
                child: child.clone(),
                index: None,
            },
        )
        .unwrap()
}
fn object<'a>(scene: &'a RuntimeSnapshot, id: &ObjectRef) -> &'a RuntimeObject {
    scene
        .objects
        .iter()
        .find(|node| &node.object_ref == id)
        .unwrap()
}

#[test]
fn runtime_component_keeps_identity_geometry_and_order_through_its_lifecycle() {
    let mut project = Project::blank();
    let root_id = project.root.clone();
    let mut symbol = project.symbols[0].clone();
    symbol.id = "component".into();
    symbol.name = "Component".into();
    symbol.timeline.layers[0].id = "component-layer".into();
    project.symbols.push(symbol);
    let placement = Element {
        id: "authored-container".into(),
        name: "holder".into(),
        transform: Transform {
            x: 100.,
            ..Transform::default()
        },
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Instance {
            symbol_id: "component".into(),
            playback: InstancePlayback::Independent,
        },
    };
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 3;
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![placement.clone()],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 2,
            elements: vec![placement],
            tween: None,
            label: String::new(),
        },
    ];
    assert!(validate(&project).is_empty());
    let original = project.to_json().unwrap();
    let authored = evaluate(&project, &project.root, 0);
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let left = player.create_display(DynamicKind::Sprite).unwrap();
    let right = player.create_display(DynamicKind::Sprite).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    let movie = player.create_display(DynamicKind::MovieClip).unwrap();
    assert!(player.object_set_property(&left, "x", 10.));
    assert!(player.object_set_property(&right, "x", 300.));
    assert!(player.object_set_property(&shape, "x", 5.));
    assert!(player.object_set_name(&shape, "drawing"));
    assert!(player.object_control(&project, &movie, PlaybackCommand::Stop));
    assert_eq!(player.object_frame_count(&project, &movie), Some(1));
    assert!(!player.object_control(&project, &movie, PlaybackCommand::GotoAndPlay(1)));
    assert!(player.object_control(&project, &movie, PlaybackCommand::Play));
    assert_eq!(
        player.object_metadata(&project, &movie),
        Some(TimelineMetadata {
            current_label: None,
            current_frame_label: None,
            labels: vec![],
            frames_loaded: 1,
            total_frames: 1,
            scene_name: String::new(),
            is_playing: false,
        })
    );
    assert!(player.object_metadata(&project, &shape).is_none());

    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginFill {
                    color: 0x336699,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 100.,
                },
                GraphicsOp::DrawRect {
                    x: 20.,
                    y: 20.,
                    width: 40.,
                    height: 40.,
                },
                GraphicsOp::EndFill,
                GraphicsOp::BeginFill {
                    color: 0xff8844,
                    alpha: 1.,
                },
                GraphicsOp::MoveTo { x: 120., y: 0. },
                GraphicsOp::LineTo { x: 160., y: 0. },
                GraphicsOp::LineStyle {
                    width: Some(2.),
                    color: 0x112233,
                    alpha: 1.,
                },
                GraphicsOp::CurveTo {
                    cx: 180.,
                    cy: 40.,
                    x: 120.,
                    y: 80.,
                },
                GraphicsOp::EndFill,
                GraphicsOp::BeginFill {
                    color: 0x11aa66,
                    alpha: 0.5,
                },
                GraphicsOp::DrawEllipse {
                    x: 200.,
                    y: 10.,
                    width: 60.,
                    height: 40.,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    let detached = player.runtime_snapshot(&project);
    assert!(!object(&detached, &shape).on_stage);
    assert!(!object(&detached, &shape).graphics.is_empty());
    assert!(detached.draws.iter().all(|draw| draw.owner_ref != shape));
    for kind in &object(&detached, &shape).graphics {
        let RuntimeDrawKind::Element(ElementKind::Path { commands, .. }) = kind else {
            panic!("shared path paint")
        };
        validate_path_commands(commands).unwrap();
    }
    let RuntimeDrawKind::Element(ElementKind::Path {
        commands,
        fill_rule,
        ..
    }) = &object(&detached, &shape).graphics[0]
    else {
        panic!("fill")
    };
    assert!(
        hit_test_path(
            commands,
            *fill_rule,
            [5., 5.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        !hit_test_path(
            commands,
            *fill_rule,
            [30., 30.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    // Changing stroke style kept the fill's preceding line, rather than splitting it.
    let RuntimeDrawKind::Element(ElementKind::Path { commands, .. }) =
        &object(&detached, &shape).graphics[1]
    else {
        panic!("fill")
    };
    assert!(
        commands
            .iter()
            .any(|op| matches!(op, PathCommand::LineTo { x: 160., .. }))
    );
    assert!(
        commands
            .iter()
            .any(|op| matches!(op, PathCommand::QuadraticTo { .. }))
    );

    add(&mut player, &project, &holder, &left);
    add(&mut player, &project, &root, &right);
    add(&mut player, &project, &left, &shape);
    add(&mut player, &project, &left, &movie);
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &shape).display.world[4], 115.);
    assert!(
        scene
            .draws
            .iter()
            .filter(|draw| draw.owner_ref == shape)
            .all(|draw| draw.item.transform[4] == 115.)
    );
    let before = serde_json::to_value(&scene).unwrap();
    for invalid in [
        DisplayMutation::Add {
            parent: root.clone(),
            child: right.clone(),
            index: Some(0),
        },
        DisplayMutation::Add {
            parent: shape.clone(),
            child: left.clone(),
            index: None,
        },
        DisplayMutation::Add {
            parent: movie.clone(),
            child: left.clone(),
            index: None,
        },
        DisplayMutation::Remove {
            parent: root.clone(),
            child: holder.clone(),
        },
    ] {
        assert!(player.display_mutate(&project, invalid).is_err());
    }
    assert_eq!(
        serde_json::to_value(player.runtime_snapshot(&project)).unwrap(),
        before
    );
    let events = add(&mut player, &project, &right, &shape).events;
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["removed", "added"]
    );
    assert_eq!(events[0].route[1], left);
    assert_eq!(events[1].route[1], right);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape)
            .display
            .world[4],
        305.
    );
    add(&mut player, &project, &right, &movie);
    player
        .display_mutate(
            &project,
            DisplayMutation::Swap {
                parent: right.clone(),
                first: shape.clone(),
                second: movie.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        object(&player.runtime_snapshot(&project), &right).children,
        vec![movie.clone(), shape.clone()]
    );
    player
        .display_mutate(
            &project,
            DisplayMutation::SetIndex {
                parent: right.clone(),
                child: shape.clone(),
                index: 0,
            },
        )
        .unwrap();
    let removed = player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: right.clone(),
                child: shape.clone(),
            },
        )
        .unwrap();
    assert_eq!(removed.events[0].kind, "removed");
    assert_eq!(removed.events[1].kind, "removed_from_stage");
    assert!(player.object_alive(&shape));
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &shape).parent_ref, None);
    assert_eq!(object(&scene, &shape).display.world[4], 5.);
    assert!(scene.draws.iter().all(|draw| draw.owner_ref != shape));
    add(&mut player, &project, &left, &shape);
    player.object_set_visible(&left, false);
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|draw| draw.owner_ref != shape)
    );
    player.object_set_visible(&left, true);

    // Batch rejection preserves all previous graphics; clear is not partially applied.
    let paints = object(&player.runtime_snapshot(&project), &shape)
        .graphics
        .clone();
    assert!(
        player
            .graphics_apply(
                &shape,
                &[
                    GraphicsOp::Clear,
                    GraphicsOp::LineTo {
                        x: f64::INFINITY,
                        y: 0.
                    }
                ]
            )
            .is_err()
    );
    assert!(
        player
            .graphics_apply(&shape, &vec![GraphicsOp::Clear; MAX_GRAPHICS_OPS + 1])
            .is_err()
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape).graphics,
        paints
    );
    assert!(!player.object_set_property(&shape, "scale_x", 1e200));
    assert!(!player.object_set_text(&project, &shape, "unsupported"));

    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    let scene = player.runtime_snapshot(&project);
    assert!(!player.object_alive(&holder));
    assert!(player.object_alive(&left));
    assert_eq!(object(&scene, &left).parent_ref, None);
    assert_eq!(object(&scene, &shape).parent_ref, Some(left.clone()));
    assert_eq!(object(&scene, &shape).display.world[4], 15.);
    assert!(!object(&scene, &shape).on_stage);
    assert!(
        player
            .drain_display_events()
            .unwrap()
            .events
            .iter()
            .any(|event| event.kind == "removed_from_stage" && event.target_ref == shape)
    );
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(replacement, holder);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &left).parent_ref,
        None
    );
    add(&mut player, &project, &replacement, &left);
    assert!(object(&player.runtime_snapshot(&project), &shape).on_stage);
    assert_eq!(evaluate(&project, &project.root, 0), authored);
    assert_eq!(project.to_json().unwrap(), original);

    let another = Player::new(&project);
    assert!(!another.object_alive(&root));
    player.reset(&project);
    assert!(!player.object_alive(&shape));
    assert!(!player.object_alive(&root));
    assert!(player.graphics_apply(&shape, &[GraphicsOp::Clear]).is_err());
    assert_eq!(player.evaluate(&project), authored);
    // Detached allocation is bounded, including objects never attached to Stage.
    for _ in 0..MAX_DYNAMIC_OBJECTS {
        player.create_display(DynamicKind::Shape).unwrap();
    }
    assert!(player.create_display(DynamicKind::Sprite).is_err());
    gradient_geometry_remains_shared_and_atomic(project.clone());
    affine_color_views_follow_the_display_lifecycle(project.clone());
    clipping_tracks_geometry_coordinates_and_lifetimes(project.clone());
    image_loading_owns_content_and_replaces_timelines(project.clone());
    video_viewports_share_display_state_and_lifetimes();
    mouse_input_capabilities_follow_identity(project.clone());
    movieclip_button_labels_follow_mouse_input(project.clone());
    linked_timelines_are_independent_namespaces(project);
}

fn movieclip_button_labels_follow_mouse_input(mut project: Project) {
    let component = project.symbol_mut("component").unwrap();
    component.linkage_id = Some("ButtonClip".into());
    component.timeline.frame_count = 3;
    component.timeline.layers[0].keyframes = ["_up", "_over", "_down"]
        .into_iter()
        .enumerate()
        .map(|(frame, label)| Keyframe {
            frame: frame as u32,
            elements: vec![],
            tween: None,
            label: label.into(),
        })
        .collect();
    component.timeline.scripts = vec![FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "over();".into(),
        },
    }];
    let mut player = Player::new(&project);
    player.control(&project, &[], PlaybackCommand::Stop);
    let root = player.object_ref(&[]).unwrap();
    let authored = player.object_ref(&["authored-container".into()]).unwrap();
    let linked = player.create_linked(&project, "ButtonClip").unwrap();
    add(&mut player, &project, &root, &linked);
    for target in [&authored, &linked] {
        let mut input = player.object_input_info(&project, target).unwrap();
        assert!(input.movieclip_enabled);
        input.button_mode = true;
        player.object_input_update(&project, target, input).unwrap();
    }
    player.drain_runtime_scripts();
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseMove", Some(&authored))
            .unwrap()
    );
    assert_eq!(player.object_frame(&authored), Some(1));
    assert!(
        !player
            .object_metadata(&project, &authored)
            .unwrap()
            .is_playing
    );
    assert_eq!(player.drain_runtime_scripts().len(), 1);
    assert!(
        !player
            .object_movieclip_pointer(&project, "mouseMove", Some(&authored))
            .unwrap()
    );
    assert!(player.drain_runtime_scripts().is_empty());
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseDown", Some(&authored))
            .unwrap()
    );
    assert_eq!(player.object_frame(&authored), Some(2));
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseMove", None)
            .unwrap()
    );
    assert_eq!(player.object_frame(&authored), Some(1)); // Held drag outside keeps _over.
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseMove", Some(&authored))
            .unwrap()
    );
    assert_eq!(player.object_frame(&authored), Some(2));
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseUp", None)
            .unwrap()
    );
    assert_eq!(player.object_frame(&authored), Some(0));

    assert!(
        player
            .object_movieclip_pointer(&project, "mouseDown", Some(&linked))
            .unwrap()
    );
    assert_eq!(player.object_frame(&linked), Some(2));
    let mut disabled = player.object_input_info(&project, &linked).unwrap();
    disabled.movieclip_enabled = false;
    player
        .object_input_update(&project, &linked, disabled.clone())
        .unwrap();
    assert!(
        !player
            .object_movieclip_pointer(&project, "mouseUp", Some(&linked))
            .unwrap()
    );
    assert_eq!(player.object_frame(&linked), Some(2)); // Disable freezes, not goto(_up).
    assert!(
        player
            .object_input_info(&project, &linked)
            .unwrap()
            .mouse_enabled
    );
    disabled.movieclip_enabled = true;
    player
        .object_input_update(&project, &linked, disabled.clone())
        .unwrap();
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseMove", Some(&linked))
            .unwrap()
    );
    assert_eq!(player.object_frame(&linked), Some(1));
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseDown", Some(&linked))
            .unwrap()
    );
    assert!(
        player
            .object_movieclip_pointer(&project, "cancel", None)
            .unwrap()
    );
    assert_eq!(player.object_frame(&linked), Some(0));
    disabled.button_mode = false;
    player
        .object_input_update(&project, &linked, disabled)
        .unwrap();
    assert!(
        !player
            .object_movieclip_pointer(&project, "mouseMove", Some(&linked))
            .unwrap()
    );
    assert_eq!(player.object_frame(&linked), Some(0));

    let empty = player.create_display(DynamicKind::MovieClip).unwrap();
    add(&mut player, &project, &root, &empty);
    let mut input = player.object_input_info(&project, &empty).unwrap();
    input.button_mode = true;
    player.object_input_update(&project, &empty, input).unwrap();
    assert!(
        !player
            .object_movieclip_pointer(&project, "mouseDown", Some(&empty))
            .unwrap()
    );
    assert_eq!(player.object_frame(&empty), Some(0)); // Missing labels do not invent frames.
    player.reset(&project);
    assert!(
        player
            .object_movieclip_pointer(&project, "mouseMove", Some(&linked))
            .is_err()
    );
    assert!(
        !player
            .object_movieclip_pointer(&project, "cancel", None)
            .unwrap()
    );
}

fn mouse_input_capabilities_follow_identity(project: Project) {
    let original = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let authored = player.object_ref(&["authored-container".into()]).unwrap();
    let target = player.create_display(DynamicKind::Sprite).unwrap();
    let source = player.create_display(DynamicKind::MovieClip).unwrap();
    let text = player.create_display(DynamicKind::TextField).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    let mut input = player.object_input_info(&project, &target).unwrap();
    assert!(input.mouse_enabled && input.mouse_children && input.use_hand_cursor);
    assert!(!input.button_mode && input.hit_area.is_none());
    input.mouse_children = false;
    input.button_mode = true;
    input.hit_area = Some(source.clone());
    player
        .object_input_update(&project, &target, input.clone())
        .unwrap();
    assert_eq!(player.object_input_info(&project, &target).unwrap(), input);
    // Sources remain ordinary display objects; the caller controls visibility
    // and mouseEnabled separately. Detached relations do not change identity.
    assert!(
        player
            .object_input_info(&project, &source)
            .unwrap()
            .mouse_enabled
    );
    add(&mut player, &project, &root, &target);
    add(&mut player, &project, &root, &source);
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &target).input, Some(input.clone()));
    assert!(object(&scene, &shape).input.is_none());
    assert!(player.object_input_info(&project, &shape).is_err());
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: root.clone(),
                child: source.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        player
            .object_input_info(&project, &target)
            .unwrap()
            .hit_area,
        Some(source.clone())
    );
    let mut reverse = player.object_input_info(&project, &source).unwrap();
    reverse.hit_area = Some(target.clone());
    assert!(
        player
            .object_input_update(&project, &source, reverse)
            .is_err()
    );
    let mut invalid = input.clone();
    invalid.mouse_enabled = false;
    invalid.hit_area = Some(shape.clone());
    assert!(
        player
            .object_input_update(&project, &target, invalid)
            .is_err()
    );
    assert_eq!(player.object_input_info(&project, &target).unwrap(), input);
    let mut text_input = player.object_input_info(&project, &text).unwrap();
    text_input.mouse_enabled = false;
    player
        .object_input_update(&project, &text, text_input.clone())
        .unwrap();
    text_input.mouse_children = false;
    assert!(
        player
            .object_input_update(&project, &text, text_input)
            .is_err()
    );
    input.hit_area = Some(authored.clone());
    player
        .object_input_update(&project, &target, input)
        .unwrap();
    let mut authored_input = player.object_input_info(&project, &authored).unwrap();
    authored_input.mouse_enabled = false;
    player
        .object_input_update(&project, &authored, authored_input)
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert!(
        player
            .object_input_info(&project, &target)
            .unwrap()
            .hit_area
            .is_none()
    );
    assert!(player.object_input_info(&project, &authored).is_err());
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(authored, replacement);
    assert!(
        player
            .object_input_info(&project, &replacement)
            .unwrap()
            .mouse_enabled
    );
    player.reset(&project);
    assert!(player.object_input_info(&project, &target).is_err());
    assert_eq!(project.to_json().unwrap(), original);
}

fn video_viewports_share_display_state_and_lifetimes() {
    let mut project = Project::blank();
    let root_id = project.root.clone();
    let timeline = &mut project.symbol_mut(&root_id).unwrap().timeline;
    timeline.frame_count = 2;
    timeline.layers[0].keyframes[0].elements.push(Element {
        id: "authored-video".into(),
        name: "screen".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Video {
            width: 160.,
            height: 90.,
        },
    });
    timeline.layers[0].keyframes.push(Keyframe {
        frame: 1,
        elements: vec![],
        tween: None,
        label: String::new(),
    });
    assert!(validate(&project).is_empty());
    let original = project.to_json().unwrap();
    assert_eq!(Project::from_json(&original).unwrap(), project);
    assert!(matches!(
        evaluate(&project, &root_id, 0)[0].kind,
        ElementKind::Video {
            width: 160.,
            height: 90.
        }
    ));
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let authored = player.object_ref(&["authored-video".into()]).unwrap();
    assert_eq!(
        player.object_video_info(&project, &authored).unwrap().width,
        160.
    );
    let holder = player.create_display(DynamicKind::Sprite).unwrap();
    let video = player.create_display(DynamicKind::Video).unwrap();
    assert_eq!(
        player.object_video_info(&project, &video).unwrap(),
        VideoState::default()
    );
    let state = VideoState {
        width: 80.,
        height: 45.,
        smoothing: true,
    };
    player.object_video_update(&project, &video, state).unwrap();
    assert!(
        player
            .object_video_update(
                &project,
                &video,
                VideoState {
                    width: f64::NAN,
                    ..state
                }
            )
            .is_err()
    );
    assert_eq!(player.object_video_info(&project, &video).unwrap(), state);
    assert!(
        player
            .object_video_update(&project, &holder, state)
            .is_err()
    );
    assert!(player.graphics_apply(&video, &[GraphicsOp::Clear]).is_err());
    assert!(
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: video.clone(),
                    child: holder.clone(),
                    index: None
                }
            )
            .is_err()
    );
    assert!(
        player
            .object_set_mask(&project, &holder, Some(video.clone()))
            .is_err()
    );
    player.object_set_property(&holder, "x", 40.);
    player.object_set_property(&video, "x", 3.);
    player.object_set_property(&holder, "alpha", 0.5);
    player
        .object_set_scroll_rect(
            &project,
            &holder,
            Some(ScrollRect {
                x: 4.,
                y: 0.,
                width: 100.,
                height: 60.,
            }),
        )
        .unwrap();
    add(&mut player, &project, &root, &holder);
    add(&mut player, &project, &holder, &video);
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &video).display.owner, "video");
    assert_eq!(object(&scene, &video).video, Some(state));
    let draws: Vec<_> = scene
        .draws
        .iter()
        .filter(|draw| draw.owner_ref == video)
        .collect();
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].item.transform[4], 39.);
    assert!(!draws[0].clip_ids.is_empty());
    let RuntimeDrawKind::VideoFrame {
        width,
        height,
        smoothing,
        color_transform,
    } = draws[0].item.kind
    else {
        panic!("Video frame descriptor")
    };
    assert_eq!((width, height, smoothing), (80., 45., true));
    assert_eq!(color_transform.multipliers[3], 0.5);
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: holder.clone(),
                child: video.clone(),
            },
        )
        .unwrap();
    assert_eq!(player.object_video_info(&project, &video).unwrap(), state);
    assert!(
        !player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|draw| draw.owner_ref == video)
    );
    add(&mut player, &project, &holder, &video);
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert!(player.object_video_info(&project, &authored).is_err());
    player.reset(&project);
    assert!(player.object_video_info(&project, &video).is_err());
    assert_eq!(project.to_json().unwrap(), original);
}

fn clipping_tracks_geometry_coordinates_and_lifetimes(mut project: Project) {
    project
        .symbol_mut("component")
        .unwrap()
        .timeline
        .frame_count = 1;
    fn hits(clip: &RuntimeClip, point: [f64; 2]) -> bool {
        clip.components.iter().any(|component| {
            let [a, b, c, d, tx, ty] = component.matrix;
            let determinant = a * d - b * c;
            let [x, y] = [point[0] - tx, point[1] - ty];
            hit_test_path(
                &component.commands,
                component.fill_rule,
                [(d * x - c * y) / determinant, (a * y - b * x) / determinant],
                true,
                0.,
                FlattenOptions::default(),
            )
            .unwrap()
        })
    }

    let authored = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let viewport = player.create_display(DynamicKind::Sprite).unwrap();
    let target = player.create_display(DynamicKind::Sprite).unwrap();
    let content = player.create_display(DynamicKind::Shape).unwrap();
    let other = player.create_display(DynamicKind::Sprite).unwrap();
    let mask = player.create_display(DynamicKind::Sprite).unwrap();
    let donut = player.create_display(DynamicKind::Shape).unwrap();
    let island = player.create_display(DynamicKind::Shape).unwrap();
    let stroke = player.create_display(DynamicKind::Shape).unwrap();
    for node in [&viewport, &other, &mask] {
        add(&mut player, &project, &root, node);
    }
    add(&mut player, &project, &viewport, &target);
    add(&mut player, &project, &target, &content);
    for node in [&donut, &island, &stroke] {
        add(&mut player, &project, &mask, node);
    }
    let base = [1., 0., 0., 1., 20., 30.];
    player.object_set_matrix(&viewport, base).unwrap();
    player.object_set_matrix(&mask, base).unwrap();
    player
        .object_set_matrix(&target, [1., 0., 0., 1., 10., 15.])
        .unwrap();
    for (node, x, width, alpha) in [
        (&content, 0., 300., 1.),
        (&donut, 0., 100., 0.),
        (&island, 120., 20., 1.),
    ] {
        let mut ops = vec![
            GraphicsOp::BeginFill {
                color: 0x6699cc,
                alpha,
            },
            GraphicsOp::DrawRect {
                x,
                y: 0.,
                width,
                height: 100.,
            },
        ];
        if node == &donut {
            ops.push(GraphicsOp::DrawRect {
                x: 20.,
                y: 20.,
                width: 40.,
                height: 40.,
            });
        }
        ops.push(GraphicsOp::EndFill);
        player.graphics_apply(node, &ops).unwrap();
    }
    player
        .graphics_apply(
            &stroke,
            &[
                GraphicsOp::LineStyle {
                    width: Some(10.),
                    color: 0xffffff,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 160.,
                    y: 0.,
                    width: 40.,
                    height: 100.,
                },
            ],
        )
        .unwrap();
    assert_eq!(player.object_mask(&target).unwrap(), None);
    assert_eq!(player.object_scroll_rect(&target).unwrap(), None);
    let parent_rect = ScrollRect {
        x: 8.,
        y: 4.,
        width: 200.,
        height: 140.,
    };
    player
        .object_set_scroll_rect(&project, &viewport, Some(parent_rect))
        .unwrap();
    let child_rect = ScrollRect {
        x: 5.,
        y: 6.,
        width: 80.,
        height: 70.,
    };
    player
        .object_set_scroll_rect(&project, &target, Some(child_rect))
        .unwrap();
    player
        .object_set_mask(&project, &target, Some(mask.clone()))
        .unwrap();

    let scene = player.runtime_snapshot(&project);
    assert_eq!(player.object_matrix(&project, &viewport).unwrap(), base);
    assert_eq!(object(&scene, &viewport).local_matrix, base);
    assert_eq!(
        object(&scene, &viewport).content_local_matrix,
        [1., 0., 0., 1., 12., 26.]
    );
    assert_eq!(
        object(&scene, &target).local_matrix,
        [1., 0., 0., 1., 10., 15.]
    );
    assert_eq!(
        object(&scene, &target).content_local_matrix,
        [1., 0., 0., 1., 5., 9.]
    );
    assert_eq!(
        object(&scene, &content).display.world,
        [1., 0., 0., 1., 17., 35.]
    );
    assert_eq!(object(&scene, &target).scroll_rect, Some(child_rect));
    assert_eq!(object(&scene, &target).mask_ref, Some(mask.clone()));
    let ids = &object(&scene, &content).clip_ids;
    assert_eq!(ids.len(), 3);
    assert_eq!(ids, &object(&scene, &target).clip_ids);
    assert!(
        scene
            .draws
            .iter()
            .filter(|draw| draw.owner_ref == content)
            .all(|draw| &draw.clip_ids == ids)
    );
    assert!(scene.draws.iter().any(|draw| draw.owner_ref == content));
    let filled_mask = scene
        .clips
        .iter()
        .find(|clip| clip.components.len() == 2)
        .unwrap();
    assert!(ids.contains(&filled_mask.id));
    // Components union; a transparent filled path keeps its even-odd hole.
    assert!(hits(filled_mask, [25., 35.]));
    assert!(!hits(filled_mask, [50., 60.]));
    assert!(hits(filled_mask, [145., 35.]));
    assert!(!hits(filled_mask, [180., 35.])); // Stroke ink is not mask fill.
    let parent_clip = scene
        .clips
        .iter()
        .find(|clip| object(&scene, &viewport).clip_ids.contains(&clip.id))
        .unwrap();
    assert_eq!(parent_clip.components[0].matrix, base);
    let child_clip = scene
        .clips
        .iter()
        .find(|clip| {
            ids.contains(&clip.id) && clip.id != parent_clip.id && clip.id != filled_mask.id
        })
        .unwrap();
    assert_eq!(child_clip.components[0].matrix, [1., 0., 0., 1., 22., 41.]);
    assert!(!hits(child_clip, [25., 35.])); // The three clip groups intersect.
    for source in [&mask, &donut, &island, &stroke] {
        assert!(player.object_alive(source));
        assert!(!object(&scene, source).display.effective_visible);
        assert!(scene.draws.iter().all(|draw| &draw.owner_ref != source));
    }
    let expected_mask = serde_json::to_value(&filled_mask.components).unwrap();
    player.object_set_visible(&mask, false);
    player.object_set_property(&mask, "alpha", 0.);
    let hidden_scene = player.runtime_snapshot(&project);
    let hidden_mask = hidden_scene
        .clips
        .iter()
        .find(|clip| clip.id == filled_mask.id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&hidden_mask.components).unwrap(),
        expected_mask
    );
    player.object_set_visible(&mask, true);
    player.object_set_property(&mask, "alpha", 1.);

    // Reusing a mask releases its previous owner; clearing restores its paints.
    player
        .object_set_mask(&project, &other, Some(mask.clone()))
        .unwrap();
    assert_eq!(player.object_mask(&target).unwrap(), None);
    assert_eq!(player.object_mask(&other).unwrap(), Some(mask.clone()));
    player.object_set_mask(&project, &other, None).unwrap();
    let released = player.runtime_snapshot(&project);
    assert!(object(&released, &island).display.effective_visible);
    assert!(released.draws.iter().any(|draw| draw.owner_ref == island));
    player
        .object_set_mask(&project, &target, Some(mask.clone()))
        .unwrap();

    // Candidate validation must not partially release, reparent or scroll state.
    let before = serde_json::to_value(player.runtime_snapshot(&project)).unwrap();
    assert!(
        player
            .object_set_mask(&project, &mask, Some(target.clone()))
            .is_err()
    );
    assert!(
        player
            .object_set_scroll_rect(
                &project,
                &target,
                Some(ScrollRect {
                    x: 0.5,
                    ..child_rect
                })
            )
            .is_err()
    );
    assert!(
        player
            .display_mutate(
                &project,
                DisplayMutation::Add {
                    parent: target.clone(),
                    child: mask.clone(),
                    index: None
                }
            )
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(player.runtime_snapshot(&project)).unwrap(),
        before
    );

    // Retiring an authored source clears its link instead of rebinding by path.
    player.object_set_mask(&project, &target, None).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    player
        .object_set_mask(&project, &other, Some(holder.clone()))
        .unwrap();
    player
        .object_set_scroll_rect(&project, &holder, Some(child_rect))
        .unwrap_err();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert_eq!(player.object_mask(&other).unwrap(), None);
    assert!(player.object_mask(&holder).is_err());
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(replacement, holder);
    assert_eq!(player.object_mask(&replacement).unwrap(), None);
    assert_eq!(player.object_scroll_rect(&replacement).unwrap(), None);
    assert_eq!(project.to_json().unwrap(), authored);
    player.reset(&project);
    assert!(player.object_mask(&target).is_err());
    assert!(player.object_scroll_rect(&viewport).is_err());
    assert!(player.runtime_snapshot(&project).clips.is_empty());
}

fn gradient_geometry_remains_shared_and_atomic(project: Project) {
    let authored = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    let copy = player.create_display(DynamicKind::Sprite).unwrap();
    add(&mut player, &project, &root, &shape);
    let linear = GradientSpec {
        kind: GradientKind::Linear,
        colors: vec![0x204060, 0x80a0c0, 0xffffff],
        alphas: vec![0., 0.5, 1.],
        ratios: vec![0., 128., 255.],
        matrix: [0.1, 0., 0., 0.1, 60., 40.],
        spread_method: "pad".into(),
        interpolation_method: "rgb".into(),
        focal_point_ratio: 0.,
    };
    let radial = GradientSpec {
        kind: GradientKind::Radial,
        focal_point_ratio: 1.,
        ..linear.clone()
    };
    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginGradientFill {
                    gradient: linear.clone(),
                },
                GraphicsOp::LineStyle {
                    width: Some(6.),
                    color: 0x123456,
                    alpha: 1.,
                },
                GraphicsOp::DrawRoundRect {
                    x: 0.,
                    y: 0.,
                    width: 120.,
                    height: 80.,
                    ellipse_width: 24.,
                    ellipse_height: 32.,
                },
                GraphicsOp::LineGradientStyle {
                    gradient: radial.clone(),
                },
                GraphicsOp::MoveTo { x: 0., y: 100. },
                GraphicsOp::CubicCurveTo {
                    cx1: 30.,
                    cy1: 75.,
                    cx2: 90.,
                    cy2: 125.,
                    x: 120.,
                    y: 100.,
                },
                GraphicsOp::DrawRoundRectComplex {
                    x: 140.,
                    y: 0.,
                    width: 100.,
                    height: 80.,
                    top_left_radius: 10.,
                    top_right_radius: 20.,
                    bottom_left_radius: 15.,
                    bottom_right_radius: 5.,
                },
                GraphicsOp::DrawPath {
                    commands: vec![
                        PathCommand::MoveTo { x: 0., y: 140. },
                        PathCommand::LineTo { x: 100., y: 140. },
                        PathCommand::LineTo { x: 100., y: 200. },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::NonZero,
                },
                GraphicsOp::LineTo { x: 20., y: 150. },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    let snapshot = player.runtime_snapshot(&project);
    let raw = object(&snapshot, &shape).graphics.clone();
    for paint in &raw {
        let commands = match paint {
            RuntimeDrawKind::Element(ElementKind::Path { commands, .. })
            | RuntimeDrawKind::GradientPath { commands, .. } => commands,
            _ => panic!("shared path geometry"),
        };
        validate_path_commands(commands).unwrap();
    }
    let RuntimeDrawKind::GradientPath {
        commands,
        fill_rule,
        fill_gradient: Some(gradient),
        stroke_gradient,
        stroke_width,
        ..
    } = &raw[0]
    else {
        panic!("gradient fill")
    };
    assert_eq!(gradient.kind, GradientKind::Linear);
    assert_eq!(gradient.stops[1].color, [128, 160, 192, 128]);
    assert!(stroke_gradient.is_none());
    assert_eq!(*stroke_width, 0.);
    assert!(
        hit_test_path(
            commands,
            *fill_rule,
            [60., 40.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        !hit_test_path(
            commands,
            *fill_rule,
            [0., 0.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(raw.iter().any(|p| matches!(
        p,
        RuntimeDrawKind::GradientPath {
            fill_rule: FillRule::NonZero,
            ..
        }
    )));
    assert!(raw.iter().any(|p|matches!(p,RuntimeDrawKind::GradientPath{stroke_gradient:Some(g),fill_gradient:None,fill,..} if g.kind==GradientKind::Radial && *fill==[0;4])));
    let transform = ColorTransform {
        multipliers: [0.5, 1., 1., 0.5],
        offsets: [10., 0., 0., 10.],
    };
    player
        .object_set_color_transform(&shape, transform)
        .unwrap();
    let transformed = player.runtime_snapshot(&project);
    assert_eq!(object(&transformed, &shape).graphics, raw);
    let first = transformed
        .draws
        .iter()
        .find(|d| d.owner_ref == shape)
        .unwrap();
    let RuntimeDrawKind::GradientPath {
        fill_gradient: Some(gradient),
        ..
    } = &first.item.kind
    else {
        panic!("transformed gradient")
    };
    assert_eq!(gradient.stops[0].color, transform.apply([32, 64, 96, 0]));
    assert_eq!(first.item.alpha, 1.);
    player.graphics_copy_from(&copy, &shape).unwrap();
    player.graphics_apply(&shape, &[GraphicsOp::Clear]).unwrap();
    assert_eq!(
        object(&player.runtime_snapshot(&project), &copy).graphics,
        raw
    );
    let mut invalid = linear;
    invalid.ratios = vec![255., 128., 0.];
    for operations in [
        vec![
            GraphicsOp::Clear,
            GraphicsOp::BeginGradientFill { gradient: invalid },
        ],
        vec![
            GraphicsOp::Clear,
            GraphicsOp::LineGradientStyle { gradient: radial },
        ],
        vec![
            GraphicsOp::Clear,
            GraphicsOp::DrawPath {
                commands: vec![PathCommand::LineTo { x: f64::NAN, y: 0. }],
                fill_rule: FillRule::EvenOdd,
            },
        ],
    ] {
        assert!(player.graphics_apply(&copy, &operations).is_err());
        assert_eq!(
            object(&player.runtime_snapshot(&project), &copy).graphics,
            raw
        );
    }
    let wire = serde_json::to_value(&raw[0]).unwrap();
    assert_eq!(wire["type"], "gradient_path");
    assert_eq!(wire["fill_rule"], "evenodd");
    assert_eq!(wire["fill_gradient"]["stops"][1]["ratio"], 128);
    assert_eq!(project.to_json().unwrap(), authored);
}

fn affine_color_views_follow_the_display_lifecycle(mut project: Project) {
    let marker = Element {
        id: "transform-marker".into(),
        name: "marker".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: Some(Accessibility {
            role: AccessibleRole::Button,
            name: "Marker".into(),
            description: String::new(),
            tab_order: 0,
            disabled: false,
        }),
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill: [32, 64, 96, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    };
    project.symbol_mut("component").unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(marker);
    let untouched = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let marker = player
        .object_ref(&["authored-container".into(), "transform-marker".into()])
        .unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    add(&mut player, &project, &holder, &shape);
    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginFill {
                    color: 0x204060,
                    alpha: 0.,
                },
                GraphicsOp::LineStyle {
                    width: Some(2.),
                    color: 0xffffff,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 0.,
                    y: 0.,
                    width: 30.,
                    height: 20.,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    player
        .object_set_matrix(&holder, [1., 0., 0.5, 1., 20., 30.])
        .unwrap();
    assert!(player.set_property(&["authored-container".into()], "x", 25.));
    assert!(player.object_set_property(&holder, "rotation", 90.));
    assert!(player.object_set_property(&holder, "scale_x", 0.));
    assert!(player.object_set_property(&holder, "scale_x", -2.));
    let expected = [0., -2., -1., 0.5, 25., 30.];
    let matrix = player.object_matrix(&project, &holder).unwrap();
    for (actual, expected) in matrix.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(
        player.property(&project, &["authored-container".into()], "scale_x"),
        Some(-2.)
    );
    let color = ColorTransform {
        multipliers: [1., 1., 1., 0.],
        offsets: [10., 20., 30., 96.],
    };
    player.object_set_color_transform(&holder, color).unwrap();
    player
        .object_set_color_transform(
            &shape,
            ColorTransform {
                multipliers: [0.5, 1., 2., 1.],
                offsets: [0.; 4],
            },
        )
        .unwrap();
    assert_eq!(player.object_property(&project, &holder, "alpha"), Some(0.));
    let scene = player.runtime_snapshot(&project);
    assert!(scene.semantics.iter().any(|node| node.owner_ref == marker));
    assert_eq!(object(&scene, &holder).local_matrix, matrix);
    let drawings: Vec<_> = scene
        .draws
        .iter()
        .filter(|draw| draw.owner_ref == shape)
        .collect();
    assert_eq!(drawings.len(), 2);
    assert_eq!(drawings[0].item.alpha, 1.);
    let RuntimeDrawKind::Element(ElementKind::Path { fill, .. }) = &drawings[0].item.kind else {
        panic!("fill");
    };
    assert_eq!(*fill, [26, 84, 222, 96]); // Intentionally transparent fill becomes visible.
    let RuntimeDrawKind::Element(ElementKind::Path { fill, stroke, .. }) = &drawings[1].item.kind
    else {
        panic!("stroke");
    };
    assert_eq!(*fill, [0; 4]); // A stroke run must not invent an interior fill.
    assert_eq!(stroke[3], 96);

    let text = player.create_display(DynamicKind::TextField).unwrap();
    add(&mut player, &project, &holder, &text);
    assert!(player.object_set_text(&project, &text, "Tinted text"));
    let pixels = player.bitmap_create(2, 1, true, 0xff204060).unwrap();
    let bitmap = player.create_display(DynamicKind::Bitmap).unwrap();
    let sibling = player.create_display(DynamicKind::Bitmap).unwrap();
    for node in [&bitmap, &sibling] {
        player
            .object_set_bitmap(&project, node, Some(pixels.clone()))
            .unwrap();
    }
    add(&mut player, &project, &holder, &bitmap);
    add(&mut player, &project, &root, &sibling);
    let scene = player.runtime_snapshot(&project);
    let text_draw = scene
        .draws
        .iter()
        .find(|draw| draw.owner_ref == text)
        .unwrap();
    let RuntimeDrawKind::TextField { field } = &text_draw.item.kind else {
        panic!("text");
    };
    assert_eq!((field.format.color, field.alpha), (0x0a141e, 96));
    assert_eq!(object(&scene, &text).text.as_ref().unwrap().format.color, 0);
    let bitmap_draw = scene
        .draws
        .iter()
        .find(|draw| draw.owner_ref == bitmap)
        .unwrap();
    let RuntimeDrawKind::BitmapPixels {
        color_transform, ..
    } = bitmap_draw.item.kind
    else {
        panic!("bitmap");
    };
    let original_pixels = player.bitmap_pixels(&pixels).unwrap();
    let original_revision = player.bitmap_info(&pixels).unwrap().revision;
    assert_eq!(
        player
            .bitmap_pixels_transformed(&pixels, color_transform)
            .unwrap(),
        [42, 84, 126, 96, 42, 84, 126, 96]
    );
    assert_eq!(
        player
            .bitmap_pixels_transformed(&pixels, ColorTransform::default())
            .unwrap(),
        original_pixels
    );
    assert_eq!(player.bitmap_pixels(&pixels).unwrap(), original_pixels);
    assert_eq!(
        player.bitmap_info(&pixels).unwrap().revision,
        original_revision
    );
    assert_eq!(
        object(&scene, &sibling).concatenated_color_transform,
        ColorTransform::default()
    );

    assert!(player.object_set_matrix(&holder, [f64::NAN; 6]).is_err());
    assert!(
        player
            .object_set_color_transform(
                &holder,
                ColorTransform {
                    offsets: [256.; 4],
                    ..color
                }
            )
            .is_err()
    );
    assert_eq!(player.object_matrix(&project, &holder).unwrap(), matrix);
    assert_eq!(
        player.object_color_transform(&project, &holder).unwrap(),
        color
    );
    player
        .object_set_color_transform(
            &holder,
            ColorTransform {
                offsets: [0.; 4],
                ..color
            },
        )
        .unwrap();
    assert!(
        !player
            .runtime_snapshot(&project)
            .semantics
            .iter()
            .any(|node| node.owner_ref == marker)
    );
    assert!(player.set_property(&["authored-container".into()], "alpha", 0.5));
    assert_eq!(
        player
            .object_color_transform(&project, &holder)
            .unwrap()
            .multipliers[3],
        0.5
    );
    player.object_set_color_transform(&holder, color).unwrap();
    player
        .object_set_matrix(&shape, [1., 0.25, 0., 1., 5., 6.])
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    let detached = player.runtime_snapshot(&project);
    assert!(!player.object_alive(&holder));
    assert!(player.object_matrix(&project, &holder).is_err());
    assert!(!object(&detached, &shape).on_stage);
    assert_eq!(
        object(&detached, &shape).local_matrix,
        [1., 0.25, 0., 1., 5., 6.]
    );
    assert_eq!(
        object(&detached, &shape)
            .concatenated_color_transform
            .multipliers,
        [0.5, 1., 2., 1.]
    );
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(holder, replacement);
    assert_eq!(
        player
            .object_color_transform(&project, &replacement)
            .unwrap(),
        ColorTransform::default()
    );
    assert_eq!(
        player.object_matrix(&project, &replacement).unwrap(),
        [1., 0., 0., 1., 100., 0.]
    );
    add(&mut player, &project, &replacement, &shape);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape)
            .display
            .world[4],
        105.
    );
    player.reset(&project);
    assert!(
        player
            .object_set_matrix(&shape, [1., 0., 0., 1., 0., 0.])
            .is_err()
    );
    assert_eq!(project.to_json().unwrap(), untouched);
}

fn linked_timelines_are_independent_namespaces(mut project: Project) {
    let script = |source: &str| Script {
        language: ScriptLanguage::JavaScript,
        source: source.into(),
    };
    let leaf = Element {
        id: "linked-leaf".into(),
        name: "action".into(),
        transform: Transform {
            x: 5.,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill: [10, 20, 30, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        accessibility: Some(Accessibility {
            role: AccessibleRole::Button,
            name: "Action".into(),
            description: String::new(),
            tab_order: 1,
            disabled: false,
        }),
        scripts: vec![
            ObjectScript {
                event: ObjectEvent::Load,
                script: script("load();"),
            },
            ObjectScript {
                event: ObjectEvent::Click,
                script: script("click();"),
            },
        ],
    };
    let text = Element {
        id: "linked-text".into(),
        name: "caption".into(),
        transform: Transform::default(),
        accessibility: None,
        scripts: vec![],
        kind: ElementKind::Text {
            text: "Original".into(),
            font_family: "Arial".into(),
            font_size: 16.,
            color: [0, 0, 0, 255],
        },
    };
    let nested = Element {
        id: "linked-nested".into(),
        name: "nested".into(),
        transform: Transform {
            x: 10.,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: "pulse".into(),
            playback: InstancePlayback::Independent,
        },
        accessibility: None,
        scripts: vec![],
    };
    let mut pulse = project.symbol("component").unwrap().clone();
    pulse.id = "pulse".into();
    pulse.name = "Pulse".into();
    pulse.timeline.frame_count = 4;
    pulse.timeline.layers[0].id = "pulse-layer".into();
    pulse.timeline.layers[0].keyframes[0].elements = vec![leaf.clone()];
    pulse.timeline.layers[0].keyframes[0].label = "PulseStart".into();
    pulse.timeline.layers[0].keyframes.push(Keyframe {
        frame: 2,
        elements: vec![leaf.clone()],
        tween: None,
        label: "PulseLater".into(),
    });
    pulse.timeline.scripts = vec![FrameScript {
        frame: 1,
        script: script("pulse();"),
    }];
    project.symbols.push(pulse);
    let component = project.symbol_mut("component").unwrap();
    component.linkage_id = Some("ExportedClip".into());
    component.timeline.frame_count = 3;
    component.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![nested.clone(), text.clone()],
            tween: None,
            label: "Start".into(),
        },
        Keyframe {
            frame: 1,
            elements: vec![nested, text],
            tween: None,
            label: "Turn".into(),
        },
        Keyframe {
            frame: 2,
            elements: vec![],
            tween: None,
            label: "Empty".into(),
        },
    ];
    component.timeline.scripts = vec![
        FrameScript {
            frame: 0,
            script: script("initial();"),
        },
        FrameScript {
            frame: 1,
            script: script("turn();"),
        },
    ];
    let mut label_layer = component.timeline.layers[0].clone();
    label_layer.id = "component-labels".into();
    label_layer.keyframes = vec![
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: "TurnAlias".into(),
        },
        Keyframe {
            frame: 2,
            elements: vec![],
            tween: None,
            label: "Start".into(),
        },
    ];
    component.timeline.layers.push(label_layer);
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.layers[0].keyframes[0].elements[0].accessibility = Some(Accessibility {
        role: AccessibleRole::Button,
        name: "Disabled holder".into(),
        description: String::new(),
        tab_order: 0,
        disabled: true,
    });
    project.enable_program();
    assert!(validate(&project).is_empty());
    let mut player = Player::new(&project);
    player.control(&project, &[], PlaybackCommand::Stop);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let root_metadata = player.object_metadata(&project, &root).unwrap();
    assert_eq!(root_metadata, player.clip_metadata(&project, &[]).unwrap());
    assert_eq!(root_metadata.frames_loaded, 3);
    assert_eq!(root_metadata.total_frames, 3);
    assert_eq!(root_metadata.scene_name, "");
    assert!(root_metadata.current_label.is_none());
    assert!(!root_metadata.is_playing);
    assert_eq!(
        player
            .object_metadata(&project, &holder)
            .unwrap()
            .current_label
            .as_deref(),
        Some("Start")
    );
    player.drain_runtime_scripts();
    assert!(player.create_linked(&project, "Component").is_err()); // Never guess by symbol name.
    let first = player.create_linked(&project, "ExportedClip").unwrap();
    let second = player.create_linked(&project, "ExportedClip").unwrap();
    let initial_metadata = player.object_metadata(&project, &first).unwrap();
    assert_eq!(initial_metadata.current_label.as_deref(), Some("Start"));
    assert_eq!(
        initial_metadata.current_frame_label.as_deref(),
        Some("Start")
    );
    assert!(!initial_metadata.is_playing);
    assert_eq!(initial_metadata.frames_loaded, 3);
    assert_eq!(initial_metadata.total_frames, 3);
    assert_eq!(initial_metadata.scene_name, "");
    assert_eq!(
        initial_metadata.labels,
        vec![
            FrameLabel::new("Start".into(), 1),
            FrameLabel::new("Turn".into(), 2),
            FrameLabel::new("TurnAlias".into(), 2),
            FrameLabel::new("Empty".into(), 3),
            FrameLabel::new("Start".into(), 3),
        ]
    );
    let initial = player.drain_runtime_scripts();
    assert!(
        initial
            .iter()
            .any(|item| item.target_ref == first && item.dispatch.script.source == "initial();")
    );
    assert!(
        initial
            .iter()
            .any(|item| item.target_ref == second && item.dispatch.script.source == "initial();")
    );
    assert!(
        initial
            .iter()
            .all(|item| player.is_runtime_dispatch_current(&project, item))
    );
    let find = |player: &Player, namespace: &ObjectRef, path: &[&str]| {
        player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|node| {
                node.namespace_ref.as_ref() == Some(namespace)
                    && node.source_path.as_deref()
                        == Some(&path.iter().map(|part| part.to_string()).collect::<Vec<_>>())
            })
            .unwrap()
            .object_ref
            .clone()
    };
    let first_nested = find(&player, &first, &["linked-nested"]);
    let first_leaf = find(&player, &first, &["linked-nested", "linked-leaf"]);
    let second_leaf = find(&player, &second, &["linked-nested", "linked-leaf"]);
    let first_text = find(&player, &first, &["linked-text"]);
    let second_text = find(&player, &second, &["linked-text"]);
    assert_ne!(first_leaf, second_leaf);
    assert!(player.object_set_text(&project, &first_text, "First only"));
    assert!(
        matches!(object(&player.runtime_snapshot(&project),&second_text).display.kind,Some(ElementKind::Text{ref text,..}) if text=="Original")
    );
    player.object_set_property(&first, "x", 100.);
    add(&mut player, &project, &holder, &first);
    add(&mut player, &project, &root, &second);
    assert!(!player.object_activate_accessible(&project, &first_leaf));
    assert!(
        player
            .runtime_snapshot(&project)
            .semantics
            .iter()
            .any(|node| node.owner_ref == first_leaf && node.node.accessibility.disabled)
    );
    add(&mut player, &project, &root, &first);
    assert!(player.object_activate_accessible(&project, &first_leaf));
    let clicked = player.drain_runtime_scripts();
    assert_eq!(
        clicked
            .iter()
            .filter(|item| item.dispatch.event == ScriptEvent::Click)
            .count(),
        1
    );
    assert_eq!(clicked[0].target_ref, first_leaf);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &first_leaf)
            .display
            .world[4],
        115.
    );
    for property in ["alpha", "scale_x"] {
        player.object_set_property(&first, property, 0.);
        assert!(
            !player
                .runtime_snapshot(&project)
                .semantics
                .iter()
                .any(|node| node.owner_ref == first_leaf)
        );
        player.object_set_property(&first, property, 1.);
    }
    player.object_set_visible(&first, false);
    assert!(!player.object_click(&project, &first_leaf));
    player.object_set_visible(&first, true);
    assert_eq!(
        player.object_resolve_frame(&project, &first, "turn", true),
        Some(1)
    );
    assert_eq!(
        player.object_resolve_frame(&project, &first, "turn", false),
        None
    );
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(1));
    let stopped_metadata = player.object_metadata(&project, &first).unwrap();
    assert_eq!(stopped_metadata.current_label.as_deref(), Some("Turn"));
    assert_eq!(
        stopped_metadata.current_frame_label.as_deref(),
        Some("Turn")
    );
    assert!(!stopped_metadata.is_playing);
    player.object_control(&project, &second, PlaybackCommand::Stop);
    let entered = player.drain_runtime_scripts();
    player.tick(&project);
    assert_eq!(player.object_frame(&first), Some(1));
    assert_eq!(player.object_frame(&second), Some(0));
    assert_eq!(player.object_frame(&first_nested), Some(1));
    let nested_metadata = player.object_metadata(&project, &first_nested).unwrap();
    assert_eq!(nested_metadata.current_label.as_deref(), Some("PulseStart"));
    assert!(nested_metadata.current_frame_label.is_none());
    assert_eq!(nested_metadata.frames_loaded, 4);
    assert_eq!(nested_metadata.total_frames, 4);
    assert_eq!(nested_metadata.scene_name, "");
    assert!(
        player.drain_runtime_scripts().iter().any(
            |item| item.target_ref == first_nested && item.dispatch.script.source == "pulse();"
        )
    );
    let decoration = player.create_display(DynamicKind::Sprite).unwrap();
    add(&mut player, &project, &first_nested, &decoration);
    assert!(player.object_control(&project, &first_nested, PlaybackCommand::GotoAndPlay(2)));
    let nested_metadata = player.object_metadata(&project, &first_nested).unwrap();
    assert_eq!(nested_metadata.current_label.as_deref(), Some("PulseLater"));
    assert_eq!(
        nested_metadata.current_frame_label.as_deref(),
        Some("PulseLater")
    );
    assert!(nested_metadata.is_playing);
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(2));
    assert!(player.object_metadata(&project, &first_nested).is_none());
    assert!(!player.object_alive(&first_leaf));
    assert!(player.object_alive(&second_leaf));
    assert!(
        entered
            .iter()
            .all(|item| !player.is_runtime_dispatch_current(&project, item))
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &decoration).parent_ref,
        None
    );
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(0));
    let recreated = find(&player, &first, &["linked-nested", "linked-leaf"]);
    assert_ne!(recreated, first_leaf);
    player.drain_runtime_scripts();
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: root.clone(),
                child: first.clone(),
            },
        )
        .unwrap();
    player.object_control(&project, &first, PlaybackCommand::Play);
    player.set_playing(false);
    assert!(player.object_metadata(&project, &first).unwrap().is_playing);
    player.tick(&project);
    assert_eq!(player.object_frame(&first), Some(0));
    player.set_playing(true);
    player.tick(&project);
    assert_eq!(player.object_frame(&first), Some(1));
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|draw| draw.owner_ref != recreated)
    );
    assert!(
        player
            .drain_runtime_scripts()
            .iter()
            .any(|item| item.target_ref == first && item.dispatch.script.source == "turn();")
    );
    add(&mut player, &project, &root, &first);
    assert!(object(&player.runtime_snapshot(&project), &recreated).on_stage);

    let nested = find(&player, &first, &["linked-nested"]);
    player.object_control(&project, &first, PlaybackCommand::Play);
    player.object_control(&project, &nested, PlaybackCommand::Play);
    player.object_control(&project, &second, PlaybackCommand::Play);
    let frames = (player.object_frame(&first), player.object_frame(&nested));
    let sibling_frame = player.object_frame(&second).unwrap();
    player
        .object_stop_all_movie_clips(&project, &first)
        .unwrap();
    assert!(!player.object_metadata(&project, &first).unwrap().is_playing);
    assert!(
        !player
            .object_metadata(&project, &nested)
            .unwrap()
            .is_playing
    );
    assert!(
        player
            .object_metadata(&project, &second)
            .unwrap()
            .is_playing
    );
    player.tick(&project);
    assert_eq!(
        (player.object_frame(&first), player.object_frame(&nested)),
        frames
    );
    assert_eq!(player.object_frame(&second), Some((sibling_frame + 1) % 3));
    assert!(
        player
            .object_stop_all_movie_clips(&project, &recreated)
            .is_err()
    );

    let children = object(&player.runtime_snapshot(&project), &root)
        .children
        .clone();
    assert!(
        player
            .object_remove_children(&project, &root, 0, i32::MAX)
            .is_err()
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &root).children,
        children
    );
    for (begin, end) in [(-1, i32::MAX), (2, 1), (0, children.len() as i32)] {
        assert!(
            player
                .object_remove_children(&project, &root, begin, end)
                .is_err()
        );
        assert_eq!(
            object(&player.runtime_snapshot(&project), &root).children,
            children
        );
    }
    let a = children.iter().position(|child| child == &first).unwrap() as i32;
    let b = children.iter().position(|child| child == &second).unwrap() as i32;
    let removed = player
        .object_remove_children(&project, &root, a.min(b), a.max(b))
        .unwrap();
    assert_eq!(
        removed
            .events
            .iter()
            .filter(|event| event.kind == "removed")
            .count(),
        2
    );
    assert!(
        removed
            .events
            .iter()
            .all(|event| event.route.last() == Some(&root))
    );
    assert!(
        removed
            .events
            .iter()
            .any(|event| event.kind == "removed_from_stage" && event.target_ref == nested)
    );
    for child in [&first, &second] {
        assert!(player.object_alive(child));
        assert!(
            object(&player.runtime_snapshot(&project), child)
                .parent_ref
                .is_none()
        );
        assert!(!object(&player.runtime_snapshot(&project), child).on_stage);
    }
    // A detached subtree can still be stopped, without stopping another
    // detached linked namespace or changing either root's identity.
    player.object_control(&project, &first, PlaybackCommand::Play);
    player.object_control(&project, &nested, PlaybackCommand::Play);
    player
        .object_stop_all_movie_clips(&project, &first)
        .unwrap();
    player.tick(&project);
    assert_eq!(
        (player.object_frame(&first), player.object_frame(&nested)),
        frames
    );
    add(&mut player, &project, &root, &second);
    add(&mut player, &project, &root, &first);
    assert!(object(&player.runtime_snapshot(&project), &nested).on_stage);
    assert!(
        player
            .object_remove_children(&project, &decoration, 0, i32::MAX)
            .unwrap()
            .events
            .is_empty()
    );
    assert!(
        player
            .object_remove_children(&project, &decoration, 0, 0)
            .is_err()
    );
    assert!(
        player
            .object_remove_children(&project, &root, 100, i32::MAX)
            .unwrap()
            .events
            .is_empty()
    );
    let loader = player.create_display(DynamicKind::Loader).unwrap();
    assert!(
        player
            .object_remove_children(&project, &loader, 0, i32::MAX)
            .is_err()
    );
    player.reset(&project);
    assert!(!player.object_alive(&first));
    assert!(!player.object_alive(&recreated));
    assert!(player.object_metadata(&project, &first).is_none());
    assert!(
        player
            .object_remove_children(&project, &first, 0, i32::MAX)
            .is_err()
    );
    assert!(
        player
            .object_stop_all_movie_clips(&project, &first)
            .is_err()
    );
    let reset_root = player.object_ref(&[]).unwrap();
    assert!(
        !player
            .object_metadata(&project, &reset_root)
            .unwrap()
            .is_playing
    );
}

fn image_loading_owns_content_and_replaces_timelines(mut project: Project) {
    use image::ImageEncoder;
    let component = project.symbol_mut("component").unwrap();
    component.timeline.frame_count = 2;
    component.timeline.scripts = vec![FrameScript {
        frame: 1,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "oldTimeline();".into(),
        },
    }];
    component.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "old-loaded-child".into(),
        name: "oldArt".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [255; 4],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    }];
    let original = project.to_json().unwrap();
    let mut png = vec![];
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &[200, 40, 80, 255, 20, 120, 240, 255],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let mut jpeg = vec![];
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .write_image(
            &[200, 40, 80, 20, 120, 240],
            2,
            1,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let mut player = Player::new(&project);
    player.control(&project, &[], PlaybackCommand::Stop);
    player.drain_runtime_scripts();
    let root = player.object_ref(&[]).unwrap();
    let loader = player.create_display(DynamicKind::Loader).unwrap();
    add(&mut player, &project, &root, &loader);
    let first = player.pixel_decode(&png).unwrap();
    assert_eq!(
        player.bitmap_get_pixel(&first, 0, 0, true).unwrap(),
        0xffc82850
    );
    assert!(player.pixel_decode(&png[..20]).is_err());
    let loaded = player
        .loader_replace(&project, &loader, Some(first.clone()), false)
        .unwrap();
    let content = loaded.content_ref.unwrap();
    assert_eq!(
        loaded
            .events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        ["added", "added_to_stage"]
    );
    assert_eq!(
        player.loader_content(&loader).unwrap(),
        Some(content.clone())
    );
    assert!(player.pixel_release(&first).is_err());
    assert!(
        player
            .display_mutate(
                &project,
                DisplayMutation::Remove {
                    parent: loader.clone(),
                    child: content.clone()
                }
            )
            .is_err()
    );
    let second = player.pixel_decode(&jpeg).unwrap();
    assert_eq!(player.bitmap_info(&second).unwrap().width, 2);
    let swapped = player
        .loader_replace(&project, &loader, Some(second.clone()), false)
        .unwrap();
    let next = swapped.content_ref.unwrap();
    assert!(swapped.retired_refs.is_empty());
    assert!(player.object_alive(&content));
    assert_eq!(
        object(&player.runtime_snapshot(&project), &content).parent_ref,
        None
    );
    assert_eq!(
        player.bitmap_get_pixel(&first, 0, 0, true).unwrap(),
        0xffc82850
    );
    player
        .loader_replace(&project, &loader, None, false)
        .unwrap();
    assert!(player.object_alive(&next));
    assert_eq!(player.loader_content(&loader).unwrap(), None);
    add(&mut player, &project, &root, &content); // Retained detached content is adoptable.
    let abandoned = player.pixel_decode(&png).unwrap();
    player.pixel_release(&abandoned).unwrap();
    assert!(player.bitmap_info(&abandoned).is_err());

    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let old_child = player
        .object_ref(&["authored-container".into(), "old-loaded-child".into()])
        .unwrap();
    let runtime_child = player.create_display(DynamicKind::MovieClip).unwrap();
    add(&mut player, &project, &holder, &runtime_child);
    let replaced = player
        .loader_replace(&project, &holder, Some(first.clone()), true)
        .unwrap();
    let as2_content = replaced.content_ref.unwrap();
    player
        .as2_input_command(
            &project,
            &holder,
            serde_json::json!({"op":"set","name":"forceSmoothing","value":true}),
        )
        .unwrap();
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|draw| draw.owner_ref == as2_content
                && matches!(
                    draw.item.kind,
                    RuntimeDrawKind::BitmapPixels {
                        smoothing: true,
                        ..
                    }
                ))
    );
    player
        .as2_input_command(
            &project,
            &holder,
            serde_json::json!({"op":"set","name":"forceSmoothing","value":false}),
        )
        .unwrap();
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|draw| draw.owner_ref == as2_content
                && matches!(
                    draw.item.kind,
                    RuntimeDrawKind::BitmapPixels {
                        smoothing: false,
                        ..
                    }
                ))
    );
    assert!(replaced.retired_refs.contains(&old_child));
    assert!(replaced.retired_refs.contains(&runtime_child));
    assert!(!player.object_alive(&old_child));
    assert!(!player.object_alive(&runtime_child));
    assert!(player.object_alive(&holder));
    assert_eq!(player.object_frame_count(&project, &holder), Some(1));
    assert_eq!(player.object_property(&project, &holder, "x"), Some(100.));
    player.tick(&project);
    assert!(
        player
            .drain_runtime_scripts()
            .iter()
            .all(|dispatch| dispatch.dispatch.script.source != "oldTimeline();")
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &holder)
            .children
            .as_slice(),
        std::slice::from_ref(&as2_content)
    );
    player
        .loader_replace(&project, &holder, None, true)
        .unwrap();
    player.tick(&project);
    assert!(!player.object_alive(&as2_content));
    assert!(
        object(&player.runtime_snapshot(&project), &holder)
            .children
            .is_empty()
    );
    let again = player
        .loader_replace(&project, &holder, Some(first.clone()), true)
        .unwrap()
        .content_ref
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert!(!player.object_alive(&again));
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let fresh_holder = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(holder, fresh_holder);
    assert!(
        player
            .object_ref(&["authored-container".into(), "old-loaded-child".into()])
            .is_some()
    );
    assert_eq!(project.to_json().unwrap(), original);
    player.reset(&project);
    assert!(!player.object_alive(&loader));
    assert!(player.bitmap_info(&first).is_err());
}
