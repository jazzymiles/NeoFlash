use neoflash_core::as2_text::{Request, execute};
use neoflash_core::{DynamicKind, GraphicsOp, PlaybackCommand, Player, Project, TextFormat};
use serde_json::json;

fn make_timeline_swap_project() -> Project {
    Project::from_json(
        &json!({
            "format_version": 6,
            "name": "Timeline Swap Test",
            "width": 400,
            "height": 300,
            "fps": 30,
            "background": [255, 255, 255, 255],
            "root": "root-sym",
            "symbols": [
                {
                    "id": "root-sym",
                    "name": "Scene 1",
                    "kind": "movie_clip",
                    "timeline": {
                        "frame_count": 3,
                        "layers": [
                            {
                                "id": "main-layer",
                                "name": "Entities",
                                "visible": true,
                                "locked": false,
                                "keyframes": [
                                    {
                                        "frame": 0,
                                        "elements": [
                                            {
                                                "id": "placed-a",
                                                "name": "placedInst",
                                                "transform": { "x": 40.0, "y": 50.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0 },
                                                "kind": { "type": "instance", "symbol_id": "hero-sym", "playback": "independent" },
                                                "scripts": []
                                            },
                                            {
                                                "id": "placed-b",
                                                "name": "otherPlaced",
                                                "transform": { "x": 90.0, "y": 95.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0 },
                                                "kind": { "type": "instance", "symbol_id": "hero-sym", "playback": "independent" },
                                                "scripts": []
                                            }
                                        ],
                                        "tween": null,
                                        "label": ""
                                    },
                                    {
                                        "frame": 1,
                                        "elements": [
                                            {
                                                "id": "placed-a",
                                                "name": "placedInst",
                                                "transform": { "x": 300.0, "y": 310.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0 },
                                                "kind": { "type": "instance", "symbol_id": "hero-sym", "playback": "independent" },
                                                "scripts": []
                                            },
                                            {
                                                "id": "placed-b",
                                                "name": "otherPlaced",
                                                "transform": { "x": 90.0, "y": 95.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0 },
                                                "kind": { "type": "instance", "symbol_id": "hero-sym", "playback": "independent" },
                                                "scripts": []
                                            }
                                        ],
                                        "tween": null,
                                        "label": ""
                                    },
                                    {
                                        "frame": 2,
                                        "elements": [],
                                        "tween": null,
                                        "label": ""
                                    }
                                ]
                            }
                        ],
                        "scripts": []
                    }
                },
                {
                    "id": "hero-sym",
                    "name": "HeroSym",
                    "kind": "movie_clip",
                    "linkage_id": "HeroSym",
                    "timeline": {
                        "frame_count": 1,
                        "layers": [
                            {
                                "id": "hero-layer",
                                "name": "Art",
                                "visible": true,
                                "locked": false,
                                "keyframes": [
                                    {
                                        "frame": 0,
                                        "elements": [
                                            {
                                                "id": "hero-rect",
                                                "name": "",
                                                "transform": { "x": 0.0, "y": 0.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0 },
                                                "kind": { "type": "rectangle", "width": 24.0, "height": 24.0, "fill": [74, 222, 128, 255], "stroke": [0, 0, 0, 0], "stroke_width": 0.0 },
                                                "scripts": []
                                            }
                                        ],
                                        "tween": null,
                                        "label": ""
                                    }
                                ]
                            }
                        ],
                        "scripts": []
                    }
                }
            ]
        })
        .to_string(),
    )
    .unwrap()
}

