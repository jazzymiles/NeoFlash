//! R-033 interactions through the real Actions editor, plus shaped-range geometry.
use super::*;
use crate::app::{Editor, ScriptTarget};
use crate::sources::{code_layout_with_prefs, jump_to};
use crate::theme::ThemeMode;
use crate::workspace::CodeEditorPrefs;
use neoflash_core::{
    Element, FrameScript, ObjectEvent, ObjectScript, Project, Script, ScriptLanguage, SourceUnit,
};
use neoflash_script::{DiagnosticDetails, DiagnosticSeverity};

const FRAME_TEXT: &str =
    "// é: Unicode before diagnostic byte offsets\ntrace(not false and true);\ntrace(not true);";
const SOURCE_TEXT: &str = "// é: Unicode before diagnostic byte offsets\nclass Helper { public function run():Boolean { return not false and true; }\npublic function again():Boolean { return not true; } }";

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn render(editor: &mut Editor, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::Key { modifiers, .. } | egui::Event::PointerButton { modifiers, .. } => {
                Some(*modifiers)
            }
            _ => None,
        })
        .unwrap_or(egui::Modifiers::NONE);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400., 650.),
            )),
            time: Some(ctx.cumulative_frame_nr() as f64 * 0.001),
            modifiers,
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| editor.actions_contents(ui));
        },
    )
}

fn press(editor: &mut Editor, ctx: &egui::Context, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = render(
        editor,
        ctx,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    let _ = render(editor, ctx, vec![button(false)]);
}

fn range_hit(output: &egui::FullOutput, source: &str, diagnostic: &Diagnostic) -> egui::Pos2 {
    fn find<'a>(shape: &'a egui::Shape, source: &str) -> Option<&'a egui::epaint::TextShape> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == source => Some(text),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, source)),
            _ => None,
        }
    }
    let text = output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, source))
        .unwrap();
    let start = nf::code::char_index(source, diagnostic.line, diagnostic.column);
    let end = nf::code::char_index(source, diagnostic.end_line, diagnostic.end_column);
    let start_rect = text.galley.pos_from_cursor(CCursor::new(start));
    let end_rect = text.galley.pos_from_cursor(CCursor::new(end));
    text.pos
        + egui::vec2(
            (start_rect.left() + end_rect.left()) / 2.,
            start_rect.center().y,
        )
}

fn checked_hover_diagnostic(editor: &mut Editor, target: ScriptTarget, text: &str) -> Diagnostic {
    if target == ScriptTarget::Source {
        editor.check_project_sources(false);
    } else {
        editor.check_action_source(
            &Script {
                language: ScriptLanguage::ActionScript2,
                source: text.into(),
            },
            false,
        );
    }
    let diagnostic = editor
        .code_feedback
        .as_mut()
        .unwrap()
        .diagnostics
        .iter_mut()
        .find(|diagnostic| diagnostic.line == 2 && !diagnostic.fixes.is_empty())
        .unwrap();
    diagnostic.message = "Deprecated Flash 4 logical operator 'not'; use '!' instead. The logical negation preserves the operand conversion and returns a Boolean result. This explanation is deliberately longer than the compact peek so its complete accessible description can be checked.".into();
    diagnostic.clone()
}

fn fixture(theme: ThemeMode, target: ScriptTarget) -> (Editor, egui::Context, String) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let text = if target == ScriptTarget::Source {
        SOURCE_TEXT
    } else {
        FRAME_TEXT
    };
    let script = Script {
        language: ScriptLanguage::ActionScript2,
        source: text.into(),
    };
    let mut project = Project::blank();
    project.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: script.clone(),
    });
    let mut element = Element {
        id: "diagnostic-object".into(),
        ..Element::default()
    };
    element.scripts.push(ObjectScript {
        event: ObjectEvent::EnterFrame,
        script: script.clone(),
    });
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(element);
    project.sources.push(SourceUnit {
        id: "diagnostic-source".into(),
        path: "Helper.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: SOURCE_TEXT.into(),
    });
    project.enable_program();
    let mut editor = Editor::from_project(&ctx, project);
    theme.apply(&ctx);
    editor.script_target = target;
    editor.script_event = ObjectEvent::EnterFrame;
    editor.selected_source = Some("diagnostic-source".into());
    editor.select_single_id("diagnostic-object".into());
    let _ = render(&mut editor, &ctx, vec![]);
    if target == ScriptTarget::Source {
        editor.check_project_sources(false);
    } else {
        editor.check_action_source(&script, false);
    }
    jump_to(&ctx, editor.actions_editor_id(), text, 2, 1);
    (editor, ctx, text.into())
}

fn source(editor: &Editor, target: ScriptTarget) -> &str {
    match target {
        ScriptTarget::Frame => &editor.symbol().timeline.scripts[0].script.source,
        ScriptTarget::Object => &editor.selected_element().unwrap().scripts[0].script.source,
        ScriptTarget::Source => &editor.project.sources[0].source,
    }
}

fn ui_state(editor: &Editor, ctx: &egui::Context) -> DiagnosticUi {
    ctx.data(|data| data.get_temp(state_id(editor.actions_editor_id())))
        .unwrap_or_default()
}

fn caret(editor: &Editor, ctx: &egui::Context) -> usize {
    egui::text_edit::TextEditState::load(ctx, editor.actions_editor_id())
        .and_then(|state| state.cursor.char_range())
        .unwrap()
        .primary
        .index
}

