use neoflash_core::*;

fn script(source: &str) -> Script {
    Script {
        language: ScriptLanguage::JavaScript,
        source: source.into(),
    }
}
fn object(id: &str, kind: ElementKind) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind,
        accessibility: Some(Accessibility {
            name: id.into(),
            description: String::new(),
            role: AccessibleRole::Button,
            tab_order: 0,
            disabled: false,
        }),
        scripts: [
            ObjectEvent::Load,
            ObjectEvent::EnterFrame,
            ObjectEvent::Click,
        ]
        .into_iter()
        .map(|event| ObjectScript {
            event,
            script: script(&format!("trace('{id}');")),
        })
        .collect(),
    }
}
fn leaf(id: &str) -> Element {
    object(
        id,
        ElementKind::Rectangle {
            width: 40.,
            height: 30.,
            fill: [10, 20, 80, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    )
}
fn key(frame: u32, elements: Vec<Element>) -> Keyframe {
    Keyframe {
        frame,
        elements,
        tween: None,
        label: String::new(),
        ..Default::default()
    }
}
fn fixture() -> Project {
    let mut p = Project::blank();
    p.symbols[0].timeline.frame_count = 12;
    p.symbols[0].timeline.layers[0].keyframes[0].elements = ["one", "two"]
        .map(|name| {
            object(
                name,
                ElementKind::Instance {
                    symbol_id: "shared".into(),
                    playback: InstancePlayback::Independent,
                },
            )
        })
        .into();
    let mut child = Project::blank().symbols.remove(0);
    child.id = "shared".into();
    child.timeline.frame_count = 12;
    child.timeline.layers[0].id = "shared-layer".into();
    child.timeline.layers[0].keyframes[0].elements = vec![leaf("leaf")];
    child.timeline.scripts = vec![FrameScript {
        frame: 1,
        script: script("trace('frame one');"),
    }];
    p.symbols.push(child);
    p.validate().unwrap();
    p
}
fn path(ids: &[&str]) -> Vec<Id> {
    ids.iter().map(|id| (*id).into()).collect()
}

#[test]
fn local_readback_and_inherited_visibility_do_not_remove_named_children() {
    let p = fixture();
    let mut player = Player::new(&p);
    let one = path(&["one"]);
    let child = path(&["one", "leaf"]);
    assert_eq!(player.visible(&p, &one), Some(true));
    assert!(player.set_visible(&one, false));
    assert_eq!(player.visible(&p, &one), Some(false));
    assert_eq!(player.visible(&p, &child), Some(true));
    assert_eq!(player.child_path(&p, &one, "leaf"), Some(child.clone()));
    assert_eq!(player.evaluate(&p).len(), 1);
    assert_eq!(player.accessibility_nodes(&p).len(), 2);
    assert!(player.set_visible(&child, false));
    assert!(player.set_visible(&one, true));
    assert_eq!(
        player.evaluate(&p).len(),
        1,
        "parent show must not override child's own flag"
    );
    assert!(player.set_visible(&child, true));
    assert_eq!(player.evaluate(&p).len(), 2);
    assert!(player.set_visible(&[], false));
    assert_eq!(player.visible(&p, &[]), Some(false));
    assert_eq!(player.visible(&p, &one), Some(true));
    assert!(player.evaluate(&p).is_empty());
    assert!(player.accessibility_nodes(&p).is_empty());
    assert!(player.set_visible(&[], true));
    assert_eq!(player.evaluate(&p).len(), 2);
}

#[test]
fn hidden_independent_clips_keep_clocks_scripts_lifetimes_and_transform_overrides() {
    let p = fixture();
    let mut player = Player::new(&p);
    player.drain_scripts();
    let one = path(&["one"]);
    let child = path(&["one", "leaf"]);
    let lifetime = player.lifetime_id(&child);
    assert!(player.set_visible(&one, false));
    assert!(player.set_property(&child, "x", 44.));
    assert!(player.control(&p, &[], PlaybackCommand::Stop));
    let mut events = Vec::new();
    for _ in 0..4 {
        player.tick(&p);
        let tick = player.drain_scripts();
        assert!(
            tick.iter()
                .all(|event| player.is_dispatch_current(&p, event))
        );
        events.extend(tick);
    }
    assert_eq!(player.frame(), 0);
    assert_eq!(player.clip_frame(&one), Some(4));
    assert_eq!(player.lifetime_id(&child), lifetime);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.target_path == one && e.event == ScriptEvent::EnterFrame)
            .count(),
        4
    );
    assert!(
        events
            .iter()
            .any(|e| e.target_path == one && e.event == ScriptEvent::Frame(1))
    );
    assert!(!events.iter().any(|e| e.event == ScriptEvent::Load));
    assert!(player.set_visible(&one, true));
    assert_eq!(player.property(&p, &child, "x"), Some(44.));
    assert_eq!(
        player
            .evaluate(&p)
            .iter()
            .find(|i| i.instance_path == child)
            .unwrap()
            .transform[4],
        44.
    );
}

