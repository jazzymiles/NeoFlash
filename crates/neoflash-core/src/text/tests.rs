use super::graphemes::clamp_utf16;
use super::*;

fn fixed_measure(strings: &[String], _format: &TextFormat) -> Result<Vec<f64>, TextError> {
    Ok(strings
        .iter()
        .map(|s| s.chars().count() as f64 * 10.0)
        .collect())
}

#[test]
fn multirun_formats_and_justify_align_modes_work() {
    let mut field = TextFieldState {
        text: "Alpha Beta Gamma".into(),
        multiline: true,
        word_wrap: true,
        box_size: Some(TextBoxSize {
            width: 124.0,
            height: 80.0,
        }),
        ..TextFieldState::default()
    };
    field
        .apply(&TextPatch {
            format: Some(TextFormatPatch {
                align_mode: Some("justify".into()),
                ..TextFormatPatch::default()
            }),
            ..TextPatch::default()
        })
        .unwrap();
    let justified = layout_text(&field, fixed_measure).unwrap();
    assert_eq!(justified.lines.len(), 2);
    assert_eq!(justified.lines[0].width, 120.0);
    assert!(justified.runs.len() >= 3);

    field
        .apply(&TextPatch {
            format_range: Some(TextFormatRangePatch {
                begin: 0,
                end: 5,
                format: TextFormatPatch {
                    bold: Some(true),
                    color: Some(0xff0000),
                    display: Some("inline".into()),
                    ..TextFormatPatch::default()
                },
            }),
            ..TextPatch::default()
        })
        .unwrap();
    assert_eq!(field.format_runs.len(), 2);
    let first_fmt = query_text(
        &field,
        &TextQuery::GetFormat { begin: 0, end: 5 },
        fixed_measure,
    )
    .unwrap();
    assert_eq!(first_fmt["bold"], serde_json::json!(true));
    assert_eq!(first_fmt["color"], serde_json::json!(0xff0000));
    assert_eq!(first_fmt["display"], serde_json::json!("inline"));
    let mixed_fmt = query_text(
        &field,
        &TextQuery::GetFormat { begin: -1, end: -1 },
        fixed_measure,
    )
    .unwrap();
    assert!(mixed_fmt["bold"].is_null());
    assert!(mixed_fmt["color"].is_null());
    assert_eq!(mixed_fmt["font"], serde_json::json!("Times New Roman"));
    let mr_layout = layout_text(&field, fixed_measure).unwrap();
    assert!(mr_layout.runs.iter().any(|r| {
        r.format
            .as_ref()
            .is_some_and(|f| f.bold && f.color == 0xff0000)
    }));
}

