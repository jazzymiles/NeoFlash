use neoflash_core::*;

fn shape(id: &str) -> Element {
    Element {
        accessibility: None,
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        ..Element::default()
    }
}

fn instance(id: &str, symbol_id: &str) -> Element {
    Element {
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback: InstancePlayback::Independent,
        },
        ..shape(id)
    }
}

fn as3(source: &str) -> Script {
    Script {
        language: ScriptLanguage::ActionScript3,
        source: source.into(),
    }
}

fn symbol(id: &str, frames: u32, keyframes: Vec<Keyframe>, scripts: Vec<FrameScript>) -> Symbol {
    Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: id.into(),
        name: id.into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            guides: vec![],
            frame_count: frames,
            layers: vec![Layer {
                properties: Default::default(),
                id: format!("{id}-layer"),
                name: "Layer".into(),
                visible: true,
                locked: false,
                keyframes,
            }],
            scripts,
        },
    }
}

fn make_project(symbols: Vec<Symbol>) -> Project {
    Project {
        library_folders: vec![],
        sources: vec![],
        resources: vec![],
        document_class: None,
        scenes: vec![],
        script_language: None,
        also_compile_compatible_scripts: true,
        format_version: 6,
        name: "LifecycleTest".into(),
        width: 400,
        height: 300,
        fps: 24,
        background: [255, 255, 255, 255],
        root: symbols[0].id.clone(),
        symbols,
    }
}

fn drain_phases(player: &mut Player) -> Vec<LifecycleStep> {
    let mut steps = Vec::new();
    while let Some(step) = player.lifecycle_next_step().expect("lifecycle step") {
        steps.push(step);
    }
    steps
}

#[test]
fn startup_and_tick_emit_ordered_lifecycle_phases_and_display_events() {
    let mut child_inst = instance("child", "child-sym");
    child_inst.scripts = vec![ObjectScript {
        event: ObjectEvent::Load,
        script: as3("child-load"),
    }];
    let leaf = shape("leaf");
    let root = symbol(
        "root",
        2,
        vec![
            Keyframe {
                frame: 0,
                elements: vec![child_inst],
                tween: None,
                label: String::new(),
                ..Default::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![],
                tween: None,
                label: String::new(),
                ..Default::default()
            },
        ],
        vec![
            FrameScript {
                frame: 0,
                script: as3("root-f0"),
            },
            FrameScript {
                frame: 1,
                script: as3("root-f1"),
            },
        ],
    );
    let child_sym = symbol(
        "child-sym",
        1,
        vec![Keyframe {
            frame: 0,
            elements: vec![leaf],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![FrameScript {
            frame: 0,
            script: as3("child-f0"),
        }],
    );
    let proj = make_project(vec![root, child_sym]);
    let mut player = Player::new(&proj);
    player.enable_lifecycle();

    let startup = drain_phases(&mut player);
    let phases: Vec<_> = startup.iter().map(|s| s.phase).collect();
    assert_eq!(
        phases,
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
            LifecyclePhase::Render,
        ]
    );
    let construct_events: Vec<_> = startup[0]
        .display_events
        .iter()
        .map(|e| (e.kind.as_str(), e.route.len()))
        .collect();
    assert!(construct_events.iter().any(|(kind, _)| *kind == "added"));
    assert!(
        construct_events
            .iter()
            .any(|(kind, _)| *kind == "added_to_stage")
    );
    let startup_scripts: Vec<_> = startup[2]
        .scripts
        .iter()
        .map(|s| s.dispatch.script.source.as_str())
        .collect();
    assert_eq!(startup_scripts, vec!["child-load", "root-f0", "child-f0"]);

    // Advance to frame 1: child subtree retires (deepest first: leaf then child), frame 1 script runs.
    player.tick(&proj);
    let tick1 = drain_phases(&mut player);
    assert_eq!(
        tick1.iter().map(|s| s.phase).collect::<Vec<_>>(),
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
            LifecyclePhase::Render,
        ]
    );
    let retired: Vec<_> = tick1[0]
        .display_events
        .iter()
        .filter(|e| e.kind == "removed")
        .collect();
    assert_eq!(
        retired.len(),
        2,
        "leaf and child retire when leaving frame 0"
    );
    assert_eq!(
        retired[0].route.len(),
        3,
        "deepest child (leaf -> child -> root) retires first"
    );
    assert_eq!(
        tick1[2]
            .scripts
            .iter()
            .map(|s| s.dispatch.script.source.as_str())
            .collect::<Vec<_>>(),
        vec!["root-f1"]
    );
}