#[test]
fn hidden_parent_blocks_direct_click_and_old_semantic_target_then_restores_bubbling() {
    let p = fixture();
    let mut player = Player::new(&p);
    player.drain_scripts();
    let child = path(&["one", "leaf"]);
    let old_target = player
        .accessibility_nodes(&p)
        .into_iter()
        .find(|n| n.instance_path == child)
        .unwrap();
    player.set_visible(&path(&["one"]), false);
    player.dispatch_click(&p, &child);
    assert!(player.drain_scripts().is_empty());
    assert!(!player.activate_accessible(&p, &child, &old_target.lifetime_id));
    player.set_visible(&path(&["one"]), true);
    assert!(player.activate_accessible(&p, &child, &old_target.lifetime_id));
    let events = player.drain_scripts();
    assert_eq!(
        events.iter().map(|e| &e.target_path).collect::<Vec<_>>(),
        vec![&child, &path(&["one"])]
    );
    assert!(events.iter().all(|e| e.event == ScriptEvent::Click));
    player.set_visible(&[], false);
    player.dispatch_click(&p, &child);
    assert!(player.drain_scripts().is_empty());
}

#[test]
fn removed_and_recreated_placements_reset_visibility_while_restart_resets_root() {
    let mut p = fixture();
    let layer = &mut p.symbols[0].timeline.layers[0];
    let original = layer.keyframes[0].elements.clone();
    layer.keyframes.extend([key(1, vec![]), key(2, original)]);
    let mut player = Player::new(&p);
    let one = path(&["one"]);
    let lifetime = player.lifetime_id(&one);
    player.set_visible(&one, false);
    player.control(&p, &[], PlaybackCommand::GotoAndStop(1));
    assert_eq!(player.visible(&p, &one), None);
    assert!(!player.set_visible(&one, true));
    assert!(!player.set_visible(&path(&["unknown"]), false));
    player.control(&p, &[], PlaybackCommand::GotoAndStop(2));
    assert_ne!(player.lifetime_id(&one), lifetime);
    assert_eq!(player.visible(&p, &one), Some(true));
    player.set_visible(&[], false);
    player.reset(&p);
    assert_eq!(player.visible(&p, &[]), Some(true));
    assert_eq!(player.evaluate(&p).len(), 2);
}