#[test]
fn paragraph_and_char_queries_respect_utf16_and_scroll() {
    let mut field = TextFieldState {
        text: "Hello\nWorld!".into(),
        multiline: true,
        box_size: Some(TextBoxSize {
            width: 200.0,
            height: 100.0,
        }),
        ..TextFieldState::default()
    };
    assert_eq!(
        query_text(
            &field,
            &TextQuery::FirstCharInParagraph { char_index: 0 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(0)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::ParagraphLength { char_index: 4 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(6)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::FirstCharInParagraph { char_index: 6 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(6)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::ParagraphLength { char_index: 8 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(6)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::FirstCharInParagraph { char_index: 99 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(-1)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::CharBoundaries { char_index: 1 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!({"x": 12.0, "y": 2.0, "width": 10.0, "height": 12.0 * 1.2})
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::CharBoundaries { char_index: -1 },
            fixed_measure
        )
        .unwrap(),
        serde_json::Value::Null
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::CharIndexAtPoint { x: 15.0, y: 8.0 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(1)
    );
    assert_eq!(
        query_text(
            &field,
            &TextQuery::LineIndexAtPoint { x: 15.0, y: 20.0 },
            fixed_measure
        )
        .unwrap(),
        serde_json::json!(1)
    );
    field
        .apply(&TextPatch {
            sharpness: Some(999.0),
            thickness: Some(-500.0),
            anti_alias_type: Some("advanced".into()),
            grid_fit_type: Some("subpixel".into()),
            ..TextPatch::default()
        })
        .unwrap();
    assert_eq!(field.sharpness, 400.0);
    assert_eq!(field.thickness, -200.0);
    assert_eq!(field.anti_alias_type, "advanced");
    assert_eq!(field.grid_fit_type, "subpixel");
}

#[test]
fn portable_font_metrics_subset_and_outline_generation_work() {
    let fmt = TextFormat {
        font: "Arial".into(),
        size: 20.0,
        ..TextFormat::default()
    };
    let widths =
        TextFormat::portable_measure_strings(&["Hello".into(), "i".into(), "W".into()], &fmt, None);
    assert_eq!(widths.len(), 3);
    assert!(
        widths[1] < widths[2],
        "proportional metrics make 'i' narrower than 'W'"
    );

    let mut used = std::collections::BTreeSet::new();
    used.insert('A' as u32);
    used.insert('B' as u32);
    let subset_bytes = TextFormat::subset_font_bytes(b"\x00\x01\x00\x00", &used).unwrap();
    let cps = TextFormat::subset_font_codepoints(&subset_bytes);
    assert!(cps.contains(&('A' as u32)));
    assert!(cps.contains(&('B' as u32)));
    assert!(!cps.contains(&('Z' as u32)));

    let cmds = TextFieldState::static_text_to_outline_commands("Hi!", "Arial", 24.0, None);
    assert!(!cmds.is_empty());
    assert!(
        cmds.iter()
            .any(|c| matches!(c, crate::PathCommand::MoveTo { .. }))
    );
    assert!(cmds.iter().any(|c| matches!(c, crate::PathCommand::Close)));
}

#[test]
fn html_css_parsing_serialization_and_auto_size_work() {
    let mut css = std::collections::BTreeMap::new();
    let mut highlight = std::collections::BTreeMap::new();
    highlight.insert("color".into(), "#ff0000".into());
    highlight.insert("font-weight".into(), "bold".into());
    css.insert(".highlight".into(), highlight);

    let field = TextFieldState::parse_html(
        "<p align=\"center\">Hello <span class=\"highlight\"><i>World</i></span> <a href=\"https://example.com\">Link</a></p>",
        &TextFormat::default(),
        Some(&css),
        true,
    )
    .unwrap();
    assert_eq!(field.text, "Hello World Link");
    assert!(field.format_runs.len() >= 3);
    assert!(
        field
            .format_runs
            .iter()
            .any(|r| r.format.bold && r.format.italic && r.format.color == 0xff0000)
    );
    assert!(
        field
            .format_runs
            .iter()
            .any(|r| r.format.url == "https://example.com")
    );
    let serialized = field.to_html_text();
    assert!(serialized.contains("<b><i>World</i></b>"));
    assert!(serialized.contains("href=\"https://example.com\""));

    let mut auto_field = TextFieldState {
        text: "AutoSize Center Test".into(),
        box_size: Some(TextBoxSize {
            width: 80.0,
            height: 20.0,
        }),
        ..TextFieldState::default()
    };
    let layout = auto_field.layout_portable(None).unwrap();
    let mut x = 100.0;
    assert!(
        auto_field
            .apply_auto_size("center", &mut x, &layout)
            .unwrap()
    );
    assert!(auto_field.box_size.unwrap().width > 80.0);
    assert!(x < 100.0);
}

#[test]
fn tlf_typography_ligatures_digit_width_digit_case_glyph_rotation_and_locale_execute_in_layout() {
    // 1. Ligature levels: none, minimum, common, uncommon.
    let mut lig_field = TextFieldState {
        text: "fine flag first".into(),
        ligatures: "minimum".into(),
        ..TextFieldState::default()
    };
    let min_layout = layout_text(&lig_field, fixed_measure).unwrap();
    let min_joined: String = min_layout.runs.iter().map(|r| r.text.as_str()).collect();
    assert!(min_joined.contains('\u{FB01}'), "minimum ligatures form fi");
    assert!(min_joined.contains('\u{FB02}'), "minimum ligatures form fl");
    assert!(
        !min_joined.contains('\u{FB06}'),
        "minimum ligatures do not form st"
    );

    lig_field.ligatures = "uncommon".into();
    let unc_layout = layout_text(&lig_field, fixed_measure).unwrap();
    let unc_joined: String = unc_layout.runs.iter().map(|r| r.text.as_str()).collect();
    assert!(
        unc_joined.contains('\u{FB06}'),
        "uncommon ligatures form st"
    );

    lig_field.text = "explicit \u{FB01}".into();
    lig_field.ligatures = "none".into();
    let none_layout = layout_text(&lig_field, fixed_measure).unwrap();
    let none_joined: String = none_layout.runs.iter().map(|r| r.text.as_str()).collect();
    assert!(
        none_joined.contains('\u{FB01}'),
        "disabling substitutions preserves an explicitly authored ligature glyph"
    );

    // 2. Digit width (tabular vs proportional) & digit case (old_style vs lining).
    let mut num_field = TextFieldState {
        text: "11111".into(),
        digit_width: "tabular".into(),
        digit_case: "old_style".into(),
        ..TextFieldState::default()
    };
    let tab_layout = layout_text(&num_field, fixed_measure).unwrap();
    num_field.digit_width = "proportional".into();
    let prop_layout = layout_text(&num_field, fixed_measure).unwrap();
    assert!(
        prop_layout.lines[0].width < tab_layout.lines[0].width,
        "proportional '11111' must be narrower than tabular '11111'"
    );
    assert!(
        tab_layout.runs.iter().all(|r| r.y > 0.0),
        "old_style digits apply x-height/descender baseline shift"
    );

    // 3. Glyph rotation in vertical and horizontal modes.
    let vert_field = TextFieldState {
        text: "A漢".into(),
        orientation: "vertical_rtl".into(),
        glyph_rotation: "auto".into(),
        ..TextFieldState::default()
    };
    let vert_layout = layout_text(&vert_field, fixed_measure).unwrap();
    assert_eq!(
        vert_layout.runs[0].rotation, 90.0,
        "Latin character rotates 90 deg in vertical auto"
    );
    assert_eq!(
        vert_layout.runs[1].rotation, 0.0,
        "CJK ideograph stays upright 0 deg in vertical auto"
    );

    // 4. Locale-aware case conversion (Turkish dotted İ) and Japanese Kinsoku Shori wrapping.
    let tr_field = TextFieldState {
        text: "istanbul".into(),
        typographic_case: "uppercase".into(),
        locale: "tr_TR".into(),
        ..TextFieldState::default()
    };
    let tr_layout = layout_text(&tr_field, fixed_measure).unwrap();
    assert!(
        tr_layout.lines[0].text.starts_with('İ'),
        "tr_TR uppercase maps 'i' to 'İ'"
    );

    let ja_field = TextFieldState {
        text: "日本語。テスト".into(),
        locale: "ja_JP".into(),
        multiline: true,
        word_wrap: true,
        box_size: Some(TextBoxSize {
            width: 30.0,
            height: 100.0,
        }),
        ..TextFieldState::default()
    };
    let ja_layout = layout_text(&ja_field, fixed_measure).unwrap();
    assert!(ja_layout.lines.len() >= 2);
    for line in &ja_layout.lines {
        assert!(
            !line.text.starts_with('。'),
            "Kinsoku Shori prevents line from starting with '。'"
        );
    }
}

#[test]
fn utf16_range_formatting_splits_merges_and_preserves_surrounding_runs() {
    let mut field = TextFieldState {
        text: "Hello brave world".into(),
        format: TextFormat {
            font: "Inter".into(),
            size: 16.0,
            color: 0x112233,
            ..TextFormat::default()
        },
        ..TextFieldState::default()
    };

    // 1. Format "brave" (UTF-16 6..11) with Bold + Georgia + 24pt + red.
    field
        .apply_format_to_utf16_range(
            6,
            11,
            &TextFormatPatch {
                font: Some("Georgia".into()),
                size: Some(24.0),
                color: Some(0xff0000),
                bold: Some(true),
                ..TextFormatPatch::default()
            },
        )
        .unwrap();
    assert_eq!(field.format_runs.len(), 3);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 6)
    );
    assert_eq!(field.format_runs[0].format.font, "Inter");
    assert!(!field.format_runs[0].format.bold);
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (6, 11)
    );
    assert_eq!(field.format_runs[1].format.font, "Georgia");
    assert_eq!(field.format_runs[1].format.size, 24.0);
    assert_eq!(field.format_runs[1].format.color, 0xff0000);
    assert!(field.format_runs[1].format.bold);
    assert_eq!(
        (field.format_runs[2].start, field.format_runs[2].end),
        (11, 17)
    );
    assert_eq!(field.format_runs[2].format.font, "Inter");
    assert_eq!(field.format_runs[2].format.size, 16.0);
    assert_eq!(field.format_runs[2].format.color, 0x112233);

    // 2. Apply Italic + letter_spacing across an overlapping sub-range "lo bra" (UTF-16 3..9).
    field
        .apply_format_to_utf16_range(
            3,
            9,
            &TextFormatPatch {
                italic: Some(true),
                letter_spacing: Some(2.5),
                ..TextFormatPatch::default()
            },
        )
        .unwrap();
    assert_eq!(field.format_runs.len(), 5);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 3)
    );
    assert!(!field.format_runs[0].format.italic);
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (3, 6)
    );
    assert_eq!(field.format_runs[1].format.font, "Inter");
    assert!(field.format_runs[1].format.italic);
    assert_eq!(field.format_runs[1].format.letter_spacing, 2.5);
    assert_eq!(
        (field.format_runs[2].start, field.format_runs[2].end),
        (6, 9)
    );
    assert_eq!(field.format_runs[2].format.font, "Georgia");
    assert!(field.format_runs[2].format.bold);
    assert!(field.format_runs[2].format.italic);
    assert_eq!(field.format_runs[2].format.letter_spacing, 2.5);
    assert_eq!(
        (field.format_runs[3].start, field.format_runs[3].end),
        (9, 11)
    );
    assert_eq!(field.format_runs[3].format.font, "Georgia");
    assert!(field.format_runs[3].format.bold);
    assert!(!field.format_runs[3].format.italic);
    assert_eq!(
        (field.format_runs[4].start, field.format_runs[4].end),
        (11, 17)
    );

    // 3. Clearing Italic + letter_spacing on 3..9 merges adjacent identical runs back to 3 runs.
    field
        .apply_format_to_utf16_range(
            3,
            9,
            &TextFormatPatch {
                italic: Some(false),
                letter_spacing: Some(0.0),
                ..TextFormatPatch::default()
            },
        )
        .unwrap();
    assert_eq!(field.format_runs.len(), 3);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 6)
    );
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (6, 11)
    );
    assert_eq!(
        (field.format_runs[2].start, field.format_runs[2].end),
        (11, 17)
    );
    field.validate().unwrap();
}

