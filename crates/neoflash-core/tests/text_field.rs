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
    input_transaction_workflow(&project);
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
        ..Element::default()
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
            ..Default::default()
        },
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
            ..Default::default()
        },
        Keyframe {
            frame: 2,
            elements: vec![legacy, boxed.clone()],
            tween: None,
            label: String::new(),
            ..Default::default()
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
        ..TextFormatPatch::default()
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
    // Both boxes have identical glyph layout, but their declared surface heights
    // differ (80 for this runtime field, 100 for the authored field).
    let mut expected_layout = layout.clone();
    expected_layout.surface.height = 80.;
    assert_eq!(layout_text(&state, measure).unwrap(), expected_layout);
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
    let before_units = before.text.encode_utf16().count() as u32;
    let changed = player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                append: Some(" more".into()),
                ..TextPatch::default()
            },
        )
        .unwrap();
    assert!(changed);
    let appended = player.object_text_info(&project, &dynamic).unwrap();
    assert_eq!(appended.format_runs.len(), 2);
    assert_eq!(appended.format_runs[0].format.color, 0x123456);
    assert_eq!(appended.format_runs[1].format.color, 0xff0000);
    player
        .object_text_update(
            &project,
            &dynamic,
            &TextPatch {
                replace: Some(TextReplacement {
                    begin: before_units,
                    end: before_units + 5,
                    text: String::new(),
                }),
                ..TextPatch::default()
            },
        )
        .unwrap();
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

fn input_transaction_workflow(project: &Project) {
    let defaults = TextFieldState::default();
    let mut old = serde_json::to_value(&defaults).unwrap();
    for key in [
        "field_type",
        "selectable",
        "display_as_password",
        "max_chars",
        "restrict",
        "selection_begin",
        "selection_end",
        "caret",
    ] {
        old.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        serde_json::from_value::<TextFieldState>(old).unwrap(),
        defaults
    );

    let mut player = Player::new(project);
    let root = player.object_ref(&[]).unwrap();
    let input = player.create_display(DynamicKind::TextField).unwrap();
    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                field_type: Some(TextFieldType::Input),
                ..TextPatch::default()
            },
        )
        .unwrap();
    player
        .display_mutate(
            project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: input.clone(),
                index: None,
            },
        )
        .unwrap();
    assert!(
        player.runtime_snapshot(project).draws.iter().any(|draw| {
            draw.owner_ref == input
                && matches!(&draw.item.kind,
            RuntimeDrawKind::TextField { field } if field.text.is_empty()
                && field.field_type == TextFieldType::Input)
        }),
        "an empty input keeps its box in the shared draw/hit graph"
    );

    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                text: Some("A😀B".into()),
                selection: Some(TextSelection { begin: 99, end: 2 }),
                restrict: Some(TextRestrictPatch {
                    value: Some("A-Z^Q".into()),
                }),
                max_chars: Some(3),
                ..TextPatch::default()
            },
        )
        .unwrap();
    let selected = player.object_text_info(project, &input).unwrap();
    assert_eq!(
        (
            selected.selection_begin,
            selected.selection_end,
            selected.caret
        ),
        (1, 4, 1)
    );
    let split = TextInputEdit {
        expected_revision: selected.revision.clone(),
        begin: 2,
        end: 3,
        text: "X".into(),
    };
    assert!(player.object_text_input(project, &input, &split).is_err());
    assert_eq!(player.object_text_info(project, &input).unwrap(), selected);
    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                replace_selected: Some("q😀".into()),
                ..TextPatch::default()
            },
        )
        .unwrap();
    let programmed = player.object_text_info(project, &input).unwrap();
    assert_eq!(
        programmed.text, "Aq😀",
        "programmatic replacement bypasses input filters"
    );
    assert_eq!(
        (
            programmed.selection_begin,
            programmed.selection_end,
            programmed.caret
        ),
        (4, 4, 4)
    );
    assert!(
        player
            .object_text_input(
                project,
                &input,
                &TextInputEdit {
                    expected_revision: selected.revision,
                    begin: 0,
                    end: 0,
                    text: "X".into(),
                }
            )
            .is_err(),
        "a script edit invalidates an in-flight DOM input transaction"
    );

    let committed = player
        .object_text_input(
            project,
            &input,
            &TextInputEdit {
                expected_revision: programmed.revision,
                begin: 0,
                end: 4,
                text: "QbA\r\nBCZ".into(),
            },
        )
        .unwrap();
    assert_eq!(committed.text, "ABC");
    assert_eq!(committed.caret, 3);
    let rejected = player
        .object_text_input(
            project,
            &input,
            &TextInputEdit {
                expected_revision: committed.revision.clone(),
                begin: 0,
                end: 3,
                text: "qQ".into(),
            },
        )
        .unwrap();
    assert_eq!(
        rejected, committed,
        "filtered typing does not erase the selection"
    );
    let deleted = player
        .object_text_input(
            project,
            &input,
            &TextInputEdit {
                expected_revision: committed.revision,
                begin: 1,
                end: 3,
                text: String::new(),
            },
        )
        .unwrap();
    assert_eq!(deleted.text, "A");

    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                text: Some(String::new()),
                restrict: Some(TextRestrictPatch { value: None }),
                ..TextPatch::default()
            },
        )
        .unwrap();
    let empty = player.object_text_info(project, &input).unwrap();
    let unicode = player
        .object_text_input(
            project,
            &input,
            &TextInputEdit {
                expected_revision: empty.revision,
                begin: 0,
                end: 0,
                text: "😀A😀".into(),
            },
        )
        .unwrap();
    assert_eq!((unicode.text.as_str(), unicode.caret), ("😀A", 3));
    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                text: Some("😀A\nB".into()),
                display_as_password: Some(true),
                multiline: Some(true),
                ..TextPatch::default()
            },
        )
        .unwrap();
    let password = player.object_text_info(project, &input).unwrap();
    let masked = layout_text(&password, |strings, format| {
        assert!(
            strings.iter().all(|text| text.chars().all(|ch| ch == '*')),
            "host measurement never receives clear password content"
        );
        measure(strings, format)
    })
    .unwrap();
    assert_eq!(password.text, "😀A\nB");
    assert_eq!(
        (
            masked.lines[0].text.as_str(),
            masked.lines[0].start,
            masked.lines[0].end
        ),
        ("***", 0, 4)
    );
    assert_eq!(
        (
            masked.lines[1].text.as_str(),
            masked.lines[1].start,
            masked.lines[1].end
        ),
        ("*", 4, 5)
    );

    let edit = TextInputEdit {
        expected_revision: password.revision.clone(),
        begin: 0,
        end: 0,
        text: "X".into(),
    };
    assert!(player.object_set_visible(&input, false));
    assert!(player.object_text_input(project, &input, &edit).is_err());
    assert!(player.object_set_visible(&input, true));
    player
        .display_mutate(
            project,
            DisplayMutation::Remove {
                parent: root,
                child: input.clone(),
            },
        )
        .unwrap();
    assert!(player.object_text_input(project, &input, &edit).is_err());
    player
        .object_text_update(
            project,
            &input,
            &TextPatch {
                replace_selected: Some("detached".into()),
                ..TextPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        player.object_text_info(project, &input).unwrap().text,
        "detached😀A\nB"
    );
}

