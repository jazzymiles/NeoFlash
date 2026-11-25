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
    outer_sym.class_name = Some("game.OuterCard".into());
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
    let mut outer_pose_2 = outer_sym.timeline.layers[0].keyframes[0].clone();
    outer_pose_2.frame = 1;
    outer_pose_2.ik_armatures[0].target_pos = [24.0, 3.0];
    outer_sym.timeline.frame_count = outer_sym.timeline.frame_count.max(2);
    outer_sym.timeline.layers[0].keyframes.push(outer_pose_2);
    donor.symbols.push(outer_sym);

    // Program code on `OuterCard` may instantiate this exported definition by
    // linkage name even though no authored placement references it directly.
    let mut dynamic_sym = donor.symbols[0].clone();
    dynamic_sym.id = "sym-dynamic-badge".into();
    dynamic_sym.name = "DynamicBadge".into();
    dynamic_sym.linkage_id = Some("DynamicBadgeExport".into());
    dynamic_sym.timeline.layers[0].id = "dynamic-badge-layer".into();
    dynamic_sym.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "dynamic-badge-shape".into(),
        name: String::new(),
        kind: ElementKind::Rectangle {
            width: 12.0,
            height: 8.0,
            fill: [70, 210, 120, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    }];
    donor.symbols.push(dynamic_sym);

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
    // Recipient started with root + conflicting `sym-outer` (2 symbols); paste adds disambiguated
    // `OuterCard 2`, `InnerBadge`, and the program-reachable linkage-only `DynamicBadge`.
    assert_eq!(symbols_after_first, 5);
    assert!(
        editor
            .project
            .symbols
            .iter()
            .any(|symbol| symbol.name == "DynamicBadge")
    );
    let imported_outer = editor
        .project
        .symbols
        .iter()
        .find(|symbol| symbol.name == "OuterCard 2")
        .expect("disambiguated nested symbol");
    let imported_armature_ids: Vec<_> = imported_outer.timeline.layers[0]
        .keyframes
        .iter()
        .map(|key| key.ik_armatures[0].id.as_str())
        .collect();
    assert_eq!(imported_armature_ids.len(), 2);
    assert_eq!(imported_armature_ids[0], imported_armature_ids[1]);
    assert_ne!(imported_armature_ids[0], "ik-outer");

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
    badge.class_name = Some("game.FrameBadge".into());
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
    let mut dynamic_frame_symbol = donor.symbols[0].clone();
    dynamic_frame_symbol.id = "sym-frame-dynamic".into();
    dynamic_frame_symbol.name = "FrameDynamic".into();
    dynamic_frame_symbol.linkage_id = Some("FrameDynamicExport".into());
    dynamic_frame_symbol.timeline.layers[0].id = "frame-dynamic-layer".into();
    dynamic_frame_symbol.timeline.layers[0].keyframes[0].elements = vec![Element {
        id: "frame-dynamic-shape".into(),
        name: String::new(),
        kind: ElementKind::Rectangle {
            width: 6.0,
            height: 6.0,
            fill: [180, 90, 220, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        ..Element::default()
    }];
    donor.symbols.push(dynamic_frame_symbol);

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
        kf.ik_armatures = vec![IkArmature {
            id: "ik-frames".into(),
            name: "Frame rig".into(),
            bones: vec![IkBone {
                id: "frame-bone".into(),
                element_id: "f-inst".into(),
                parent_bone_id: None,
                length: 20.0,
                min_angle: None,
                max_angle: None,
                stiffness: 1.0,
            }],
            effector_bone_id: "frame-bone".into(),
            target_pos: [20.0, 0.0],
            pole_pos: None,
        }];
        let mut pose_2 = kf.clone();
        pose_2.frame = 2;
        pose_2.sound = None;
        pose_2.ik_armatures[0].target_pos = [24.0, 4.0];
        tl.layers[0].keyframes.push(pose_2);
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
    assert!(
        editor
            .project
            .symbols
            .iter()
            .any(|s| s.name == "FrameDynamic"),
        "linkage-only symbols reachable from copied program code must be imported"
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
    let pasted_armature_ids: Vec<_> = editor.symbol().timeline.layers[0]
        .keyframes
        .iter()
        .filter(|key| matches!(key.frame, 0 | 2))
        .map(|key| key.ik_armatures[0].id.as_str())
        .collect();
    assert_eq!(pasted_armature_ids.len(), 2);
    assert_eq!(pasted_armature_ids[0], pasted_armature_ids[1]);
    assert_ne!(pasted_armature_ids[0], "ik-frames");

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

#[test]
fn t326_stage_rich_text_copy_cut_paste_preserves_mixed_runs_and_survives_tab_switch() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.add_element(
        ElementKind::TextBox {
            text: "Bold👨‍👩‍👧‍👦 Blue".into(),
            width: 300.0,
            height: 80.0,
            format: neoflash_core::TextFormat {
                font: "Inter".into(),
                size: 16.0,
                color: 0x222222,
                ..neoflash_core::TextFormat::default()
            },
            word_wrap: true,
            multiline: true,
        },
        20.0,
        20.0,
    );
    // "Bold👨‍👩‍👧‍👦" = 4 + 11 = 15 UTF-16 units (bold red), " " = 15..16, "Blue" = 16..20 (italic blue).
    assert!(editor.select_stage_text_utf16_range(0, 15));
    assert!(
        editor
            .apply_stage_text_format_patch(&neoflash_core::TextFormatPatch {
                bold: Some(true),
                color: Some(0xdd1111),
                ..neoflash_core::TextFormatPatch::default()
            })
            .unwrap()
    );
    assert!(editor.select_stage_text_utf16_range(16, 20));
    assert!(
        editor
            .apply_stage_text_format_patch(&neoflash_core::TextFormatPatch {
                italic: Some(true),
                color: Some(0x1155dd),
                ..neoflash_core::TextFormatPatch::default()
            })
            .unwrap()
    );

    // Select mixed-run span "👨‍👩‍👧‍👦 Blue" (UTF-16 4..20) and Copy inside inline Stage text edit.
    assert!(editor.select_stage_text_utf16_range(4, 20));
    let tab_a_project_before_copy = editor.project.clone();
    let out_copy = frame(
        &mut editor,
        &ctx,
        1.0,
        vec![Event::Copy],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(
        copied(&out_copy),
        ["👨‍👩‍👧‍👦 Blue"],
        "Stage inline Copy must copy only the selected substring, never the whole TextBox element"
    );
    assert_eq!(editor.project, tab_a_project_before_copy);

    // Open a second document tab (Tab B): verify Tab A's text stays intact and inline edit focus does NOT leak to Tab B!
    editor.open_new_session_tab(&ctx, Project::blank(), None);
    assert_eq!(editor.active_session, 1);
    assert!(
        !editor.stage_text_edit.is_active(&editor),
        "inline Stage text edit focus must not leak across document tabs"
    );
    editor.add_element(
        ElementKind::TextBox {
            text: "Start End".into(),
            width: 300.0,
            height: 80.0,
            format: neoflash_core::TextFormat {
                font: "Georgia".into(),
                size: 18.0,
                color: 0x008844,
                ..neoflash_core::TextFormat::default()
            },
            word_wrap: true,
            multiline: true,
        },
        30.0,
        30.0,
    );
    assert!(editor.select_stage_text_utf16_range(0, 5));
    assert!(
        editor
            .apply_stage_text_format_patch(&neoflash_core::TextFormatPatch {
                underline: Some(true),
                ..neoflash_core::TextFormatPatch::default()
            })
            .unwrap()
    );
    // Place caret at UTF-16 index 6 (before "End") and Paste the rich mixed-run payload from Tab A!
    assert!(editor.place_stage_text_caret_utf16(6));
    let tab_b_before_paste = editor.project.clone();
    let undo_before_paste = editor.history.undo_len();
    frame(
        &mut editor,
        &ctx,
        2.0,
        vec![Event::Paste("👨‍👩‍👧‍👦 Blue".into())],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(
        editor.history.undo_len(),
        undo_before_paste + 1,
        "rich paste records cleanly as one Undo step"
    );
    let pasted_state =
        neoflash_core::TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
    assert_eq!(pasted_state.text, "Start 👨‍👩‍👧‍👦 BlueEnd");
    // Surrounding underline green "Start" (0..5) and green "End" (22..25) are preserved,
    // while pasted "👨‍👩‍👧‍👦" (6..17) is bold red and "Blue" (18..22) is italic blue!
    assert!(
        pasted_state
            .format_runs
            .iter()
            .any(|r| r.start == 0 && r.end == 5 && r.format.underline)
    );
    assert!(
        pasted_state
            .format_runs
            .iter()
            .any(|r| r.start == 6 && r.end == 17 && r.format.bold && r.format.color == 0xdd1111)
    );
    assert!(
        pasted_state
            .format_runs
            .iter()
            .any(|r| r.start == 18 && r.end == 22 && r.format.italic && r.format.color == 0x1155dd)
    );
    assert!(
        pasted_state
            .format_runs
            .iter()
            .any(|r| r.start == 22 && r.end == 25 && r.format.color == 0x008844)
    );
    editor.project.validate().unwrap();

    // One Undo reverts the entire rich paste cleanly.
    editor.undo();
    assert_eq!(editor.project, tab_b_before_paste);

    // Switch back to Tab A and test inline Cut on "Bold👨‍👩‍👧‍👦 " (0..16): preserves "Blue" italic run!
    assert!(editor.switch_session_tab(&ctx, 0));
    assert_eq!(editor.project, tab_a_project_before_copy);
    assert!(editor.select_stage_text_utf16_range(0, 16));
    let out_cut = frame(
        &mut editor,
        &ctx,
        3.0,
        vec![Event::Cut],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(copied(&out_cut), ["Bold👨‍👩‍👧‍👦 "]);
    let after_cut =
        neoflash_core::TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
    assert_eq!(after_cut.text, "Blue");
    assert!(after_cut.format.italic);
    assert_eq!(after_cut.format.color, 0x1155dd);
    editor.project.validate().unwrap();

    // Equal plain text can represent two different copies. The latest copy must
    // retain its formatting across tabs, while a later external write must not.
    assert!(editor.select_stage_text_utf16_range(0, 4));
    assert!(editor.copy_stage_text_selection(&ctx, false));
    let mut older = editor.clipboard.rich_text.clone().unwrap();
    older.generation = Some(7);
    assert!(
        editor
            .apply_stage_text_format_patch(&TextFormatPatch {
                bold: Some(true),
                italic: Some(false),
                color: Some(0xdd1111),
                ..TextFormatPatch::default()
            })
            .unwrap()
    );
    assert!(editor.select_stage_text_utf16_range(0, 4));
    assert!(editor.copy_stage_text_selection(&ctx, false));
    let latest = editor.clipboard.rich_text.clone().unwrap();
    assert_eq!(older.plain_text, latest.plain_text);
    assert_ne!(older.runs, latest.runs);
    assert_eq!(latest.generation, Some(0), "headless copy stays injected");
    assert!(editor.switch_session_tab(&ctx, 1));

    // Each retained cache can outlive its native clipboard write. A stale cache
    // nearer the caret must not hide the matching session/process candidate.
    let candidates: Vec<_> = (0..4)
        .map(|index| {
            let mut rich = latest.clone();
            rich.generation = Some(11 + index);
            rich.runs[0].format.color = 0x112233 + index as u32;
            rich
        })
        .collect();
    editor.stage_text_edit.copied_rich_text = Some(candidates[0].clone());
    editor.clipboard.rich_text = Some(candidates[1].clone());
    editor.sessions[0].as_mut().unwrap().clipboard.rich_text = Some(candidates[2].clone());
    for candidate in &candidates {
        assert_eq!(
            editor.cached_rich_text("Blue", candidate.generation, Some(candidates[3].clone())),
            Some(candidate.clone())
        );
    }
    for generation in [Some(15), None] {
        assert!(
            editor
                .cached_rich_text("Blue", generation, Some(candidates[3].clone()))
                .is_none()
        );
    }
    let mut unknown = latest.clone();
    unknown.generation = None;
    assert!(
        editor
            .cached_rich_text("Blue", None, Some(unknown))
            .is_none()
    );
    assert!(
        editor
            .cached_rich_text("different text", Some(14), Some(candidates[3].clone()))
            .is_none()
    );

    editor.stage_text_edit.copied_rich_text = Some(older.clone());
    editor.clipboard.rich_text = Some(older);
    editor.sessions[0].as_mut().unwrap().clipboard.rich_text = Some(latest);
    for (generation, expected_color, expected_bold) in [
        (Some(0), 0xdd1111, true),
        (Some(1), 0x008844, false),
        (None, 0x008844, false),
    ] {
        assert!(editor.select_stage_text_utf16_range(0, 9));
        let before = editor.project.clone();
        let undo_before = editor.history.undo_len();
        editor.paste_objects_with_generation(&ctx, "Blue", false, generation);
        let pasted = TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
        assert_eq!(pasted.text, "Blue");
        for offset in 0..4 {
            let format = pasted.format_at_utf16_offset(offset);
            assert_eq!(format.color, expected_color, "generation={generation:?}");
            assert_eq!(format.bold, expected_bold, "generation={generation:?}");
            assert!(!format.italic, "stale blue italic runs must not return");
            if generation != Some(0) {
                assert_eq!(format.font, "Georgia", "plain paste uses destination style");
            }
        }
        assert_eq!(editor.history.undo_len(), undo_before + 1);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
}

#[test]
fn rich_clipboard_stage_copy_after_paste_snapshot_keeps_payload_style_and_skips_idle_reads() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let destination_format = TextFormat {
        font: "Georgia".into(),
        color: 0x008844,
        ..TextFormat::default()
    };
    editor.add_element(
        ElementKind::TextBox {
            text: "Base Same".into(),
            width: 300.,
            height: 80.,
            format: destination_format.clone(),
            word_wrap: true,
            multiline: true,
        },
        20.,
        20.,
    );
    assert!(editor.select_stage_text_utf16_range(5, 9));
    assert!(
        editor
            .apply_stage_text_format_patch(&TextFormatPatch {
                bold: Some(true),
                color: Some(0xdd1111),
                ..TextFormatPatch::default()
            })
            .unwrap()
    );
    assert!(editor.select_stage_text_utf16_range(5, 9));
    editor.stage_text_edit.typing_format = Some(destination_format);
    editor.history.clear();
    let before = editor.project.clone();
    let reads = std::cell::Cell::new(0);

    let output = ctx.run(
        egui::RawInput {
            events: vec![Event::Copy, Event::Paste("Same".into())],
            ..Default::default()
        },
        |ctx| {
            // Match shortcuts(): snapshot native Paste metadata before the
            // Stage key handler consumes Copy, then dispatch the saved payload.
            let read_generation = || {
                reads.set(reads.get() + 1);
                assert!(
                    ctx.output(|output| output.commands.is_empty()),
                    "snapshot the already-captured Paste before Copy writes a new clipboard"
                );
                Some(7)
            };
            let generation = read_generation();
            editor.handle_stage_text_edit_keys(ctx);
            assert_eq!(
                editor.clipboard.rich_text.as_ref().unwrap().generation,
                Some(0)
            );
            editor.clipboard_events_with_generation(ctx, || generation);
        },
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(copied(&output), ["Same"]);
    let freshly_copied = editor.clipboard.rich_text.as_ref().unwrap();
    assert_eq!(freshly_copied.generation, Some(0));
    assert!(freshly_copied.runs[0].format.bold);
    assert_eq!(freshly_copied.runs[0].format.color, 0xdd1111);
    let pasted = TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
    assert_eq!(pasted.text, "Base Same");
    for offset in 5..9 {
        let format = pasted.format_at_utf16_offset(offset);
        assert!(
            !format.bold,
            "earlier Paste must not inherit the new Copy's runs"
        );
        assert_eq!(format.color, 0x008844);
    }
    assert_eq!(editor.history.undo_len(), 1);
    editor.project.validate().unwrap();
    editor.undo();
    assert_eq!(editor.project, before);

    let _ = ctx.run(
        egui::RawInput {
            events: vec![Event::PointerMoved(Pos2::new(70., 80.))],
            ..Default::default()
        },
        |ctx| {
            editor.clipboard_events_with_generation(ctx, || {
                panic!("a frame without Paste must not read clipboard metadata")
            });
        },
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
}

#[allow(clippy::too_many_lines)]
fn build_editor_figma_clipboard_html(
    origin_x: f32,
    origin_y: f32,
    escaped: bool,
) -> (String, Vec<u8>) {
    fn deflate_stored(raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let chunks: Vec<&[u8]> = if raw.is_empty() {
            vec![&[]]
        } else {
            raw.chunks(65_535).collect()
        };
        for (idx, chunk) in chunks.iter().enumerate() {
            let is_last = idx + 1 == chunks.len();
            out.push(u8::from(is_last));
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
        out
    }
    fn zstd_raw_frame(raw: &[u8]) -> Vec<u8> {
        let mut out = vec![0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x40];
        let chunks: Vec<&[u8]> = if raw.is_empty() {
            vec![&[]]
        } else {
            raw.chunks(131_071).collect()
        };
        for (idx, chunk) in chunks.iter().enumerate() {
            let is_last = u32::from(idx + 1 == chunks.len());
            let hdr = ((chunk.len() as u32) << 3) | is_last;
            out.extend_from_slice(&hdr.to_le_bytes()[..3]);
            out.extend_from_slice(chunk);
        }
        out
    }
    fn b64_encode(bytes: &[u8]) -> String {
        ResourceUnit::from_bytes("tmp", "tmp.bin", "application/octet-stream", bytes)
            .unwrap()
            .data_base64
            .to_string()
    }

    fn wu(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let mut b = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }
    fn wi(out: &mut Vec<u8>, v: i64) {
        wu(out, ((v << 1) ^ (v >> 63)) as u64);
    }
    fn wf(out: &mut Vec<u8>, f: f32) {
        if f == 0.0 {
            out.push(0);
        } else {
            out.extend_from_slice(&f.to_bits().rotate_left(9).to_le_bytes());
        }
    }
    fn ws(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }

    // Schema: 0=GUID, 1=ParentIndex, 2=Vector, 3=Matrix, 4=Color, 5=ColorStop, 6=Paint,
    // 7=Path, 8=FontName, 9=TextData, 10=SymbolOverride, 11=SymbolData, 12=Effect, 13=NodeChange, 14=Blob, 15=Message
    let mut schema = Vec::new();
    wu(&mut schema, 16);
    ws(&mut schema, "GUID");
    schema.push(1);
    wu(&mut schema, 2);
    for (i, n) in ["sessionID", "localID"].iter().enumerate() {
        ws(&mut schema, n);
        wi(&mut schema, -4);
        schema.push(0);
        wu(&mut schema, (i + 1) as u64);
    }
    ws(&mut schema, "ParentIndex");
    schema.push(1);
    wu(&mut schema, 2);
    ws(&mut schema, "guid");
    wi(&mut schema, 0);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "position");
    wi(&mut schema, -6);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "Vector");
    schema.push(1);
    wu(&mut schema, 2);
    for (i, n) in ["x", "y"].iter().enumerate() {
        ws(&mut schema, n);
        wi(&mut schema, -5);
        schema.push(0);
        wu(&mut schema, (i + 1) as u64);
    }
    ws(&mut schema, "Matrix");
    schema.push(1);
    wu(&mut schema, 6);
    for (i, n) in ["m00", "m01", "m02", "m10", "m11", "m12"]
        .iter()
        .enumerate()
    {
        ws(&mut schema, n);
        wi(&mut schema, -5);
        schema.push(0);
        wu(&mut schema, (i + 1) as u64);
    }
    ws(&mut schema, "Color");
    schema.push(1);
    wu(&mut schema, 4);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        ws(&mut schema, n);
        wi(&mut schema, -5);
        schema.push(0);
        wu(&mut schema, (i + 1) as u64);
    }
    ws(&mut schema, "ColorStop");
    schema.push(1);
    wu(&mut schema, 2);
    ws(&mut schema, "color");
    wi(&mut schema, 4);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "position");
    wi(&mut schema, -5);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "Paint");
    schema.push(2);
    wu(&mut schema, 3);
    ws(&mut schema, "type");
    wi(&mut schema, -6);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "color");
    wi(&mut schema, 4);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "stops");
    wi(&mut schema, 5);
    schema.push(1);
    wu(&mut schema, 3);
    ws(&mut schema, "Path");
    schema.push(2);
    wu(&mut schema, 2);
    ws(&mut schema, "windingRule");
    wi(&mut schema, -6);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "commandsBlob");
    wi(&mut schema, -4);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "FontName");
    schema.push(1);
    wu(&mut schema, 2);
    for (i, n) in ["family", "style"].iter().enumerate() {
        ws(&mut schema, n);
        wi(&mut schema, -6);
        schema.push(0);
        wu(&mut schema, (i + 1) as u64);
    }
    ws(&mut schema, "TextData");
    schema.push(2);
    wu(&mut schema, 1);
    ws(&mut schema, "characters");
    wi(&mut schema, -6);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "SymbolOverride");
    schema.push(2);
    wu(&mut schema, 2);
    ws(&mut schema, "guidPath");
    wi(&mut schema, 0);
    schema.push(1);
    wu(&mut schema, 1);
    ws(&mut schema, "textData");
    wi(&mut schema, 9);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "SymbolData");
    schema.push(2);
    wu(&mut schema, 2);
    ws(&mut schema, "symbolID");
    wi(&mut schema, 0);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "symbolOverrides");
    wi(&mut schema, 10);
    schema.push(1);
    wu(&mut schema, 2);
    ws(&mut schema, "Effect");
    schema.push(2);
    wu(&mut schema, 3);
    ws(&mut schema, "type");
    wi(&mut schema, -6);
    schema.push(0);
    wu(&mut schema, 1);
    ws(&mut schema, "visible");
    wi(&mut schema, -1);
    schema.push(0);
    wu(&mut schema, 2);
    ws(&mut schema, "radius");
    wi(&mut schema, -5);
    schema.push(0);
    wu(&mut schema, 3);
    ws(&mut schema, "NodeChange");
    schema.push(2);
    let nc = [
        (1u64, "guid", 0i64, 0u8),
        (2, "parentIndex", 1, 0),
        (3, "type", -6, 0),
        (4, "name", -6, 0),
        (5, "internalOnly", -1, 0),
        (6, "size", 2, 0),
        (7, "transform", 3, 0),
        (8, "fillPaints", 6, 1),
        (9, "strokePaints", 6, 1),
        (10, "strokeWeight", -5, 0),
        (11, "fillGeometry", 7, 1),
        (12, "cornerRadius", -5, 0),
        (13, "effects", 12, 1),
        (14, "textData", 9, 0),
        (15, "fontName", 8, 0),
        (16, "fontSize", -5, 0),
        (17, "symbolData", 11, 0),
    ];
    wu(&mut schema, nc.len() as u64);
    for (tag, n, tid, arr) in nc {
        ws(&mut schema, n);
        wi(&mut schema, tid);
        schema.push(arr);
        wu(&mut schema, tag);
    }
    ws(&mut schema, "Blob");
    schema.push(2);
    wu(&mut schema, 1);
    ws(&mut schema, "bytes");
    wi(&mut schema, -2);
    schema.push(1);
    wu(&mut schema, 1);
    ws(&mut schema, "Message");
    schema.push(2);
    wu(&mut schema, 2);
    ws(&mut schema, "nodeChanges");
    wi(&mut schema, 13);
    schema.push(1);
    wu(&mut schema, 1);
    ws(&mut schema, "blobs");
    wi(&mut schema, 14);
    schema.push(1);
    wu(&mut schema, 2);

    // Bezier vector blob: 0..100 x 0..60
    let mut cmd_blob = Vec::new();
    cmd_blob.push(1);
    for f in [0.0f32, 0.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(2);
    for f in [100.0f32, 0.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(4);
    for f in [100.0f32, 40.0, 60.0, 60.0, 0.0, 60.0] {
        cmd_blob.extend_from_slice(&f.to_le_bytes());
    }
    cmd_blob.push(0);

    let write_solid = |m: &mut Vec<u8>, tag: u64, rgba: [f32; 4]| {
        wu(m, tag);
        wu(m, 1);
        wu(m, 1);
        ws(m, "SOLID");
        wu(m, 2);
        for c in rgba {
            wf(m, c);
        }
        wu(m, 0);
    };
    let write_matrix = |m: &mut Vec<u8>, tx: f32, ty: f32| {
        wu(m, 7);
        for v in [1.0f32, 0.0, tx, 0.0, 1.0, ty] {
            wf(m, v);
        }
    };

    // Total selection bounding box in Figma space:
    // x: origin_x .. origin_x + 240.0 (width = 240.0)
    // y: origin_y .. origin_y + 120.0 (height = 120.0)
    // Center in Figma space: (origin_x + 120.0, origin_y + 60.0)
    let mut msg = Vec::new();
    wu(&mut msg, 1);
    wu(&mut msg, 9);

    // 0: DOCUMENT 0:0
    wu(&mut msg, 1);
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    wu(&mut msg, 3);
    ws(&mut msg, "DOCUMENT");
    wu(&mut msg, 4);
    ws(&mut msg, "Doc");
    wu(&mut msg, 0);

    // 1: Internal Only Canvas 0:99
    wu(&mut msg, 1);
    wu(&mut msg, 0);
    wu(&mut msg, 99);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    ws(&mut msg, "!");
    wu(&mut msg, 3);
    ws(&mut msg, "CANVAS");
    wu(&mut msg, 4);
    ws(&mut msg, "Internal Only Canvas");
    wu(&mut msg, 5);
    msg.push(1);
    wu(&mut msg, 0);

    // 2: Master COMPONENT 9:1 ("BadgeComp", 120x40)
    wu(&mut msg, 1);
    wu(&mut msg, 9);
    wu(&mut msg, 1);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 99);
    ws(&mut msg, "a");
    wu(&mut msg, 3);
    ws(&mut msg, "COMPONENT");
    wu(&mut msg, 4);
    ws(&mut msg, "BadgeComp");
    wu(&mut msg, 6);
    wf(&mut msg, 120.0);
    wf(&mut msg, 40.0);
    write_matrix(&mut msg, 0.0, 0.0);
    write_solid(&mut msg, 8, [0.1, 0.2, 0.4, 1.0]);
    wu(&mut msg, 0);

    // 3: Master child TEXT 9:2 ("Default")
    wu(&mut msg, 1);
    wu(&mut msg, 9);
    wu(&mut msg, 2);
    wu(&mut msg, 2);
    wu(&mut msg, 9);
    wu(&mut msg, 1);
    ws(&mut msg, "a");
    wu(&mut msg, 3);
    ws(&mut msg, "TEXT");
    wu(&mut msg, 4);
    ws(&mut msg, "BadgeText");
    wu(&mut msg, 6);
    wf(&mut msg, 100.0);
    wf(&mut msg, 24.0);
    write_matrix(&mut msg, 10.0, 8.0);
    write_solid(&mut msg, 8, [1.0, 1.0, 1.0, 1.0]);
    wu(&mut msg, 14);
    wu(&mut msg, 1);
    ws(&mut msg, "Default");
    wu(&mut msg, 0);
    wu(&mut msg, 15);
    ws(&mut msg, "Inter");
    ws(&mut msg, "Bold");
    wu(&mut msg, 16);
    wf(&mut msg, 14.0);
    wu(&mut msg, 0);

    // 4: User CANVAS 0:1
    wu(&mut msg, 1);
    wu(&mut msg, 0);
    wu(&mut msg, 1);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    ws(&mut msg, "a");
    wu(&mut msg, 3);
    ws(&mut msg, "CANVAS");
    wu(&mut msg, 4);
    ws(&mut msg, "Page 1");
    wu(&mut msg, 0);

    // 5: VECTOR 1:1 at (origin_x, origin_y), 100x60, linear gradient + 2.5px stroke
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 1);
    ws(&mut msg, "a");
    wu(&mut msg, 3);
    ws(&mut msg, "VECTOR");
    wu(&mut msg, 4);
    ws(&mut msg, "GradPath");
    wu(&mut msg, 6);
    wf(&mut msg, 100.0);
    wf(&mut msg, 60.0);
    write_matrix(&mut msg, origin_x, origin_y);
    wu(&mut msg, 8);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    ws(&mut msg, "GRADIENT_LINEAR");
    wu(&mut msg, 3);
    wu(&mut msg, 2);
    for (rgba, pos) in [
        ([1.0f32, 0.3, 0.2, 1.0], 0.0f32),
        ([0.2, 0.5, 1.0, 1.0], 1.0),
    ] {
        for c in rgba {
            wf(&mut msg, c);
        }
        wf(&mut msg, pos);
    }
    wu(&mut msg, 0);
    write_solid(&mut msg, 9, [1.0, 1.0, 1.0, 1.0]);
    wu(&mut msg, 10);
    wf(&mut msg, 2.5);
    wu(&mut msg, 11);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    ws(&mut msg, "NONZERO");
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    wu(&mut msg, 0);

    // 6: RECTANGLE 1:2 at (origin_x + 120, origin_y), 120x60, cornerRadius=14 + LAYER_BLUR(9.0)
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 2);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 1);
    ws(&mut msg, "b");
    wu(&mut msg, 3);
    ws(&mut msg, "RECTANGLE");
    wu(&mut msg, 4);
    ws(&mut msg, "BlurPill");
    wu(&mut msg, 6);
    wf(&mut msg, 120.0);
    wf(&mut msg, 60.0);
    write_matrix(&mut msg, origin_x + 120.0, origin_y);
    write_solid(&mut msg, 8, [0.2, 0.8, 0.5, 1.0]);
    wu(&mut msg, 12);
    wf(&mut msg, 14.0);
    wu(&mut msg, 13);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    ws(&mut msg, "LAYER_BLUR");
    wu(&mut msg, 2);
    msg.push(1);
    wu(&mut msg, 3);
    wf(&mut msg, 9.0);
    wu(&mut msg, 0);
    wu(&mut msg, 0);

    // 7: TEXT 1:3 at (origin_x, origin_y + 80), 110x40, "Hero Title" Inter Bold Italic 22px
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 3);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 1);
    ws(&mut msg, "c");
    wu(&mut msg, 3);
    ws(&mut msg, "TEXT");
    wu(&mut msg, 4);
    ws(&mut msg, "TitleNode");
    wu(&mut msg, 6);
    wf(&mut msg, 110.0);
    wf(&mut msg, 40.0);
    write_matrix(&mut msg, origin_x, origin_y + 80.0);
    write_solid(&mut msg, 8, [0.95, 0.85, 0.25, 1.0]);
    wu(&mut msg, 14);
    wu(&mut msg, 1);
    ws(&mut msg, "Hero Title");
    wu(&mut msg, 0);
    wu(&mut msg, 15);
    ws(&mut msg, "Inter");
    ws(&mut msg, "Bold Italic");
    wu(&mut msg, 16);
    wf(&mut msg, 22.0);
    wu(&mut msg, 0);

    // 8: INSTANCE 1:4 at (origin_x + 120, origin_y + 80), 120x40 referencing 9:1 with override "VIP"
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 4);
    wu(&mut msg, 2);
    wu(&mut msg, 0);
    wu(&mut msg, 1);
    ws(&mut msg, "d");
    wu(&mut msg, 3);
    ws(&mut msg, "INSTANCE");
    wu(&mut msg, 4);
    ws(&mut msg, "BadgeInst");
    wu(&mut msg, 6);
    wf(&mut msg, 120.0);
    wf(&mut msg, 40.0);
    write_matrix(&mut msg, origin_x + 120.0, origin_y + 80.0);
    wu(&mut msg, 17);
    wu(&mut msg, 1);
    wu(&mut msg, 9);
    wu(&mut msg, 1);
    wu(&mut msg, 2);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, 9);
    wu(&mut msg, 2);
    wu(&mut msg, 2);
    wu(&mut msg, 1);
    ws(&mut msg, "VIP");
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    wu(&mut msg, 0);
    wu(&mut msg, 0);

    // blobs = [cmd_blob]
    wu(&mut msg, 2);
    wu(&mut msg, 1);
    wu(&mut msg, 1);
    wu(&mut msg, cmd_blob.len() as u64);
    msg.extend_from_slice(&cmd_blob);
    wu(&mut msg, 0);
    wu(&mut msg, 0);

    let c0 = deflate_stored(&schema);
    let c1 = if escaped {
        zstd_raw_frame(&msg)
    } else {
        deflate_stored(&msg)
    };
    let mut kiwi = Vec::new();
    kiwi.extend_from_slice(b"fig-kiwi");
    kiwi.extend_from_slice(&106u32.to_le_bytes());
    kiwi.extend_from_slice(&(c0.len() as u32).to_le_bytes());
    kiwi.extend_from_slice(&c0);
    kiwi.extend_from_slice(&(c1.len() as u32).to_le_bytes());
    kiwi.extend_from_slice(&c1);

    let meta_b64 = b64_encode(br#"{"fileKey":"EditorPaste","pasteID":"p-413","dataType":"scene"}"#);
    let kiwi_b64 = b64_encode(&kiwi);
    let html = if escaped {
        format!(
            "<meta charset=\"utf-8\"><span data-metadata=\"&lt;!--(figmeta){meta_b64}(/figmeta)--&gt;\"></span><span data-buffer=\"&lt;!--(figma){kiwi_b64}(/figma)--&gt;\"></span>"
        )
    } else {
        format!(
            "<meta charset=\"utf-8\"><span data-metadata=\"<!--(figmeta){meta_b64}(/figmeta)-->\"></span><span data-buffer=\"<!--(figma){kiwi_b64}(/figma)-->\"></span>"
        )
    };

    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 50, 50, 255]));
    let mut flat_png = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut flat_png),
        image::ImageFormat::Png,
    )
    .unwrap();
    (html, flat_png)
}

