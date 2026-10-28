use neoflash_core::*;
use serde_json::{Value, json};
fn command(player: &mut Player, project: &Project, target: &ObjectRef, value: Value) -> Value {
    serde_json::from_str(&player.as2_input_command(project, target, value).unwrap()).unwrap()
}
#[test]
fn as2_focus_flags_and_custom_tab_order_respect_graph_ancestry() {
    let project = Project::blank();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let a = player.create_display(DynamicKind::MovieClip).unwrap();
    let b = player.create_display(DynamicKind::MovieClip).unwrap();
    player.as2_place_depth(&project, &root, &a, 1).unwrap();
    player.as2_place_depth(&project, &root, &b, 2).unwrap();
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"focusable","has_button_handler":false})
        ),
        false
    );
    command(
        &mut player,
        &project,
        &a,
        json!({"op":"set","name":"focusEnabled","value":true}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"focusable","has_button_handler":false})
        ),
        true
    );
    for (target, index) in [(&a, 2), (&b, 1)] {
        command(
            &mut player,
            &project,
            target,
            json!({"op":"set","name":"tabEnabled","value":true}),
        );
        command(
            &mut player,
            &project,
            target,
            json!({"op":"set","name":"tabIndex","value":index}),
        );
    }
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([b, a])
    );
    command(
        &mut player,
        &project,
        &b,
        json!({"op":"set","name":"enabled","value":false}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([a])
    );
    command(
        &mut player,
        &project,
        &root,
        json!({"op":"set","name":"tabChildren","value":false}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([])
    );
    command(
        &mut player,
        &project,
        &root,
        json!({"op":"set","name":"tabChildren","value":true}),
    );
    assert!(
        player
            .as2_input_command(
                &project,
                &a,
                json!({"op":"set","name":"tabIndex","value":-1})
            )
            .is_err()
    );
    assert_eq!(
        command(&mut player, &project, &a, json!({"op":"get"}))["tab_index"],
        2
    );
    player.reset(&project);
    assert!(
        player
            .as2_input_command(&project, &a, json!({"op":"get"}))
            .is_err()
    );
}
