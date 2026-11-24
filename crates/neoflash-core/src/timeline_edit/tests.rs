use super::*;

fn format13_range_script_project() -> (Project, Id, Id, Id) {
    let mut project = Project::blank();
    project.format_version = 13;
    let root = project.root.clone();
    let first_layer: Id = "range-script-first".into();
    let second_layer: Id = "range-script-second".into();
    let timeline = &mut project.symbol_mut(&root).unwrap().timeline;
    timeline.frame_count = 10;
    timeline.layers[0].id = first_layer.clone();
    timeline.layers[0].name = "First".into();
    timeline.layers[0].properties.end_frame = Some(10);
    timeline.layers.push(Layer {
        id: second_layer.clone(),
        name: "Second".into(),
        visible: true,
        locked: false,
        keyframes: vec![blank(0)],
        properties: LayerProperties {
            end_frame: Some(10),
            ..LayerProperties::default()
        },
    });
    timeline.scripts = [
        ("first-2", first_layer.clone(), 2),
        ("second-2", second_layer.clone(), 2),
        ("first-6", first_layer.clone(), 6),
        ("second-6", second_layer.clone(), 6),
    ]
    .into_iter()
    .map(|(source, layer, frame)| FrameScript {
        layer: Some(layer),
        frame,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: source.into(),
        },
    })
    .collect();
    project.validate().unwrap();
    (project, root, first_layer, second_layer)
}

fn script_position(project: &Project, root: &Id, source: &str) -> (Option<Id>, u32) {
    let script = project
        .symbol(root)
        .unwrap()
        .timeline
        .scripts
        .iter()
        .find(|script| script.script.source == source)
        .unwrap_or_else(|| panic!("missing test frame script {source}"));
    (script.layer.clone(), script.frame)
}

