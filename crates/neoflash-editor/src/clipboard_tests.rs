//! Clipboard routing tests use actual egui events and menus, never the OS clipboard.
use crate::{
    app::{Editor, Pending},
    clipboard::ClipboardCommand,
    tests::{primary_button, rectangle_stage},
};
use eframe::egui::{self, Event, Key, Modifiers, Pos2};
use neoflash_core::*;

fn fixture() -> (Editor, egui::Context) {
    let (mut editor, ctx, _) = rectangle_stage();
    editor.add_element(
        ElementKind::Rectangle {
            width: 80.,
            height: 40.,
            fill: [20, 100, 170, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        100.,
        120.,
    );
    editor.selected_element_mut().unwrap().scripts = vec![ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "throw new Error('clipboard must not execute me');".into(),
        },
    }];
    editor.history.clear();
    editor.saved_project = Some(editor.project.clone());
    (editor, ctx)
}

fn field_id() -> egui::Id {
    egui::Id::new("clipboard-test-field")
}

fn frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
    modifiers: Modifiers,
    mut text: Option<&mut String>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1280., 900.),
            )),
            time: Some(time),
            events,
            modifiers,
            ..Default::default()
        },
        |ctx| {
            editor.shortcuts(ctx);
            editor.menu(ctx);
            if let Some(text) = text.as_deref_mut() {
                egui::TopBottomPanel::bottom("clipboard-test-panel").show(ctx, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .id(field_id())
                            .desired_rows(4),
                    );
                });
            }
            editor.stage(ctx);
        },
    )
}

fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        pressed,
        physical_key: None,
        repeat: false,
        modifiers,
    }
}

fn copied(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn copy_objects(editor: &mut Editor, ctx: &egui::Context) -> String {
    let output = frame(
        editor,
        ctx,
        0.1,
        vec![Event::Copy],
        Modifiers::COMMAND,
        None,
    );
    let texts = copied(&output);
    assert_eq!(texts.len(), 1);
    ClipboardPayload::from_text(&texts[0])
        .unwrap()
        .expect("object clipboard must decode");
    texts[0].clone()
}

fn count(editor: &Editor) -> usize {
    editor
        .symbol()
        .timeline
        .layers
        .iter()
        .map(|layer| sample_layer(layer, editor.frame).len())
        .sum()
}

fn focus_and_select_text(editor: &mut Editor, ctx: &egui::Context, text: &mut String) {
    frame(editor, ctx, 0.2, vec![], Modifiers::NONE, Some(text));
    ctx.memory_mut(|memory| memory.request_focus(field_id()));
    frame(
        editor,
        ctx,
        0.3,
        vec![key(Key::A, true, Modifiers::COMMAND)],
        Modifiers::COMMAND,
        Some(text),
    );
    assert!(ctx.wants_keyboard_input());
}

fn text_center(output: &egui::FullOutput, wanted: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == wanted => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("menu must paint {wanted:?}"))
}

#[test]
fn keyboard_copy_and_cut_emit_one_valid_snapshot_and_cut_is_one_undo() {
    let (mut editor, ctx) = fixture();
    let mut endpoint = editor.selected_element().unwrap().clone();
    endpoint.transform.x = 300.;
    editor.symbol_mut().timeline.layers[0].keyframes[0].tween = Some(Easing::Linear);
    editor.symbol_mut().timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 10,
            elements: vec![endpoint],
            tween: None,
            label: String::new(),
        });
    editor.frame = 5;
    editor.saved_project = Some(editor.project.clone());
    let before = editor.project.clone();
    let expected = editor.selected_sampled().unwrap();
    let text = copy_objects(&mut editor, &ctx);
    let payload = ClipboardPayload::from_text(&text).unwrap().unwrap();
    assert_eq!(payload.anchor(), [240., 140.]);
    let value: serde_json::Value =
        serde_json::from_str(text.strip_prefix(CLIPBOARD_PREFIX).unwrap()).unwrap();
    assert_eq!(
        value["elements"][0],
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    let cut = frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Cut],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(copied(&cut), [text]);
    assert_eq!(count(&editor), 0);
    assert_eq!(editor.symbol().timeline.layers[0].keyframes.len(), 3);
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.dirty());
    assert!(!editor.history.can_undo());
}