#[test]
fn utf16_replace_range_shifts_runs_handles_surrogate_pairs_and_applies_typing_style() {
    // Surrogate pair '😀' is 2 UTF-16 code units: "A😀B" has UTF-16 length 4 (A=0..1, 😀=1..3, B=3..4).
    let mut field = TextFieldState {
        text: "A😀B".into(),
        format: TextFormat {
            font: "Inter".into(),
            size: 14.0,
            color: 0x000000,
            ..TextFormat::default()
        },
        ..TextFieldState::default()
    };
    field
        .apply_format_to_utf16_range(
            3,
            4,
            &TextFormatPatch {
                bold: Some(true),
                color: Some(0x00ff00),
                ..TextFormatPatch::default()
            },
        )
        .unwrap();
    assert_eq!(field.format_runs.len(), 2);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 3)
    );
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (3, 4)
    );

    // Insert "XY" at caret 1 (right after 'A') with an explicit italic typing style.
    let mut typing = field.format_at_utf16_caret(1);
    typing.italic = true;
    typing.color = 0x0000ff;
    let new_caret = field
        .replace_utf16_range(1, 1, "XY", Some(&typing))
        .unwrap();
    assert_eq!(new_caret, 3);
    assert_eq!(field.text, "AXY😀B");
    assert_eq!(field.utf16_len(), 6);
    // "A" = 0..1 (plain), "XY" = 1..3 (italic blue), "😀" = 3..5 (plain), "B" = 5..6 (bold green).
    assert_eq!(field.format_runs.len(), 4);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 1)
    );
    assert!(!field.format_runs[0].format.italic);
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (1, 3)
    );
    assert!(field.format_runs[1].format.italic);
    assert_eq!(field.format_runs[1].format.color, 0x0000ff);
    assert_eq!(
        (field.format_runs[2].start, field.format_runs[2].end),
        (3, 5)
    );
    assert!(!field.format_runs[2].format.italic);
    assert!(!field.format_runs[2].format.bold);
    assert_eq!(
        (field.format_runs[3].start, field.format_runs[3].end),
        (5, 6)
    );
    assert!(field.format_runs[3].format.bold);
    assert_eq!(field.format_runs[3].format.color, 0x00ff00);

    // Delete "XY😀" (UTF-16 1..5): "A" (0..1) and "B" (1..2) remain intact with their styles!
    let after_del = field.replace_utf16_range(1, 5, "", None).unwrap();
    assert_eq!(after_del, 1);
    assert_eq!(field.text, "AB");
    assert_eq!(field.format_runs.len(), 2);
    assert_eq!(
        (field.format_runs[0].start, field.format_runs[0].end),
        (0, 1)
    );
    assert!(!field.format_runs[0].format.bold);
    assert_eq!(
        (field.format_runs[1].start, field.format_runs[1].end),
        (1, 2)
    );
    assert!(field.format_runs[1].format.bold);
    assert_eq!(field.format_runs[1].format.color, 0x00ff00);
    field.validate().unwrap();
}

