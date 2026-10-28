use neoflash_core::*;

#[test]
fn as2_sparse_depths_duplicate_paths_and_as3_indices_share_one_graph() {
    let project = Project::blank();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let first = player.create_display(DynamicKind::MovieClip).unwrap();
    let second = player.create_display(DynamicKind::MovieClip).unwrap();
    player.object_set_name(&first, "first");
    player.object_set_name(&second, "second");
    player
        .as2_place_depth(&project, &root, &first, 100)
        .unwrap();
    player.as2_place_depth(&project, &root, &second, 4).unwrap();
    assert_eq!(player.as2_next_depth(&root).unwrap(), 101);
    assert_eq!(
        player.as2_child_at_depth(&root, 100).unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|n| n.object_ref == root)
            .unwrap()
            .children,
        vec![second.clone(), first.clone()]
    );
    player.as2_swap_depth(&first, 4).unwrap();
    assert_eq!(player.as2_get_depth(&second).unwrap(), 100);
    player.as2_swap_depth(&first, 999).unwrap();
    assert_eq!(player.as2_child_at_depth(&root, 4).unwrap(), None);
    player
        .object_set_matrix(&first, [1., 0., 0., 1., 42., 17.])
        .unwrap();
    player
        .graphics_apply(
            &first,
            &[
                GraphicsOp::BeginFill {
                    color: 0x44cc99,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 0.,
                    y: 0.,
                    width: 50.,
                    height: 30.,
                },
            ],
        )
        .unwrap();
    let (copy, events) = player.as2_duplicate(&project, &first, "copy", 100).unwrap();
    assert_eq!(events.events.first().unwrap().kind, "removed");
    let scene = player.runtime_snapshot(&project);
    assert!(
        !scene
            .objects
            .iter()
            .find(|n| n.object_ref == second)
            .unwrap()
            .on_stage
    );
    assert_eq!(
        player.object_matrix(&project, &copy).unwrap(),
        [1., 0., 0., 1., 42., 17.]
    );
    assert!(
        !scene
            .objects
            .iter()
            .find(|n| n.object_ref == copy)
            .unwrap()
            .graphics
            .is_empty()
    );
    player.graphics_apply(&first, &[GraphicsOp::Clear]).unwrap();
    assert!(
        !player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|n| n.object_ref == copy)
            .unwrap()
            .graphics
            .is_empty()
    );
    let nested = player.create_display(DynamicKind::MovieClip).unwrap();
    player.object_set_name(&nested, "nested");
    player
        .as2_place_depth(&project, &copy, &nested, 23)
        .unwrap();
    assert_eq!(
        player.as2_target_path(&project, &nested, true).unwrap(),
        "/copy/nested"
    );
    assert_eq!(
        player.as2_target_path(&project, &nested, false).unwrap(),
        "_level0.copy.nested"
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &first, "_root.copy.nested")
            .unwrap(),
        Some(nested.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &nested, "../nested")
            .unwrap(),
        Some(nested.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &root, "/missing")
            .unwrap(),
        None
    );
    assert!(player.as2_swap_depth(&nested, -1).is_err());
    // AS3's ordinary index API still controls order without implicit depth sorts.
    player
        .display_mutate(
            &project,
            DisplayMutation::SetIndex {
                parent: root.clone(),
                child: first.clone(),
                index: 0,
            },
        )
        .unwrap();
    assert_eq!(
        player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|n| n.object_ref == root)
            .unwrap()
            .children[0],
        first
    );
    // A later AS2 mutation explicitly restores sparse-depth order.
    player.as2_swap_depth(&copy, 2000).unwrap();
    assert_eq!(
        player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|n| n.object_ref == root)
            .unwrap()
            .children
            .last(),
        Some(&copy)
    );
    player.reset(&project);
    assert!(player.as2_get_depth(&nested).is_err());
}
