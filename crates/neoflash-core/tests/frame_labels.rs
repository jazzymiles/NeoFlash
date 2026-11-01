use neoflash_core::{
    Element, ElementKind, FrameLabel, FrameScript, GotoFrame, GotoRequest, InstancePlayback,
    Keyframe, Layer, PlaybackCommand, Player, Project, SceneRange, Script, ScriptLanguage, Symbol,
    SymbolKind, Timeline, Transform, flatten_scenes_with_ranges,
};

fn layer(id: &str, labels: &[(u32, &str)]) -> Layer {
    Layer {
        properties: Default::default(),
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        keyframes: labels
            .iter()
            .map(|&(frame, label)| Keyframe {
                frame,
                label: label.into(),
                elements: vec![],
                tween: None,
                ..Default::default()
            })
            .collect(),
    }
}

fn fixture() -> Project {
    let mut project = Project::blank();
    project.symbols[0].timeline = Timeline {
        guides: vec![],
        frame_count: 10,
        layers: vec![
            layer("first-layer", &[(0, ""), (4, "end"), (7, "SomeFrame")]),
            layer("second-layer", &[(0, ""), (2, "SomeFrame"), (6, "BADà")]),
        ],
        scripts: vec![],
    };
    project.validate().unwrap();
    project
}

#[test]
fn as2_folds_ascii_case_but_preserves_non_ascii_case() {
    let project = fixture();
    let player = Player::new(&project);
    assert_eq!(player.resolve_frame_as2(&project, &[], "END"), Some(4));
    assert_eq!(player.resolve_frame_as2(&project, &[], "eNd"), Some(4));
    assert_eq!(player.resolve_frame_as2(&project, &[], "badà"), Some(6));
    assert_eq!(player.resolve_frame_as2(&project, &[], "BADÀ"), None);

    // Existing strict host semantics do not gain AS2's case folding.
    assert_eq!(player.resolve_frame(&project, &[], "END"), None);
    assert_eq!(player.resolve_frame(&project, &[], "end"), Some(4));
    assert_eq!(player.resolve_frame(&project, &[], "badà"), None);
    assert_eq!(player.resolve_frame(&project, &[], "BADà"), Some(6));
}

#[test]
fn as2_duplicates_choose_earliest_frame_across_layers() {
    let project = fixture();
    let player = Player::new(&project);
    assert_eq!(
        player.resolve_frame_as2(&project, &[], "SOMEFRAME"),
        Some(2)
    );
    assert_eq!(
        player.resolve_frame_as2(&project, &[], "SomeFrame"),
        Some(2)
    );
    // Preserve the existing strict resolver's authored layer traversal.
    assert_eq!(player.resolve_frame(&project, &[], "SomeFrame"), Some(7));
}

#[test]
fn absent_as2_labels_do_not_change_frame_or_playback() {
    let project = fixture();
    let mut player = Player::new(&project);
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(3)));
    for label in ["", "missing", "BADÀ"] {
        assert_eq!(player.resolve_frame_as2(&project, &[], label), None);
    }
    assert_eq!(
        player.resolve_frame_as2(&project, &["missing-instance".into()], "end"),
        None
    );
    player.tick(&project);
    assert_eq!(
        player.frame(),
        3,
        "resolution must preserve a stopped clock"
    );
    assert!(player.control(&project, &[], PlaybackCommand::Play));
    assert_eq!(player.resolve_frame_as2(&project, &[], "missing"), None);
    player.tick(&project);
    assert_eq!(
        player.frame(),
        4,
        "resolution must preserve a playing clock"
    );
}

#[test]
fn as2_resolves_only_the_target_timeline() {
    let mut project = fixture();
    project.symbols.push(Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: "child-symbol".into(),
        name: "Child".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            guides: vec![],
            frame_count: 10,
            layers: vec![layer("child-layer", &[(0, ""), (1, "END")])],
            scripts: vec![],
        },
    });
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: "child-instance".into(),
            name: "child".into(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: "child-symbol".into(),
                playback: InstancePlayback::Independent,
            },
            scripts: vec![],
            accessibility: None,
        });
    project.validate().unwrap();
    let player = Player::new(&project);
    assert_eq!(player.resolve_frame_as2(&project, &[], "end"), Some(4));
    assert_eq!(
        player.resolve_frame_as2(&project, &["child-instance".into()], "end"),
        Some(1)
    );
}