#[test]
fn utf16_rejected_replacement_preserves_selection_text_and_runs() {
    let mut field = TextFieldState {
        text: "A12B".into(),
        restrict: Some("0-9".into()),
        format: TextFormat {
            font: "Inter".into(),
            ..TextFormat::default()
        },
        ..TextFieldState::default()
    };
    field
        .apply_format_to_utf16_range(
            1,
            3,
            &TextFormatPatch {
                bold: Some(true),
                ..TextFormatPatch::default()
            },
        )
        .unwrap();
    let before = field.clone();

    let caret = field.replace_utf16_range(1, 3, "x", None).unwrap();
    assert_eq!(caret, 1);
    assert_eq!(field, before, "rejected typing must be a complete no-op");

    let caret = field.replace_utf16_range(1, 3, "", None).unwrap();
    assert_eq!(caret, 1);
    assert_eq!(
        field.text, "AB",
        "an explicit empty replacement still deletes"
    );
    field.validate().unwrap();
}

#[test]
fn t326_grapheme_boundaries_selection_delete_replace_and_maxchars_never_split_clusters() {
    // String combining: decomposed Latin "e\u{0301}" (2), decomposed Cyrillic "и\u{0306}" (2),
    // Japanese kana+dakuten "か\u{3099}" (2), regional flag "🇺🇸" (4), ZWJ family "👨‍👩‍👧‍👦" (11),
    // rainbow flag "🏳️‍🌈" (6), skin-tone ZWJ "👩🏽‍💻" (7).
    let sample = "e\u{0301}и\u{0306}か\u{3099}🇺🇸👨‍👩‍👧‍👦🏳️‍🌈👩🏽‍💻";
    let bounds = grapheme_utf16_boundaries(sample);
    assert_eq!(bounds, vec![0, 2, 4, 6, 10, 21, 27, 34]);
    // Unicode 15.1 Extend and GB9c Indic-conjunct regressions. The former was
    // missing from NeoFlash's old hand-maintained property table.
    assert_eq!(grapheme_utf16_boundaries("\u{1E922}\u{1E944}"), vec![0, 4]);
    assert_eq!(
        grapheme_utf16_boundaries("\u{0915}\u{094D}\u{0937}"),
        vec![0, 3]
    );

    // Stepping forward and backward visits exact cluster boundaries only.
    let mut forward = Vec::new();
    let mut cur = 0usize;
    forward.push(cur as u32);
    while cur < 34 {
        cur = next_grapheme_utf16(sample, cur);
        forward.push(cur as u32);
    }
    assert_eq!(forward, bounds);

    let mut backward = Vec::new();
    while cur > 0 {
        cur = prev_grapheme_utf16(sample, cur);
        backward.push(cur as u32);
    }
    backward.reverse();
    assert_eq!(backward, vec![0u32, 2, 4, 6, 10, 21, 27]);

    // Mid-cluster clamping snaps cleanly without panicking on surrogate halves or combining marks.
    assert_eq!(clamp_utf16(sample, 1), 0); // inside e\u{0301}
    assert_eq!(clamp_utf16(sample, 3), 2); // inside и\u{0306}
    assert_eq!(clamp_utf16(sample, 5), 4); // inside か\u{3099}
    assert_eq!(clamp_utf16(sample, 7), 6); // surrogate midpoint inside 🇺🇸
    assert_eq!(clamp_utf16(sample, 15), 10); // ZWJ midpoint inside 👨‍👩‍👧‍👦

    // Mixed-run input field with max_chars: ZWJ cluster exceeding remaining UTF-16 capacity is rejected whole.
    let mut field = TextFieldState {
        field_type: TextFieldType::Input,
        text: "A".into(),
        max_chars: 10,
        format: TextFormat {
            font: "Inter".into(),
            color: 0xff0000,
            bold: true,
            ..TextFormat::default()
        },
        ..TextFieldState::default()
    };
    let rev0 = field.revision.clone();
    // 👨‍👩‍👧‍👦 is 11 UTF-16 code units; capacity remaining is 9, so whole cluster must be rejected cleanly.
    field
        .apply_input(&TextInputEdit {
            expected_revision: rev0,
            begin: 1,
            end: 1,
            text: "👨‍👩‍👧‍👦".into(),
        })
        .unwrap();
    assert_eq!(field.text, "A");

    // Inserting "e\u{0301}🇺🇸" (2 + 4 = 6 UTF-16 units) fits within max_chars=10 -> total 7 units.
    let rev1 = field.revision.clone();
    field
        .apply_input(&TextInputEdit {
            expected_revision: rev1,
            begin: 1,
            end: 1,
            text: "e\u{0301}🇺🇸👨‍👩‍👧‍👦".into(),
        })
        .unwrap();
    assert_eq!(field.text, "Ae\u{0301}🇺🇸");
    assert_eq!(field.caret, 7);
    field.validate().unwrap();

    // Mid-cluster deletion via apply_input expands to full cluster (deleting all of "e\u{0301}" at 1..2).
    let rev2 = field.revision.clone();
    field
        .apply_input(&TextInputEdit {
            expected_revision: rev2,
            begin: 1,
            end: 2,
            text: String::new(),
        })
        .unwrap();
    assert_eq!(field.text, "A🇺🇸");
    assert_eq!(field.caret, 1);

    // textInteractionMode is a read-only host/user state in AS3, not a persisted
    // script-set editing lock. A legacy document value must not disable input.
    field
        .apply(&TextPatch {
            interaction_mode: Some("selection".into()),
            selection: Some(TextSelection { begin: 1, end: 5 }),
            ..TextPatch::default()
        })
        .unwrap();
    assert_eq!(field.interaction_mode, "selection");
    assert_eq!((field.selection_begin, field.selection_end), (1, 5));
    let rev_sel = field.revision.clone();
    field
        .apply_input(&TextInputEdit {
            expected_revision: rev_sel,
            begin: 1,
            end: 5,
            text: "edited".into(),
        })
        .unwrap();
    assert_eq!(field.text, "Aedited");
    field.validate().unwrap();
}
