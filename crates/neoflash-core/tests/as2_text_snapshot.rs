use neoflash_core::*;
use serde_json::json;
#[test]
fn static_snapshot_search_selection_geometry_and_live_paint_work_together() {
    let mut project = Project::blank();
    let make = |id: &str, value: &str, x: f64| Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y: 20.,
            ..Transform::default()
        },
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Text {
            text: value.into(),
            font_family: "Arial".into(),
            font_size: 10.,
            color: [10, 20, 30, 255],
        },
    };
    project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![make("first", "One\n", 10.), make("second", "Two", 60.)];
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let handle = player
        .as2_snapshot_create(&project, &root, |strings, _| {
            Ok(strings
                .iter()
                .map(|value| value.chars().count() as f64 * 6.)
                .collect())
        })
        .unwrap();
    assert_eq!(
        player
            .as2_snapshot_command(&handle, json!({"op":"count"}))
            .unwrap(),
        7
    );
    assert_eq!(
        player
            .as2_snapshot_command(
                &handle,
                json!({"op":"get_text","start":0,"end":7,"include_line_endings":true})
            )
            .unwrap()["units"],
        json!([79, 110, 101, 10, 10, 84, 119, 111])
    );
    assert_eq!(
        player
            .as2_snapshot_command(
                &handle,
                json!({"op":"find","start":0,"query":[116,119,111],"case_sensitive":false})
            )
            .unwrap(),
        4
    );
    player
        .as2_snapshot_command(
            &handle,
            json!({"op":"set_selected","start":0,"end":3,"selected":true}),
        )
        .unwrap();
    assert_eq!(
        player
            .as2_snapshot_command(&handle, json!({"op":"selected_text"}))
            .unwrap()["units"],
        json!([79, 110, 101])
    );
    assert_eq!(
        player
            .as2_snapshot_command(&handle, json!({"op":"get_selected","start":2,"end":7}))
            .unwrap(),
        true
    );
    let info = player
        .as2_snapshot_command(&handle, json!({"op":"run_info","start":0,"end":1}))
        .unwrap();
    assert_eq!(info[0]["font"], "Arial");
    assert_eq!(info[0]["matrix_tx"], 10.);
    assert_eq!(
        player
            .as2_snapshot_command(&handle, json!({"op":"hit_test","x":11.,"y":21.}))
            .unwrap(),
        0
    );
    player
        .as2_snapshot_command(&handle, json!({"op":"select_color","color":65280}))
        .unwrap();
    let scene = player.runtime_snapshot(&project);
    let highlights: Vec<_> = scene
        .draws
        .iter()
        .filter(|draw| draw.item.element_id.starts_with("as2-selection-"))
        .collect();
    assert_eq!(highlights.len(), 3);
    assert!(
        matches!(&highlights[0].item.kind,RuntimeDrawKind::Element(ElementKind::Rectangle{stroke,..})if *stroke==[0,255,0,255])
    );
    let first = player.object_ref(&["first".into()]).unwrap();
    player.object_set_property(&first, "x", 100.);
    assert_eq!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .find(|draw| draw.item.element_id.starts_with("as2-selection-"))
            .unwrap()
            .item
            .transform[4],
        100.
    );
    player.object_set_text(&project, &first, "changed");
    assert!(
        !player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|draw| draw.item.element_id.starts_with("as2-selection-"))
    );
    assert_eq!(
        player
            .as2_snapshot_command(&handle, json!({"op":"count"}))
            .unwrap(),
        7
    );
    player.reset(&project);
    assert!(
        player
            .as2_snapshot_command(&handle, json!({"op":"count"}))
            .is_err()
    );
}
