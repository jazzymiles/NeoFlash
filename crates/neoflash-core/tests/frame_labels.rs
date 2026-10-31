use neoflash_core::{
    Element, ElementKind, InstancePlayback, Keyframe, Layer, PlaybackCommand, Player, Project,
    Symbol, SymbolKind, Timeline, Transform,
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