#[test]
fn t413_figma_clipboard_paste_centre_and_in_place_near_and_infinite_canvas_double_paste_and_undo() {
    use crate::clipboard::{StageClipboardFlavors, set_test_stage_clipboard_flavors};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    // Default Project::blank() is 960 x 600 -> center is (480.0, 300.0).
    let before = editor.project.clone();

    // 1. Copy from Figma near origin `(100, 70)` (bbox `240 x 120`, center `(220, 130)`) with BOTH
    //    `public.html` AND plain-text `"Hero Title"` AND flat preview PNG on the pasteboard!
    let (near_html, flat_png) = build_editor_figma_clipboard_html(100.0, 70.0, false);
    set_test_stage_clipboard_flavors(
        &ctx,
        Some(StageClipboardFlavors {
            generation: Some(0),
            plain_text: Some("Hero Title".into()),
            html: Some(near_html.clone()),
            svg: None,
            png: Some(flat_png.clone()),
            ..StageClipboardFlavors::default()
        }),
    );

    // 1a. `⌘V` (`Paste in centre`): translates top-level elements rigidly by `(480 - 220, 300 - 130) = (260, 170)`
    //     so GradPath lands at `(360, 240)` at exact `1:1` scale!
    frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste("Hero Title".into()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(
        editor.history.undo_len(),
        1,
        "Figma paste is 1 single Undo step"
    );
    assert!(
        editor.project.resources.is_empty(),
        "must NOT paste the flat preview PNG screenshot"
    );
    assert_eq!(
        editor.selection.targets.len(),
        4,
        "all 4 top-level pasted Figma objects selected"
    );

    let first_paste_el_ids: Vec<Id> = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let first_paste_sym_ids: Vec<Id> = editor
        .project
        .symbols
        .iter()
        .filter(|s| s.id != editor.project.root)
        .map(|s| s.id.clone())
        .collect();
    assert!(
        !first_paste_sym_ids.is_empty(),
        "nested BadgeComp variant symbol imported"
    );

    {
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        assert_eq!(els.len(), 4);
        let grad_path = els.iter().find(|e| e.name == "GradPath").unwrap();
        assert!((grad_path.transform.x - 360.0).abs() < 1e-6);
        assert!((grad_path.transform.y - 240.0).abs() < 1e-6);
        assert!((grad_path.transform.scale_x - 1.0).abs() < 1e-6);
        assert!((grad_path.transform.scale_y - 1.0).abs() < 1e-6);
        assert!(matches!(
            grad_path.kind,
            ElementKind::PaintedPath {
                stroke_width,
                fill_gradient: Some(_),
                ..
            } if (stroke_width - 2.5).abs() < 1e-6
        ));

        let blur_pill = els.iter().find(|e| e.name == "BlurPill").unwrap();
        assert!((blur_pill.transform.x - 480.0).abs() < 1e-6);
        assert!((blur_pill.transform.y - 240.0).abs() < 1e-6);
        assert!(matches!(
            blur_pill.kind,
            ElementKind::Rectangle { width, height, .. } if (width - 120.0).abs() < 1e-6 && (height - 60.0).abs() < 1e-6
        ));
        assert!(matches!(
            blur_pill.transform.primitive,
            Some(PrimitiveOptions::Rectangle(RectangleOptions {
                corner_radii: [14.0, 14.0, 14.0, 14.0],
                ..
            }))
        ));
        assert_eq!(
            blur_pill.filters,
            vec![FilterSpec::Blur {
                blur_x: 9.0,
                blur_y: 9.0,
                quality: 2,
            }]
        );

        let title_node = els.iter().find(|e| e.name == "TitleNode").unwrap();
        assert!(matches!(
            &title_node.kind,
            ElementKind::TextBox { text, format, .. }
                if text == "Hero Title" && format.font == "Inter" && format.bold && format.italic && (format.size - 22.0).abs() < 1e-6
        ));

        let badge_inst = els.iter().find(|e| e.name == "BadgeInst").unwrap();
        assert!(matches!(badge_inst.kind, ElementKind::Instance { .. }));
    }

    // 1b. Paste the EXACT SAME Figma clipboard payload a SECOND time in a row:
    //     produces distinct non-colliding `Element` & `Symbol` IDs and passes `validate()`!
    frame(
        &mut editor,
        &ctx,
        1.2,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste("Hero Title".into()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 2);
    {
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        assert_eq!(els.len(), 8);
        let second_el_ids: Vec<&Id> = els[4..].iter().map(|e| &e.id).collect();
        for eid in second_el_ids {
            assert!(
                !first_paste_el_ids.contains(eid),
                "second paste Element ID {eid} must not collide with first paste"
            );
        }
        let all_non_root_syms: Vec<&Id> = editor
            .project
            .symbols
            .iter()
            .filter(|s| s.id != editor.project.root)
            .map(|s| &s.id)
            .collect();
        assert_eq!(
            all_non_root_syms.len(),
            first_paste_sym_ids.len(),
            "second paste must deduplicate identical Figma component symbols instead of duplicating them"
        );
        let unique_syms: std::collections::HashSet<&&Id> = all_non_root_syms.iter().collect();
        assert_eq!(unique_syms.len(), all_non_root_syms.len());
    }

    // Two single-step `⌘Z` Undos restore exact `before`!
    editor.undo();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        4
    );
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());

    // 2. `⌘⇧V` (`Paste in place`) near-origin `(100, 70)` preserves exact `(100, 70)`!
    frame(
        &mut editor,
        &ctx,
        1.5,
        vec![
            key(Key::V, true, Modifiers::COMMAND | Modifiers::SHIFT),
            Event::Paste("Hero Title".into()),
        ],
        Modifiers::COMMAND | Modifiers::SHIFT,
        None,
    );
    editor.project.validate().unwrap();
    {
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        let grad_path = els.iter().find(|e| e.name == "GradPath").unwrap();
        assert!((grad_path.transform.x - 100.0).abs() < 1e-6);
        assert!((grad_path.transform.y - 70.0).abs() < 1e-6);
    }
    editor.undo();
    assert_eq!(editor.project, before);

    // 3. `⌘⇧V` (`Paste in place`) from far out on Figma's infinite canvas `(15000, -8000)`
    //    (HTML-escaped `&lt;!--(figma)...(/figma)--&gt;` AND NO plain-text flavor via `request_stage_clipboard_with`!)
    //    normalizes top-left to `(0, 0)` while keeping `1:1` relative layout!
    let (far_html, far_png) = build_editor_figma_clipboard_html(15_000.0, -8_000.0, true);
    set_test_stage_clipboard_flavors(&ctx, None);
    editor.request_stage_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
        Ok(StageClipboardFlavors {
            generation: Some(9),
            plain_text: None,
            html: Some(far_html),
            svg: None,
            png: Some(far_png),
            ..StageClipboardFlavors::default()
        })
    });
    editor.project.validate().unwrap();
    {
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        let grad_path = els.iter().find(|e| e.name == "GradPath").unwrap();
        let blur_pill = els.iter().find(|e| e.name == "BlurPill").unwrap();
        assert!(grad_path.transform.x.abs() < 1e-6);
        assert!(grad_path.transform.y.abs() < 1e-6);
        assert!((blur_pill.transform.x - 120.0).abs() < 1e-6);
        assert!(blur_pill.transform.y.abs() < 1e-6);
    }
    editor.undo();
    assert_eq!(editor.project, before);
}