fn named_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label().or(node.value()) == Some(label))
                .then(|| node.bounds())
                .flatten()
                .map(|rect| {
                    egui::Rect::from_min_max(
                        egui::pos2(rect.x0 as f32, rect.y0 as f32),
                        egui::pos2(rect.x1 as f32, rect.y1 as f32),
                    )
                })
        })
        .unwrap_or_else(|| {
            panic!(
                "Missing {label:?}; labels: {:?}",
                nodes
                    .iter()
                    .filter_map(|(_, node)| node.label().or(node.value()))
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn diagnostic_choice_keyboard_mouse_and_undo_for_all_actions_targets() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        for target in [
            ScriptTarget::Frame,
            ScriptTarget::Object,
            ScriptTarget::Source,
        ] {
            let (mut editor, ctx, original_text) = fixture(theme, target);
            let original = editor.project.clone();
            // Corrupt/no-op suggestions must never become selectable UI, even if a provider
            // accidentally returns them alongside ordinary compiler fixes.
            let diagnostic = editor
                .code_feedback
                .as_mut()
                .unwrap()
                .diagnostics
                .iter_mut()
                .find(|diagnostic| diagnostic.line == 2 && !diagnostic.fixes.is_empty())
                .unwrap();
            let valid = diagnostic.fixes[0].clone();
            diagnostic.fixes.push(valid.clone());
            let mut invalid = valid.clone();
            invalid.title = "Invalid UTF-8 edit".into();
            let unicode = original_text.find('é').unwrap();
            invalid.span = (unicode + 1, unicode + 2);
            diagnostic.fixes.push(invalid);
            let mut no_op = valid;
            no_op.title = "No change".into();
            no_op.replacement = original_text[no_op.span.0..no_op.span.1].into();
            diagnostic.fixes.push(no_op);
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
            );
            let menu = ui_state(&editor, &ctx)
                .menu
                .expect("Several fixes open a chooser");
            assert_eq!(menu.fixes.len(), 2);
            assert_eq!(menu.selected, 0);
            assert_eq!(source(&editor, target), original_text);
            assert!(
                !editor.history.can_undo(),
                "Opening the chooser cannot edit source"
            );
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            );
            assert_eq!(ui_state(&editor, &ctx).menu.unwrap().selected, 1);
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(
                source(&editor, target),
                original_text.replacen(" and ", " && ", 1)
            );
            assert!(!ui_state(&editor, &ctx).is_open());
            editor.project.validate().unwrap();
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(
                !editor.history.can_undo(),
                "One selected fix is one document Undo"
            );

            // Reopen the same real chooser and select the other fix by its accessible hit bounds.
            jump_to(&ctx, editor.actions_editor_id(), &original_text, 2, 1);
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
            );
            let _ = render(&mut editor, &ctx, vec![]);
            let output = render(&mut editor, &ctx, vec![]);
            let at = named_rect(&output, "Replace deprecated 'not' with '!'").center();
            press(&mut editor, &ctx, at);
            assert_eq!(
                source(&editor, target),
                original_text.replacen("not false", "! false", 1)
            );
            editor.project.validate().unwrap();
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());

            // Hover the actual rendered diagnostic range. The peek exposes the full message
            // even though its compact painted component truncates long explanations.
            let diagnostic = checked_hover_diagnostic(&mut editor, target, &original_text);
            jump_to(&ctx, editor.actions_editor_id(), &original_text, 2, 1);
            let output = render(&mut editor, &ctx, vec![egui::Event::PointerGone]);
            let at = range_hit(&output, &original_text, &diagnostic);
            let _ = render(&mut editor, &ctx, vec![egui::Event::PointerMoved(at)]);
            let _ = render(&mut editor, &ctx, vec![]);
            let output = render(&mut editor, &ctx, vec![]);
            let label = format!(
                "Warning: [{}] {}",
                diagnostic.code.as_deref().unwrap(),
                diagnostic.message
            );
            assert!(named_rect(&output, &label).is_positive());
            assert!(ui_state(&editor, &ctx).peek.is_some());
            assert!(!ui_state(&editor, &ctx).pinned);
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(
                ctx.memory(|m| m.has_focus(editor.actions_editor_id())),
                "Dismissing a peek preserves Actions keyboard focus"
            );
            let _ = render(&mut editor, &ctx, vec![]);
            assert!(
                !ui_state(&editor, &ctx).is_open(),
                "Escape stays dismissed under a stationary pointer"
            );
            let _ = render(
                &mut editor,
                &ctx,
                vec![egui::Event::PointerMoved(egui::pos2(4., 4.))],
            );
            let _ = render(&mut editor, &ctx, vec![egui::Event::PointerMoved(at)]);
            let output = render(&mut editor, &ctx, vec![]);
            let action_at = named_rect(&output, "Quick fix").center();
            press(&mut editor, &ctx, action_at);
            assert_eq!(
                source(&editor, target),
                original_text.replacen("not false", "! false", 1)
            );
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());

            // A peek with two safe alternatives opens the chooser by mouse, after which
            // keyboard navigation must keep working without first clicking the source again.
            let mut diagnostic = checked_hover_diagnostic(&mut editor, target, &original_text);
            let alternative = editor
                .code_feedback
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .find(|d| d.line == 2 && d.span.0 != diagnostic.span.0 && !d.fixes.is_empty())
                .unwrap()
                .fixes[0]
                .clone();
            diagnostic.fixes.push(alternative);
            let index = editor
                .code_feedback
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .position(|d| d.span == diagnostic.span)
                .unwrap();
            editor.code_feedback.as_mut().unwrap().diagnostics[index] = diagnostic.clone();
            jump_to(&ctx, editor.actions_editor_id(), &original_text, 2, 1);
            let output = render(&mut editor, &ctx, vec![egui::Event::PointerGone]);
            let at = range_hit(&output, &original_text, &diagnostic);
            let _ = render(&mut editor, &ctx, vec![egui::Event::PointerMoved(at)]);
            let _ = render(&mut editor, &ctx, vec![]);
            let output = render(&mut editor, &ctx, vec![]);
            press(&mut editor, &ctx, named_rect(&output, "2 fixes").center());
            assert!(ui_state(&editor, &ctx).menu.is_some());
            assert!(ctx.memory(|m| m.has_focus(editor.actions_editor_id())));
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            );
            let _ = render(
                &mut editor,
                &ctx,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(
                source(&editor, target),
                original_text.replacen(" and ", " && ", 1)
            );
            editor.undo();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());
        }
    }
}