#[test]
fn convert_to_blank_keyframes_preserves_comment_and_anchor_labels() {
    let mut project = Project::blank();
    let root = project.root.clone();
    let timeline = &mut project.symbol_mut(&root).unwrap().timeline;
    timeline.frame_count = 8;
    let layer_id = timeline.layers[0].id.clone();
    let artwork = Element {
        id: "labelled-artwork".into(),
        kind: ElementKind::Rectangle {
            width: 10.0,
            height: 10.0,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Default::default()
    };
    timeline.layers[0].keyframes = [
        (0, "intro", FrameLabelType::Name),
        (2, "author note", FrameLabelType::Comment),
        (4, "chapter", FrameLabelType::Anchor),
        (6, "outro", FrameLabelType::Name),
    ]
    .into_iter()
    .map(|(frame, label, label_type)| Keyframe {
        frame,
        label: label.into(),
        label_type,
        elements: vec![artwork.clone()],
        ..Default::default()
    })
    .collect();
    project.upgrade_format_for_features();
    project.validate().unwrap();

    edit_frames(
        &mut project,
        &root,
        TimelineEdit::ConvertToKeyframes {
            range: FrameRange {
                layers: vec![layer_id],
                start: 2,
                end: 4,
            },
            blank: true,
        },
    )
    .unwrap();

    let timeline = &project.symbol(&root).unwrap().timeline;
    let layer = &timeline.layers[0];
    assert_eq!(layer.key_at(2).unwrap().label, "author note");
    assert_eq!(layer.key_at(2).unwrap().label_type, FrameLabelType::Comment);
    assert!(layer.key_at(3).unwrap().label.is_empty());
    assert_eq!(layer.key_at(4).unwrap().label_type, FrameLabelType::Anchor);
    assert_eq!(
        timeline.navigable_labels(),
        [("intro", 0), ("chapter", 4), ("outro", 6)]
    );
    for frame in 2..=4 {
        assert_eq!(layer.key_at(frame).unwrap().frame, frame);
        assert!(sample_layer(layer, frame).is_empty());
    }
    for frame in [1, 5, 6] {
        assert_eq!(
            sample_layer(layer, frame).as_slice(),
            std::slice::from_ref(&artwork)
        );
    }
    project.validate().unwrap();
}

#[test]
fn reverse_frames_inverts_easing_curves_to_mirror_poses_in_time() {
    for ease in [
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Flash { acceleration: 60.0 },
        Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        Easing::Bounce,
        Easing::Elastic,
    ] {
        let mut p = Project::blank();
        p.format_version = 6;
        let root = p.root.clone();
        let lid = p.symbols[0].timeline.layers[0].id.clone();
        let rect = |x: f64| Element {
            id: "r".into(),
            transform: Transform {
                x,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [0, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Default::default()
        };
        p.symbols[0].timeline.frame_count = 11;
        p.symbols[0].timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![rect(0.0)],
                tween: Some(ease),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 10,
                elements: vec![rect(100.0)],
                ..Keyframe::default()
            },
        ];
        p.upgrade_format_for_features();
        p.validate().unwrap();
        let original_xs: Vec<f64> = (0..=10)
            .map(|f| {
                sample_layer(&p.symbols[0].timeline.layers[0], f)[0]
                    .transform
                    .x
            })
            .collect();

        edit_frames(
            &mut p,
            &root,
            TimelineEdit::Reverse {
                range: FrameRange {
                    layers: vec![lid],
                    start: 0,
                    end: 10,
                },
            },
        )
        .unwrap();

        for f in 0..=10 {
            let reversed_x = sample_layer(&p.symbols[0].timeline.layers[0], f)[0]
                .transform
                .x;
            let expected_x = original_xs[(10 - f) as usize];
            assert!(
                (reversed_x - expected_x).abs() < 1e-4,
                "ease {ease:?} frame {f}: reversed_x={reversed_x} != expected_x={expected_x}"
            );
        }
    }
}

#[test]
fn move_keyframe_moves_and_copies_attached_frame_script_without_stealing_from_other_layers() {
    let mut demo = Project::demo();
    let root = demo.root.clone();
    // In Project::demo() ("Ideas in motion"), Scene 1 has layers:
    // [0: Typography, 1: Accents, 2: Orbit instances].
    // Give two layers independent scripts at the same timeline frame. Runtime execution is
    // timeline-wide, while authoring ownership and direct-key drag remain layer-specific.
    demo.format_version = 13;
    let orbit_layer = demo.symbol(&root).unwrap().timeline.layers[2].id.clone();
    let accents_layer = demo.symbol(&root).unwrap().timeline.layers[1].id.clone();
    demo.symbol_mut(&root).unwrap().timeline.scripts[0].layer = Some(orbit_layer.clone());
    demo.symbol_mut(&root)
        .unwrap()
        .timeline
        .scripts
        .push(FrameScript {
            layer: Some(accents_layer.clone()),
            frame: 0,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "trace(\"accent action\");".into(),
            },
        });
    demo.validate().unwrap();

    let timeline = &demo.symbol(&root).unwrap().timeline;
    assert_eq!(timeline.action_layer_for_frame(0), Some(2));
    assert!(timeline.key_carries_frame_script(2, 0));
    assert!(timeline.key_carries_frame_script(1, 0));
    assert!(!timeline.key_carries_frame_script(0, 0));

    // Moving a non-bearing layer's keyframe at frame 0 must not steal either script.
    let mut non_bearing = demo.clone();
    let moved_script = non_bearing
        .move_keyframe(&root, 0, 0, 13, false, true)
        .unwrap();
    assert!(!moved_script);
    assert_eq!(
        non_bearing
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .map(|script| script.frame)
            .collect::<Vec<_>>(),
        vec![0, 0]
    );
    non_bearing.validate().unwrap();

    // Moving the action-bearing keyframe on Orbit instances (layer 2) from frame 0 to 13
    // moves only its attached FrameScript; the Accents script stays at frame 0.
    let mut moved = demo.clone();
    moved.symbol_mut(&root).unwrap().timeline.layers[2].keyframes[0].tween = Some(Easing::Linear);
    moved.symbol_mut(&root).unwrap().timeline.layers[2].keyframes[0].tween_kind = TweenKind::Motion;
    let moved_script = moved.move_keyframe(&root, 2, 0, 13, false, true).unwrap();
    assert!(moved_script);
    let tl = &moved.symbol(&root).unwrap().timeline;
    assert_eq!(
        tl.layers[2]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![13]
    );
    assert_eq!(
        tl.scripts.iter().map(|s| s.frame).collect::<Vec<_>>(),
        vec![0, 13]
    );
    assert!(tl.scripts.iter().any(|script| {
        script.layer.as_ref() == Some(&accents_layer)
            && script.frame == 0
            && script.script.source.contains("accent action")
    }));
    assert!(
        tl.scripts
            .iter()
            .any(|script| { script.layer.as_ref() == Some(&orbit_layer) && script.frame == 13 })
    );
    assert_eq!(tl.action_layer_for_frame(0), Some(1));
    assert_eq!(tl.action_layer_for_frame(13), Some(2));
    moved.validate().unwrap();

    // Option-drag / copy duplicates the Orbit script and leaves the same-frame Accents script.
    let mut copied = demo.clone();
    let copied_script = copied.move_keyframe(&root, 2, 0, 13, true, true).unwrap();
    assert!(copied_script);
    let tl = &copied.symbol(&root).unwrap().timeline;
    assert_eq!(
        tl.layers[2]
            .keyframes
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 13]
    );
    assert_eq!(
        tl.scripts.iter().map(|s| s.frame).collect::<Vec<_>>(),
        vec![0, 0, 13]
    );
    assert_eq!(tl.action_layer_for_frame(0), Some(2));
    assert_eq!(tl.action_layer_for_frame(13), Some(2));
    assert_eq!(
        tl.scripts
            .iter()
            .filter(|script| script.layer.as_ref() == Some(&accents_layer))
            .count(),
        1
    );
    assert_eq!(
        tl.scripts
            .iter()
            .filter(|script| script.layer.as_ref() == Some(&orbit_layer))
            .count(),
        2
    );
    copied.validate().unwrap();

    // Destination key collision or script collision refuses the whole gesture atomically.
    let before_collision = copied.clone();
    assert_eq!(
        copied.move_keyframe(&root, 2, 0, 13, false, true),
        Err(KeyMoveError::DestinationKeyConflict)
    );
    assert_eq!(copied, before_collision);

    // A script on another layer at the destination is not a collision.
    let mut neighbor_destination = demo.clone();
    neighbor_destination
        .symbol_mut(&root)
        .unwrap()
        .timeline
        .scripts
        .push(FrameScript {
            layer: Some(accents_layer.clone()),
            frame: 13,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "this.stop();".into(),
            },
        });
    assert!(
        neighbor_destination
            .move_keyframe(&root, 2, 0, 13, false, true)
            .unwrap()
    );
    let destination_scripts = neighbor_destination
        .symbol(&root)
        .unwrap()
        .timeline
        .scripts
        .iter()
        .filter(|script| script.frame == 13)
        .collect::<Vec<_>>();
    assert_eq!(destination_scripts.len(), 2);
    assert!(
        destination_scripts
            .iter()
            .any(|script| script.layer.as_ref() == Some(&accents_layer))
    );
    assert!(
        destination_scripts
            .iter()
            .any(|script| script.layer.as_ref() == Some(&orbit_layer))
    );
    neighbor_destination.validate().unwrap();

    // A script owned by the moved key's layer at the destination remains a collision.
    let mut script_conflict = demo.clone();
    script_conflict
        .symbol_mut(&root)
        .unwrap()
        .timeline
        .scripts
        .push(FrameScript {
            layer: Some(orbit_layer),
            frame: 13,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "this.stop();".into(),
            },
        });
    let before_script_conflict = script_conflict.clone();
    assert_eq!(
        script_conflict.move_keyframe(&root, 2, 0, 13, false, true),
        Err(KeyMoveError::DestinationScriptConflict)
    );
    assert_eq!(script_conflict, before_script_conflict);
}