#[test]
fn t413_locked_and_solo_hidden_layers_block_figma_and_svg_paste_and_svg_respects_text_focus() {
    use crate::clipboard::{StageClipboardFlavors, set_test_stage_clipboard_flavors};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.symbol_mut().timeline.layers.push(Layer {
        id: new_id("layer"),
        name: "Solo Peer".into(),
        visible: true,
        locked: false,
        keyframes: vec![Keyframe::default()],
        properties: Default::default(),
    });
    editor.layer = 0;
    editor.history.clear();
    let before = editor.project.clone();

    let (figma_html, flat_png) = build_editor_figma_clipboard_html(40.0, 50.0, false);
    let copy_as_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><path fill="#ff5533" stroke="#112233" stroke-width="2" d="M0 0L120 0L100 80L0 80Z"/></svg>"##;

    // 1. Locked layer auto-places Figma paste and SVG paste onto a new Normal layer, undone in 1 `⌘Z`!
    editor.symbol_mut().timeline.layers[0].locked = true;
    let locked_baseline = editor.project.clone();
    editor.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
        Ok(StageClipboardFlavors {
            generation: Some(1),
            plain_text: None,
            html: Some(figma_html.clone()),
            svg: None,
            png: Some(flat_png.clone()),
            ..StageClipboardFlavors::default()
        })
    });
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 3);
    editor.undo();
    assert_eq!(editor.project, locked_baseline);
    assert!(!editor.history.can_undo());
    editor.layer = 0;
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Paste(copy_as_svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 3);
    editor.undo();
    assert_eq!(editor.project, locked_baseline);
    assert!(!editor.history.can_undo());
    editor.symbol_mut().timeline.layers[0].locked = false;
    editor.layer = 0;
    assert_eq!(editor.project, before);

    // 2. Solo-hidden layer (`layer 1` is soloed while `editor.layer == 0`) auto-places Figma & SVG paste onto a solo-visible Normal layer, undone in 1 `⌘Z`!
    assert!(editor.toggle_layer_solo(1, true));
    assert!(!editor.layer_editable());
    editor.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
        Ok(StageClipboardFlavors {
            generation: Some(2),
            plain_text: Some("Hero Title".into()),
            html: Some(figma_html.clone()),
            svg: None,
            png: Some(flat_png.clone()),
            ..StageClipboardFlavors::default()
        })
    });
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 3);
    assert!(editor.is_layer_effectively_soloed(editor.layer));
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.layer = 0;
    frame(
        &mut editor,
        &ctx,
        0.4,
        vec![Event::Paste(copy_as_svg.to_owned())],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 3);
    editor.undo();
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    editor.layer = 0;
    editor.clear_all_solo_layers();
    assert!(editor.layer_editable());

    // 3. `"Copy as SVG"` on editable Stage layer pastes editable vector `Path` (preserving stroke_width=2, even with stroke at x=0!)
    //    instead of a `<svg>` TextBox or raster bitmap!
    frame(
        &mut editor,
        &ctx,
        0.6,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(copy_as_svg.to_owned()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert!(
        editor.project.resources.is_empty(),
        "must remain pure vector, not raster PNG"
    );
    let pasted_svg = editor
        .selected_element()
        .expect("pasted SVG vector selected");
    assert!(matches!(
        pasted_svg.kind,
        ElementKind::Path { stroke_width, .. } if (stroke_width - 2.0).abs() < 1e-6
    ));
    editor.undo();
    assert_eq!(editor.project, before);

    // 4. Pasting `<svg>` (even while Figma HTML is on the pasteboard!) inside `Actions` / focused text field
    //    inserts literal `<svg...>` text without touching Stage!
    set_test_stage_clipboard_flavors(
        &ctx,
        Some(StageClipboardFlavors {
            generation: Some(0),
            plain_text: Some(copy_as_svg.to_owned()),
            html: Some(figma_html.clone()),
            svg: Some(copy_as_svg.to_owned()),
            png: None,
            ..StageClipboardFlavors::default()
        }),
    );
    let mut actions_code = String::new();
    focus_and_select_text(&mut editor, &ctx, &mut actions_code);
    frame(
        &mut editor,
        &ctx,
        0.8,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(copy_as_svg.to_owned()),
        ],
        Modifiers::COMMAND,
        Some(&mut actions_code),
    );
    assert_eq!(actions_code, copy_as_svg);
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    ctx.memory_mut(|m| m.surrender_focus(field_id()));

    // 5. Pasting `<svg>` while actively inline-editing a Stage `TextBox` inserts `<svg...>` as text!
    editor.add_element(
        ElementKind::TextBox {
            text: "Code: ".into(),
            width: 320.0,
            height: 80.0,
            format: TextFormat::default(),
            word_wrap: true,
            multiline: true,
        },
        40.0,
        40.0,
    );
    assert!(editor.place_stage_text_caret_utf16(6));
    assert!(editor.is_stage_inline_text_editing());
    let elements_before = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .len();
    frame(
        &mut editor,
        &ctx,
        1.0,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(copy_as_svg.to_owned()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        elements_before,
        "inline Stage TextBox edit must NOT spawn new vector shapes on Stage"
    );
    let state = TextFieldState::from_kind(&editor.selected_sampled().unwrap().kind).unwrap();
    assert_eq!(state.text, format!("Code: {copy_as_svg}"));
    set_test_stage_clipboard_flavors(&ctx, None);
}

fn crafted_editor_png_ihdr_bomb(width: u32, height: u32) -> Vec<u8> {
    let crc32 = |bytes: &[u8]| -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    };
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::with_capacity(17);
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&ihdr);
    out.extend_from_slice(&crc32(&ihdr).to_be_bytes());
    let iend = b"IEND";
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(iend);
    out.extend_from_slice(&crc32(iend).to_be_bytes());
    out
}