#[test]
fn diagnostic_navigation_dismissal_and_context_change_do_not_apply_stale_fixes() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let (mut editor, ctx, text) = fixture(theme, ScriptTarget::Frame);
        let original = editor.project.clone();
        let first = text.find("not false").unwrap();
        let second = text.find("and true").unwrap();
        let third = text.find("not true").unwrap();
        jump_to(&ctx, editor.actions_editor_id(), &text, 1, 1);
        for (key_code, modifiers, offset) in [
            (egui::Key::F8, egui::Modifiers::NONE, first),
            (egui::Key::F8, egui::Modifiers::NONE, second),
            (egui::Key::F8, egui::Modifiers::NONE, third),
            (egui::Key::F8, egui::Modifiers::NONE, first),
            (egui::Key::F8, egui::Modifiers::SHIFT, third),
        ] {
            let _ = render(&mut editor, &ctx, vec![key(key_code, modifiers)]);
            assert_eq!(caret(&editor, &ctx), text[..offset].chars().count());
            assert!(ui_state(&editor, &ctx).pinned);
            assert!(ui_state(&editor, &ctx).peek.is_some());
        }
        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            ctx.memory(|m| m.has_focus(editor.actions_editor_id())),
            "Dismissing a pinned diagnostic preserves Actions keyboard focus"
        );
        assert!(!ui_state(&editor, &ctx).is_open());
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());

        jump_to(&ctx, editor.actions_editor_id(), &text, 2, 1);
        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
        );
        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            ctx.memory(|m| m.has_focus(editor.actions_editor_id())),
            "Dismissing a fix chooser preserves Actions keyboard focus"
        );
        assert!(!ui_state(&editor, &ctx).is_open());
        assert_eq!(source(&editor, ScriptTarget::Frame), text);
        assert!(!editor.history.can_undo());

        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
        );
        assert!(ui_state(&editor, &ctx).menu.is_some());
        // Context changes invalidate diagnostics even if the visible source bytes are identical.
        editor.project.sources[0].source = "class Helper { public var added:Number; }".into();
        let _ = render(&mut editor, &ctx, vec![]);
        assert!(!ui_state(&editor, &ctx).is_open());
        assert_eq!(source(&editor, ScriptTarget::Frame), text);
        assert!(!editor.history.can_undo());

        // A chooser must also disappear when a different editor target acquires focus.
        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
        );
        assert!(ui_state(&editor, &ctx).menu.is_some());
        editor.script_target = ScriptTarget::Source;
        let _ = render(&mut editor, &ctx, vec![]);
        assert!(!ui_state(&editor, &ctx).is_open());
        let _ = render(&mut editor, &ctx, vec![]);
        editor.script_target = ScriptTarget::Frame;
        let _ = render(&mut editor, &ctx, vec![]);
        assert!(!ui_state(&editor, &ctx).is_open());
        assert_eq!(source(&editor, ScriptTarget::Frame), text);

        jump_to(&ctx, editor.actions_editor_id(), &text, 2, 1);
        let _ = render(
            &mut editor,
            &ctx,
            vec![key(egui::Key::Period, egui::Modifiers::COMMAND)],
        );
        assert!(ui_state(&editor, &ctx).menu.is_some());
        let replacement = text.replace("not false and true", "true");
        editor.symbol_mut().timeline.scripts[0].script.source = replacement.clone();
        let _ = render(&mut editor, &ctx, vec![]);
        assert!(!ui_state(&editor, &ctx).is_open());
        assert_eq!(source(&editor, ScriptTarget::Frame), replacement);
        assert!(!editor.history.can_undo());
    }
}

fn ranged_diagnostic(source: &str, span: (usize, usize)) -> Diagnostic {
    let (line, column) = neoflash_script::byte_offset_to_line_col(source, span.0);
    let (end_line, end_column) = neoflash_script::byte_offset_to_line_col(source, span.1);
    Diagnostic {
        language: ScriptLanguage::ActionScript2,
        line,
        column,
        message: "Range fixture".into(),
        details: Box::new(DiagnosticDetails {
            span,
            end_line,
            end_column,
            severity: DiagnosticSeverity::Warning,
            ..Default::default()
        }),
    }
}

