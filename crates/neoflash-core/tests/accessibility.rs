use neoflash_core::*;

fn button(id: &str, order: u32) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 80.,
            height: 40.,
            fill: [20, 50, 100, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        accessibility: Some(Accessibility {
            name: format!("Activate {id}"),
            description: "An authored action".into(),
            role: AccessibleRole::Button,
            tab_order: order,
            disabled: false,
        }),
        scripts: vec![ObjectScript {
            event: ObjectEvent::Click,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: format!("trace('{id}');"),
            },
        }],
    }
}
fn project(elements: Vec<Element>) -> Project {
    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0].elements = elements;
    project
}

#[test]
fn authored_metadata_roundtrips_and_older_documents_stay_unchanged() {
    let mut old = Project::blank();
    let mut shape = button("legacy", 0);
    shape.accessibility = None;
    old.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(shape);
    let json = old.to_json().unwrap();
    assert!(!json.contains("accessibility"));
    assert_eq!(Project::from_json(&json).unwrap(), old);
    let current = project(vec![button("first", 3)]);
    assert_eq!(
        Project::from_json(&current.to_json().unwrap()).unwrap(),
        current
    );
    let mut json = serde_json::to_value(&current).unwrap();
    let metadata =
        json["symbols"][0]["timeline"]["layers"][0]["keyframes"][0]["elements"][0]["accessibility"]
            .as_object_mut()
            .unwrap();
    metadata.remove("description");
    metadata.remove("tab_order");
    metadata.remove("disabled");
    let loaded = Project::from_json(&json.to_string()).unwrap();
    let data = loaded.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .accessibility
        .as_ref()
        .unwrap();
    assert_eq!(data.description, "");
    assert_eq!(data.tab_order, 0);
    assert!(!data.disabled);
}

#[test]
fn validation_requires_names_and_bounds_authored_metadata() {
    let mut p = project(vec![button("label", 0)]);
    let element = &mut p.symbols[0].timeline.layers[0].keyframes[0].elements[0];
    element.accessibility.as_mut().unwrap().name = "  ".into();
    assert!(
        p.validate()
            .unwrap_err()
            .to_string()
            .contains("accessible name")
    );
    let data = p.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .accessibility
        .as_mut()
        .unwrap();
    data.name = "Открыть сцену".into();
    data.tab_order = 10001;
    assert!(
        p.validate()
            .unwrap_err()
            .to_string()
            .contains("focus order")
    );
    let data = p.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .accessibility
        .as_mut()
        .unwrap();
    data.tab_order = 10000;
    data.description = "x".repeat(4097);
    assert!(
        p.validate()
            .unwrap_err()
            .to_string()
            .contains("description")
    );
}

#[test]
fn focus_order_is_stable_and_activation_routes_one_click_to_the_named_instance() {
    let p = project(vec![
        button("back", 2),
        button("first", 1),
        button("tie", 2),
    ]);
    let mut player = Player::new(&p);
    player.drain_scripts();
    let nodes = player.accessibility_nodes(&p);
    assert_eq!(
        nodes
            .iter()
            .map(|n| n.instance_path[0].as_str())
            .collect::<Vec<_>>(),
        vec!["first", "back", "tie"]
    );
    assert!(player.activate_accessible(&p, &nodes[0].instance_path, &nodes[0].lifetime_id));
    let dispatched = player.drain_scripts();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].target_path, vec!["first"]);
    assert!(matches!(dispatched[0].event, ScriptEvent::Click));
}

#[test]
fn images_disabled_and_invisible_nodes_cannot_be_keyboard_activated() {
    let mut image = button("image", 0);
    image.accessibility.as_mut().unwrap().role = AccessibleRole::Image;
    let mut disabled = button("disabled", 1);
    disabled.accessibility.as_mut().unwrap().disabled = true;
    let p = project(vec![image, disabled, button("visible", 2)]);
    let mut player = Player::new(&p);
    player.drain_scripts();
    for node in player.accessibility_nodes(&p).iter().take(2) {
        assert!(!player.activate_accessible(&p, &node.instance_path, &node.lifetime_id));
    }
    player.dispatch_click(&p, &["disabled".into()]);
    assert!(player.drain_scripts().is_empty());
    let visible = player.accessibility_nodes(&p).pop().unwrap();
    assert!(player.set_property(&visible.instance_path, "alpha", 0.));
    assert_eq!(player.accessibility_nodes(&p).len(), 2);
    assert!(!player.activate_accessible(&p, &visible.instance_path, &visible.lifetime_id));
    assert!(player.set_property(&visible.instance_path, "alpha", 1.));
    assert!(player.set_property(&visible.instance_path, "scale_x", 0.));
    assert!(!player.activate_accessible(&p, &visible.instance_path, &visible.lifetime_id));
}