#[test]
fn new_children_load_inside_hidden_clips_and_authoring_remains_side_effect_free() {
    let mut p = fixture();
    p.symbols[1].timeline.layers[0]
        .keyframes
        .push(key(1, vec![leaf("new-child")]));
    let authored = p.to_json().unwrap();
    let mut player = Player::new(&p);
    player.drain_scripts();
    player.set_visible(&[], false);
    player.tick(&p);
    let fresh = path(&["one", "new-child"]);
    let events = player.drain_scripts();
    assert!(
        events
            .iter()
            .any(|e| e.target_path == fresh && e.event == ScriptEvent::Load)
    );
    assert!(events.iter().all(|e| player.is_dispatch_current(&p, e)));
    assert!(player.lifetime_id(&fresh).is_some());
    assert!(player.evaluate(&p).is_empty());
    assert_eq!(
        evaluate(&p, &p.root, 1).len(),
        2,
        "authoring preview ignores script overrides"
    );
    assert_eq!(p.to_json().unwrap(), authored);
}

#[test]
fn symbol_swap_recreates_subtree_clocks_lifetimes_visibility_and_transform_state() {
    let mut p = fixture();
    let mut detail = p.symbols[1].clone();
    detail.id = "detail".into();
    detail.timeline.layers[0].id = "detail-layer".into();
    p.symbols[1].timeline.layers[0].keyframes[0].elements = vec![object(
        "nested",
        ElementKind::Instance {
            symbol_id: "detail".into(),
            playback: InstancePlayback::Independent,
        },
    )];
    let mut replacement = p.symbols[1].clone();
    replacement.id = "replacement".into();
    replacement.timeline.layers[0].id = "replacement-layer".into();
    replacement.timeline.layers[0].keyframes[0].elements[0].id = "fresh-nested".into();
    replacement.timeline.layers[0].keyframes[0].elements[0].name = "fresh-nested".into();
    p.symbols.extend([detail, replacement]);
    let mut after = p.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .clone();
    after[0].kind = ElementKind::Instance {
        symbol_id: "replacement".into(),
        playback: InstancePlayback::Independent,
    };
    p.symbols[0].timeline.layers[0]
        .keyframes
        .push(key(1, after));
    p.validate().unwrap();
    let mut player = Player::new(&p);
    player.control(&p, &[], PlaybackCommand::Stop);
    for _ in 0..4 {
        player.tick(&p);
    }
    player.drain_scripts();
    let old_paths = [
        path(&["one"]),
        path(&["one", "nested"]),
        path(&["one", "nested", "leaf"]),
    ];
    let old_lifetimes: Vec<_> = old_paths.iter().map(|p| player.lifetime_id(p)).collect();
    for path in &old_paths {
        assert!(player.set_visible(path, false));
        assert!(player.set_property(path, "x", 77.));
    }
    let sibling = path(&["two", "nested"]);
    let sibling_lifetime = player.lifetime_id(&sibling);
    player.set_visible(&sibling, false);
    player.set_property(&sibling, "x", 19.);
    player.control(&p, &[], PlaybackCommand::GotoAndStop(1));
    let new_paths = [
        path(&["one"]),
        path(&["one", "fresh-nested"]),
        path(&["one", "fresh-nested", "leaf"]),
    ];
    for (path, old) in new_paths.iter().zip(old_lifetimes) {
        assert_ne!(player.lifetime_id(path), old);
        assert_eq!(player.visible(&p, path), Some(true));
        assert_eq!(player.property(&p, path, "x"), Some(0.));
    }
    assert_eq!(player.clip_frame(&new_paths[0]), Some(0));
    assert_eq!(player.clip_frame(&new_paths[1]), Some(0));
    assert_eq!(player.lifetime_id(&old_paths[1]), None);
    assert_eq!(player.visible(&p, &old_paths[2]), None);
    assert_eq!(player.lifetime_id(&sibling), sibling_lifetime);
    assert_eq!(player.clip_frame(&sibling), Some(4));
    assert_eq!(player.visible(&p, &sibling), Some(false));
    assert_eq!(player.property(&p, &sibling, "x"), Some(19.));
    let events = player.drain_scripts();
    for path in new_paths {
        assert!(
            events
                .iter()
                .any(|e| e.target_path == path && e.event == ScriptEvent::Load)
        );
    }
    assert!(events.iter().all(|e| player.is_dispatch_current(&p, e)));
}