#[test]
fn diagnostic_ranges_follow_unicode_tabs_wraps_and_scroll_translation() {
    let ctx = egui::Context::default();
    let text = "é\t漢字 alpha beta gamma delta\nsecond line";
    let start = text.find('漢').unwrap();
    let end = text.find("second").unwrap() + "second".len();
    let diagnostic = ranged_diagnostic(text, (start, end));
    let mut invalid = diagnostic.clone();
    invalid.span = (1, 2); // Inside the UTF-8 encoding of é; source coordinates remain valid.
    assert_eq!(char_range(text, &invalid), char_range(text, &diagnostic));
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            for wrap in [false, true] {
                let prefs = CodeEditorPrefs {
                    word_wrap: wrap,
                    ..Default::default()
                };
                let galley = code_layout_with_prefs(ui, &text, 90., &prefs);
                let rects = range_rects(text, &diagnostic, &galley, egui::Pos2::ZERO);
                assert!(rects.len() >= if wrap { 3 } else { 2 });
                assert!(
                    rects
                        .windows(2)
                        .all(|pair| pair[0].bottom() <= pair[1].top())
                );
                let range = char_range(text, &diagnostic);
                let a = galley.pos_from_cursor(CCursor::new(range.start));
                let b = galley.pos_from_cursor(CCursor::new(range.end));
                assert!((rects.first().unwrap().left() - a.left()).abs() < 0.01);
                assert!((rects.last().unwrap().right() - b.left()).abs() < 0.01);
                let shift = egui::vec2(-37., -83.);
                let shifted = range_rects(text, &diagnostic, &galley, shift.to_pos2());
                assert_eq!(
                    shifted,
                    rects
                        .iter()
                        .map(|rect| rect.translate(shift))
                        .collect::<Vec<_>>()
                );

                // The exclusive end at a wrap boundary cannot decorate the next visual row.
                if wrap {
                    let count = galley.rows[0].char_count_excluding_newline();
                    let boundary = text.char_indices().nth(count).unwrap().0;
                    let first_row = ranged_diagnostic(text, (0, boundary));
                    assert_eq!(
                        range_rects(text, &first_row, &galley, egui::Pos2::ZERO).len(),
                        1
                    );
                }
            }
        });
    });
}

#[test]
fn t363_status_bar_diagnostics_click_jump_cross_file_and_skipped_foreign() {
    // 1. In `Actions` (height 650 pt, toolbar 45 pt, bottom bar 32 pt): no `code_feedback` row
    //    steals height below the editor; clicking `script_status` cycles diagnostics in-file!
    let (mut editor, ctx, text) = fixture(ThemeMode::Light, ScriptTarget::Frame);
    jump_to(&ctx, editor.actions_editor_id(), &text, 1, 1);
    let output = render(&mut editor, &ctx, vec![]);
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    assert!(
        !nodes
            .iter()
            .filter_map(|(_, n)| n.label().or(n.value()))
            .any(|l| l.starts_with("Warning: [") && l.contains("Line 2, column")),
        "old code_feedback CompilerMessage rows must not render inside Actions"
    );
    let status_rect = nodes
        .iter()
        .find_map(|(_, n)| {
            let lbl = n.label().or(n.value())?;
            (lbl.starts_with("Line 1, Col 1") && lbl.contains("warning"))
                .then(|| n.bounds())
                .flatten()
                .map(|r| {
                    egui::Rect::from_min_max(
                        egui::pos2(r.x0 as f32, r.y0 as f32),
                        egui::pos2(r.x1 as f32, r.y1 as f32),
                    )
                })
        })
        .expect("bottom script_status button exposes warning summary");
    // Verify bottom status bar sits right at the bottom (`650 - 8 margin - 32 bar = 610`) and hugs the right side.
    assert!(status_rect.top() >= 608.0, "status_rect={status_rect:?}");
    assert!(status_rect.left() > 900.0, "status_rect={status_rect:?}");

    // Tooltip helper formats both passing checks and diagnostic summaries cleanly.
    let ok_tip = Editor::format_script_status_tooltip(
        true,
        "Code accepted for current language support.",
        &[],
    );
    assert_eq!(ok_tip, "✓ Code accepted for current language support.");
    let fb = editor.code_feedback.as_ref().unwrap();
    let warn_tip = Editor::format_script_status_tooltip(fb.success, &fb.text, &fb.diagnostics);
    assert!(
        warn_tip.contains("Line 2, col ") && warn_tip.contains("— click to jump"),
        "unexpected tooltip: {warn_tip}"
    );

    // Click 1 jumps from line 1 to line 2 (`not false`); click 2 jumps to `and true` (line 2 col > first);
    // click 3 jumps to line 3 (`not true`); click 4 wraps back to line 2!
    press(&mut editor, &ctx, status_rect.center());
    let c1 = nf::code::line_col(&text, caret(&editor, &ctx));
    assert_eq!(c1.0, 2);
    press(&mut editor, &ctx, status_rect.center());
    let c2 = nf::code::line_col(&text, caret(&editor, &ctx));
    assert_eq!(c2.0, 2);
    assert!(c2.1 > c1.1);
    press(&mut editor, &ctx, status_rect.center());
    let c3 = nf::code::line_col(&text, caret(&editor, &ctx));
    assert_eq!(c3.0, 3);
    press(&mut editor, &ctx, status_rect.center());
    let c4 = nf::code::line_col(&text, caret(&editor, &ctx));
    assert_eq!(c4, c1);

    // 2. `!success && errors == 0` must still report `"1 error"`, never `"No errors"`.
    editor.code_feedback.as_mut().unwrap().success = false;
    editor.code_feedback.as_mut().unwrap().diagnostics.clear();
    let fail_out = render(&mut editor, &ctx, vec![]);
    let fail_labels: Vec<String> = fail_out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned))
        .collect();
    assert!(
        fail_labels
            .iter()
            .any(|l| l.contains("1 error") && !l.contains("No errors")),
        "failed check with empty diagnostics must never claim No errors: {fail_labels:?}"
    );

    // 3. Cross-file `SourceUnit` jump: clicking `script_status` while viewing `good.ts` switches
    //    `selected_source` to `"bad"` and places the caret on `bad.ts`'s error line!
    let mut cross_proj = Project::blank();
    cross_proj.script_language = Some(ScriptLanguage::TypeScript);
    cross_proj.sources = vec![
        SourceUnit {
            id: "good".into(),
            path: "good.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: "export const ok: number = 1;\n".into(),
        },
        SourceUnit {
            id: "bad".into(),
            path: "bad.ts".into(),
            language: ScriptLanguage::TypeScript,
            source: "// line 1\nexport const broken: number = \"oops\";\n".into(),
        },
    ];
    cross_proj.enable_program();
    let mut cross_ed = Editor::from_project(&ctx, cross_proj);
    cross_ed.script_target = ScriptTarget::Source;
    cross_ed.selected_source = Some("good".into());
    cross_ed.check_project_sources(false);
    let cross_out = render(&mut cross_ed, &ctx, vec![]);
    let cross_status = cross_out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, n)| {
            let lbl = n.label().or(n.value())?;
            (lbl.starts_with("Line 1, Col 1") && lbl.contains("error"))
                .then(|| n.bounds())
                .flatten()
                .map(|r| {
                    egui::Rect::from_min_max(
                        egui::pos2(r.x0 as f32, r.y0 as f32),
                        egui::pos2(r.x1 as f32, r.y1 as f32),
                    )
                })
        })
        .expect("cross-file error count in script_status");
    press(&mut cross_ed, &ctx, cross_status.center());
    assert_eq!(cross_ed.selected_source.as_deref(), Some("bad"));
    let bad_src = &cross_ed.project.sources[1].source;
    assert_eq!(nf::code::line_col(bad_src, caret(&cross_ed, &ctx)).0, 2);

    // 4. `SkippedForeign` frame script in a TypeScript project with a missing TS `document_class`
    //    must show the skipped foreign notice row AND report `No errors` for valid AS2 syntax
    //    without polluting the skipped script with foreign TS `document_class` errors!
    let mut skip_proj = Project::blank();
    skip_proj.script_language = Some(ScriptLanguage::TypeScript);
    skip_proj.document_class = Some("app.MissingTsDocClass".into());
    skip_proj.enable_program();
    let as2_script = Script {
        language: ScriptLanguage::ActionScript2,
        source: "stop();\n".into(),
    };
    skip_proj.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: as2_script.clone(),
    });
    let mut skip_ed = Editor::from_project(&ctx, skip_proj);
    skip_ed.script_target = ScriptTarget::Frame;
    skip_ed.check_action_source(&as2_script, false);
    let skip_out = render(&mut skip_ed, &ctx, vec![]);
    let skip_labels: Vec<String> = skip_out
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned))
        .collect();
    assert!(
        skip_labels.iter().any(|l| l.contains("No errors")),
        "valid skipped AS2 script must report No errors without foreign TS document_class errors: {skip_labels:?}"
    );
}