#[test]
fn t414_p0_2_crafted_50000x50000_png_ihdr_rejected_across_bitmap_data_url_and_svg_paste() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let before = editor.project.clone();
    let bomb_png = crafted_editor_png_ihdr_bomb(50_000, 50_000);
    let bomb_b64 = ResourceUnit::from_bytes("tmp", "tmp.png", "image/png", &bomb_png)
        .unwrap()
        .data_base64;

    // 1. Direct `paste_image_bytes` rejects 50000x50000 header via `ImageReader::into_dimensions` before RGBA decode.
    assert!(!editor.paste_image_bytes(&ctx, &bomb_png, "image/png", false));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Paste failed:"));

    // 2. `data:image/png;base64,...` clipboard text rejected cleanly.
    let data_url = format!("data:image/png;base64,{bomb_b64}");
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![Event::Paste(data_url)],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Paste failed:"));

    // 3. `<svg><image href="data:image/png;base64,..."/></svg>` rejected via bounded `usvg` `image_href_resolver`.
    let bomb_svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image width="100" height="100" href="data:image/png;base64,{bomb_b64}"/></svg>"#
    );
    assert!(!editor.paste_svg(&ctx, &bomb_svg, false));
    assert_eq!(editor.project, before);
    assert!(!editor.history.can_undo());
    assert!(
        editor.status.contains("4096") || editor.status.starts_with("Paste failed:"),
        "unexpected status: {}",
        editor.status
    );
}