fn as3_script(frame: u32, source: &str) -> FrameScript {
    FrameScript {
        frame,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: source.into(),
        },
    }
}

fn scene_fixture() -> Project {
    let mut project = fixture();
    project.format_version = 6;
    let root = project.root.clone();
    project.symbols[0].timeline = Timeline {
        frame_count: 3,
        layers: vec![layer("intro-layer", &[(0, "begin")])],
        scripts: vec![as3_script(0, "trace('intro')")],
        guides: vec![],
    };
    let mut second = layer("main-layer", &[(1, "end")]);
    second.keyframes[0].elements.push(Element {
        id: "main-box".into(),
        name: "box".into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        accessibility: None,
    });
    project.symbols.push(Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: "scene-2".into(),
        name: "Main".into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![second],
            scripts: vec![as3_script(0, "trace('main')")],
            guides: vec![],
        },
    });
    project.scenes = vec![root, "scene-2".into()];
    project.validate().unwrap();
    project
}

#[test]
fn two_scenes_play_in_order_and_goto_resolves_scene_names() {
    let (project, ranges) = flatten_scenes_with_ranges(&scene_fixture());
    let mut player = Player::new(&project);
    player.set_scenes(&project, ranges).unwrap();
    let mut frames = vec![];
    let mut scripts = vec![];
    for _ in 0..6 {
        frames.push(player.frame());
        scripts.extend(player.drain_scripts().into_iter().map(|d| d.script.source));
        player.tick(&project);
    }
    assert_eq!(
        frames,
        [0, 1, 2, 3, 4, 0],
        "scene 2 follows scene 1, then the movie loops"
    );
    assert_eq!(
        scripts,
        ["trace('intro')", "trace('main')", "trace('intro')"]
    );
    assert!(
        player.evaluate(&project).is_empty(),
        "scene 2 art is absent in scene 1"
    );
    let metadata = player.clip_metadata(&project, &[]).unwrap();
    assert_eq!(
        (metadata.scene_name.as_str(), metadata.total_frames),
        ("Scene 1", 3)
    );
    assert_eq!(metadata.labels, vec![FrameLabel::new("begin".into(), 1)]);
    assert_eq!(metadata.scenes.len(), 2);
    assert_eq!(
        metadata.scenes[1].labels,
        vec![FrameLabel::new("end".into(), 2)]
    );

    let request = |frame, scene: &str| GotoRequest {
        frame,
        scene: Some(scene.into()),
        as2: false,
    };
    let target = player
        .resolve_goto(
            &project,
            &[],
            &request(GotoFrame::Label("end".into()), "Main"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(target, 4);
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(target)));
    assert_eq!(player.evaluate(&project).len(), 1, "scene 2 art is shown");
    let metadata = player.clip_metadata(&project, &[]).unwrap();
    assert_eq!(
        (
            metadata.scene_name.as_str(),
            metadata.scene_frame,
            metadata.total_frames
        ),
        ("Main", 2, 2)
    );
    assert_eq!(metadata.current_label.as_deref(), Some("end"));
    let error = player
        .resolve_goto(&project, &[], &request(GotoFrame::Number(1.), "Credits"))
        .unwrap_err();
    assert!(error.contains("Scene 'Credits' was not found"), "{error}");
    assert_eq!(player.frame(), 4, "an unknown scene never jumps");
    assert_eq!(player.resolve_scene_step(&project, &[], -1), Some(0));
    assert_eq!(player.resolve_scene_step(&project, &[], 1), None);
    let partial = vec![SceneRange {
        name: "x".into(),
        start: 0,
        frames: 2,
    }];
    assert!(
        player.set_scenes(&project, partial).is_err(),
        "ranges must cover the main timeline"
    );
}
