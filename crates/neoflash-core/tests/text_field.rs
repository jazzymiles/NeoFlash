use neoflash_core::*;

fn measure(strings: &[String], _: &TextFormat) -> Result<Vec<f64>, TextError> {
    Ok(strings
        .iter()
        .map(|s| s.chars().map(|c| if c == '😀' { 10. } else { 5. }).sum())
        .collect())
}
#[test]
fn uniform_fields_share_layout_but_keep_runtime_formats_and_placement_lifetimes_independent() {
    let mut project = Project::blank();
    let legacy = Element {
        id: "heading".into(),
        name: "heading".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Text {
            text: "Old heading\nSecond line".into(),
            font_family: "Arial".into(),
            font_size: 20.,
            color: [12, 34, 56, 128],
        },
    };
    let mut boxed = legacy.clone();
    boxed.id = "box".into();
    boxed.name = "box".into();
    boxed.kind = ElementKind::TextBox {
        text: "alpha beta\n😀tailz".into(),
        width: 34.,
        height: 100.,
        format: TextFormat {
            font: "Arial".into(),
            size: 10.,
            leading: 2.,
            align: TextAlign::Center,
            ..TextFormat::default()
        },
        word_wrap: true,
        multiline: true,
    };
    project.symbols[0].timeline.frame_count = 3;
    project.symbols[0].timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![legacy.clone(), boxed.clone()],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 2,
            elements: vec![legacy, boxed.clone()],
            tween: None,
            label: String::new(),
        },
    ];
    let original = project.to_json().unwrap();
    let pure = evaluate(&project, &project.root, 0);
    assert_eq!(Project::from_json(&original).unwrap(), project);
    assert!(
        !serde_json::to_string(&pure[0].kind)
            .unwrap()
            .contains("word_wrap")
    );
    let field = TextFieldState::from_kind(&boxed.kind).unwrap();
    let mut batches = 0;
    let layout = layout_text(&field, |strings, format| {
        batches += 1;
        measure(strings, format)
    })
    .unwrap();
    assert_eq!(
        batches, 3,
        "one batch each for segments, oversized-word glyphs and rendered lines"
    );
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["alpha ", "beta", "😀tail", "z"]
    );
    assert_eq!(
        (layout.lines[1].start, layout.lines[1].end),
        (6, 11),
        "hard newline belongs to the UTF-16 line range"
    );
    assert_eq!((layout.lines[2].start, layout.lines[2].end), (11, 17));
    assert_eq!((layout.lines[3].x, layout.lines[3].y), (14.5, 44.));
    assert_eq!(
        (layout.text_width, layout.text_height, layout.line_height),
        (30., 54., 14.)
    );
    assert!(layout_text(&field, |_, _| Ok(vec![f64::NAN])).is_err());
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let authored = player.object_ref(&["box".into()]).unwrap();
    let heading = player.object_ref(&["heading".into()]).unwrap();
    let old = player.object_text_info(&project, &heading).unwrap();
    assert!(old.box_size.is_none());
    assert_eq!(old.alpha, 128);
    assert!(old.multiline);
    let old_layout = layout_text(&old, measure).unwrap();
    assert_eq!(old_layout.lines[0].x, 0.);
    assert_eq!(old_layout.lines[1].y, 24.);
    let dynamic = player.create_display(DynamicKind::TextField).unwrap();
    assert_eq!(
        player
            .object_text_info(&project, &dynamic)
            .unwrap()
            .box_size,
        Some(TextBoxSize {
            width: 100.,
            height: 100.
        })
    );
    player.object_set_property(&dynamic, "scale_x", 2.);
    let format = TextFormatPatch {
        font: Some("Arial".into()),
        size: Some(10.),
        color: Some(0x123456),
        bold: Some(true),
        italic: Some(true),
        leading: Some(2.),
        align: Some(TextAlign::Center),
    };
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                text: Some(field.text.clone()),
                width: Some(34.),
                height: Some(80.),
                multiline: Some(true),
                word_wrap: Some(true),
                default_format: Some(format),
                ..TextPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        player.object_property(&project, &dynamic, "scale_x"),
        Some(2.)
    );
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: dynamic.clone(),
                index: None,
            },
        )
        .unwrap();
    let state = player.object_text_info(&project, &dynamic).unwrap();
    assert!(state.format.bold && state.format.italic);
    assert_eq!(layout_text(&state, measure).unwrap(), layout);
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|draw| draw.owner_ref == dynamic
                && matches!(draw.item.kind, RuntimeDrawKind::TextField { .. }))
    );
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                default_format: Some(TextFormatPatch {
                    color: Some(0xff0000),
                    ..TextFormatPatch::default()
                }),
                ..TextPatch::default()
            },
        )
        .unwrap();
    let before = player.object_text_info(&project, &dynamic).unwrap();
    assert_eq!(before.format.color, 0x123456);
    assert_eq!(before.default_format.color, 0xff0000);
    assert!(
        player
            .object_text_update(
                &project,
                &dynamic,
                &TextPatch {
                    append: Some(" more".into()),
                    ..TextPatch::default()
                }
            )
            .is_err()
    );
    assert_eq!(
        player.object_text_info(&project, &dynamic).unwrap(),
        before,
        "mixed formatting rejection is atomic"
    );
    assert!(
        player
            .object_text_update(
                &project,
                &dynamic,
                &TextPatch {
                    replace: Some(TextReplacement {
                        begin: 12,
                        end: 13,
                        text: "X".into()
                    }),
                    ..TextPatch::default()
                }
            )
            .is_err(),
        "UTF-16 edit cannot split a surrogate pair"
    );
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                text: Some("New\r\ntext".into()),
                ..TextPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        player
            .object_text_info(&project, &dynamic)
            .unwrap()
            .format
            .color,
        0xff0000
    );
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                append: Some("!".into()),
                ..TextPatch::default()
            },
        )
        .unwrap();
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                replace: Some(TextReplacement {
                    begin: 4,
                    end: 8,
                    text: "body".into(),
                }),
                ..TextPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        player.object_text_info(&project, &dynamic).unwrap().text,
        "New\nbody!"
    );
    let good = player.object_text_info(&project, &dynamic).unwrap();
    assert!(
        player
            .object_text_update(
                &project,
                &dynamic,
                &TextPatch {
                    width: Some(-1.),
                    text: Some("Rejected".into()),
                    ..TextPatch::default()
                }
            )
            .is_err()
    );
    assert_eq!(player.object_text_info(&project, &dynamic).unwrap(), good);
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: root.clone(),
                child: dynamic.clone(),
            },
        )
        .unwrap();
    assert_eq!(player.object_text_info(&project, &dynamic).unwrap(), good);
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|draw| draw.owner_ref != dynamic)
    );
    player
        .object_text_update(
            &project,
            &authored,
            &TextPatch {
                format: Some(TextFormatPatch {
                    size: Some(30.),
                    ..TextFormatPatch::default()
                }),
                ..TextPatch::default()
            },
        )
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    assert!(player.object_text_info(&project, &authored).is_err());
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["box".into()]).unwrap();
    assert_ne!(replacement, authored);
    assert_eq!(
        player
            .object_text_info(&project, &replacement)
            .unwrap()
            .format
            .size,
        10.
    );
    assert_eq!(player.object_text_info(&project, &dynamic).unwrap(), good);
    assert_eq!(project.to_json().unwrap(), original);
    assert_eq!(evaluate(&project, &project.root, 0), pure);
    player.reset(&project);
    assert!(player.object_text_info(&project, &dynamic).is_err());
    assert_eq!(
        player
            .object_text_info(&project, &player.object_ref(&["box".into()]).unwrap())
            .unwrap(),
        field
    );
}