#[test]
fn t414_p0_4_multinode_figma_paste_with_unresolved_image_surfaces_warning_and_never_misbinds_preview()
 {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // Build a synthetic multi-node `ImportOutput` with an unresolved `figma-missing-image-asset` warning
    // (and verify end-to-end with `paste_figma_clipboard_output` surfacing status + Output log).
    let mut imported = Project::blank();
    imported
        .symbol_mut(&imported.root.clone())
        .unwrap()
        .timeline
        .layers[0]
        .keyframes[0]
        .elements
        .push(Element {
            id: new_id("el"),
            name: "CardShape".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [40, 80, 160, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        });
    let report = neoflash_import::ImportReport {
        source: "Figma clipboard".into(),
        format: neoflash_import::ImportFormat::Figma,
        version: None,
        can_import: true,
        statistics: neoflash_import::Statistics::default(),
        diagnostics: vec![neoflash_import::Diagnostic {
            severity: neoflash_import::Severity::Warning,
            code: "figma-missing-image-asset".into(),
            location: "Page 1 / Avatar".into(),
            message: "Missing bitmap asset 'deadbeef' on 'Avatar'; skipped image fill.".into(),
        }],
        binary_fla: None,
        swf: None,
        swc: None,
    };
    let out = neoflash_import::ImportOutput {
        project: imported,
        report,
        figma_bounds: Some(neoflash_core::PathBounds {
            x_min: 10.0,
            y_min: 20.0,
            x_max: 130.0,
            y_max: 100.0,
        }),
        local_bounds: Some(neoflash_core::PathBounds {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 120.0,
            y_max: 80.0,
        }),
        meta: None,
    };

    assert!(editor.paste_figma_clipboard_output(&ctx, out, true));
    editor.project.validate().unwrap();
    assert!(
        editor.status.contains("figma-missing-image-asset"),
        "status must surface missing-image warning code, got: {}",
        editor.status
    );
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("figma-missing-image-asset")),
        "Output panel log must record figma-missing-image-asset warning"
    );
}