#[test]
fn fo01_and_fo07_permissive_html_and_text_color_default_inheritance() {
    let parsed = execute(
        serde_json::from_value::<Request>(json!({
            "op": "html",
            "source": "Clear Cave Rats & Brood | HP < 10 | <3 | <bogus>kept</bogus> | <b><i>misnested</b></i> | <font color='#FACC15'>Gold",
            "format": {
                "font": "Courier New",
                "size": 12.0,
                "color": 0xBBF7D0u32,
                "bold": false,
                "italic": false,
                "leading": 2.0,
                "align": "left",
                "leftMargin": 0.0,
                "rightMargin": 0.0,
                "indent": 0.0,
                "blockIndent": 0.0,
                "underline": false,
                "letterSpacing": 0.0,
                "kerning": null,
                "bullet": false,
                "tabStops": [],
                "url": "",
                "target": ""
            },
            "styles": { "styles": {} },
            "condense_white": false,
            "multi_run": true
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        parsed["text"],
        "Clear Cave Rats & Brood | HP < 10 | <3 | kept | misnested | Gold"
    );
    let runs = parsed["format_runs"].as_array().unwrap();
    assert_eq!(runs[0]["format"]["color"], 0xBBF7D0);
    assert_eq!(runs[0]["format"]["font"], "Courier New");
    assert_eq!(runs.last().unwrap()["format"]["color"], 0xFACC15);
    assert_eq!(runs.last().unwrap()["format"]["font"], "Courier New");

    // FO-07: object_set_text_color updates both format.color and default_format.color.
    let project = Project::blank();
    let mut player = Player::new(&project);
    let tf = player.create_display(DynamicKind::TextField).unwrap();
    assert!(player.object_set_text_color(&project, &tf, 0xBBF7D0));
    let info = player.object_text_info(&project, &tf).unwrap();
    assert_eq!(info.format.color, 0xBBF7D0);
    assert_eq!(info.default_format.color, 0xBBF7D0);
    let default_fmt: TextFormat = info.default_format;
    assert_eq!(default_fmt.color, 0xBBF7D0);
}

#[test]
fn fo02_timeline_swap_depths_promotes_and_detaches_from_keyframe_resets() {
    let project = make_timeline_swap_project();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let placed_a = player.object_ref(&["placed-a".into()]).unwrap();
    let placed_b = player.object_ref(&["placed-b".into()]).unwrap();

    let orig_a = player.as2_get_depth(&placed_a).unwrap();
    let orig_b = player.as2_get_depth(&placed_b).unwrap();
    assert!(orig_a < 0 && orig_b < 0);

    // Swap two negative-depth timeline instances first, then promote placed_a to depth 200.
    player.as2_swap_depth(&placed_a, orig_b).unwrap();
    assert_eq!(player.as2_get_depth(&placed_a).unwrap(), orig_b);
    assert_eq!(player.as2_get_depth(&placed_b).unwrap(), orig_a);

    player.object_set_property(&placed_a, "x", 125.0);
    player.as2_swap_depth(&placed_a, 200).unwrap();
    assert_eq!(player.as2_get_depth(&placed_a).unwrap(), 200);
    assert_eq!(
        player.as2_child_at_depth(&root, 200).unwrap(),
        Some(placed_a.clone())
    );

    // Advance to Frame 1 (where authored keyframe has x=300): promoted placed_a keeps x=125 and depth 200,
    // and no duplicate instance is created!
    player.tick(&project);
    assert_eq!(player.as2_get_depth(&placed_a).unwrap(), 200);
    assert_eq!(
        player.object_property(&project, &placed_a, "x"),
        Some(125.0)
    );
    let snap_f1 = player.runtime_snapshot(&project);
    let named_placed: Vec<_> = snap_f1
        .objects
        .iter()
        .filter(|o| o.display.name == "placedInst" && o.on_stage)
        .collect();
    assert_eq!(named_placed.len(), 1);

    // Advance to Frame 2 (blank keyframe): promoted positive-depth clip survives on Stage with its symbol draw!
    player.tick(&project);
    assert!(player.object_alive(&placed_a));
    assert_eq!(player.as2_get_depth(&placed_a).unwrap(), 200);
    let snap_f2 = player.runtime_snapshot(&project);
    assert!(
        snap_f2
            .objects
            .iter()
            .any(|o| o.object_ref == placed_a && o.on_stage)
    );
    assert!(!snap_f2.draws.is_empty());

    // Once promoted to depth >= 0, AS2 remove removes it cleanly.
    player
        .as2_depth_manager(&placed_a, json!({ "op": "remove" }))
        .unwrap();
    let snap_after = player.runtime_snapshot(&project);
    assert!(
        !snap_after
            .objects
            .iter()
            .any(|o| o.object_ref == placed_a && o.on_stage)
    );
}

#[test]
fn fo02_removed_promoted_clip_is_not_recreated_during_same_timeline_span() {
    let project = make_timeline_swap_project();
    let mut player = Player::new(&project);
    let placed_a = player.object_ref(&["placed-a".into()]).unwrap();
    player.as2_swap_depth(&placed_a, 200).unwrap();
    player
        .as2_depth_manager(&placed_a, json!({ "op": "remove" }))
        .unwrap();

    // Tick into Frame 1 (same authored span): placedInst must NOT be recreated at the old layer slot!
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    let snap = player.runtime_snapshot(&project);
    assert!(
        !snap
            .objects
            .iter()
            .any(|o| o.display.name == "placedInst" && o.on_stage)
    );
}

#[test]
fn fo06_and_fo15_recursive_descendant_cleanup_and_large_graphics_batches() {
    let project = make_timeline_swap_project();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();

    // Create holder at depth 201 with child TextField + Bitmap.
    let holder = player.create_display(DynamicKind::MovieClip).unwrap();
    player
        .as2_place_depth(&project, &root, &holder, 201)
        .unwrap();
    let tf = player.create_display(DynamicKind::TextField).unwrap();
    player.as2_place_depth(&project, &holder, &tf, 1).unwrap();
    assert!(player.object_set_text(&project, &tf, "LOOTED!"));

    let px = player.bitmap_create(4, 4, true, 0xFFFF0000).unwrap();
    let bmp = player.create_display(DynamicKind::Bitmap).unwrap();
    player.object_set_bitmap(&project, &bmp, Some(px)).unwrap();
    player.as2_place_depth(&project, &holder, &bmp, 2).unwrap();

    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|d| d.owner_ref == tf)
    );

    // Occupied-depth replacement at 201 retires old holder's descendants!
    let replacement = player.create_display(DynamicKind::MovieClip).unwrap();
    player
        .as2_place_depth(&project, &root, &replacement, 201)
        .unwrap();
    assert!(!player.object_alive(&tf));
    assert!(!player.object_alive(&bmp));
    let snap = player.runtime_snapshot(&project);
    assert!(
        !snap
            .draws
            .iter()
            .any(|d| d.owner_ref == tf || d.owner_ref == bmp)
    );

    // Also verify explicit remove on replacement retires its child TextField.
    let tf2 = player.create_display(DynamicKind::TextField).unwrap();
    player
        .as2_place_depth(&project, &replacement, &tf2, 1)
        .unwrap();
    assert!(player.object_set_text(&project, &tf2, "LOOTED AGAIN!"));
    player
        .as2_depth_manager(&replacement, json!({ "op": "remove" }))
        .unwrap();
    assert!(!player.object_alive(&tf2));
    assert!(
        !player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|d| d.owner_ref == tf2)
    );

    // FO-15: single Graphics batch > 4096 operations (e.g. 6000 vertices) succeeds cleanly.
    let terrain = player.create_display(DynamicKind::MovieClip).unwrap();
    player
        .as2_place_depth(&project, &root, &terrain, 10)
        .unwrap();
    let mut ops = Vec::with_capacity(6003);
    ops.push(GraphicsOp::BeginFill {
        color: 0x336699,
        alpha: 1.0,
    });
    ops.push(GraphicsOp::MoveTo { x: 0.0, y: 0.0 });
    for i in 1..=6000 {
        ops.push(GraphicsOp::LineTo {
            x: (i % 100) as f64,
            y: (i / 100) as f64,
        });
    }
    ops.push(GraphicsOp::EndFill);
    player.graphics_apply(&terrain, &ops).unwrap();
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|d| d.owner_ref == terrain)
    );

    // [P0-04]: Multi-byte UTF-8 straddling byte index `i + 9` after `<` must never panic in CDATA check.
    for sample in [
        "<b>AПривет</b>",
        "<1234567🎉",
        "<font color='#BBF7D0'>Статус & <Уровень 5></font>",
        "<![CDATA[Привет & <мир>]]>",
    ] {
        let parsed = execute(
            serde_json::from_value::<Request>(json!({
                "op": "html",
                "source": sample,
                "format": {
                    "font": "Courier New",
                    "size": 12.0,
                    "color": 0xBBF7D0u32,
                    "bold": false,
                    "italic": false,
                    "leading": 0.0,
                    "align": "left",
                    "leftMargin": 0.0,
                    "rightMargin": 0.0,
                    "indent": 0.0,
                    "blockIndent": 0.0,
                    "underline": false,
                    "letterSpacing": 0.0,
                    "kerning": null,
                    "bullet": false,
                    "tabStops": [],
                    "url": "",
                    "target": ""
                },
                "styles": { "styles": {} },
                "condense_white": false,
                "multi_run": true
            }))
            .unwrap(),
        )
        .expect("multi-byte UTF-8 htmlText must never fail or panic");
        assert!(!parsed["text"].as_str().unwrap_or("").is_empty());
    }
}