#[test]
fn native_paste_modes_and_combined_key_event_apply_exactly_once() {
    for (variant, expected_in_place) in [
        ("normal", false),
        ("shift-pressed-after-paste", false),
        ("shift-released-before-v", true),
        ("v-released-before-shift", true),
        ("key-and-paste", true),
        ("unpaired-shift-held", false),
        ("unpaired-shift-on-release", false),
        ("dedicated-paste", false),
        ("nonadjacent-key", false),
    ] {
        let (mut editor, ctx) = fixture();
        let text = copy_objects(&mut editor, &ctx);
        let original = editor.selected_element().unwrap().clone();
        editor.remember_paste_view([500., 300.]);
        let mut events = vec![Event::Paste(text)];
        let modifiers = match variant {
            "shift-pressed-after-paste" => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND));
                Modifiers::COMMAND | Modifiers::SHIFT
            }
            "shift-released-before-v" => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT));
                events.push(key(Key::V, false, Modifiers::NONE));
                Modifiers::NONE
            }
            "v-released-before-shift" => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT));
                events.push(key(Key::V, false, Modifiers::COMMAND | Modifiers::SHIFT));
                Modifiers::NONE
            }
            "key-and-paste" => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT));
                Modifiers::COMMAND | Modifiers::SHIFT
            }
            "unpaired-shift-held" => Modifiers::COMMAND | Modifiers::SHIFT,
            "unpaired-shift-on-release" => {
                events.push(key(Key::V, false, Modifiers::COMMAND | Modifiers::SHIFT));
                Modifiers::NONE
            }
            "dedicated-paste" => {
                events.insert(0, key(Key::Paste, true, Modifiers::SHIFT));
                Modifiers::SHIFT
            }
            "nonadjacent-key" => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT));
                events.insert(1, Event::PointerMoved(Pos2::ZERO));
                Modifiers::NONE
            }
            _ => {
                events.insert(0, key(Key::V, true, Modifiers::COMMAND));
                Modifiers::COMMAND
            }
        };
        let output = frame(&mut editor, &ctx, 0.2, events, modifiers, None);
        assert!(copied(&output).is_empty());
        assert_eq!(count(&editor), 2, "variant={variant}");
        let pasted = editor.selected_element().unwrap();
        assert_ne!(pasted.id, original.id);
        assert_eq!(pasted.scripts, original.scripts);
        let expected = if expected_in_place {
            (100., 120.)
        } else {
            (460., 280.)
        };
        assert_eq!(
            (pasted.transform.x, pasted.transform.y),
            expected,
            "variant={variant}"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(count(&editor), 1);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn multiple_paste_pairs_and_repeat_each_keep_their_own_mode_and_one_undo() {
    let (mut editor, ctx) = fixture();
    let text = copy_objects(&mut editor, &ctx);
    editor.remember_paste_view([500., 300.]);
    let mut repeated = key(Key::V, true, Modifiers::COMMAND);
    if let Event::Key { repeat, .. } = &mut repeated {
        *repeat = true;
    }
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT),
            Event::Paste(text.clone()),
            key(Key::V, false, Modifiers::NONE),
            repeated,
            Event::Paste(text.clone()),
            // A payload without its own preceding press cannot inherit the first mode.
            Event::Paste(text),
        ],
        Modifiers::NONE,
        None,
    );
    let elements = sample_layer(&editor.symbol().timeline.layers[0], editor.frame);
    let positions: Vec<_> = elements
        .iter()
        .map(|e| (e.transform.x, e.transform.y))
        .collect();
    assert_eq!(
        positions,
        [(100., 120.), (100., 120.), (460., 280.), (460., 280.)]
    );
    for remaining in [3, 2, 1] {
        editor.undo();
        assert_eq!(count(&editor), remaining);
    }
    assert!(!editor.history.can_undo());
}