#[test]
fn t414_p1_5_standalone_as_session_blocks_unfocused_stage_paste_and_menu_commands() {
    use crate::clipboard::{StageClipboardFlavors, set_test_stage_clipboard_flavors};

    let (mut editor, ctx) = fixture();
    let stage_payload = copy_objects(&mut editor, &ctx);
    let (figma_html, flat_png) = build_editor_figma_clipboard_html(20.0, 30.0, false);
    let svg_text = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40"><rect width="60" height="40" fill="#33aa55"/></svg>"##;

    let tmp_dir = std::env::temp_dir().join(format!("nf-t414-as-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let as_path = tmp_dir.join("standalone_guard.as");
    std::fs::write(&as_path, "trace('standalone');\n").unwrap();

    assert!(editor.try_open_path(as_path.clone(), &ctx));
    assert!(editor.is_standalone_script_session());
    ctx.memory_mut(|m| {
        if let Some(focused) = m.focused() {
            m.surrender_focus(focused);
        }
    });
    let dummy_before = editor.project.clone();

    // Menu states for Paste / Paste in Place / Cut / Copy are disabled when no text widget is focused!
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::EditPaste)
            .enabled
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::EditPasteInPlace)
            .enabled
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::EditCopy)
            .enabled
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::EditCut)
            .enabled
    );
    assert!(!editor.execute_menu_command(&ctx, CommandId::EditPaste));
    assert!(!editor.execute_menu_command(&ctx, CommandId::EditPasteInPlace));

    // Unfocused `⌘V` with Figma HTML, SVG, or Stage object payload never mutates dummy `editor.project`!
    set_test_stage_clipboard_flavors(
        &ctx,
        Some(StageClipboardFlavors {
            generation: Some(1),
            plain_text: Some("Hero Title".into()),
            html: Some(figma_html),
            svg: Some(svg_text.to_owned()),
            png: Some(flat_png),
            ..StageClipboardFlavors::default()
        }),
    );
    for payload in [stage_payload, svg_text.to_owned(), "Hero Title".to_owned()] {
        frame(
            &mut editor,
            &ctx,
            0.3,
            vec![key(Key::V, true, Modifiers::COMMAND), Event::Paste(payload)],
            Modifiers::COMMAND,
            None,
        );
        assert_eq!(editor.project, dummy_before);
        assert!(!editor.history.can_undo());
    }
    assert!(!editor.paste_svg(&ctx, svg_text, false));
    assert!(!editor.paste_image_bytes(
        &ctx,
        &crafted_editor_png_ihdr_bomb(2, 2),
        "image/png",
        false
    ));
    assert_eq!(editor.project, dummy_before);

    set_test_stage_clipboard_flavors(&ctx, None);
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn t414_p1_6_auto_normal_layer_placement_on_mask_guide_folder_and_hidden_with_1_undo() {
    let ctx = egui::Context::default();
    let svg_text = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="50"><rect width="80" height="50" fill="#4488ff"/></svg>"##;
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 200, 120, 255]));
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    let (figma_html, flat_png) = build_editor_figma_clipboard_html(10.0, 15.0, false);
    let figma_out = neoflash_import::import_figma_clipboard(&figma_html, Some(&flat_png)).unwrap();

    for mode in ["mask", "masked_normal", "guide", "folder", "hidden"] {
        let mut editor = Editor::from_project(&ctx, Project::blank());
        match mode {
            "mask" => {
                editor.symbol_mut().timeline.layers[0].properties.role = LayerRole::Mask;
            }
            "masked_normal" => {
                let mask_id = new_id("mask-layer");
                editor.symbol_mut().timeline.layers.push(Layer {
                    id: mask_id.clone(),
                    name: "Mask".into(),
                    visible: true,
                    locked: true,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        ..LayerProperties::default()
                    },
                });
                editor.symbol_mut().timeline.layers[0].properties.mask = Some(mask_id);
                editor.layer = 0;
            }
            "guide" => {
                editor.symbol_mut().timeline.layers[0].properties.role = LayerRole::Guide;
            }
            "folder" => {
                // Keep layer 0 Normal and add a Folder at layer 1 so project starts valid.
                editor.symbol_mut().timeline.layers.push(Layer {
                    id: new_id("folder"),
                    name: "Group Folder".into(),
                    visible: true,
                    locked: false,
                    keyframes: Vec::new(),
                    properties: LayerProperties {
                        role: LayerRole::Folder,
                        ..LayerProperties::default()
                    },
                });
                editor.layer = 1;
            }
            "hidden" => {
                editor.symbol_mut().timeline.layers[0].visible = false;
            }
            _ => unreachable!(),
        }
        crate::timeline_commands::upgrade_all_format_versions(&mut editor.project);
        editor.project.validate().unwrap();
        editor.history.clear();
        let before = editor.project.clone();

        // 1. Figma paste (with Instances/TextBoxes that would fail validation on Mask/Folder layers!)
        assert!(
            editor.paste_figma_clipboard_output(&ctx, figma_out.clone(), false),
            "Figma paste failed on mode={mode}: {}",
            editor.status
        );
        editor.project.validate().unwrap();
        let active_layer = &editor.symbol().timeline.layers[editor.layer];
        assert_eq!(active_layer.properties.role, LayerRole::Normal);
        assert!(active_layer.properties.mask.is_none());
        assert!(active_layer.visible && !active_layer.locked);
        editor.undo();
        assert_eq!(
            editor.project, before,
            "1 Undo must restore exact before on mode={mode}"
        );
        assert!(!editor.history.can_undo());

        // 2. SVG paste + 1 Undo
        if mode == "folder" {
            editor.layer = 1;
        } else {
            editor.layer = 0;
        }
        assert!(
            editor.paste_svg(&ctx, svg_text, false),
            "SVG paste failed on mode={mode}"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);

        // 3. PNG bitmap paste + 1 Undo
        if mode == "folder" {
            editor.layer = 1;
        } else {
            editor.layer = 0;
        }
        assert!(
            editor.paste_image_bytes(&ctx, &png_bytes, "image/png", false),
            "PNG paste failed on mode={mode}"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
    }
}

#[test]
fn t414_p1_7_paste_center_and_edit_in_place_inverse_transform_and_svg_viewbox() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // 1. `remember_paste_view([920.0, 640.0])` centers `⌘V` (`!in_place`) Figma paste at `(920, 640)`!
    editor.remember_paste_view([920.0, 640.0]);
    let (figma_html, flat_png) = build_editor_figma_clipboard_html(100.0, 70.0, false);
    let figma_out = neoflash_import::import_figma_clipboard(&figma_html, Some(&flat_png)).unwrap();
    assert!(editor.paste_figma_clipboard_output(&ctx, figma_out.clone(), false));
    editor.project.validate().unwrap();
    {
        // Local bounds of `build_editor_figma_clipboard_html` are `240 x 120` (center `(120, 60)`),
        // so `GradPath` (at local `(0, 0)`) lands at `(920 - 120, 640 - 60) = (800, 580)`!
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        let grad_path = els.iter().find(|e| e.name == "GradPath").unwrap();
        assert!((grad_path.transform.x - 800.0).abs() < 1e-6);
        assert!((grad_path.transform.y - 580.0).abs() < 1e-6);
    }
    editor.undo();

    // 2. Non-zero SVG `viewBox="100 200 120 80"` + `workspace.paste_in_place = true` lands at `(0, 0)`!
    editor.workspace.paste_in_place = true;
    let vb_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="100 200 120 80"><path fill="#ff4422" d="M100 200L220 200L220 280L100 280Z"/></svg>"##;
    assert!(editor.paste_svg(&ctx, vb_svg, false));
    editor.project.validate().unwrap();
    {
        let pasted = editor.selected_element().unwrap();
        assert!(
            (pasted.transform.x + 100.0).abs() < 1e-6 && (pasted.transform.y + 200.0).abs() < 1e-6,
            "SVG viewBox (100,200) must shift element by (-100,-200) so (100,200) lands at Stage (0,0), got {:?}",
            pasted.transform
        );
    }
    editor.workspace.paste_in_place = false;
    editor.undo();

    // 3. `Edit in Place` inside a translated `(200, 100)` + `2x` scaled MovieClip instance:
    //    `⌘⇧V` (`Paste in Place`) maps Stage `(100, 70)` through `inv([2,0,0,2,200,100])` -> local `(-50, -15)` at `0.5x` scale!
    let child_sym_id = new_id("mc");
    editor.project.symbols.push(Symbol {
        id: child_sym_id.clone(),
        name: "ScaledContainer".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    let inst_id = new_id("inst");
    let root_layer_id = editor.symbol().timeline.layers[0].id.clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: inst_id.clone(),
            name: "ContainerInst".into(),
            transform: Transform {
                x: 200.0,
                y: 100.0,
                scale_x: 2.0,
                scale_y: 2.0,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: child_sym_id.clone(),
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });
    editor.set_selection(vec![crate::selection::SelectionTarget {
        layer: root_layer_id,
        element: inst_id,
    }]);
    assert!(editor.enter_selected_symbol_in_place());
    assert!(editor.editing_in_place());

    assert!(editor.paste_figma_clipboard_output(&ctx, figma_out, true));
    editor.project.validate().unwrap();
    {
        let els = &editor.symbol().timeline.layers[0].keyframes[0].elements;
        let grad_path = els.iter().find(|e| e.name == "GradPath").unwrap();
        assert!(
            (grad_path.transform.x + 50.0).abs() < 1e-6
                && (grad_path.transform.y + 15.0).abs() < 1e-6,
            "Stage (100, 70) inside (200,100)@2x symbol must map to local (-50, -15), got {:?}",
            grad_path.transform
        );
        assert!(
            (grad_path.transform.scale_x - 0.5).abs() < 1e-6
                && (grad_path.transform.scale_y - 0.5).abs() < 1e-6,
            "1:1 Stage size inside 2x symbol must scale by 0.5x locally, got {:?}",
            grad_path.transform
        );
    }
}