#[test]
fn authored_text_box_properties_and_html_flow_to_runtime_state_and_layout() {
    let mut format = TextFormat {
        font: "Georgia".into(),
        size: 18.,
        color: 0x336699,
        bold: false,
        italic: true,
        leading: 4.,
        align: TextAlign::Center,
        left_margin: 6.,
        right_margin: 8.,
        indent: 10.,
        block_indent: 4.,
        underline: true,
        letter_spacing: 1.5,
        kerning: Some(true),
        bullet: false,
        tab_stops: vec![24., 48.],
        url: "https://neoflash.example/docs".into(),
        target: "_blank".into(),
    };
    format.set_align_mode(Some("justify".into()));
    format.set_field_options(Some(neoflash_core::AuthoredTextFieldOptions {
        field_type: TextFieldType::Input,
        selectable: true,
        display_as_password: false,
        max_chars: 64,
        restrict: Some("A-Za-z0-9 ".into()),
        variable: Some("heroLabel".into()),
        auto_size: "left".into(),
        border: true,
        border_color: 0x112233,
        background: true,
        background_color: 0xfefefe,
        html: true,
        embed_fonts: true,
        format_runs: Vec::new(),
        ..Default::default()
    }));
    let kind = neoflash_core::ElementKind::TextBox {
        text: "<b>Hello</b> <font color=\"#ff0000\">World</font>".into(),
        width: 240.,
        height: 60.,
        format,
        word_wrap: true,
        multiline: true,
    };
    let state = TextFieldState::from_kind(&kind).expect("TextBox produces TextFieldState");
    assert_eq!(state.field_type, TextFieldType::Input);
    assert!(state.selectable);
    assert_eq!(state.max_chars, 64);
    assert_eq!(state.restrict.as_deref(), Some("A-Za-z0-9 "));
    assert_eq!(state.variable.as_deref(), Some("heroLabel"));
    assert_eq!(state.as2_viewport.auto_size, "left");
    assert!(state.as2_viewport.active);
    assert!(state.as2_viewport.border);
    assert_eq!(state.as2_viewport.border_color, 0x112233);
    assert!(state.as2_viewport.background);
    assert_eq!(state.as2_viewport.background_color, 0xfefefe);
    assert!(state.html);
    assert!(state.embed_fonts);
    assert_eq!(state.format.font, "Georgia");
    assert_eq!(state.format.size, 18.);
    assert!(state.format.italic);
    assert!(state.format.underline);
    assert_eq!(state.format.letter_spacing, 1.5);
    assert_eq!(state.format.kerning, Some(true));
    assert_eq!(state.format.effective_align_mode(), "justify");
    assert_eq!(state.format.url, "https://neoflash.example/docs");
    assert_eq!(state.format.target_str(), "_blank");

    let layout = layout_text(&state, measure).expect("HTML layout succeeds");
    assert_eq!(layout.lines.len(), 1);
    assert_eq!(layout.lines[0].text, "Hello World");
    assert!(layout.runs.len() >= 2);
    assert!(layout.runs[0].format.as_ref().unwrap().bold);
    assert_eq!(
        layout.runs.last().unwrap().format.as_ref().unwrap().color,
        0xff0000
    );
    assert_eq!(layout.surface.border, Some(0x112233));
    assert_eq!(layout.surface.background, Some(0xfefefe));

    let mut roundtrip = kind.clone();
    state.write_to_kind(&mut roundtrip);
    let state2 = TextFieldState::from_kind(&roundtrip).unwrap();
    assert_eq!(state2, state);
}

