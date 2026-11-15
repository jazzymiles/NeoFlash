//! Clipboard routing tests use actual egui events and menus, never the OS clipboard.
use crate::{
    app::{Editor, Pending},
    clipboard::ClipboardCommand,
    commands::{CommandId, Shortcut},
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
            ..Default::default()
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
        ("payload-before-key", false),
        ("adjacent-custom", true),
        ("nonadjacent-custom", false),
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
            "payload-before-key" => {
                events.push(key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT));
                Modifiers::NONE
            }
            "adjacent-custom" | "nonadjacent-custom" => {
                editor.workspace.shortcuts.set_shortcuts(
                    CommandId::EditPasteInPlace,
                    vec![Shortcut::new(Modifiers::COMMAND, Key::F8)],
                );
                events.insert(0, key(Key::F8, true, Modifiers::COMMAND));
                if variant == "nonadjacent-custom" {
                    events.insert(1, Event::PointerMoved(Pos2::ZERO));
                }
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
fn clipboard_routing_preserves_bare_shortcuts_and_unrelated_input() {
    for with_payload in [false, true] {
        let (mut editor, ctx) = fixture();
        editor.workspace.shortcuts.set_shortcuts(
            CommandId::EditPaste,
            vec![Shortcut::new(Modifiers::COMMAND, Key::F8)],
        );
        let press = key(Key::F8, true, Modifiers::COMMAND);
        let unrelated = vec![
            Event::PointerMoved(Pos2::new(70., 80.)),
            key(Key::F8, false, Modifiers::NONE),
            key(Key::ArrowRight, true, Modifiers::NONE),
        ];
        let mut events = vec![press.clone()];
        events.extend(unrelated.clone());
        if with_payload {
            events.push(Event::Paste("external text".into()));
        }
        let before = editor.project.clone();
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                editor.clipboard_events(ctx);
                let mut expected = unrelated.clone();
                if !with_payload {
                    expected.insert(0, press.clone());
                }
                assert_eq!(ctx.input(|input| input.events.clone()), expected);
                assert_eq!(
                    editor.take_command_shortcut(ctx, CommandId::EditPaste),
                    !with_payload,
                    "only a shortcut without a native payload needs a clipboard read"
                );
            },
        );
        assert_eq!(editor.project, before);
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
    for (modifiers, interleaved) in [
        (Modifiers::COMMAND, false),
        (Modifiers::COMMAND | Modifiers::SHIFT, false),
        (Modifiers::COMMAND, true),
        (Modifiers::COMMAND | Modifiers::SHIFT, true),
    ] {
        let (mut editor, ctx) = fixture();
        let before = editor.project.clone();
        let before_tool = editor.tool;
        let mut text = "replace this".to_owned();
        focus_and_select_text(&mut editor, &ctx, &mut text);
        let mut events = vec![
            key(Key::V, true, modifiers),
            Event::Paste("first\nλ".into()),
            key(Key::V, false, Modifiers::NONE),
        ];
        if interleaved {
            events.insert(1, Event::PointerMoved(Pos2::ZERO));
        }
        frame(
            &mut editor,
            &ctx,
            0.4,
            events,
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
        let item = text_center(&output, if cut { "Cut" } else { "Copy" });
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

#[test]
fn svg_clipboard_paste_respects_editable_paths_toggle_position_and_undo() {
    let svg = concat!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
        "<!-- vector tool clipboard -->\n",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"80\" height=\"40\">",
        "<path fill=\"#2288dd\" d=\"M10 10H70V30H10Z\"/>",
        "</svg>",
    );

    // 1. editable_vector_paste = true (default): pastes editable Path at paste_center, one Undo.
    let (mut editor, ctx) = fixture();
    assert!(editor.workspace.clipboard_prefs.editable_vector_paste);
    let before = editor.project.clone();
    editor.remember_paste_view([500., 300.]);
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Paste(svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert!(editor.project.resources.is_empty());
    let pasted = editor.selected_element().expect("pasted SVG path selected");
    assert!(matches!(pasted.kind, ElementKind::Path { .. }));
    assert_eq!((pasted.transform.x, pasted.transform.y), (460., 280.));
    editor.undo();
    assert_eq!(editor.project, before);

    // 2. paste_in_place = true: keeps SVG coordinates (0, 0).
    editor.workspace.paste_in_place = true;
    frame(
        &mut editor,
        &ctx,
        0.3,
        vec![Event::Paste(svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    let in_place = editor.selected_element().unwrap();
    assert!(matches!(in_place.kind, ElementKind::Path { .. }));
    assert_eq!((in_place.transform.x, in_place.transform.y), (0., 0.));
    editor.undo();
    assert_eq!(editor.project, before);

    // 3. editable_vector_paste = false: rasterizes SVG to one PNG resource + Bitmap element, one Undo.
    editor.workspace.paste_in_place = false;
    editor.workspace.clipboard_prefs.editable_vector_paste = false;
    editor.remember_paste_view([500., 300.]);
    frame(
        &mut editor,
        &ctx,
        0.4,
        vec![Event::Paste(svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.project.resources.len(), 1);
    assert_eq!(editor.project.resources[0].media_type, "image/png");
    let bitmap = editor
        .selected_element()
        .expect("pasted SVG bitmap selected");
    match &bitmap.kind {
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } => {
            assert_eq!(resource_id, &editor.project.resources[0].id);
            assert_eq!((*width, *height), (80, 40));
        }
        other => panic!("expected Bitmap, got {other:?}"),
    }
    assert_eq!((bitmap.transform.x, bitmap.transform.y), (460., 280.));
    editor.undo();
    assert_eq!(editor.project, before);

    // 4. Unsafe SVG or locked destination layer fails atomically without adding Undo entries.
    let unsafe_svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><script>alert(1)</script></svg>";
    editor.history.clear();
    frame(
        &mut editor,
        &ctx,
        0.5,
        vec![Event::Paste(unsafe_svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Paste failed:"));
}

#[test]
fn new_stage_text_and_selected_text_accept_plain_text_paste_and_focus_editor() {
    use crate::{
        app::Tool,
        tests::{rectangle_stage, stage_input_frame},
    };
    let (mut editor, ctx, page) = rectangle_stage();
    editor.set_tool(Tool::Text);
    for step in 1..4 {
        stage_input_frame(&mut editor, &ctx, step as f64 * 0.1, vec![]);
    }
    let click_pos = page.min + egui::vec2(120., 120.);
    stage_input_frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            Event::PointerMoved(click_pos),
            primary_button(click_pos, true),
        ],
    );
    stage_input_frame(
        &mut editor,
        &ctx,
        1.1,
        vec![primary_button(click_pos, false)],
    );
    let created = editor
        .selected_element()
        .expect("new point text stays selected")
        .clone();
    assert!(matches!(
        &created.kind,
        ElementKind::Text { text, .. } if text == "Your text"
    ));
    assert_eq!(
        ctx.data(|d| d.get_temp::<bool>(egui::Id::new("properties-tool-tab"))),
        None,
        "the contextual inspector no longer stores a Selection/Tool tab"
    );
    assert_eq!(
        ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-focus-text-edit"))),
        Some(true)
    );

    // Plain-text Paste while point text is selected replaces its text in one Undo step.
    let after_create = editor.project.clone();
    frame(
        &mut editor,
        &ctx,
        1.2,
        vec![Event::Paste("Midnight Express\r\nEpisode 3".into())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.status, "Pasted text.");
    match &editor.selected_element().unwrap().kind {
        ElementKind::Text { text, .. } => {
            assert_eq!(text, "Midnight Express Episode 3");
        }
        other => panic!("expected point Text, got {other:?}"),
    }
    editor.undo();
    assert_eq!(editor.project, after_create);

    // Rendering Properties when focus is requested focuses the point-text editor and selects all text.
    editor.focus_selected_text_editor(&ctx);
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1280., 900.),
            )),
            time: Some(1.3),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.properties(ui, ctx);
            });
        },
    );
    assert!(
        ctx.wants_keyboard_input(),
        "Properties text editor should receive keyboard focus"
    );
}

#[test]
fn t318_cross_tab_copy_cut_paste_and_paste_in_place_transitively_deduplicate_all_assets() {
    use crate::panels::font_embedding::EmbeddedFontEntry;
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    let png_bytes = |rgba: [u8; 4]| -> Vec<u8> {
        let mut img = RgbaImage::new(4, 4);
        for px in img.pixels_mut() {
            *px = Rgba(rgba);
        }
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    };

    let ctx = egui::Context::default();
    let mut donor = Project::blank();
    donor.format_version = 12;

    // Donor resources: hero bitmap, tile bitmap-fill texture, embedded font + metadata, video AVI,
    // plus an UNREFERENCED heavy resource and donor publish settings that must NOT leak to Tab B!
    let hero_png = png_bytes([20, 140, 240, 255]);
    let tile_png = png_bytes([240, 180, 30, 255]);
    let unused_png = png_bytes([99, 99, 99, 255]);
    donor.resources.push(
        ResourceUnit::from_bytes("res-hero", "assets/hero.png", "image/png", &hero_png).unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes("res-tile", "assets/tile.png", "image/png", &tile_png).unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes("res-unused", "assets/unused.png", "image/png", &unused_png)
            .unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-font-mono",
            "fonts/EmbeddedMono-Regular.ttf",
            "font/ttf",
            b"FAKE_TTF_BYTES_V1",
        )
        .unwrap(),
    );
    let font_entries = vec![EmbeddedFontEntry {
        id: "res-font-mono".into(),
        name: "EmbeddedMono Regular".into(),
        family: "EmbeddedMono".into(),
        style: "Regular".into(),
        ranges: [true, true, true, false, false, false, false, false],
        extra_chars: String::new(),
        export_for_actionscript: false,
        class_name: String::new(),
    }];
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-font-meta",
            "fonts/embedding.json",
            "application/json",
            serde_json::to_string(&font_entries).unwrap().as_bytes(),
        )
        .unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-vid",
            "video/intro.mp4",
            "video/mp4",
            b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom",
        )
        .unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-donor-pub",
            "publish/settings.json",
            "application/json",
            br#"{"donor":true}"#,
        )
        .unwrap(),
    );

    // Donor global swatch & vector style.
    donor.set_global_swatches(&[GlobalSwatch {
        id: "sw-brand".into(),
        name: "Brand Gold".into(),
        color: [250, 200, 40, 255],
    }]);
    donor.set_vector_styles(&[VectorStyle {
        id: "st-card".into(),
        name: "Card Outline".into(),
        fill: [40, 60, 90, 255],
        stroke: [250, 200, 40, 255],
        stroke_width: 3.0,
        modifiers: Vec::new(),
    }]);

    // Nested symbols: `sym-inner` (9-slice + linkage + bitmap) inside `sym-outer` (IK armature + group).
    let mut inner_sym = donor.symbols[0].clone();
    inner_sym.id = "sym-inner".into();
    inner_sym.name = "InnerBadge".into();
    inner_sym.linkage_id = Some("HeroBadge".into());
    inner_sym.timeline.layers[0].id = "inner-layer".into();
    inner_sym.timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![Element {
            id: "inner-bmp".into(),
            name: "inner_bmp".into(),
            kind: ElementKind::Bitmap {
                resource_id: "res-hero".into(),
                width: 4,
                height: 4,
                smoothing: false,
            },
            ..Element::default()
        }],
        ..Default::default()
    }];
    donor.symbols.push(inner_sym);
    donor
        .symbol_mut("sym-inner")
        .unwrap()
        .set_scale9_grid(Some([1.0, 1.0, 2.0, 2.0]));
    donor.set_shared_library(
        "sym-inner",
        Some(SharedLibraryLinkage {
            symbol_id: "sym-inner".into(),
            url: "shared/badges.swf".into(),
            export_for_runtime: true,
            import_for_runtime: false,
            always_update: true,
        }),
    );

    let mut outer_sym = donor.symbols[0].clone();
    outer_sym.id = "sym-outer".into();
    outer_sym.name = "OuterCard".into();
    outer_sym.timeline.layers[0].id = "outer-layer".into();
    outer_sym.timeline.layers[0].properties.groups = vec![AuthoringGroup {
        id: "grp-outer".into(),
        name: "BadgeGroup".into(),
        members: vec!["outer-inst".into()],
    }];
    outer_sym.timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![Element {
            id: "outer-inst".into(),
            name: "badge".into(),
            kind: ElementKind::Instance {
                symbol_id: "sym-inner".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        }],
        ik_armatures: vec![IkArmature {
            id: "ik-outer".into(),
            name: "Arm".into(),
            bones: vec![IkBone {
                id: "bone-0".into(),
                element_id: "outer-inst".into(),
                parent_bone_id: None,
                length: 20.0,
                min_angle: None,
                max_angle: None,
                stiffness: 1.0,
            }],
            effector_bone_id: "bone-0".into(),
            target_pos: [20.0, 0.0],
            pole_pos: None,
        }],
        ..Default::default()
    }];
    donor.symbols.push(outer_sym);

    // Stage elements on Donor root frame 0: nested Instance, Bitmap, bitmap-filled + styled Path,
    // embedded-font Text, and Video with keyframe `TimelineVideoSpan`.
    {
        let root_id = donor.root.clone();
        let root_layer = &mut donor.symbol_mut(&root_id).unwrap().timeline.layers[0];
        root_layer.properties.groups = vec![AuthoringGroup {
            id: "grp-stage".into(),
            name: "HeroBundle".into(),
            members: vec!["el-inst".into(), "el-styled".into()],
        }];
        let kf = &mut root_layer.keyframes[0];
        kf.elements = vec![
            Element {
                id: "el-inst".into(),
                name: "card_mc".into(),
                transform: Transform {
                    x: 40.0,
                    y: 50.0,
                    ..Default::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: "sym-outer".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            },
            Element {
                id: "el-bmp".into(),
                name: "hero_bmp".into(),
                transform: Transform {
                    x: 90.0,
                    y: 50.0,
                    ..Default::default()
                },
                kind: ElementKind::Bitmap {
                    resource_id: "res-hero".into(),
                    width: 4,
                    height: 4,
                    smoothing: false,
                },
                ..Element::default()
            },
            Element {
                id: "el-styled".into(),
                name: "tiled_shape".into(),
                transform: Transform {
                    x: 140.0,
                    y: 50.0,
                    ..Default::default()
                },
                kind: ElementKind::Path {
                    commands: rectangle_path_commands(30.0, 20.0, RectangleOptions::default()),
                    fill: [40, 60, 90, 255],
                    stroke: [250, 200, 40, 255],
                    stroke_width: 3.0,
                    fill_rule: FillRule::NonZero,
                },
                ..Element::default()
            },
            Element {
                id: "el-text".into(),
                name: "label_txt".into(),
                transform: Transform {
                    x: 40.0,
                    y: 110.0,
                    ..Default::default()
                },
                kind: ElementKind::Text {
                    text: "SCORE 100".into(),
                    font_family: "EmbeddedMono".into(),
                    font_size: 18.0,
                    color: [255, 255, 255, 255],
                },
                ..Element::default()
            },
            Element {
                id: "el-video".into(),
                name: "intro_vid".into(),
                transform: Transform {
                    x: 200.0,
                    y: 110.0,
                    ..Default::default()
                },
                kind: ElementKind::Video {
                    width: 64.0,
                    height: 48.0,
                },
                ..Element::default()
            },
        ];
        kf.set_bitmap_fill(AuthoredBitmapFill {
            element_id: "el-styled".into(),
            resource_id: "res-tile".into(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            repeat: true,
            smoothing: true,
        });
        kf.set_appearance(ElementAppearance {
            element_id: "el-styled".into(),
            fill_swatch_id: Some("sw-brand".into()),
            stroke_swatch_id: None,
            style_id: Some("st-card".into()),
            entries: Vec::new(),
            modifiers: Vec::new(),
            text_binding: None,
        });
        kf.video = Some(TimelineVideoSpan {
            resource: "res-vid".into(),
            ..Default::default()
        });
    }
    donor.upgrade_format_for_features();
    donor.validate().expect("donor project must validate");

    // Build Recipient (Tab B) with conflicting `publish/settings.json`, colliding `res-hero` ID/path
    // with DIFFERENT pixel bytes, and colliding `OuterCard` / `HeroBadge` symbol!
    let mut recipient = Project::blank();
    let existing_hero_png = png_bytes([10, 220, 80, 255]);
    recipient.resources.push(
        ResourceUnit::from_bytes(
            "res-hero",
            "assets/hero.png",
            "image/png",
            &existing_hero_png,
        )
        .unwrap(),
    );
    recipient.resources.push(
        ResourceUnit::from_bytes(
            "res-dest-pub",
            "publish/settings.json",
            "application/json",
            br#"{"recipient":true}"#,
        )
        .unwrap(),
    );
    let mut dest_conflict_sym = recipient.symbols[0].clone();
    dest_conflict_sym.id = "sym-outer".into();
    dest_conflict_sym.name = "OuterCard".into();
    dest_conflict_sym.linkage_id = Some("HeroBadge".into());
    dest_conflict_sym.timeline.layers[0].id = "dest-conflict-layer".into();
    dest_conflict_sym.timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![Element {
            id: "dest-only-rect".into(),
            name: "dest_only".into(),
            kind: ElementKind::Rectangle {
                width: 99.0,
                height: 77.0,
                fill: [12, 34, 56, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }],
        ..Default::default()
    }];
    recipient.symbols.push(dest_conflict_sym);
    crate::timeline_commands::upgrade_all_format_versions(&mut recipient);
    recipient
        .validate()
        .expect("recipient project must validate before paste");

    let mut editor = Editor::from_project(&ctx, donor);
    // Also copy only the Bitmap (`el-bmp`) first and verify cross-tab pasting into a fresh v1 `Project::blank()`
    // automatically upgrades `format_version` to >= 3!
    editor.select_single_id("el-bmp".into());
    let bmp_only_clip = copy_objects(&mut editor, &ctx);
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    assert_eq!(editor.project.format_version, 1);
    frame(
        &mut editor,
        &ctx,
        0.5,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(bmp_only_clip),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor
        .project
        .validate()
        .expect("cross-tab pasting a Bitmap into a v1 blank document must upgrade format_version and validate");
    assert!(editor.project.format_version >= 3);
    assert!(editor.switch_session_tab(&ctx, 0));

    editor.select_all();
    assert_eq!(editor.selection.targets.len(), 5);
    let clip_text = copy_objects(&mut editor, &ctx);

    // Switch to Tab B and paste via `⌘V` (view-centered) then `⇧⌘V` (Paste in Place).
    let tab_b_before = recipient.clone();
    editor.open_new_session_tab(&ctx, recipient, None);
    editor.project.validate().unwrap();
    editor.remember_paste_view([400.0, 300.0]);

    frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(clip_text.clone()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor
        .project
        .validate()
        .expect("Tab B must validate after first cross-tab paste");

    let symbols_after_first = editor.project.symbols.len();
    let resources_after_first = editor.project.resources.len();
    let swatches_after_first = editor.project.global_swatches().len();
    let styles_after_first = editor.project.vector_styles().len();

    // Verify Tab B's pre-existing assets and publish settings were NOT overwritten, and unreferenced
    // donor resource `assets/unused.png` was NOT copied!
    let dest_pub = editor
        .project
        .resources
        .iter()
        .find(|r| r.path == "publish/settings.json")
        .unwrap();
    assert_eq!(dest_pub.decoded_bytes().unwrap(), br#"{"recipient":true}"#);
    assert_eq!(
        editor
            .project
            .resources
            .iter()
            .filter(|r| r.path == "publish/settings.json")
            .count(),
        1
    );
    assert_eq!(
        editor
            .project
            .resources
            .iter()
            .find(|r| r.id == "res-hero")
            .unwrap()
            .decoded_bytes()
            .unwrap(),
        existing_hero_png,
        "Tab B's pre-existing res-hero bytes must remain untouched"
    );
    assert!(
        !editor
            .project
            .resources
            .iter()
            .any(|r| r.path.contains("unused.png")),
        "unreferenced heavy donor assets must not bloat clipboard or destination"
    );
    assert!(
        editor
            .project
            .resources
            .iter()
            .any(neoflash_core::is_font_resource_unit),
        "embedded font TTF resource must be copied transitively"
    );
    assert_eq!(swatches_after_first, 1);
    assert_eq!(styles_after_first, 1);
    // Recipient started with root + conflicting `sym-outer` (2 symbols); paste adds disambiguated `OuterCard 2` + `InnerBadge` = 4 symbols.
    assert_eq!(symbols_after_first, 4);

    // Second paste in Tab B via `⇧⌘V` (Paste in Place): MUST reuse all imported assets (zero duplicate bloat!)
    // and place elements at their exact authored coordinates!
    frame(
        &mut editor,
        &ctx,
        1.2,
        vec![
            key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT),
            Event::Paste(clip_text),
        ],
        Modifiers::COMMAND | Modifiers::SHIFT,
        None,
    );
    editor
        .project
        .validate()
        .expect("Tab B must validate after repeated Paste in Place");
    assert_eq!(
        editor.project.symbols.len(),
        symbols_after_first,
        "pasting twice into Tab B must reuse imported symbols without duplicate bloat"
    );
    assert_eq!(
        editor.project.resources.len(),
        resources_after_first,
        "pasting twice into Tab B must reuse imported media/font resources without duplicate bloat"
    );
    assert_eq!(editor.project.global_swatches().len(), swatches_after_first);
    assert_eq!(editor.project.vector_styles().len(), styles_after_first);

    let selected_in_place: Vec<(f64, f64)> = editor
        .sampled_targets()
        .into_iter()
        .map(|(_, el)| (el.transform.x, el.transform.y))
        .collect();
    assert!(selected_in_place.contains(&(40.0, 50.0)));
    assert!(selected_in_place.contains(&(200.0, 110.0)));

    // Undo x2 -> exact initial Tab B; Redo x2 -> valid at every step!
    editor.undo();
    editor.project.validate().unwrap();
    editor.undo();
    editor.project.validate().unwrap();
    assert_eq!(editor.project, tab_b_before);
    editor.redo();
    editor.project.validate().unwrap();
    editor.redo();
    editor.project.validate().unwrap();
    assert_eq!(editor.project.symbols.len(), symbols_after_first);
}

#[test]
fn t318_cross_tab_and_cross_scene_copy_cut_paste_frames_preserve_assets_and_side_tables() {
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    let mut img = RgbaImage::new(2, 2);
    for px in img.pixels_mut() {
        *px = Rgba([60, 180, 120, 255]);
    }
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();

    let pcm: Vec<u8> = (0..400usize)
        .flat_map(|i| (((i % 8) as i16 - 4) * 512).to_le_bytes())
        .collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), 8000, 16000, 2 | (16 << 16)] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);

    let ctx = egui::Context::default();
    let mut donor = Project::blank();
    donor.format_version = 9;
    donor
        .resources
        .push(ResourceUnit::from_bytes("res-tex", "assets/tex.png", "image/png", &png).unwrap());
    donor
        .resources
        .push(ResourceUnit::from_bytes("res-sfx", "audio/chime.wav", "audio/wav", &wav).unwrap());
    donor.set_global_swatches(&[GlobalSwatch {
        id: "sw-mint".into(),
        name: "Mint".into(),
        color: [60, 220, 160, 255],
    }]);

    let mut badge = donor.symbols[0].clone();
    badge.id = "sym-frame-badge".into();
    badge.name = "FrameBadge".into();
    badge.timeline.layers[0].id = "badge-layer".into();
    badge.timeline.layers[0].keyframes = vec![Keyframe {
        frame: 0,
        elements: vec![Element {
            id: "badge-bmp".into(),
            name: "bmp".into(),
            kind: ElementKind::Bitmap {
                resource_id: "res-tex".into(),
                width: 2,
                height: 2,
                smoothing: false,
            },
            ..Element::default()
        }],
        ..Default::default()
    }];
    donor.symbols.push(badge);

    {
        let root_id = donor.root.clone();
        let tl = &mut donor.symbol_mut(&root_id).unwrap().timeline;
        tl.frame_count = 6;
        tl.layers[0].properties.groups = vec![AuthoringGroup {
            id: "grp-frames".into(),
            name: "FramePair".into(),
            members: vec!["f-inst".into(), "f-path".into()],
        }];
        let kf = &mut tl.layers[0].keyframes[0];
        kf.sound = Some(FrameSound::new("res-sfx"));
        kf.elements = vec![
            Element {
                id: "f-inst".into(),
                name: "badge_inst".into(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-frame-badge".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            },
            Element {
                id: "f-path".into(),
                name: "textured_path".into(),
                kind: ElementKind::Path {
                    commands: rectangle_path_commands(24.0, 24.0, RectangleOptions::default()),
                    fill: [60, 220, 160, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                    fill_rule: FillRule::NonZero,
                },
                ..Element::default()
            },
        ];
        kf.set_bitmap_fill(AuthoredBitmapFill {
            element_id: "f-path".into(),
            resource_id: "res-tex".into(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            repeat: true,
            smoothing: false,
        });
        kf.set_appearance(ElementAppearance {
            element_id: "f-path".into(),
            fill_swatch_id: Some("sw-mint".into()),
            stroke_swatch_id: None,
            style_id: None,
            entries: Vec::new(),
            modifiers: Vec::new(),
            text_binding: None,
        });
    }
    donor.upgrade_format_for_features();
    donor.validate().unwrap();

    let mut editor = Editor::from_project(&ctx, donor);
    let src_layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![src_layer_id],
        start: 0,
        end: 2,
    });
    assert!(editor.copy_frame_range(false));

    // Open Tab B, create Scene 2, and Paste Frames into Scene 2!
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    editor.new_scene();
    assert_eq!(editor.scene_list().len(), 2);
    let scene2_before_paste = editor.project.clone();
    let dst_layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![dst_layer_id],
        start: 0,
        end: 0,
    });
    editor.paste_frame_range();
    editor
        .project
        .validate()
        .expect("cross-tab cross-scene Paste Frames must produce a valid project");
    assert_eq!(editor.history.undo_label(), Some("Paste frames"));
    assert!(
        editor
            .project
            .symbols
            .iter()
            .any(|s| s.name == "FrameBadge"),
        "nested symbol must be imported by cross-tab Paste Frames"
    );
    assert_eq!(editor.project.global_swatches().len(), 1);
    assert!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .sound
            .is_some()
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        1
    );

    let sym_count_after_first = editor.project.symbols.len();
    let res_count_after_first = editor.project.resources.len();

    // Add a second layer in Scene 2 and Paste Frames again: element IDs are remapped cleanly without
    // cross-layer duplicate IDs, and imported symbols/resources are reused without bloat!
    editor.insert_timeline_layer();
    let second_layer_id = editor.symbol().timeline.layers[editor.layer].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![second_layer_id],
        start: 0,
        end: 0,
    });
    editor.paste_frame_range();
    editor
        .project
        .validate()
        .expect("same-symbol cross-layer Paste Frames must remap colliding element IDs");
    assert_eq!(editor.project.symbols.len(), sym_count_after_first);
    assert_eq!(editor.project.resources.len(), res_count_after_first);

    // Undo all 3 steps (Paste 2, insert_timeline_layer, Paste 1) -> exact pre-paste Scene 2 state; Redo -> valid!
    editor.undo();
    editor.project.validate().unwrap();
    editor.undo();
    editor.project.validate().unwrap();
    editor.undo();
    editor.project.validate().unwrap();
    assert_eq!(editor.project, scene2_before_paste);
    editor.redo();
    editor.project.validate().unwrap();
}

