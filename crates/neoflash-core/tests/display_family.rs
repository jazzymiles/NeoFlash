use neoflash_core::*;

#[test]
fn active_graph_retains_hidden_geometry_names_identity_and_root_transforms() {
    let mut project = Project::demo();
    let authored = evaluate(&project, &project.root, 0);
    let mut player = Player::new(&project);
    let original = player.display_nodes(&project);
    let child = original
        .iter()
        .find(|node| node.path.len() == 1 && node.owner == "movie_clip")
        .unwrap();
    let path = child.path.clone();
    let lifetime = child.lifetime_id.clone();
    let world = child.world;
    let text_node = original
        .iter()
        .find(|node| node.path.len() == 1 && node.owner == "text")
        .unwrap();
    let text_path = text_node.path.clone();
    let authored_text = text_node.kind.clone();
    let text_lifetime = player.lifetime_id(&text_path);
    assert!(player.set_text(&text_path, "Zoe:45\nAda:30"));
    assert!(player.set_text_color(&text_path, 0x23aabb));
    assert!(!player.set_text(&path, "Not a TextField"));
    assert!(!player.set_text_color(&text_path, 0x1000000));
    assert!(!player.set_text(&text_path, &"x".repeat(1_048_577)));
    let rendered_text = player
        .evaluate(&project)
        .into_iter()
        .find(|item| item.instance_path == text_path)
        .unwrap();
    assert!(
        matches!(rendered_text.kind, RuntimeDrawKind::TextField { ref field }
        if field.text == "Zoe:45\nAda:30" && field.format.color == 0x23aabb)
    );
    assert_eq!(player.lifetime_id(&text_path), text_lifetime);
    assert!(original.iter().any(|node| node.kind.is_some()));
    assert!(player.set_property(&[], "x", 43.));
    assert!(player.set_property(&[], "y", -17.));
    assert!(player.set_name(&path, "renamed"));
    assert_eq!(
        player.child_path(&project, &[], "renamed"),
        Some(path.clone())
    );
    assert!(player.set_visible(&path, false));
    let graph = player.display_nodes(&project);
    let hidden = graph.iter().find(|node| node.path == path).unwrap();
    assert_eq!(hidden.lifetime_id, lifetime);
    assert!(!hidden.visible);
    assert_eq!(hidden.world[4], world[4] + 43.);
    assert_eq!(hidden.world[5], world[5] - 17.);
    assert!(
        player
            .evaluate(&project)
            .iter()
            .all(|item| !item.instance_path.starts_with(&path))
    );
    assert!(
        graph
            .iter()
            .filter(|node| node.path.starts_with(&path))
            .all(|node| !node.effective_visible)
    );
    // Metadata is supplied at runtime, without leaking it into authored placement fields.
    project.document_class = Some("example.Main".into());
    assert_eq!(
        player.display_nodes(&project)[0].class_name.as_deref(),
        Some("example.Main")
    );
    assert_eq!(evaluate(&project, &project.root, 0), authored);
    player.reset(&project);
    assert_ne!(player.lifetime_id(&path).unwrap().to_string(), lifetime);
    assert_eq!(player.property(&project, &[], "x"), Some(0.));
    assert_eq!(
        player
            .display_nodes(&project)
            .into_iter()
            .find(|node| node.path == text_path)
            .unwrap()
            .kind,
        authored_text
    );
    assert_ne!(
        player
            .display_nodes(&project)
            .iter()
            .find(|node| node.path == path)
            .unwrap()
            .name,
        "renamed"
    );

    // Disappearing text loses its override; re-entry creates a fresh authored field.
    assert!(player.set_text(&text_path, "Temporary"));
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    let layer = root
        .timeline
        .layers
        .iter_mut()
        .find(|layer| {
            layer.keyframes[0]
                .elements
                .iter()
                .any(|element| element.id == text_path[0])
        })
        .unwrap();
    let mut absent = layer.keyframes[0].clone();
    absent.frame = 1;
    absent.elements.retain(|element| element.id != text_path[0]);
    layer.keyframes.push(absent);
    layer.keyframes.sort_by_key(|key| key.frame);
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(1)));
    assert!(!player.set_text(&text_path, "Inactive"));
    assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(0)));
    assert_eq!(
        player
            .display_nodes(&project)
            .into_iter()
            .find(|node| node.path == text_path)
            .unwrap()
            .kind,
        authored_text
    );
}