#[test]
fn rectangular_frame_edits_follow_only_the_selected_format13_script_owner() {
    let (project, root, first, second) = format13_range_script_project();
    let range = FrameRange {
        layers: vec![first.clone()],
        start: 2,
        end: 2,
    };

    let clip = copy_frames(&project, &root, &range, true).unwrap();
    assert_eq!(clip.scripts.len(), 1);
    assert_eq!(clip.scripts[0].layer.as_ref(), Some(&first));
    assert_eq!(clip.scripts[0].script.source, "first-2");

    let mut pasted = project.clone();
    edit_frames(
        &mut pasted,
        &root,
        TimelineEdit::Paste {
            clip: clip.clone(),
            layers: vec![second.clone()],
            at: 4,
            include_scripts: true,
        },
    )
    .unwrap();
    assert!(
        pasted
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .any(|script| {
                script.script.source == "first-2"
                    && script.layer.as_ref() == Some(&second)
                    && script.frame == 4
            })
    );
    assert_eq!(
        script_position(&pasted, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    pasted.validate().unwrap();

    let mut moved = project.clone();
    edit_frames(
        &mut moved,
        &root,
        TimelineEdit::Move {
            range: range.clone(),
            layers: vec![first.clone()],
            at: 4,
            copy: false,
            include_scripts: true,
        },
    )
    .unwrap();
    assert_eq!(
        script_position(&moved, &root, "first-2"),
        (Some(first.clone()), 4)
    );
    assert_eq!(
        script_position(&moved, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    moved.validate().unwrap();

    let mut cleared = project.clone();
    edit_frames(
        &mut cleared,
        &root,
        TimelineEdit::Clear {
            range: range.clone(),
            include_scripts: true,
        },
    )
    .unwrap();
    assert!(
        cleared
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .all(|script| script.script.source != "first-2")
    );
    assert_eq!(
        script_position(&cleared, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    cleared.validate().unwrap();

    let mut deleted = project.clone();
    edit_frames(
        &mut deleted,
        &root,
        TimelineEdit::Delete {
            range: FrameRange {
                layers: vec![first.clone()],
                start: 2,
                end: 3,
            },
            include_scripts: true,
        },
    )
    .unwrap();
    assert!(
        deleted
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .all(|script| script.script.source != "first-2")
    );
    assert_eq!(
        script_position(&deleted, &root, "first-6"),
        (Some(first.clone()), 4)
    );
    assert_eq!(
        script_position(&deleted, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    assert_eq!(
        script_position(&deleted, &root, "second-6"),
        (Some(second.clone()), 6)
    );
    deleted.validate().unwrap();

    let mut inserted = project.clone();
    edit_frames(
        &mut inserted,
        &root,
        TimelineEdit::Insert {
            layers: vec![first.clone()],
            at: 1,
            count: 2,
            include_scripts: true,
        },
    )
    .unwrap();
    assert_eq!(
        script_position(&inserted, &root, "first-2"),
        (Some(first.clone()), 4)
    );
    assert_eq!(
        script_position(&inserted, &root, "first-6"),
        (Some(first.clone()), 8)
    );
    assert_eq!(
        script_position(&inserted, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    assert_eq!(
        script_position(&inserted, &root, "second-6"),
        (Some(second), 6)
    );
    inserted.validate().unwrap();

    let mut invalid_clip = clip;
    invalid_clip.scripts[0].layer = Some("not-a-copied-layer".into());
    let mut atomic = project.clone();
    let before = atomic.clone();
    let error = edit_frames(
        &mut atomic,
        &root,
        TimelineEdit::Paste {
            clip: invalid_clip,
            layers: vec![first],
            at: 4,
            include_scripts: true,
        },
    )
    .unwrap_err();
    assert!(error.contains("owning layer was not copied"));
    assert_eq!(atomic, before);
}

#[test]
fn legacy_layerless_scripts_keep_global_rectangular_edit_semantics() {
    let (mut project, root, first, second) = format13_range_script_project();
    project.symbol_mut(&root).unwrap().timeline.scripts.extend([
        FrameScript {
            layer: None,
            frame: 2,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "legacy-2".into(),
            },
        },
        FrameScript {
            layer: None,
            frame: 6,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: "legacy-6".into(),
            },
        },
    ]);
    project.validate().unwrap();

    // Layerless records intentionally retain the old global Include frame scripts contract:
    // selecting any row copies/removes them and ripples their time.
    let clip = copy_frames(
        &project,
        &root,
        &FrameRange {
            layers: vec![first.clone()],
            start: 2,
            end: 2,
        },
        true,
    )
    .unwrap();
    assert!(
        clip.scripts
            .iter()
            .any(|script| script.layer.is_none() && script.script.source == "legacy-2")
    );
    assert!(
        clip.scripts
            .iter()
            .all(|script| script.script.source != "second-2")
    );

    let mut cleared = project.clone();
    edit_frames(
        &mut cleared,
        &root,
        TimelineEdit::Clear {
            range: FrameRange {
                layers: vec![first.clone()],
                start: 2,
                end: 2,
            },
            include_scripts: true,
        },
    )
    .unwrap();
    assert!(
        cleared
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .all(|script| script.script.source != "legacy-2")
    );
    assert_eq!(
        script_position(&cleared, &root, "second-2"),
        (Some(second.clone()), 2)
    );

    let mut deleted = project.clone();
    edit_frames(
        &mut deleted,
        &root,
        TimelineEdit::Delete {
            range: FrameRange {
                layers: vec![first.clone()],
                start: 2,
                end: 3,
            },
            include_scripts: true,
        },
    )
    .unwrap();
    assert_eq!(script_position(&deleted, &root, "legacy-6"), (None, 4));
    assert_eq!(
        script_position(&deleted, &root, "second-6"),
        (Some(second.clone()), 6)
    );

    let mut inserted = project;
    edit_frames(
        &mut inserted,
        &root,
        TimelineEdit::Insert {
            layers: vec![first],
            at: 1,
            count: 2,
            include_scripts: true,
        },
    )
    .unwrap();
    assert_eq!(script_position(&inserted, &root, "legacy-2"), (None, 4));
    assert_eq!(script_position(&inserted, &root, "legacy-6"), (None, 8));
    assert_eq!(
        script_position(&inserted, &root, "second-2"),
        (Some(second.clone()), 2)
    );
    assert_eq!(
        script_position(&inserted, &root, "second-6"),
        (Some(second), 6)
    );
    inserted.validate().unwrap();
}

#[test]
fn convert_layer_to_keyframes_stepped_samples_on_twos_and_threes_with_or_without_stripping_tween() {
    let mut p = Project::blank();
    p.format_version = 6;
    p.symbols[0].timeline.frame_count = 13;
    let rect = |x: f64| Element {
        id: "r".into(),
        transform: Transform {
            x,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 10.0,
            height: 10.0,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..Default::default()
    };
    p.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![rect(0.0)],
            tween: Some(Easing::EaseInOut),
            ..Keyframe::default()
        },
        Keyframe {
            frame: 12,
            elements: vec![rect(120.0)],
            ..Keyframe::default()
        },
    ];

    // Tween2Keys (on 2s, strip_tween = true)
    let mut p_twos = p.clone();
    let tl_snapshot = p_twos.symbols[0].timeline.clone();
    let count = convert_layer_to_keyframes_stepped(
        &tl_snapshot,
        &mut p_twos.symbols[0].timeline.layers[0],
        0,
        12,
        2,
        true,
        24.0,
    );
    assert_eq!(count, 7);
    let kfs = &p_twos.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(
        kfs.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 2, 4, 6, 8, 10, 12]
    );
    assert!(
        kfs.iter()
            .all(|k| k.tween.is_none() && k.keyed_properties.is_none())
    );
    p_twos.validate().unwrap();

    // Convert to Keyframes Advanced (on 3s, strip_tween = false, preserves easing)
    let mut p_threes = p;
    let tl_snapshot = p_threes.symbols[0].timeline.clone();
    let count = convert_layer_to_keyframes_stepped(
        &tl_snapshot,
        &mut p_threes.symbols[0].timeline.layers[0],
        0,
        12,
        3,
        false,
        24.0,
    );
    assert_eq!(count, 5);
    let kfs = &p_threes.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(
        kfs.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 3, 6, 9, 12]
    );
    assert_eq!(kfs[0].tween, Some(Easing::EaseInOut));
    assert_eq!(kfs[1].tween, Some(Easing::EaseInOut));
    assert_eq!(kfs[4].tween, None);
    p_threes.validate().unwrap();
}

#[test]
fn video_and_sound_montage_splits_advance_in_points_preserve_metadata_and_reject_event_splits() {
    let mut p = Project::blank();
    p.fps = 20; // 50 ms per frame for exact integer verification
    p.format_version = 12;
    let root_id = p.root.clone();
    p.resources.push(
        ResourceUnit::from_bytes(
            "vid_1",
            "video/clip.mp4",
            "video/mp4",
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41",
        )
        .unwrap(),
    );
    p.resources.push(
        ResourceUnit::from_bytes(
            "aud_1",
            "audio/bed.wav",
            "audio/wav",
            b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00",
        )
        .unwrap(),
    );

    let video_layer_id = p.symbols[0].timeline.layers[0].id.clone();
    p.symbols[0].timeline.frame_count = 40;
    p.symbols[0].timeline.layers[0].properties.end_frame = Some(40);
    p.symbols[0].timeline.layers[0].keyframes[0] = Keyframe {
        frame: 0,
        elements: vec![Element {
            id: "vid_el".into(),
            name: "clip.mp4".into(),
            kind: ElementKind::Video {
                width: 320.0,
                height: 180.0,
            },
            ..Element::default()
        }],
        video: Some(TimelineVideoSpan {
            resource: "vid_1".into(),
            sync: VideoSyncMode::Stream,
            start_offset_ms: 200,
            end_offset_ms: Some(3000),
            loop_video: false,
            muted: true,
            cue_points: vec![
                VideoCuePoint {
                    name: "intro".into(),
                    time_ms: 350,
                    kind: VideoCuePointKind::Navigation,
                    parameters: vec![],
                },
                VideoCuePoint {
                    name: "beat".into(),
                    time_ms: 900,
                    kind: VideoCuePointKind::Event,
                    parameters: vec![("tag".into(), "drop".into())],
                },
            ],
            deployment: VideoDeploymentMode::Embedded,
            external_path: None,
            stream_url: None,
        }),
        ..Keyframe::default()
    };

    let mut audio_sound = FrameSound::new("aud_1");
    audio_sound.sync = SoundSync::Stream;
    audio_sound.in_point = Some(0.2);
    audio_sound.out_point = Some(3.0);
    let audio_layer_id: Id = "aud_layer".into();
    p.symbols[0].timeline.layers.push(Layer {
        id: audio_layer_id.clone(),
        name: "Audio".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe {
            frame: 0,
            sound: Some(audio_sound),
            ..Keyframe::default()
        }],
        properties: LayerProperties {
            end_frame: Some(40),
            linked_layer: Some(video_layer_id.clone()),
            ..LayerProperties::default()
        },
    });
    p.symbols[0].timeline.layers[0].properties.linked_layer = Some(audio_layer_id.clone());
    p.validate().unwrap();

    // 1. Atomic SplitClip at frame 10 (500 ms after start -> 200 + 500 = 700 ms).
    edit_frames(
        &mut p,
        &root_id,
        TimelineEdit::SplitClip {
            layers: vec![video_layer_id.clone(), audio_layer_id.clone()],
            at: 10,
        },
    )
    .unwrap();
    let vkfs = &p.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(vkfs.len(), 2);
    let left_v = vkfs[0].video.as_ref().unwrap();
    let right_v = vkfs[1].video.as_ref().unwrap();
    assert_eq!(left_v.start_offset_ms, 200);
    assert_eq!(left_v.end_offset_ms, Some(700));
    assert!(left_v.muted);
    assert_eq!(left_v.cue_points.len(), 1);
    assert_eq!(left_v.cue_points[0].name, "intro");
    assert_eq!(right_v.start_offset_ms, 700);
    assert_eq!(right_v.end_offset_ms, Some(3000));
    assert!(right_v.muted);
    assert_eq!(right_v.cue_points.len(), 1);
    assert_eq!(right_v.cue_points[0].name, "beat");

    let akfs = &p.symbols[0].timeline.layers[1].keyframes;
    assert_eq!(akfs.len(), 2);
    let left_s = akfs[0].sound.as_ref().unwrap();
    let right_s = akfs[1].sound.as_ref().unwrap();
    assert!((left_s.out_point.unwrap() - 0.7).abs() < 1e-6);
    assert!((right_s.in_point.unwrap() - 0.7).abs() < 1e-6);

    // 2. Ripple Delete of frames 10..15 advances the second segment's in-point from 700 -> 950 ms.
    edit_frames(
        &mut p,
        &root_id,
        TimelineEdit::Delete {
            range: FrameRange {
                layers: vec![video_layer_id.clone()],
                start: 10,
                end: 14,
            },
            include_scripts: false,
        },
    )
    .unwrap();
    let vkfs = &p.symbols[0].timeline.layers[0].keyframes;
    assert_eq!(vkfs[1].frame, 10);
    assert_eq!(vkfs[1].video.as_ref().unwrap().start_offset_ms, 950);

    // 3. Copy & Move preserve Keyframe.video metadata.
    let copied = copy_frames(
        &p,
        &root_id,
        &FrameRange {
            layers: vec![video_layer_id.clone()],
            start: 10,
            end: 19,
        },
        false,
    )
    .unwrap();
    assert_eq!(
        copied.layers[0].keyframes[0]
            .video
            .as_ref()
            .unwrap()
            .start_offset_ms,
        950
    );

    // 4. Event / Stop video mid-span split or partial delete rejects atomically without mutating p.
    p.symbols[0].timeline.layers[0].keyframes[1]
        .video
        .as_mut()
        .unwrap()
        .sync = VideoSyncMode::Event;
    let snapshot = p.clone();
    let err = edit_frames(
        &mut p,
        &root_id,
        TimelineEdit::SplitClip {
            layers: vec![video_layer_id.clone(), audio_layer_id.clone()],
            at: 18,
        },
    )
    .unwrap_err();
    assert!(err.contains("Event or Stop"));
    assert_eq!(
        p, snapshot,
        "failed Event split must leave both linked layers untouched"
    );
}

#[test]
fn sync_symbols_and_split_clip_respect_per_instance_behavior_overrides() {
    let mut p = Project::blank();
    let root_id = p.root.clone();
    p.symbols[0].timeline.frame_count = 10;
    let layer_id = p.symbols[0].timeline.layers[0].id.clone();

    let mut mc_sym = p.symbols[0].clone();
    mc_sym.id = "mc_lib".into();
    mc_sym.name = "HeroMC".into();
    mc_sym.kind = SymbolKind::MovieClip;
    mc_sym.timeline.layers[0].id = "mc-layer".into();
    mc_sym.timeline.frame_count = 8;

    let mut gfx_sym = p.symbols[0].clone();
    gfx_sym.id = "gfx_lib".into();
    gfx_sym.name = "BadgeGfx".into();
    gfx_sym.kind = SymbolKind::Graphic;
    gfx_sym.timeline.layers[0].id = "gfx-layer".into();
    gfx_sym.timeline.frame_count = 8;
    p.symbols.push(mc_sym);
    p.symbols.push(gfx_sym);

    let inst_with_behavior =
        |id: &str, sym: &str, behavior: SymbolKind, playback: InstancePlayback| Element {
            id: id.into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: sym.into(),
                playback,
            },
            instance_behavior: Some(behavior),
            ..Element::default()
        };

    p.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![
                inst_with_behavior(
                    "mc_as_gfx_0",
                    "mc_lib",
                    SymbolKind::Graphic,
                    InstancePlayback::LoopFrom(2),
                ),
                inst_with_behavior(
                    "gfx_as_mc_0",
                    "gfx_lib",
                    SymbolKind::MovieClip,
                    InstancePlayback::LoopFrom(2),
                ),
            ],
            ..Default::default()
        },
        Keyframe {
            frame: 5,
            elements: vec![
                inst_with_behavior(
                    "mc_as_gfx_5",
                    "mc_lib",
                    SymbolKind::Graphic,
                    InstancePlayback::Loop,
                ),
                inst_with_behavior(
                    "gfx_as_mc_5",
                    "gfx_lib",
                    SymbolKind::MovieClip,
                    InstancePlayback::Loop,
                ),
            ],
            ..Default::default()
        },
    ];

    // 1. sync_layer_symbols syncs the Graphic-overridden MovieClip ((2 + 5) % 8 = 7)
    // while leaving the MovieClip-overridden Graphic untouched (even though its playback field is Loop).
    let mut layer_copy = p.symbols[0].timeline.layers[0].clone();
    let changed = sync_layer_symbols(&p, &mut layer_copy, 0, 9);
    assert_eq!(changed, 1);
    assert_eq!(
        layer_copy.keyframes[1].elements[0].kind,
        ElementKind::Instance {
            symbol_id: "mc_lib".into(),
            playback: InstancePlayback::LoopFrom(7),
        }
    );
    assert_eq!(
        layer_copy.keyframes[1].elements[1].kind,
        ElementKind::Instance {
            symbol_id: "gfx_lib".into(),
            playback: InstancePlayback::Loop,
        }
    );

    // 2. TimelineEdit::SplitClip and TimelineEdit::SyncSymbols also respect instance_behavior.
    edit_frames(
        &mut p,
        &root_id,
        TimelineEdit::SplitClip {
            layers: vec![layer_id.clone()],
            at: 3,
        },
    )
    .unwrap();
    let split_key = p.symbols[0].timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 3)
        .unwrap();
    // Same element ID across split: placement_start == 0, so LoopFrom(2) stays LoopFrom(2).
    assert_eq!(
        split_key.elements[0].kind,
        ElementKind::Instance {
            symbol_id: "mc_lib".into(),
            playback: InstancePlayback::LoopFrom(2),
        }
    );
    assert_eq!(
        split_key.elements[1].kind,
        ElementKind::Instance {
            symbol_id: "gfx_lib".into(),
            playback: InstancePlayback::LoopFrom(2),
        }
    );

    edit_frames(
        &mut p,
        &root_id,
        TimelineEdit::SyncSymbols {
            range: FrameRange {
                layers: vec![layer_id],
                start: 5,
                end: 5,
            },
        },
    )
    .unwrap();
    let k5 = p.symbols[0].timeline.layers[0]
        .keyframes
        .iter()
        .find(|k| k.frame == 5)
        .unwrap();
    assert_eq!(
        k5.elements[0].kind,
        ElementKind::Instance {
            symbol_id: "mc_lib".into(),
            playback: InstancePlayback::LoopFrom(7),
        }
    );
    assert_eq!(
        k5.elements[1].kind,
        ElementKind::Instance {
            symbol_id: "gfx_lib".into(),
            playback: InstancePlayback::Loop,
        }
    );
}