#[test]
fn removed_then_recreated_button_rejects_a_stale_dom_lifetime() {
    let mut p = project(vec![button("reused", 0)]);
    let first = p.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .clone();
    p.symbols[0].timeline.layers[0].keyframes.extend([
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
            ..Default::default()
        },
        Keyframe {
            frame: 2,
            elements: first,
            tween: None,
            label: String::new(),
            ..Default::default()
        },
    ]);
    let mut player = Player::new(&p);
    player.drain_scripts();
    let old = player.accessibility_nodes(&p).pop().unwrap();
    player.control(&p, &[], PlaybackCommand::GotoAndStop(1));
    assert!(player.accessibility_nodes(&p).is_empty());
    player.control(&p, &[], PlaybackCommand::GotoAndStop(2));
    let new = player.accessibility_nodes(&p).pop().unwrap();
    assert_ne!(old.lifetime_id, new.lifetime_id);
    assert!(!player.activate_accessible(&p, &old.instance_path, &old.lifetime_id));
    assert!(player.activate_accessible(&p, &new.instance_path, &new.lifetime_id));
    assert_eq!(player.drain_scripts().len(), 1);
}

#[test]
fn parent_visibility_and_disabled_state_apply_to_nested_semantic_targets() {
    let mut parent = button("parent", 0);
    parent.kind = ElementKind::Instance {
        symbol_id: "child-symbol".into(),
        playback: InstancePlayback::Independent,
    };
    parent.accessibility.as_mut().unwrap().disabled = true;
    let mut p = project(vec![parent]);
    let mut child = Project::blank().symbols.remove(0);
    child.id = "child-symbol".into();
    child.timeline.layers[0].keyframes[0].elements = vec![button("nested", 1)];
    p.symbols.push(child);
    p.validate().unwrap();
    let mut player = Player::new(&p);
    player.drain_scripts();
    let targets = player.accessibility_nodes(&p);
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|n| n.accessibility.disabled));
    assert_eq!(targets[1].instance_path, vec!["parent", "nested"]);
    assert!(!player.activate_accessible(&p, &targets[1].instance_path, &targets[1].lifetime_id));
    player.dispatch_click(&p, &targets[1].instance_path);
    assert!(player.drain_scripts().is_empty());
    player.set_property(&["parent".into()], "alpha", 0.);
    assert!(player.accessibility_nodes(&p).is_empty());
}

#[test]
fn singular_ancestor_cannot_become_focusable_through_matrix_roundoff() {
    let mut parent = button("collapsed", 0);
    parent.accessibility = None;
    parent.kind = ElementKind::Instance {
        symbol_id: "nested-content".into(),
        playback: InstancePlayback::Independent,
    };
    parent.transform = Transform {
        rotation: 5.,
        scale_x: 0.,
        scale_y: 100.,
        ..Default::default()
    };
    let mut p = project(vec![parent]);
    let mut content = Project::blank().symbols.remove(0);
    content.id = "nested-content".into();
    let mut nested = button("nested", 1);
    nested.transform = Transform {
        rotation: 37.,
        scale_x: 10.,
        scale_y: 100.,
        ..Default::default()
    };
    content.timeline.layers[0].keyframes[0].elements = vec![nested];
    p.symbols.push(content);
    let mut player = Player::new(&p);
    player.drain_scripts();
    let path = vec!["collapsed".into(), "nested".into()];
    let lifetime = player.lifetime_id(&path).unwrap().to_string();
    assert!(player.accessibility_nodes(&p).is_empty());
    assert!(!player.activate_accessible(&p, &path, &lifetime));
    assert!(player.drain_scripts().is_empty());
}