#[test]
fn t367_narrow_floating_actions_status_fits_and_pin_never_flickers() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut project = Project::blank();
    project.format_version = 13;
    project.script_language = Some(ScriptLanguage::ActionScript2);
    project.symbols[0].timeline.frame_count = 1;
    project.symbols[0].timeline.layers[0].name = "filed".into();
    let mut src = String::new();
    for i in 1..=20 {
        src.push_str(&format!(
            "var line_{i}:Number = {i}; // AllFigures = new Array(TFigure, IFigure, QFigure, L1Figure, L2Figure);\n"
        ));
    }
    src.push_str(
        "function populateDropSet(){\n    proxy = new Array(AllFigures);\n    for (i=proxy.length();i>0;i--){\n        pos=Math.floor(Math.random(proxy.length));\n        dropSet[i]=proxy.splice(pos, 1)[0];\n    }\n}\n\n\nfunction NewGame(){\n    createEmptyGF();\n    populateDropSet();\n}\n\n\n\n\n\n\n",
    );
    let script = Script {
        language: ScriptLanguage::ActionScript2,
        source: src.clone(),
    };
    let layer_id = project.symbols[0].timeline.layers[0].id.clone();
    project.symbols[0].timeline.scripts.push(FrameScript {
        layer: Some(layer_id.clone()),
        frame: 0,
        script: script.clone(),
    });
    let mut editor = Editor::from_project(&ctx, project);
    editor.fonts.install_ui(&ctx);
    neoflash_ui::typography::install_text_styles(&mut (*ctx.style()).clone());
    editor.set_panel_placement(
        &ctx,
        crate::workspace::PanelId::Actions,
        crate::workspace::PanelPlacement::Detached,
    );
    editor.show_panel(crate::workspace::PanelId::Actions);
    editor.check_action_source(&script, false);
    jump_to(&ctx, editor.actions_editor_id(), &src, 40, 1);

    let run_floating = |editor: &mut Editor,
                        win_rect: egui::Rect,
                        t: f64,
                        events: Vec<egui::Event>|
     -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(win_rect),
                time: Some(t),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(neoflash_ui::components::Surface::PanelFlush.frame(ctx))
                    .show(ctx, |ui| {
                        ui.set_min_size(win_rect.size());
                        let _ = editor.native_floating_panel(
                            ui,
                            crate::workspace::PanelId::Actions,
                            &[crate::workspace::PanelId::Actions],
                            [320.0, 220.0],
                            1.0,
                        );
                    });
            },
        )
    };

    let ax_rect = |out: &egui::FullOutput, label: &str| -> egui::Rect {
        let nodes = &out.platform_output.accesskit_update.as_ref().unwrap().nodes;
        let b = nodes
            .iter()
            .find_map(|(_, n)| {
                n.label()
                    .or(n.value())
                    .filter(|l| *l == label)
                    .and_then(|_| n.bounds())
            })
            .unwrap_or_else(|| {
                let all: Vec<_> = nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().or(n.value()))
                    .collect();
                panic!("missing AX node {label:?} in {all:?}");
            });
        egui::Rect::from_min_max(
            egui::pos2(b.x0 as f32, b.y0 as f32),
            egui::pos2(b.x1 as f32, b.y1 as f32),
        )
    };

    fn collect_texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                out.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                ));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect_texts(s, out)),
            _ => {}
        }
    }

    // 1. At 580x440 (narrow floating Actions window matching the user's screenshot), the toolbar
    //    stays one row inside <= 580 px: tool groups that do not fit move into the `More script
    //    tools` menu. The bottom-right `script_status` fits completely inside the right border
    //    with `"20"` warnings visible.
    let win_580 = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(580.0, 440.0));
    let out_580 = run_floating(&mut editor, win_580, 0.0, vec![]);
    let status_580 = ax_rect(&out_580, "Line 40, Col 1  ·  40 lines  ·  20 warnings");
    let tab_580 = ax_rect(&out_580, "filed : Frame 1");
    let pin_580 = ax_rect(&out_580, "Pin script tab: filed : Frame 1");
    assert!(
        status_580.right() <= 580.0,
        "script_status must fit inside 580 px floating window: {status_580:?}"
    );
    assert!(
        status_580.left() >= pin_580.right(),
        "script_status ({status_580:?}) must not overlap ScriptTab ({tab_580:?} / {pin_580:?})"
    );
    let ax_find = |out: &egui::FullOutput, label: &str| -> Option<egui::Rect> {
        let nodes = &out.platform_output.accesskit_update.as_ref().unwrap().nodes;
        nodes.iter().find_map(|(_, n)| {
            n.label()
                .or(n.value())
                .filter(|l| *l == label)
                .and_then(|_| n.bounds())
                .map(|b| {
                    egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    )
                })
        })
    };
    let toolbar_row = ax_rect(&out_580, "Check script");
    let mut overflowed = 0;
    for toolbar_lbl in [
        "Check script",
        "Auto-format",
        "Collapse between braces",
        "Remove comment",
        "Show toolbox sidebar",
        "Pin script tab",
        "Script assist",
        "About Actions",
    ] {
        let Some(r) = ax_find(&out_580, toolbar_lbl) else {
            overflowed += 1;
            continue;
        };
        assert!(
            r.left() >= 0.0 && r.right() <= 580.0,
            "toolbar button {toolbar_lbl:?} overflowed 580 px window: {r:?}"
        );
        assert!(
            (r.center().y - toolbar_row.center().y).abs() < 0.75,
            "toolbar button {toolbar_lbl:?} left the single toolbar row: {r:?} vs {toolbar_row:?}"
        );
    }
    // Check, Pin and About stay on the bar; whatever left it is reachable from the More menu.
    for kept in ["Check script", "Pin script tab", "About Actions"] {
        assert!(
            ax_find(&out_580, kept).is_some(),
            "{kept:?} must stay on the bar"
        );
    }
    let more = ax_find(&out_580, "More script tools");
    assert_eq!(
        more.is_some(),
        overflowed > 0,
        "the More menu appears exactly when a tool group left the bar"
    );
    if let Some(more) = more {
        assert!(
            more.right() <= 580.0,
            "More menu button overflowed: {more:?}"
        );
    }
    let mut texts_580 = Vec::new();
    for cs in &out_580.shapes {
        collect_texts(&cs.shape, &mut texts_580);
    }
    let bottom_texts_580: Vec<_> = texts_580
        .into_iter()
        .filter(|(_, r)| r.top() >= 404.0)
        .collect();
    assert!(
        bottom_texts_580.iter().any(|(t, _)| t == "20"),
        "warning count '20' must be painted in bottom bar: {bottom_texts_580:?}"
    );
    for (txt, rect) in &bottom_texts_580 {
        assert!(
            rect.right() <= 580.0,
            "bottom bar text {txt:?} overflowed 580 px right edge: {rect:?}"
        );
    }

    // 2. At minimum 320x240 floating Actions width, progressive compact position formatting keeps
    //    warning/error counts completely inside <= 320 px without colliding with ScriptTab.
    let win_320 = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 240.0));
    let out_320 = run_floating(&mut editor, win_320, 0.1, vec![]);
    let status_320 = ax_rect(&out_320, "Line 40, Col 1  ·  40 lines  ·  20 warnings");
    assert!(
        status_320.right() <= 320.0,
        "script_status must fit inside minimum 320 px window: {status_320:?}"
    );
    let mut texts_320 = Vec::new();
    for cs in &out_320.shapes {
        collect_texts(&cs.shape, &mut texts_320);
    }
    let warn_320 = texts_320
        .iter()
        .find(|(t, r)| t == "20" && r.top() >= 204.0)
        .map(|(_, r)| *r)
        .expect("warning count '20' must still render at 320 px width");
    assert!(
        warn_320.right() <= 320.0,
        "warning count '20' clipped at 320 px width: {warn_320:?}"
    );

    // 3. Hovering the bottom `ScriptTab` `📌` pin past `tooltip_delay` must NEVER flicker cursor
    //    or hover state, flips the tooltip above the bottom bar, and pins/unpins reliably on click.
    let _ = run_floating(&mut editor, win_580, 0.2, vec![]);
    let pin_pos = pin_580.center();
    let _ = run_floating(
        &mut editor,
        win_580,
        1.0,
        vec![egui::Event::PointerMoved(pin_pos)],
    );
    for i in 1..=5 {
        let out_hover = run_floating(&mut editor, win_580, 2.0 + i as f64 * 0.016, vec![]);
        assert_eq!(
            out_hover.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "bottom pin cursor flickered on hover frame {i}"
        );
        let mut hover_texts = Vec::new();
        for cs in &out_hover.shapes {
            collect_texts(&cs.shape, &mut hover_texts);
        }
        let tip_rect = hover_texts
            .iter()
            .find(|(t, _)| t == "Pin script tab")
            .map(|(_, r)| *r)
            .expect("tooltip 'Pin script tab' must stay visible across every hover frame");
        assert!(
            tip_rect.bottom() <= pin_580.top(),
            "bottom-bar tooltip ({tip_rect:?}) must flip above pin button ({pin_580:?})"
        );
    }
    // Click bottom pin -> pins cleanly!
    let _ = run_floating(
        &mut editor,
        win_580,
        2.2,
        vec![egui::Event::PointerButton {
            pos: pin_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = run_floating(
        &mut editor,
        win_580,
        2.22,
        vec![egui::Event::PointerButton {
            pos: pin_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(editor.actions_ui.pinned_tabs.len(), 1);
    let out_pinned = run_floating(&mut editor, win_580, 2.24, vec![]);
    let _ = ax_rect(&out_pinned, "Unpin script tab: filed : Frame 1");
    let _ = ax_rect(&out_pinned, "Unpin script tab");

    // Hover again past tooltip_delay and click bottom pin -> unpins cleanly!
    let unpin_pos = ax_rect(&out_pinned, "Unpin script tab: filed : Frame 1").center();
    let _ = run_floating(
        &mut editor,
        win_580,
        3.0,
        vec![egui::Event::PointerMoved(unpin_pos)],
    );
    let out_unpin_hover = run_floating(&mut editor, win_580, 4.0, vec![]);
    assert_eq!(
        out_unpin_hover.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
    let _ = run_floating(
        &mut editor,
        win_580,
        4.1,
        vec![egui::Event::PointerButton {
            pos: unpin_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = run_floating(
        &mut editor,
        win_580,
        4.12,
        vec![egui::Event::PointerButton {
            pos: unpin_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.actions_ui.pinned_tabs.is_empty());

    // 4. Unextended playhead frame (`frame = 10` on a 1-frame timeline) pins cleanly via toolbar
    //    `Pin script tab`, stays pinned when playhead moves back to Frame 1, survives clicking the
    //    floating `"Actions"` header tab, and unpins via toolbar `"Unpin script tab"`.
    editor.frame = 10;
    let out_f11 = run_floating(&mut editor, win_580, 5.0, vec![]);
    let toolbar_pin_pos = ax_rect(&out_f11, "Pin script tab").center();
    let _ = run_floating(
        &mut editor,
        win_580,
        5.1,
        vec![
            egui::Event::PointerMoved(toolbar_pin_pos),
            egui::Event::PointerButton {
                pos: toolbar_pin_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let _ = run_floating(
        &mut editor,
        win_580,
        5.12,
        vec![egui::Event::PointerButton {
            pos: toolbar_pin_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let frame11_loc = crate::panels::actions_sidebar::ScriptLocation::Frame {
        symbol: editor.project.root.clone(),
        layer: Some(layer_id),
        frame: 10,
    };
    assert_eq!(editor.actions_ui.pinned_tabs, vec![frame11_loc.clone()]);
    assert_eq!(editor.actions_ui.active_pinned, Some(frame11_loc.clone()));

    // Move playhead to Frame 1 and click floating `"Actions"` header tab -> `Frame 11` stays pinned and active!
    editor.frame = 0;
    let out_after_move = run_floating(&mut editor, win_580, 5.2, vec![]);
    assert_eq!(editor.actions_ui.pinned_tabs, vec![frame11_loc.clone()]);
    assert_eq!(editor.actions_ui.active_pinned, Some(frame11_loc.clone()));
    let header_tab_pos = egui::pos2(64.0, 18.0);
    let _ = run_floating(
        &mut editor,
        win_580,
        5.3,
        vec![
            egui::Event::PointerMoved(header_tab_pos),
            egui::Event::PointerButton {
                pos: header_tab_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let out_after_hdr = run_floating(
        &mut editor,
        win_580,
        5.32,
        vec![egui::Event::PointerButton {
            pos: header_tab_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(
        editor.actions_ui.active_pinned,
        Some(frame11_loc),
        "clicking floating Actions header tab must not clear active_pinned"
    );
    let _ = ax_rect(&out_after_move, "filed : Frame 1");
    let frame11_ed_id = editor.actions_editor_id();
    ctx.memory_mut(|m| m.request_focus(frame11_ed_id));
    let _ = run_floating(&mut editor, win_580, 5.34, vec![]);
    let _ = run_floating(
        &mut editor,
        win_580,
        5.36,
        vec![egui::Event::Text("stop();".into())],
    );
    assert_eq!(editor.symbol().timeline.frame_count, 11);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.symbol().timeline.frame_count, 1);
    editor.project.validate().unwrap();
    let toolbar_unpin_pos = ax_rect(&out_after_hdr, "Unpin script tab").center();
    let _ = run_floating(
        &mut editor,
        win_580,
        5.4,
        vec![
            egui::Event::PointerMoved(toolbar_unpin_pos),
            egui::Event::PointerButton {
                pos: toolbar_unpin_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let _ = run_floating(
        &mut editor,
        win_580,
        5.42,
        vec![egui::Event::PointerButton {
            pos: toolbar_unpin_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(editor.actions_ui.pinned_tabs.is_empty());
    assert_eq!(editor.actions_ui.active_pinned, None);
    editor.project.validate().unwrap();
}

#[test]
fn t382_runtime_and_preflight_compile_diagnostics_highlight_actions_line_and_clear_cleanly() {
    use crate::output_pipeline::OutputLocation;
    use crate::panels::compiler_errors::CompilerDiagnosticItem;

    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let root_id = project.root.clone();
    let figures_source = [
        "stop();",
        "var figure = [[0, 1, 0], [1, 1, 1]];",
        "function rotateFigure(fig) {",
        "    var n = fig.length();",
        "    return n;",
        "}",
        "rotateFigure(figure);",
    ]
    .join("\n");
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: figures_source.clone(),
        },
    }];
    project.sources.push(SourceUnit {
        id: "unit-board".into(),
        path: "src/Board.as".into(),
        language: ScriptLanguage::ActionScript2,
        source: "class Board {\n    function tick():Void {\n        var bad = ; \n    }\n}\n"
            .into(),
    });

    let mut editor = Editor::from_project(&ctx, project);
    editor.script_target = ScriptTarget::Frame;
    editor.frame = 0;

    // 1. Valid AS2 syntax (`fig.length()` compiles cleanly, fails at runtime on Line 4):
    //    recording a runtime diagnostic immediately populates `code_feedback` on Line 4 and
    //    renders the inline diagnostic span in Actions (hoverable + navigable with F8).
    let runtime_loc = OutputLocation::for_frame(&root_id, Some("Scene 1".into()), Some(0), 0)
        .with_line_col(4, 13);
    editor.record_runtime_diagnostic_for_location(
        &runtime_loc,
        "TypeError: 'length' is a numeric property, not a method — use '.length' without '()'.",
    );

    let fb = editor
        .code_feedback
        .as_ref()
        .expect("runtime error immediately populates code_feedback");
    assert!(!fb.success);
    let runtime_diags: Vec<Diagnostic> = fb
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("RUNTIME"))
        .cloned()
        .collect();
    assert_eq!(runtime_diags.len(), 1);
    let diag = runtime_diags[0].clone();
    assert_eq!(diag.line, 4);

    // Render Actions and hover the highlighted span on Line 4 -> opens ErrorPeek with the reason!
    let out = render(&mut editor, &ctx, vec![egui::Event::PointerGone]);
    let hit = range_hit(&out, &figures_source, &diag);
    let _ = render(&mut editor, &ctx, vec![egui::Event::PointerMoved(hit)]);
    let _ = render(&mut editor, &ctx, vec![]);
    let hover_out = render(&mut editor, &ctx, vec![]);
    assert!(
        ui_state(&editor, &ctx).peek.is_some(),
        "hovering runtime-highlighted Line 4 must open DiagnosticUi peek"
    );
    let expected_peek_label = format!("Error: [RUNTIME] {}", diag.message);
    assert!(named_rect(&hover_out, &expected_peek_label).is_positive());

    // 2. Switch to external SourceUnit `src/Board.as` via CompilerErrors jump -> immediately
    //    highlights compile syntax error on Line 3 without waiting for the 0.6s live_check timer!
    editor.compiler_errors.items = vec![CompilerDiagnosticItem {
        is_error: true,
        message: "Unexpected token ';'".into(),
        location_label: "src/Board.as · Line 3".into(),
        script: Some(crate::panels::actions_sidebar::ScriptLocation::Source(
            "unit-board".into(),
        )),
        line: 3,
        column: 19,
        quick_fixes: Vec::new(),
    }];
    editor.jump_to_compiler_diagnostic(&ctx, 0);
    assert!(editor.script_target == ScriptTarget::Source);
    let board_fb = editor
        .code_feedback
        .as_ref()
        .expect("jumping to CompilerErrors row immediately highlights Line 3 in Actions");
    assert!(board_fb.diagnostics.iter().any(|d| d.line == 3));

    // 3. Jump back to Frame 1 via OutputLocation -> runtime highlight on Line 4 is preserved!
    assert!(editor.navigate_to_output_location(&ctx, &runtime_loc));
    assert!(editor.script_target == ScriptTarget::Frame);
    assert!(
        editor.code_feedback.as_ref().is_some_and(|f| f
            .diagnostics
            .iter()
            .any(|d| d.line == 4 && d.code.as_deref() == Some("RUNTIME"))),
        "navigating from Output row must preserve runtime Line 4 highlight in Actions"
    );

    // 4. Editing Line 4 (`fig.length()` -> `fig.length`) invalidates the fingerprint so stale
    //    runtime diagnostics no longer attach, AND `clear_output` wipes stored runtime overlays!
    editor
        .project
        .symbol_mut(&root_id)
        .unwrap()
        .timeline
        .scripts[0]
        .script
        .source = figures_source.replace("fig.length()", "fig.length");
    editor.code_feedback = None;
    editor.sync_actions_diagnostics_after_navigation();
    assert!(
        editor.code_feedback.as_ref().is_none_or(|f| !f
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("RUNTIME"))),
        "editing the offending line must drop the stale runtime diagnostic overlay"
    );

    editor.clear_output(Some(&ctx));
    assert!(editor.code_feedback.as_ref().is_none_or(|f| {
        !f.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("RUNTIME"))
    }));
}