#[test]
fn flash_cs5_multicolumn_container_flow_and_advanced_text_layout() {
    let mut format = TextFormat {
        font: "Times New Roman".into(),
        size: 16.,
        color: 0x123456,
        ..Default::default()
    };
    format.set_field_options(Some(neoflash_core::AuthoredTextFieldOptions {
        column_count: 2,
        column_gutter: 20.0,
        vertical_align: "center".into(),
        padding_left: 10.0,
        padding_right: 10.0,
        padding_top: 8.0,
        padding_bottom: 8.0,
        border: true,
        border_color: 0x334455,
        border_width: 2.5,
        first_line_offset_mode: "pt".into(),
        first_line_offset: 6.0,
        highlight: true,
        highlight_color: 0xffee55,
        strikethrough: true,
        typographic_case: "uppercase".into(),
        space_before: 4.0,
        space_after: 6.0,
        ..Default::default()
    }));
    let kind = neoflash_core::ElementKind::TextBox {
        text: "Alpha one\nBeta two\nGamma three\nDelta four".into(),
        width: 300.0,
        height: 120.0,
        format,
        word_wrap: true,
        multiline: true,
    };
    let state = TextFieldState::from_kind(&kind).expect("TextBox produces TextFieldState");
    assert_eq!(state.column_count, 2);
    assert_eq!(state.column_gutter, 20.0);
    assert_eq!(state.vertical_align, "center");
    assert_eq!(state.padding_left, 10.0);
    assert_eq!(state.border_width, 2.5);
    assert!(state.highlight);
    assert!(state.strikethrough);
    assert_eq!(state.typographic_case, "uppercase");

    let layout = layout_text(&state, measure).expect("multi-column layout succeeds");
    assert_eq!(layout.lines.len(), 4);
    // Uppercase typographic case transforms line text
    assert_eq!(layout.lines[0].text, "ALPHA ONE");
    assert_eq!(layout.lines[2].text, "GAMMA THREE");
    // Lines 0..2 are in column 0 (x = padding_left = 10.0); lines 2..4 are in column 1
    // col_width = ((300 - 10 - 10) - 20) / 2 = 130 => col 1 x = 10 + 130 + 20 = 160.0
    assert!((layout.lines[0].x - 10.0).abs() < 1e-6);
    assert!((layout.lines[1].x - 10.0).abs() < 1e-6);
    assert!((layout.lines[2].x - 160.0).abs() < 1e-6);
    assert!((layout.lines[3].x - 160.0).abs() < 1e-6);
    // Vertical alignment "center" pushes lines down below padding_top + first_line_offset (8 + 6 = 14)
    assert!(layout.lines[0].y > 14.0);
    // Highlight and strikethrough decorations are emitted
    assert!(!layout.highlights.is_empty());
    assert_eq!(layout.highlights[0].color, Some(0xffee55));
    assert!(!layout.decorations.is_empty());
    assert_eq!(layout.surface.column_count, 2);
    assert_eq!(layout.surface.border_width, 2.5);

    let mut roundtrip = kind.clone();
    state.write_to_kind(&mut roundtrip);
    let state2 = TextFieldState::from_kind(&roundtrip).unwrap();
    assert_eq!(state2, state);
}