#[test]
fn t318_review_regressions_vector_blend_overlapping_groups_external_video_class_name_and_textbox_runs()
 {
    use crate::panels::font_embedding::EmbeddedFontEntry;

    let ctx = egui::Context::default();
    let mut donor = Project::blank();
    donor.format_version = 12;

    // Finding 4a: Two distinct external video resources with empty `data_base64` (`intro.mp4` vs `outro.mp4`).
    donor.resources.push(
        ResourceUnit::from_bytes("vid-intro", "assets/video/intro.mp4", "video/mp4", &[]).unwrap(),
    );
    donor.resources.push(
        ResourceUnit::from_bytes("vid-outro", "assets/video/outro.mp4", "video/mp4", &[]).unwrap(),
    );

    // Finding 5: Embedded font referenced ONLY inside a `TextBox`'s substring `format_runs`.
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-subrun-ttf",
            "fonts/SubRunFont-Regular.ttf",
            "font/ttf",
            b"SUBRUN_TTF_BYTES",
        )
        .unwrap(),
    );
    let font_entries = vec![EmbeddedFontEntry {
        id: "res-subrun-ttf".into(),
        name: "SubRunFont Regular".into(),
        family: "SubRunFont".into(),
        style: "Regular".into(),
        ranges: [true, true, false, false, false, false, false, false],
        extra_chars: String::new(),
        export_for_actionscript: false,
        class_name: String::new(),
    }];
    donor.resources.push(
        ResourceUnit::from_bytes(
            "res-subrun-meta",
            "fonts/embedding.json",
            "application/json",
            serde_json::to_string(&font_entries).unwrap().as_bytes(),
        )
        .unwrap(),
    );

    // Finding 1 & 4b: Symbol `Hero` with `class_name: Some("game.Hero")`, `vector_blend.target_element_id`,
    // and two external video layers referencing `vid-intro` and `vid-outro`.
    let blend_shape_a = Element {
        id: "hero-blend-a".into(),
        name: "a".into(),
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [240, 80, 60, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let blend_shape_b = Element {
        id: "hero-blend-b".into(),
        name: "b".into(),
        transform: Transform {
            x: 40.0,
            y: 0.0,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [60, 140, 240, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    };
    let hero_sym = Symbol {
        id: "sym-hero".into(),
        name: "Hero".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        class_name: Some("game.Hero".into()),
        linkage_id: None,
        timeline: Timeline {
            frame_count: 4,
            layers: vec![
                Layer {
                    id: "hero-art".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![blend_shape_a.clone(), blend_shape_b.clone()],
                        vector_effects: vec![ElementVectorEffects {
                            element_id: "hero-blend-a".into(),
                            vector_blend: Some(VectorBlendSpec {
                                target_element_id: "hero-blend-b".into(),
                                spacing: VectorBlendSpacing::SpecifiedSteps(3),
                                align_to_path: false,
                                spine: None,
                            }),
                            ..ElementVectorEffects::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "hero-vid-1".into(),
                    name: "IntroVid".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "hero-vid-el-1".into(),
                            name: "v1".into(),
                            kind: ElementKind::Video {
                                width: 64.0,
                                height: 36.0,
                            },
                            ..Element::default()
                        }],
                        video: Some(TimelineVideoSpan::external(
                            "vid-intro",
                            "assets/video/intro.mp4",
                        )),
                        ..Keyframe::default()
                    }],
                },
                Layer {
                    id: "hero-vid-2".into(),
                    name: "OutroVid".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "hero-vid-el-2".into(),
                            name: "v2".into(),
                            kind: ElementKind::Video {
                                width: 64.0,
                                height: 36.0,
                            },
                            ..Element::default()
                        }],
                        video: Some(TimelineVideoSpan::external(
                            "vid-outro",
                            "assets/video/outro.mp4",
                        )),
                        ..Keyframe::default()
                    }],
                },
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
    };
    donor.symbols.push(hero_sym);

    let mut tb_format = TextFormat {
        font: "Arial".into(),
        size: 16.0,
        ..TextFormat::default()
    };
    tb_format.set_field_options(Some(AuthoredTextFieldOptions {
        format_runs: vec![TextFormatRun {
            start: 0,
            end: 5,
            format: TextFormat {
                font: "SubRunFont".into(),
                size: 16.0,
                ..TextFormat::default()
            },
        }],
        ..AuthoredTextFieldOptions::default()
    }));

    {
        let root_id = donor.root.clone();
        let tl = &mut donor.symbol_mut(&root_id).unwrap().timeline;
        tl.frame_count = 6;
        // Finding 2: Layer group `[g-e1, g-e2]` on frame 0, while frame 2 holds only `[g-e1]`,
        // plus a stage-level vector blend pair on frame 0 for cross-layer `Paste Frames`.
        tl.layers[0].properties.groups = vec![AuthoringGroup {
            id: "grp-overlap".into(),
            name: "Pair".into(),
            members: vec!["g-e1".into(), "g-e2".into()],
        }];
        let mut g_e1 = Element {
            id: "g-e1".into(),
            name: "inst".into(),
            kind: ElementKind::Instance {
                symbol_id: "sym-hero".into(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        };
        let g_e2 = Element {
            id: "g-e2".into(),
            name: "box_txt".into(),
            transform: Transform {
                x: 120.0,
                y: 20.0,
                ..Default::default()
            },
            kind: ElementKind::TextBox {
                text: "Hello world".into(),
                width: 140.0,
                height: 36.0,
                format: tb_format,
                word_wrap: true,
                multiline: false,
            },
            ..Element::default()
        };
        tl.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![g_e1.clone(), g_e2],
                vector_effects: vec![ElementVectorEffects {
                    element_id: "g-e1".into(),
                    vector_blend: Some(VectorBlendSpec {
                        target_element_id: "g-e2".into(),
                        spacing: VectorBlendSpacing::SpecifiedSteps(2),
                        align_to_path: false,
                        spine: None,
                    }),
                    ..ElementVectorEffects::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 2,
                elements: {
                    g_e1.transform.x = 30.0;
                    vec![g_e1]
                },
                ..Keyframe::default()
            },
        ];
    }
    crate::timeline_commands::upgrade_all_format_versions(&mut donor);
    donor.validate().expect("donor must validate");

    let mut editor = Editor::from_project(&ctx, donor);

    // 1. Finding 2: Same-layer `Paste Frames` of frame 2 (`[g-e1]`) onto frame 4 of the SAME layer
    // must NOT push an overlapping `[g-e1]` group when `g-e1` is already in `[g-e1, g-e2]`!
    let root_layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![root_layer_id.clone()],
        start: 2,
        end: 2,
    });
    assert!(editor.copy_frame_range(false));
    editor.select_frame_range(FrameRange {
        layers: vec![root_layer_id.clone()],
        start: 4,
        end: 4,
    });
    editor.paste_frame_range();
    editor.project.validate().expect(
        "same-layer Paste Frames with partial group member must not create overlapping groups",
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].properties.groups.len(),
        1,
        "existing [g-e1, g-e2] group must remain unique without overlapping singleton"
    );

    // 2. Finding 1 (Paste Frames): Copy frame 0 (which has `vector_blend.target_element_id = "g-e2"`)
    // and Paste Frames onto a NEW layer -> `target_element_id` must be remapped to the new layer's target ID!
    editor.select_frame_range(FrameRange {
        layers: vec![root_layer_id],
        start: 0,
        end: 0,
    });
    assert!(editor.copy_frame_range(false));
    editor.insert_timeline_layer();
    let layer2_id = editor.symbol().timeline.layers[editor.layer].id.clone();
    editor.select_frame_range(FrameRange {
        layers: vec![layer2_id],
        start: 0,
        end: 0,
    });
    editor.paste_frame_range();
    editor.project.validate().expect(
        "cross-layer Paste Frames with vector_blend must remap target_element_id and validate",
    );
    let pasted_kf = &editor.symbol().timeline.layers[editor.layer].keyframes[0];
    assert_eq!(
        pasted_kf.vector_effects[0]
            .vector_blend
            .as_ref()
            .unwrap()
            .target_element_id,
        pasted_kf.elements[1].id
    );
    editor.undo(); // undo paste_frame_range on layer2
    editor.undo(); // undo insert_timeline_layer

    // 3. Finding 5: Copy ONLY `g-e2` (`TextBox` with non-embedded base `"Arial"` and substring run `"SubRunFont"`)
    // and paste into a fresh blank Tab B -> `SubRunFont` TTF + `fonts/embedding.json` must be copied!
    editor.frame = 0;
    editor.select_single_id("g-e2".into());
    let textbox_clip = copy_objects(&mut editor, &ctx);
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(textbox_clip),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert!(
        editor
            .project
            .resources
            .iter()
            .any(neoflash_core::is_font_resource_unit),
        "TextBox substring format_runs embedded font TTF must be copied across tabs"
    );
    assert!(
        editor
            .project
            .resources
            .iter()
            .any(|r| r.path == "fonts/embedding.json"),
        "fonts/embedding.json must accompany TextBox substring format_runs embedded font"
    );

    // 4. Finding 1 + 4a + 4b: Switch back to Tab A, copy `g-e1` (`sym-hero` with `class_name: Some("game.Hero")`
    // and 2 external empty-bytes videos), also test `duplicate_selected_instance_symbol`, then paste into Tab C
    // which already has a geometry-identical `Hero` symbol with `class_name: None`!
    assert!(editor.switch_session_tab(&ctx, 0));
    editor.frame = 0;
    editor.select_single_id("g-e1".into());
    let hero_clip = copy_objects(&mut editor, &ctx);

    let dup_id = editor
        .duplicate_selected_instance_symbol()
        .expect("duplicate_selected_instance_symbol must succeed");
    editor
        .project
        .validate()
        .expect("duplicated symbol with vector_blend must remap target_element_id and validate");
    let dup_sym = editor.project.symbol(&dup_id).unwrap();
    let dup_kf = &dup_sym.timeline.layers[0].keyframes[0];
    assert_eq!(
        dup_kf.vector_effects[0]
            .vector_blend
            .as_ref()
            .unwrap()
            .target_element_id,
        dup_kf.elements[1].id
    );
    editor.undo();

    // Build Tab C with a geometry-identical `Hero` symbol WITHOUT `class_name` (and with the same 2 external videos).
    let mut tab_c = Project::blank();
    tab_c.format_version = 12;
    tab_c.resources.push(
        ResourceUnit::from_bytes("c-intro", "assets/video/intro.mp4", "video/mp4", &[]).unwrap(),
    );
    tab_c.resources.push(
        ResourceUnit::from_bytes("c-outro", "assets/video/outro.mp4", "video/mp4", &[]).unwrap(),
    );
    let mut unlinked_hero = editor.project.symbol("sym-hero").unwrap().clone();
    unlinked_hero.id = "c-hero-unlinked".into();
    unlinked_hero.class_name = None;
    unlinked_hero.timeline.layers[1].keyframes[0].video = Some(TimelineVideoSpan::external(
        "c-intro",
        "assets/video/intro.mp4",
    ));
    unlinked_hero.timeline.layers[2].keyframes[0].video = Some(TimelineVideoSpan::external(
        "c-outro",
        "assets/video/outro.mp4",
    ));
    tab_c.symbols.push(unlinked_hero);
    tab_c.validate().unwrap();

    let tab_c_idx = editor.open_new_session_tab(&ctx, tab_c, None);
    // Also paste into a clean Tab D first to prove the 2 empty-bytes external videos do NOT collapse into 1!
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    frame(
        &mut editor,
        &ctx,
        2.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(hero_clip.clone()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor
        .project
        .validate()
        .expect("Tab D cross-tab paste with 2 external videos and vector_blend must validate");
    let ext_video_paths: Vec<&str> = editor
        .project
        .resources
        .iter()
        .filter(|r| r.is_video())
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(
        ext_video_paths.len(),
        2,
        "two distinct external empty-data_base64 video resources must NOT collapse into one"
    );
    assert!(ext_video_paths.contains(&"assets/video/intro.mp4"));
    assert!(ext_video_paths.contains(&"assets/video/outro.mp4"));

    // Now switch to Tab C (`tab_c_idx`) and paste `hero_clip` twice:
    // - First paste MUST keep `class_name: Some("game.Hero")` as a separate symbol instead of conflating with `c-hero-unlinked` (`class_name: None`).
    // - Second paste MUST deduplicate against the imported `game.Hero` symbol (`canonicalize_timeline_for_dedup` remapping `vector_blend.target_element_id`).
    assert!(editor.switch_session_tab(&ctx, tab_c_idx));
    frame(
        &mut editor,
        &ctx,
        3.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(hero_clip.clone()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert!(
        editor
            .project
            .symbols
            .iter()
            .any(|s| s.class_name.as_deref() == Some("game.Hero")),
        "pasted symbol with class_name must NOT be conflated with unlinked symbol of identical geometry"
    );
    let syms_after_first = editor.project.symbols.len();
    assert_eq!(syms_after_first, 3); // root + c-hero-unlinked + imported Hero 2 (game.Hero)

    frame(
        &mut editor,
        &ctx,
        3.5,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(hero_clip),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(
        editor.project.symbols.len(),
        syms_after_first,
        "repeated paste of symbol with vector_blend and class_name must deduplicate cleanly"
    );
}