#[test]
fn reentrant_inner_goto_pushes_bounded_continuation_and_supersedes_old_visit() {
    let child_sym = symbol(
        "child-sym",
        2,
        vec![Keyframe {
            frame: 0,
            elements: vec![shape("mark")],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![
            FrameScript {
                frame: 0,
                script: as3("child-f0"),
            },
            FrameScript {
                frame: 1,
                script: as3("child-f1"),
            },
        ],
    );
    let root = symbol(
        "root",
        3,
        vec![Keyframe {
            frame: 0,
            elements: vec![instance("child", "child-sym")],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![
            FrameScript {
                frame: 0,
                script: as3("root-f0"),
            },
            FrameScript {
                frame: 1,
                script: as3("root-f1"),
            },
            FrameScript {
                frame: 2,
                script: as3("root-f2"),
            },
        ],
    );
    let proj = make_project(vec![root, child_sym]);
    let mut player = Player::new(&proj);
    player.enable_lifecycle();

    // Outer FrameScripts returns [root-f0, child-f0]. While processing root-f0, navigate child to frame 1!
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::Construct
    );
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::FrameConstructed
    );
    let outer_scripts = player.lifecycle_next_step().unwrap().unwrap();
    assert_eq!(outer_scripts.phase, LifecyclePhase::FrameScripts);
    assert_eq!(outer_scripts.scripts.len(), 2);
    let child_old_dispatch = outer_scripts.scripts[1].clone();

    let child_path = vec!["child".to_string()];
    let inner_depth = player
        .begin_inner_goto(&proj, &child_path, PlaybackCommand::GotoAndStop(1))
        .expect("begin_inner_goto");
    assert_eq!(inner_depth, 2);

    // Inner scope runs Construct -> FrameConstructed -> FrameScripts (child-f1) -> ExitFrame (no Render!).
    let mut inner_phases = Vec::new();
    let mut inner_sources = Vec::new();
    while player.lifecycle_depth() >= inner_depth {
        let step = player.lifecycle_next_step().unwrap().expect("inner step");
        inner_phases.push(step.phase);
        assert!(step.inner_goto);
        for s in step.scripts {
            inner_sources.push(s.dispatch.script.source);
        }
    }
    assert_eq!(
        inner_phases,
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
        ]
    );
    assert_eq!(inner_sources, vec!["child-f1"]);
    assert!(
        !player.is_runtime_dispatch_current(&proj, &child_old_dispatch),
        "old child-f0 dispatch must be invalidated by visit_id after goto"
    );

    // Outer scope resumes cleanly at ExitFrame -> Render.
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::ExitFrame
    );
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::Render
    );
    assert!(player.lifecycle_next_step().unwrap().is_none());

    // Bounded nesting depth refuses infinite recursive goto chains.
    for _ in 0..MAX_LIFECYCLE_DEPTH {
        let _ = player.begin_inner_goto(&proj, &[], PlaybackCommand::GotoAndStop(2));
    }
    let overflow = player.begin_inner_goto(&proj, &[], PlaybackCommand::GotoAndStop(2));
    assert!(
        overflow.is_err(),
        "exceeding MAX_LIFECYCLE_DEPTH must error"
    );
}