#[test]
fn t414_p1_11_p1_12_root_mask_wraps_as_symbol_and_15x_paste_deduplicates_assets() {
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());

    // Build an imported Figma clipboard project whose root timeline has a `LayerRole::Mask` layer + bitmap resource.
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([80, 120, 240, 255]));
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    let res = ResourceUnit::from_bytes("res-1", "figma/hero.png", "image/png", &png_bytes).unwrap();

    let mask_layer_id = "mask-l".to_owned();
    let mut imported = Project::blank();
    imported.resources.push(res.clone());
    {
        let root_sym = imported.symbol_mut(&imported.root.clone()).unwrap();
        root_sym.timeline.layers = vec![
            Layer {
                id: "masked-photo".into(),
                name: "Photo".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-bmp".into(),
                        name: "Photo".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Bitmap {
                            resource_id: res.id.clone(),
                            width: 4,
                            height: 4,
                            smoothing: true,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role: LayerRole::Normal,
                    mask: Some(mask_layer_id.clone()),
                    ..LayerProperties::default()
                },
            },
            Layer {
                id: mask_layer_id,
                name: "ClipMask".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-mask".into(),
                        name: "MaskShape".into(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 4.0,
                            height: 4.0,
                            fill: [0, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    ..LayerProperties::default()
                },
            },
        ];
    }
    crate::timeline_commands::upgrade_all_format_versions(&mut imported);
    imported.validate().unwrap();
    let out = neoflash_import::ImportOutput {
        project: imported,
        report: neoflash_import::ImportReport {
            source: "Figma clipboard".into(),
            format: neoflash_import::ImportFormat::Figma,
            version: None,
            can_import: true,
            statistics: neoflash_import::Statistics::default(),
            diagnostics: Vec::new(),
            binary_fla: None,
            swf: None,
            swc: None,
        },
        figma_bounds: Some(neoflash_core::PathBounds {
            x_min: 10.0,
            y_min: 10.0,
            x_max: 14.0,
            y_max: 14.0,
        }),
        local_bounds: Some(neoflash_core::PathBounds {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 4.0,
            y_max: 4.0,
        }),
        meta: None,
    };

    // Paste 15 times: root mask MUST be preserved inside a wrapped MovieClip Instance,
    // AND both the bitmap ResourceUnit and wrapped MovieClip Symbol MUST deduplicate to count == 1!
    for _ in 0..15 {
        assert!(editor.paste_figma_clipboard_output(&ctx, out.clone(), false));
    }
    editor.project.validate().unwrap();
    assert_eq!(
        editor.project.resources.len(),
        1,
        "15x repeated paste must deduplicate identical bitmap ResourceUnit"
    );
    let non_root_syms: Vec<&Symbol> = editor
        .project
        .symbols
        .iter()
        .filter(|s| s.id != editor.project.root)
        .collect();
    assert_eq!(
        non_root_syms.len(),
        1,
        "15x repeated paste must deduplicate identical wrapped Figma symbol"
    );
    assert!(
        non_root_syms[0]
            .timeline
            .layers
            .iter()
            .any(|l| l.properties.role == LayerRole::Mask),
        "top-level Figma mask must be preserved inside wrapped MovieClip symbol"
    );
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        15
    );
}

#[test]
fn t414_p2_14_stage_inline_text_edit_ignores_object_payload_and_paste_in_place() {
    let (mut editor, ctx) = fixture();
    let object_payload = copy_objects(&mut editor, &ctx);

    editor.add_element(
        ElementKind::TextBox {
            text: "Inline".into(),
            width: 200.0,
            height: 50.0,
            format: TextFormat::default(),
            word_wrap: true,
            multiline: false,
        },
        60.0,
        60.0,
    );
    assert!(editor.place_stage_text_caret_utf16(6));
    assert!(editor.is_stage_inline_text_editing());
    editor.history.clear();
    let before = editor.project.clone();

    // Menu state: EditPaste enabled, EditPasteInPlace disabled during inline Stage text editing!
    assert!(
        editor
            .command_menu_state(Some(&ctx), CommandId::EditPaste)
            .enabled
    );
    assert!(
        !editor
            .command_menu_state(Some(&ctx), CommandId::EditPasteInPlace)
            .enabled
    );

    // Pressing `⌘V` or `⌘⇧V` while `object_payload` is on the clipboard must NOT duplicate Stage objects or exit inline edit!
    frame(
        &mut editor,
        &ctx,
        0.2,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(object_payload.clone()),
        ],
        Modifiers::COMMAND,
        None,
    );
    assert_eq!(editor.project, before);
    assert!(editor.is_stage_inline_text_editing());
    assert!(!editor.history.can_undo());

    editor.request_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
        Ok("IgnoredInPlace".into())
    });
    assert_eq!(editor.project, before);
    assert!(editor.is_stage_inline_text_editing());
}