#[test]
fn paired_shortcut_text_paste_inserts_once_without_object_or_tool_side_effects() {
    for modifiers in [Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT] {
        let (mut editor, ctx) = fixture();
        let before = editor.project.clone();
        let before_tool = editor.tool;
        let mut text = "replace this".to_owned();
        focus_and_select_text(&mut editor, &ctx, &mut text);
        frame(
            &mut editor,
            &ctx,
            0.4,
            vec![
                key(Key::V, true, modifiers),
                Event::Paste("first\nλ".into()),
                key(Key::V, false, Modifiers::NONE),
            ],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert_eq!(text, "first\nλ");
        frame(
            &mut editor,
            &ctx,
            0.5,
            vec![
                key(Key::V, true, modifiers),
                Event::Paste("second".into()),
                key(Key::V, true, modifiers),
                Event::Paste("third".into()),
            ],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert_eq!(text, "first\nλsecondthird");
        assert_eq!(editor.project, before);
        assert!(editor.tool == before_tool);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn foreign_malformed_and_locked_paste_are_atomic_and_do_not_overwrite_clipboard() {
    for variant in ["foreign", "malformed", "locked", "hidden"] {
        let (mut editor, ctx) = fixture();
        let text = copy_objects(&mut editor, &ctx);
        let input = match variant {
            "foreign" => "ordinary external text".to_owned(),
            "malformed" => format!("{CLIPBOARD_PREFIX}{{bad json"),
            "locked" => {
                editor.symbol_mut().timeline.layers[0].locked = true;
                text
            }
            _ => {
                editor.symbol_mut().timeline.layers[0].visible = false;
                text
            }
        };
        let before = editor.project.clone();
        let output = frame(
            &mut editor,
            &ctx,
            0.2,
            vec![Event::Paste(input)],
            Modifiers::COMMAND,
            None,
        );
        assert_eq!(editor.project, before, "variant={variant}");
        assert!(copied(&output).is_empty());
        assert!(!editor.history.can_undo());
        assert!(!editor.status.is_empty());
    }
}

#[test]
fn cut_failure_keeps_artwork_and_the_existing_system_clipboard() {
    let (mut editor, ctx) = fixture();
    if let ElementKind::Rectangle { width, .. } = &mut editor.selected_element_mut().unwrap().kind {
        *width = -1.;
    }
    let before = editor.project.clone();
    let output = frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Cut],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(editor.project, before);
    assert_eq!(count(&editor), 1);
    assert!(
        copied(&output).is_empty(),
        "failed serialization must not replace the clipboard"
    );
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Copy failed:"));
}

#[test]
fn text_edit_owns_native_copy_cut_and_even_a_valid_object_payload_paste() {
    let (mut editor, ctx) = fixture();
    let payload = copy_objects(&mut editor, &ctx);
    let before = editor.project.clone();
    let selection = editor.selection.clone();
    let mut text = "trace('selected text');".to_owned();
    focus_and_select_text(&mut editor, &ctx, &mut text);
    let authored_frame = editor.frame;
    editor.set_playing(true);
    let output = frame(
        &mut editor,
        &ctx,
        0.4,
        vec![Event::Copy],
        Modifiers::COMMAND,
        Some(&mut text),
    );
    assert_eq!(copied(&output), ["trace('selected text');"]);
    assert!(!editor.playing, "Copy must pause before the playback tick");
    editor.set_playing(true);
    let output = frame(
        &mut editor,
        &ctx,
        0.5,
        vec![Event::Cut],
        Modifiers::COMMAND,
        Some(&mut text),
    );
    assert_eq!(copied(&output), ["trace('selected text');"]);
    assert!(text.is_empty());
    assert!(!editor.playing, "Cut must pause before the playback tick");
    editor.set_playing(true);
    frame(
        &mut editor,
        &ctx,
        0.6,
        vec![Event::Paste(payload.clone())],
        Modifiers::COMMAND,
        Some(&mut text),
    );
    assert_eq!(text, payload);
    assert!(!editor.playing, "Paste must pause before the playback tick");
    assert_eq!(editor.frame, authored_frame);
    assert_eq!(editor.project, before);
    assert_eq!(editor.selection, selection);
    assert!(!editor.history.can_undo());
}

#[test]
fn edit_menu_copy_and_cut_preserve_the_text_recipient_after_focus_is_lost() {
    for cut in [false, true] {
        let (mut editor, ctx) = fixture();
        let before = editor.project.clone();
        let mut text = "selected script text".to_owned();
        focus_and_select_text(&mut editor, &ctx, &mut text);
        let output = frame(
            &mut editor,
            &ctx,
            0.4,
            vec![],
            Modifiers::NONE,
            Some(&mut text),
        );
        let edit = text_center(&output, "Edit");
        frame(
            &mut editor,
            &ctx,
            0.5,
            vec![Event::PointerMoved(edit), primary_button(edit, true)],
            Modifiers::NONE,
            Some(&mut text),
        );
        frame(
            &mut editor,
            &ctx,
            0.6,
            vec![primary_button(edit, false)],
            Modifiers::NONE,
            Some(&mut text),
        );
        let output = frame(
            &mut editor,
            &ctx,
            0.7,
            vec![],
            Modifiers::NONE,
            Some(&mut text),
        );
        let item = text_center(
            &output,
            if cut {
                "Cut       ⌘X"
            } else {
                "Copy      ⌘C"
            },
        );
        frame(
            &mut editor,
            &ctx,
            0.8,
            vec![Event::PointerMoved(item), primary_button(item, true)],
            Modifiers::NONE,
            Some(&mut text),
        );
        let authored_frame = editor.frame;
        editor.set_playing(true);
        let requested = frame(
            &mut editor,
            &ctx,
            0.9,
            vec![primary_button(item, false)],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert!(
            copied(&requested).is_empty(),
            "text command is delivered after the menu click pass"
        );
        assert!(!editor.playing, "menu text command must freeze its frame");
        let delivered = frame(
            &mut editor,
            &ctx,
            1.,
            vec![],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert_eq!(copied(&delivered), ["selected script text"], "cut={cut}");
        assert_eq!(text, if cut { "" } else { "selected script text" });
        assert_eq!(editor.frame, authored_frame);
        let repeated = frame(
            &mut editor,
            &ctx,
            1.1,
            vec![],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert!(copied(&repeated).is_empty());
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn synchronous_object_menu_paste_has_no_request_roundtrip_and_explicit_mode() {
    for in_place in [false, true] {
        let (mut editor, ctx) = fixture();
        let text = copy_objects(&mut editor, &ctx);
        editor.remember_paste_view([500., 300.]);
        let before = editor.project.clone();
        let output = ctx.run(
            egui::RawInput {
                time: Some(0.2),
                ..Default::default()
            },
            |ctx| {
                editor.request_clipboard_with(
                    ctx,
                    if in_place {
                        ClipboardCommand::PasteInPlace
                    } else {
                        ClipboardCommand::Paste
                    },
                    || Ok(text.clone()),
                );
            },
        );
        assert_eq!(count(&editor), 2);
        let transform = editor.selected_element().unwrap().transform;
        assert_eq!(
            (transform.x, transform.y),
            if in_place { (100., 120.) } else { (460., 280.) }
        );
        assert!(output.viewport_output.values().all(|viewport| {
            !viewport
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::RequestPaste))
        }));
        let pasted = editor.project.clone();
        frame(&mut editor, &ctx, 0.3, vec![], Modifiers::NONE, None);
        assert_eq!(editor.project, pasted);
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}

#[test]
fn deferred_text_paste_is_delivered_once_with_crlf_normalization() {
    let (mut editor, ctx) = fixture();
    let before = editor.project.clone();
    let mut text = "replace this".to_owned();
    focus_and_select_text(&mut editor, &ctx, &mut text);
    let authored_frame = editor.frame;
    editor.set_playing(true);
    editor.request_clipboard_with(&ctx, ClipboardCommand::Paste, || {
        Ok("first\r\nsecond".into())
    });
    assert!(
        !editor.playing,
        "deferred Paste must freeze its target frame"
    );
    ctx.memory_mut(|memory| memory.surrender_focus(field_id()));
    assert_eq!(text, "replace this");
    frame(
        &mut editor,
        &ctx,
        0.4,
        vec![],
        Modifiers::NONE,
        Some(&mut text),
    );
    assert_eq!(text, "first\nsecond");
    assert_eq!(editor.frame, authored_frame);
    frame(
        &mut editor,
        &ctx,
        0.5,
        vec![],
        Modifiers::NONE,
        Some(&mut text),
    );
    assert_eq!(text, "first\nsecond");
    assert_eq!(editor.project, before);
}

#[test]
fn deferred_text_paste_cancels_on_scope_change_expiry_or_new_user_command() {
    for change in [
        "frame",
        "layer",
        "symbol",
        "selection",
        "document",
        "expiry",
        "native-command",
    ] {
        let (mut editor, ctx) = fixture();
        let mut text = "keep this".to_owned();
        focus_and_select_text(&mut editor, &ctx, &mut text);
        editor.request_clipboard_with(&ctx, ClipboardCommand::Paste, || {
            Ok("stale pending text".into())
        });
        let old_session = editor.clipboard.session.clone();
        match change {
            "frame" => editor.frame = 1,
            "layer" => {
                editor.symbol_mut().timeline.layers.push(Layer {
                    properties: Default::default(),
                    id: new_id("layer"),
                    name: "Other".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![],
                });
                editor.layer = 1;
            }
            "document" => {
                editor.perform(Pending::New, &ctx);
                assert_ne!(editor.clipboard.session, old_session);
            }
            "symbol" => {
                let child = Project::blank().symbols.remove(0);
                let id = child.id.clone();
                editor.project.symbols.push(child);
                editor.enter_symbol(id);
            }
            "selection" => editor.clear_selection(),
            _ => {}
        }
        let before = editor.project.clone();
        let (time, events) = if change == "expiry" {
            (6., vec![])
        } else if change == "native-command" {
            (0.4, vec![Event::Paste("fresh text".into())])
        } else {
            (0.4, vec![])
        };
        frame(
            &mut editor,
            &ctx,
            time,
            events,
            Modifiers::NONE,
            Some(&mut text),
        );
        assert_eq!(
            text,
            if change == "native-command" {
                "fresh text"
            } else {
                "keep this"
            },
            "change={change}"
        );
        assert_eq!(editor.project, before, "change={change}");
        frame(
            &mut editor,
            &ctx,
            time + 0.1,
            vec![],
            Modifiers::NONE,
            Some(&mut text),
        );
        assert!(!text.contains("stale pending text"));
    }
}

#[test]
fn empty_or_unavailable_menu_clipboard_leaves_no_later_paste_mode() {
    for unavailable in [false, true] {
        let (mut editor, ctx) = fixture();
        let text = copy_objects(&mut editor, &ctx);
        let before = editor.project.clone();
        editor.request_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
            if unavailable {
                Err("unavailable in test".into())
            } else {
                Ok(String::new())
            }
        });
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
        editor.remember_paste_view([500., 300.]);
        frame(
            &mut editor,
            &ctx,
            0.2,
            vec![Event::Paste(text)],
            Modifiers::COMMAND,
            None,
        );
        let transform = editor.selected_element().unwrap().transform;
        assert_eq!((transform.x, transform.y), (460., 280.));
        assert_eq!(count(&editor), 2);
    }
}

#[test]
fn detached_text_command_waits_for_its_own_viewport_and_never_reaches_stage() {
    let (mut editor, ctx) = fixture();
    let before = editor.project.clone();
    let child = egui::ViewportId::from_hash_of("clipboard-detached-actions-test");
    let mut text = String::new();
    for pass in 0..6 {
        let id = if pass == 2 || pass == 4 {
            egui::ViewportId::ROOT
        } else {
            child
        };
        let mut input = egui::RawInput {
            viewport_id: id,
            time: Some(0.1 + f64::from(pass) * 0.1),
            ..Default::default()
        };
        input.viewports.insert(
            child,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                ..Default::default()
            },
        );
        let _ = ctx.run(input, |ctx| {
            editor.clipboard_events(ctx);
            if id == child {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.add(egui::TextEdit::multiline(&mut text).id(field_id()));
                    if pass == 0 {
                        response.request_focus();
                    }
                });
                if pass == 1 {
                    assert!(ctx.wants_keyboard_input());
                    editor.request_clipboard_with(ctx, ClipboardCommand::Paste, || {
                        Ok("only in Actions".into())
                    });
                    ctx.memory_mut(|memory| memory.surrender_focus(field_id()));
                }
            } else {
                assert!(!ctx.input(|i| {
                    i.events
                        .iter()
                        .any(|event| matches!(event, Event::Paste(_)))
                }));
                editor.stage(ctx);
            }
        });
        if pass < 3 {
            assert!(text.is_empty());
        } else {
            assert_eq!(text, "only in Actions");
        }
        assert_eq!(editor.project, before);
    }
}
