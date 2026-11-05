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
            let action_at = named_rect(&output, "Quick fix ⌘.").center();
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
            press(
                &mut editor,
                &ctx,
                named_rect(&output, "2 fixes ⌘.").center(),
            );
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