#[test]
fn t415_editor_illustrator_pdf_and_eps_placement_edit_in_place_locked_layer_dedup_1_undo_and_bounds_rejection()
 {
    use crate::clipboard::{StageClipboardFlavors, set_test_stage_clipboard_flavors};

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank()); // 960 x 600

    // Build a realistic Illustrator PDF clipboard payload with `/MediaBox [0 0 960 600]` and
    // `/ArtBox [120 330 420 510]` -> Stage `[120, 90, 420, 270]` (300 x 180) containing:
    // - `/FmBadge Do` Form XObject,
    // - `2x2` RGBA `/SMask` Image XObject,
    // - `q ... W n ... Q` clipped path,
    // - unsupported `/ShMissing sh` operator so an approximation warning is emitted to `Output`!
    let build_pdf_with_artbox = |ax0: f64, ay0: f64, ax1: f64, ay1: f64| -> Vec<u8> {
        use std::io::Write as _;
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = [0usize; 9];
        offsets[1] = pdf.len();
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets[2] = pdf.len();
        pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offsets[3] = pdf.len();
        let _ = write!(
            pdf,
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 960 600] /ArtBox [{ax0} {ay0} {ax1} {ay1}] /Resources 5 0 R /Contents 4 0 R >>\nendobj\n"
        );
        let stream = format!(
            "0.9 0.2 0.3 rg {ax0} {ay0} 60 40 re f \
             q {} {} 50 50 re W n 0.1 0.7 0.4 rg {} {} 60 60 re f Q \
             q 1 0 0 1 {ax0} {ay0} cm /FmB Do Q \
             q 16 0 0 16 {} {ay0} cm /ImA Do Q \
             /ShMissing sh\n",
            ax0 + 80.0,
            ay0 + 10.0,
            ax0 + 75.0,
            ay0 + 5.0,
            ax0 + 160.0
        );
        offsets[4] = pdf.len();
        let _ = write!(
            pdf,
            "4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
            stream.len()
        );
        offsets[5] = pdf.len();
        pdf.extend_from_slice(b"5 0 obj\n<< /XObject << /FmB 6 0 R /ImA 7 0 R >> >>\nendobj\n");
        let fm_stream = b"0.2 0.4 0.9 rg 0 0 20 20 re f\n";
        offsets[6] = pdf.len();
        let _ = write!(
            pdf,
            "6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length {} >>\nstream\n",
            fm_stream.len()
        );
        pdf.extend_from_slice(fm_stream);
        pdf.extend_from_slice(b"endstream\nendobj\n");

        let rgb_raw = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
        offsets[7] = pdf.len();
        let _ = write!(
            pdf,
            "7 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 8 0 R /Length {} >>\nstream\n",
            rgb_raw.len()
        );
        pdf.extend_from_slice(&rgb_raw);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");

        let alpha_raw = [255u8, 128, 64, 255];
        offsets[8] = pdf.len();
        let _ = write!(
            pdf,
            "8 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
            alpha_raw.len()
        );
        pdf.extend_from_slice(&alpha_raw);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");

        let xref_pos = pdf.len();
        let _ = writeln!(pdf, "xref\n0 9\n0000000000 65535 f ");
        for &off in &offsets[1..9] {
            let _ = writeln!(pdf, "{off:010} 00000 n ");
        }
        let _ = write!(
            pdf,
            "trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n"
        );
        pdf
    };

    let near_pdf = build_pdf_with_artbox(120.0, 330.0, 420.0, 510.0);
    let far_pdf = build_pdf_with_artbox(25_000.0, -18_000.0, 25_300.0, -17_820.0);
    let aicb_eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%HiResBoundingBox: 60.0 340.0 260.0 500.0\n%AI5_ArtSize: 960 600\n0.9 0.3 0.1 XA 60 340 m 260 340 l 260 500 l 60 500 l closepath fill\n%%EOF\n".to_vec();

    let mut flat_png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 0, 200, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut flat_png),
            image::ImageFormat::Png,
        )
        .unwrap();

    let before = editor.project.clone();

    // 1. `⌘V` (`Paste`) with `com.adobe.pdf` + `AICB` EPS + flat PNG on pasteboard:
    //    PDF wins over EPS & PNG, centers `300x180` artwork at `remember_paste_view([700, 400])`
    //    -> top-left `(550, 310)`, emits warning to `Output`, and undoes in 1 `⌘Z`!
    editor.remember_paste_view([700.0, 400.0]);
    set_test_stage_clipboard_flavors(
        &ctx,
        Some(StageClipboardFlavors {
            generation: Some(0),
            pdf: Some(near_pdf.clone()),
            eps: Some(aicb_eps.clone()),
            png: Some(flat_png.clone()),
            ..StageClipboardFlavors::default()
        }),
    );
    frame(
        &mut editor,
        &ctx,
        0.5,
        vec![
            key(Key::V, true, Modifiers::COMMAND),
            Event::Paste(String::new()),
        ],
        Modifiers::COMMAND,
        None,
    );
    editor.project.validate().unwrap();
    assert_eq!(editor.history.undo_len(), 1);
    assert!(
        editor
            .output
            .iter()
            .any(|line| line.contains("pdf-unsupported-operator")),
        "PDF approximation warning must be logged to Output panel"
    );
    {
        let first_path = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Path { .. }))
            .unwrap();
        assert!((first_path.transform.x - 550.0).abs() < 1e-6);
        assert!((first_path.transform.y - 310.0).abs() < 1e-6);
    }
    editor.undo();
    assert_eq!(editor.project, before);

    // 2. Repeated `5x` PDF paste deduplicates `/Form` symbol, `Clipped Group` mask symbol, AND RGBA `ResourceUnit`!
    for _ in 0..5 {
        assert!(editor.paste_pdf_clipboard(&ctx, &near_pdf, true));
    }
    editor.project.validate().unwrap();
    assert_eq!(
        editor.project.resources.len(),
        1,
        "5x repeated PDF paste must deduplicate identical RGBA ResourceUnit"
    );
    let non_root_sym_count = editor
        .project
        .symbols
        .iter()
        .filter(|s| s.id != editor.project.root)
        .count();
    assert_eq!(
        non_root_sym_count, 2,
        "5x repeated PDF paste must deduplicate Clipped Group + FmB Form symbols to 2 total"
    );
    for _ in 0..5 {
        editor.undo();
    }
    assert_eq!(editor.project, before);

    // 3. Off-stage `/ArtBox` `(25_000, -18_000)` on `⌘⇧V` (`Paste in Place`) normalizes top-left to `(0, 0)`!
    assert!(editor.paste_pdf_clipboard(&ctx, &far_pdf, true));
    editor.project.validate().unwrap();
    {
        let first_path = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Path { .. }))
            .unwrap();
        assert!(first_path.transform.x.abs() < 1e-6 && first_path.transform.y.abs() < 1e-6);
    }
    editor.undo();
    assert_eq!(editor.project, before);

    // 4. Locked active layer auto-creates an unlocked `Normal` layer on BOTH PDF & EPS paste and undoes in 1 `⌘Z`!
    editor.symbol_mut().timeline.layers[0].locked = true;
    let locked_before = editor.project.clone();
    assert!(editor.paste_pdf_clipboard(&ctx, &near_pdf, true));
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    editor.undo();
    assert_eq!(editor.project, locked_before);

    editor.layer = 0;
    assert!(editor.paste_eps_clipboard(&ctx, &aicb_eps, true));
    editor.project.validate().unwrap();
    assert_eq!(editor.symbol().timeline.layers.len(), 2);
    {
        // HiResBoundingBox [60, 340, 260, 500] on 960x600 -> Stage top-left `(60, 100)`!
        let eps_el = &editor.symbol().timeline.layers[editor.layer].keyframes[0].elements[0];
        assert!((eps_el.transform.x - 60.0).abs() < 1e-6);
        assert!((eps_el.transform.y - 100.0).abs() < 1e-6);
    }
    editor.undo();
    assert_eq!(editor.project, locked_before);
    editor.symbol_mut().timeline.layers[0].locked = false;
    editor.layer = 0;
    assert_eq!(editor.project, before);

    // 5. `Edit in Place` inside a `(200, 100)` translated + `2x` scaled MovieClip container:
    //    `⌘⇧V` (`Paste in Place`) of PDF `/ArtBox` Stage `(120, 90)` maps through inverse `StageMapping`
    //    `[0.5, 0, 0, 0.5, -100, -50]` -> local `(-40, -5)` at `0.5x` scale!
    let mc_id = new_id("mc");
    editor.project.symbols.push(Symbol {
        id: mc_id.clone(),
        name: "EditInPlaceContainer".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    let inst_id = new_id("inst");
    let root_lid = editor.symbol().timeline.layers[0].id.clone();
    editor.symbol_mut().timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: inst_id.clone(),
            name: "Container".into(),
            transform: Transform {
                x: 200.0,
                y: 100.0,
                scale_x: 2.0,
                scale_y: 2.0,
                ..Transform::default()
            },
            kind: ElementKind::Instance {
                symbol_id: mc_id,
                playback: InstancePlayback::Independent,
            },
            ..Element::default()
        });
    editor.set_selection(vec![crate::selection::SelectionTarget {
        layer: root_lid,
        element: inst_id,
    }]);
    assert!(editor.enter_selected_symbol_in_place());
    assert!(editor.editing_in_place());
    editor.history.clear();
    let in_place_before = editor.project.clone();

    assert!(editor.paste_pdf_clipboard(&ctx, &near_pdf, true));
    editor.project.validate().unwrap();
    {
        let first_path = editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| matches!(e.kind, ElementKind::Path { .. }))
            .unwrap();
        assert!(
            (first_path.transform.x + 40.0).abs() < 1e-6
                && (first_path.transform.y + 5.0).abs() < 1e-6,
            "Stage (120, 90) inside (200,100)@2x symbol must map to local (-40, -5), got {:?}",
            first_path.transform
        );
        assert!(
            (first_path.transform.scale_x - 0.5).abs() < 1e-6
                && (first_path.transform.scale_y - 0.5).abs() < 1e-6
        );
    }
    editor.undo();
    assert_eq!(editor.project, in_place_before);
    assert!(editor.leave_symbol_to_level(0));

    // 6. Malformed PDF / malformed EPS / oversized NeoFlash SVG comment rejected with `0` Undo steps
    //    and NEVER fall back to flat PNG screenshot when corrupt vector flavor was present!
    editor.history.clear();
    let clean_state = editor.project.clone();
    editor.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
        Ok(StageClipboardFlavors {
            generation: Some(99),
            pdf: Some(b"%PDF-1.5\ncorrupt-not-a-valid-xref".to_vec()),
            png: Some(flat_png.clone()),
            ..StageClipboardFlavors::default()
        })
    });
    assert_eq!(editor.project, clean_state);
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Paste failed:"));

    let oversized_comment_svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><!--(neoflash){}(/neoflash)--></svg>",
        "A".repeat((neoflash_core::MAX_CLIPBOARD_BYTES * 4 / 3) + 64)
    );
    editor.paste_objects(&ctx, &oversized_comment_svg, false);
    assert_eq!(editor.project, clean_state);
    assert!(!editor.history.can_undo());
    assert!(editor.status.starts_with("Paste failed:"));

    // 7. Plain `⌘V` (`!in_place`) AND outbound `capture_stage_clipboard_bundle` during `Edit in Place`
    //    inside a `(200, 100) @ 2x` + `90°` rotated MovieClip container!
    {
        let mut eip_editor = Editor::from_project(&ctx, Project::blank());
        let rot_sym_id = new_id("rot-mc");
        let inner_el_id = new_id("inner-rect");
        let inner_layer_id = new_id("inner-layer");
        eip_editor.project.symbols.push(Symbol {
            id: rot_sym_id.clone(),
            name: "RotScaledContainer".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: inner_layer_id.clone(),
                    name: "Inner".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: inner_el_id.clone(),
                            name: "LocalBox".into(),
                            transform: Transform {
                                x: 10.0,
                                y: 20.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 30.0,
                                height: 15.0,
                                fill: [220, 80, 40, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        let c_inst_id = new_id("c-inst");
        let r_lid = eip_editor.symbol().timeline.layers[0].id.clone();
        eip_editor.symbol_mut().timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: c_inst_id.clone(),
                name: "RotContainerInst".into(),
                transform: Transform {
                    x: 200.0,
                    y: 100.0,
                    scale_x: 2.0,
                    scale_y: 2.0,
                    rotation: 90.0,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: rot_sym_id,
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            });
        eip_editor.set_selection(vec![crate::selection::SelectionTarget {
            layer: r_lid,
            element: c_inst_id,
        }]);
        assert!(eip_editor.enter_selected_symbol_in_place());
        assert!(eip_editor.editing_in_place());

        // 7a. Outbound `capture_stage_clipboard_bundle` while `editing_in_place()` exports PDF in world Stage space!
        eip_editor.set_selection(vec![crate::selection::SelectionTarget {
            layer: inner_layer_id,
            element: inner_el_id,
        }]);
        let (eip_bundle, _) = eip_editor.capture_stage_clipboard_bundle(&ctx).unwrap();
        assert!(
            ClipboardPayload::from_text(&eip_bundle.neoflash_text)
                .unwrap()
                .is_some()
        );
        assert!(
            eip_bundle
                .eps_bytes
                .as_deref()
                .unwrap()
                .starts_with(b"%!PS-Adobe")
        );
        assert!(eip_bundle.svg_text.as_deref().unwrap().contains("<svg"));
        let reimported = neoflash_import::import_pdf_clipboard(
            eip_bundle.pdf_bytes.as_deref().unwrap(),
            "eip-export.pdf",
        )
        .unwrap();
        // Local `[10..40, 20..35]` under `(200, 100) @ 2x, +90°` (`x' = 200 - 2*y, y' = 100 + 2*x`)
        // maps to Stage `[130..160, 120..180]` (`width = 30, height = 60`)!
        assert!(
            (reimported.stage_bounds[0] - 130.0).abs() < 2.0
                && (reimported.stage_bounds[1] - 120.0).abs() < 2.0
                && (reimported.stage_bounds[2] - 160.0).abs() < 2.0
                && (reimported.stage_bounds[3] - 180.0).abs() < 2.0,
            "outbound PDF from Edit in Place must use world Stage /ArtBox [130, 120, 160, 180], got {:?}",
            reimported.stage_bounds
        );

        // 7b. Plain `⌘V` (`!in_place`) inside `(200, 100) @ 2x, +90°` scales by `0.5x` and counter-rotates `-90°`!
        assert!(eip_editor.paste_pdf_clipboard(&ctx, &near_pdf, false));
        eip_editor.project.validate().unwrap();
        let pasted_plain_path = eip_editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .skip(1)
            .find(|e| matches!(e.kind, ElementKind::Path { .. }))
            .unwrap();
        assert!(
            (pasted_plain_path.transform.scale_x - 0.5).abs() < 1e-4
                && (pasted_plain_path.transform.scale_y - 0.5).abs() < 1e-4
                && ((pasted_plain_path.transform.rotation + 90.0).abs() < 1e-4
                    || (pasted_plain_path.transform.rotation - 270.0).abs() < 1e-4),
            "plain Paste inside 2x +90° Edit in Place symbol must apply 0.5x scale & -90° counter-rotation, got {:?}",
            pasted_plain_path.transform
        );
    }

    // 8. Copying & PDF-pasting 16 sibling `Instance`s (`export_clipboard_pdf` -> `paste_pdf_clipboard`)
    //    completes in milliseconds (`O(K)` lazy `/Form` resolution, zero `16!` hang)!
    {
        let mut sib_editor = Editor::from_project(&ctx, Project::blank());
        let root_lid = sib_editor.symbol().timeline.layers[0].id.clone();
        let mut sel = Vec::new();
        for i in 0..16 {
            let sid = format!("sib-sym-{i}");
            sib_editor.project.symbols.push(Symbol {
                id: sid.clone(),
                name: format!("SibSym{i}"),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: "L".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: new_id("el"),
                                name: "Chip".into(),
                                transform: Transform::default(),
                                kind: ElementKind::Rectangle {
                                    width: 12.0 + f64::from(i),
                                    height: 10.0,
                                    fill: [(i * 15) as u8, 120, 200, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                        properties: LayerProperties::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            let eid = new_id("sib-inst");
            sib_editor.symbol_mut().timeline.layers[0].keyframes[0]
                .elements
                .push(Element {
                    id: eid.clone(),
                    name: format!("Inst{i}"),
                    transform: Transform {
                        x: 20.0 + f64::from(i) * 30.0,
                        y: 40.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: sid,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
            sel.push(crate::selection::SelectionTarget {
                layer: root_lid.clone(),
                element: eid,
            });
        }
        sib_editor.set_selection(sel);
        let (sib_bundle, count) = sib_editor.capture_stage_clipboard_bundle(&ctx).unwrap();
        assert_eq!(count, 16);

        let mut dst_sib = Editor::from_project(&ctx, Project::blank());
        assert!(dst_sib.paste_pdf_clipboard(&ctx, sib_bundle.pdf_bytes.as_deref().unwrap(), true));
        dst_sib.project.validate().unwrap();
        assert_eq!(
            dst_sib.symbol().timeline.layers[0].keyframes[0]
                .elements
                .len(),
            16
        );
    }

    set_test_stage_clipboard_flavors(&ctx, None);
}
