//! End-to-end headless UI and loopback HTTP tests for `T-317` (`R-103` variant 4:
//! Stage `Bug?` chip, `"Beta feedback"` modal dialog, validation, session persistence, and
//! privacy-preserving `multipart/form-data` delivery).

use crate::{
    app::{Editor, Tool},
    panels::beta_feedback::{is_beta_build, set_beta_build_for_test},
    tests::primary_button,
    workspace::StageToolbarMode,
};
use eframe::egui::{self, Event, Key, Modifiers, Pos2, Rect, Vec2};
use neoflash_core::Project;
use neoflash_export::MAX_FEEDBACK_ATTACHMENT_BYTES;
use neoflash_ui::{components as c, tokens::space};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

struct BetaFlagGuard;

impl BetaFlagGuard {
    fn set(val: Option<bool>) -> Self {
        set_beta_build_for_test(val);
        Self
    }
}

impl Drop for BetaFlagGuard {
    fn drop(&mut self) {
        set_beta_build_for_test(None);
    }
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn valid_png() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R',
    ]
}

fn valid_jpeg() -> Vec<u8> {
    vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F']
}

fn valid_gif() -> Vec<u8> {
    b"GIF89a\x01\x00\x01\x00\x00\xff\x00,".to_vec()
}

fn valid_webp() -> Vec<u8> {
    b"RIFF\x14\x00\x00\x00WEBPVP8 \x08\x00\x00\x00".to_vec()
}

fn run_editor_frame(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: f64,
    events: Vec<Event>,
    dropped_files: Vec<egui::DroppedFile>,
) -> egui::FullOutput {
    ctx.enable_accesskit();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            time: Some(time),
            events,
            dropped_files,
            ..Default::default()
        },
        |ctx| {
            editor.poll_beta_feedback(ctx);
            editor.shortcuts(ctx);
            editor.menu(ctx);
            editor.status_bar(ctx);
            editor.bitmap_textures.begin_frame(&editor.project);
            editor.prepare_docked_panel_focus(ctx);
            if editor.has_document() {
                if !editor.panels_hidden {
                    editor.dock_grid(ctx);
                }
                editor.document_bar(ctx);
                editor.stage(ctx);
            }
            editor.draw_dialogs(ctx);
            editor.notification_host(
                ctx,
                editor.has_document().then_some(editor.stage_view.canvas),
            );
            crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
        },
    )
}

fn all_ax_bounds(output: &egui::FullOutput, label: &str) -> Vec<Rect> {
    let Some(update) = output.platform_output.accesskit_update.as_ref() else {
        return Vec::new();
    };
    let mut rects: Vec<Rect> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some(label))
        .filter_map(|(_, node)| {
            node.bounds().map(|r| {
                Rect::from_min_max(
                    Pos2::new(r.x0 as f32, r.y0 as f32),
                    Pos2::new(r.x1 as f32, r.y1 as f32),
                )
            })
        })
        .collect();
    rects.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then_with(|| a.left().total_cmp(&b.left()))
    });
    rects
}

fn ax_bounds(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    all_ax_bounds(output, label).into_iter().next()
}

fn shape_contains_text(shape: &egui::Shape, needle: &str) -> bool {
    match shape {
        egui::Shape::Text(text) => text.galley.job.text.contains(needle),
        egui::Shape::Vec(children) => children.iter().any(|s| shape_contains_text(s, needle)),
        _ => false,
    }
}

fn has_ax_substring(output: &egui::FullOutput, needle: &str) -> bool {
    if output
        .shapes
        .iter()
        .any(|clipped| shape_contains_text(&clipped.shape, needle))
    {
        return true;
    }
    let Some(update) = output.platform_output.accesskit_update.as_ref() else {
        return false;
    };
    update.nodes.iter().any(|(_, node)| {
        node.label().is_some_and(|l| l.contains(needle))
            || node.value().is_some_and(|v| v.contains(needle))
    })
}

fn warmup_dialog(editor: &mut Editor, ctx: &egui::Context, time: &mut f64) -> egui::FullOutput {
    *time += 0.02;
    run_editor_frame(editor, ctx, *time, Vec::new(), Vec::new());
    *time += 0.02;
    run_editor_frame(editor, ctx, *time, Vec::new(), Vec::new())
}

fn click_at(
    editor: &mut Editor,
    ctx: &egui::Context,
    time: &mut f64,
    pos: Pos2,
) -> egui::FullOutput {
    *time += 0.02;
    run_editor_frame(
        editor,
        ctx,
        *time,
        vec![Event::PointerMoved(pos), primary_button(pos, true)],
        Vec::new(),
    );
    *time += 0.02;
    run_editor_frame(
        editor,
        ctx,
        *time,
        vec![primary_button(pos, false)],
        Vec::new(),
    )
}

fn spawn_loopback_feedback_server(
    status_line: &'static str,
    json_body: &'static str,
) -> (String, thread::JoinHandle<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking loopback listener");
    let addr = listener.local_addr().expect("local addr");
    let handle = thread::spawn(move || {
        let accept_deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < accept_deadline,
                        "loopback feedback server timed out waiting for HTTP POST"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("accept feedback request failed: {err}"),
            }
        };
        stream
            .set_nonblocking(false)
            .expect("restore blocking stream");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let mut buf = Vec::new();
        let mut header_end = None;
        let mut content_length = 0usize;
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).expect("read request");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if header_end.is_none()
                && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
            {
                let end = pos + 4;
                header_end = Some(end);
                let hdr = String::from_utf8_lossy(&buf[..end]);
                for line in hdr.lines() {
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_length = v.trim().parse::<usize>().unwrap_or(0);
                    }
                }
            }
            if let Some(end) = header_end
                && buf.len() >= end + content_length
            {
                break;
            }
        }
        let end = header_end.unwrap_or(buf.len());
        let headers = String::from_utf8_lossy(&buf[..end]).into_owned();
        let body = buf[end..].to_vec();
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json_body}",
            json_body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        (headers, body)
    });
    (format!("http://{addr}/ajax/developer@redinger.cc"), handle)
}

#[test]
fn bug_chip_beta_visibility_ruler_offset_editbar_and_focus_on_click() {
    let _guard = BetaFlagGuard::set(None);
    assert!(
        is_beta_build(),
        "0.1.0-beta.1 workspace version must enable beta feedback chip"
    );

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let mut t = 0.0;

    // Frame 1 & 2 with rulers shown (NF2 default).
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    t += 0.02;
    let out_rulers = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    let canvas = editor.stage_view.canvas;
    let chip_rulers = ax_bounds(&out_rulers, "Bug?").expect("Bug? chip visible in beta build");
    let ruler_w = neoflash_ui::components::canvas::RULER;
    assert!(
        (chip_rulers.left() - (canvas.left() + ruler_w + space::S4)).abs() <= 1.5,
        "expected chip left at canvas.left + 20 + 16, got {} vs {}",
        chip_rulers.left(),
        canvas.left() + ruler_w + space::S4
    );
    assert!(
        (chip_rulers.bottom() - (canvas.bottom() - 10.0)).abs() <= 1.5,
        "expected chip bottom 10 pt above canvas.bottom, got {} vs {}",
        chip_rulers.bottom(),
        canvas.bottom() - 10.0
    );

    // Hide rulers: chip shifts 20 pt left (`canvas.left() + 16 pt`).
    editor.stage_view.prefs.rulers = false;
    t += 0.02;
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    t += 0.02;
    let out_no_rulers = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    let chip_no_rulers =
        ax_bounds(&out_no_rulers, "Bug?").expect("Bug? chip visible without rulers");
    assert!(
        (chip_no_rulers.left() - (canvas.left() + space::S4)).abs() <= 1.5,
        "expected chip left at canvas.left + 16 when rulers hidden"
    );
    assert!((chip_rulers.left() - chip_no_rulers.left() - ruler_w).abs() <= 1.5);

    // EditBar Stage toolbar mode still displays the bottom-left `Bug?` chip.
    editor.workspace.stage_toolbar = StageToolbarMode::EditBar;
    t += 0.02;
    let out_editbar = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    assert!(
        ax_bounds(&out_editbar, "Bug?").is_some(),
        "Bug? chip must remain visible in StageToolbarMode::EditBar"
    );

    // Release build override hides the chip completely.
    set_beta_build_for_test(Some(false));
    t += 0.02;
    let out_release = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    assert!(
        ax_bounds(&out_release, "Bug?").is_none(),
        "Release build must not show the Bug? chip"
    );

    // Restore beta build and click `Bug?`: opens dialog and immediately focuses the TextArea.
    set_beta_build_for_test(Some(true));
    editor.workspace.stage_toolbar = StageToolbarMode::Expanded;
    t += 0.02;
    let out_beta = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    let chip_center = ax_bounds(&out_beta, "Bug?").unwrap().center();
    click_at(&mut editor, &ctx, &mut t, chip_center);
    assert!(editor.beta_feedback.open);

    // Typing on the very next frame goes directly into `beta_feedback.text`.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Text("Focused immediately".into())],
        Vec::new(),
    );
    assert_eq!(editor.beta_feedback.text, "Focused immediately");
}

#[test]
fn typing_single_keys_space_and_enter_in_textarea_never_triggers_stage_shortcuts_or_closes() {
    let _guard = BetaFlagGuard::set(Some(true));
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.tool = Tool::Select;
    editor.playing = false;
    editor.open_beta_feedback();

    let mut t = 0.02;
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());

    // Type single-letter tool keys (`b`, `v`), `Space`, and `Enter` while the TextArea is focused.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![
            Event::Key {
                key: Key::B,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text("b".into()),
            Event::Key {
                key: Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text(" ".into()),
            Event::Key {
                key: Key::V,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text("v".into()),
            Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text("second line".into()),
        ],
        Vec::new(),
    );

    assert!(editor.beta_feedback.open, "Enter must not close the dialog");
    assert_eq!(
        editor.tool,
        Tool::Select,
        "B key must not switch Stage tool"
    );
    assert!(!editor.playing, "Enter must not toggle timeline playback");
    assert_eq!(editor.beta_feedback.text, "b v\nsecond line");
}

#[test]
fn send_disabled_when_empty_or_whitespace_even_with_images_and_cmd_enter_is_noop() {
    let _guard = BetaFlagGuard::set(Some(true));
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.open_beta_feedback();
    editor
        .beta_feedback
        .attach_bytes("screenshot.png", &valid_png());
    editor.beta_feedback.text = "   \n\t  ".into();

    let mut t = 0.02;
    let out = warmup_dialog(&mut editor, &ctx, &mut t);
    let send_rect = ax_bounds(&out, "Send").expect("Send button present");

    // Clicking disabled `Send` and pressing `⌘↵` must not start a submission.
    click_at(&mut editor, &ctx, &mut t, send_rect.center());
    assert!(!editor.beta_feedback.is_sending());
    assert!(editor.beta_feedback.open);

    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        }],
        Vec::new(),
    );
    assert!(!editor.beta_feedback.is_sending());
}

#[test]
fn attachments_via_paths_clipboard_paste_and_drag_drop_plus_validation_limit_and_remove() {
    let _guard = BetaFlagGuard::set(Some(true));
    let tmp = TempDirGuard::new("neoflash-t317-attachments");
    let png_path = tmp.0.join("first.png");
    let jpg_path = tmp.0.join("second.jpg");
    let bad_path = tmp.0.join("notes.txt");
    let huge_path = tmp.0.join("oversized.png");
    std::fs::write(&png_path, valid_png()).unwrap();
    std::fs::write(&jpg_path, valid_jpeg()).unwrap();
    std::fs::write(&bad_path, b"not an image").unwrap();
    let mut huge = valid_png();
    huge.resize(MAX_FEEDBACK_ATTACHMENT_BYTES + 1024, 0);
    std::fs::write(&huge_path, huge).unwrap();

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    let stage_elements_before = editor.symbol().timeline.layers[0].keyframes[0]
        .elements
        .len();
    editor.open_beta_feedback();
    let mut t = 0.02;
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());

    // 1. File-picker path attachment (`Attach image…` backend).
    editor
        .beta_feedback
        .attach_paths(std::slice::from_ref(&png_path));
    assert_eq!(editor.beta_feedback.attachments.len(), 1);
    assert_eq!(editor.beta_feedback.attachments[0].filename, "first.png");
    assert!(editor.beta_feedback.error.is_none());

    // 2. Plain text paste still pastes into `TextArea` without adding an image row.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Paste("Steps to reproduce".into())],
        Vec::new(),
    );
    assert_eq!(editor.beta_feedback.text, "Steps to reproduce");
    assert_eq!(editor.beta_feedback.attachments.len(), 1);

    // 3. Clipboard image paste (`⌘V`) names files "Pasted image 1.png", "Pasted image 2.gif".
    editor.beta_feedback.test_clipboard_image = Some(valid_png());
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        }],
        Vec::new(),
    );
    editor.beta_feedback.test_clipboard_image = Some(valid_gif());
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Paste(String::new())],
        Vec::new(),
    );
    assert_eq!(editor.beta_feedback.attachments.len(), 3);
    assert_eq!(
        editor.beta_feedback.attachments[1].filename,
        "Pasted image 1.png"
    );
    assert_eq!(
        editor.beta_feedback.attachments[2].filename,
        "Pasted image 2.gif"
    );

    // 4. Drag-and-drop onto the open dialog attaches images and never imports onto the Stage.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        Vec::new(),
        vec![
            egui::DroppedFile {
                path: Some(jpg_path),
                ..Default::default()
            },
            egui::DroppedFile {
                name: "fifth.webp".into(),
                bytes: Some(valid_webp().into()),
                ..Default::default()
            },
        ],
    );
    assert_eq!(editor.beta_feedback.attachments.len(), 5);
    assert_eq!(
        editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .len(),
        stage_elements_before,
        "Stage elements must remain untouched when dropping files on Beta feedback dialog"
    );

    // 5. With 5 images attached, a 6th image is rejected with an Error InlineMessage naming it,
    // and `Attach image…` is disabled with the limit reason tooltip on hover.
    editor.beta_feedback.attach_paths(&[png_path]);
    assert_eq!(editor.beta_feedback.attachments.len(), 5);
    assert!(
        editor
            .beta_feedback
            .error
            .as_deref()
            .is_some_and(|e| e.contains("first.png") && e.contains("5 images"))
    );

    let out_five = warmup_dialog(&mut editor, &ctx, &mut t);
    assert!(has_ax_substring(&out_five, "at most 5 images"));
    let attach_rect = ax_bounds(&out_five, "Attach image…").expect("Attach image… present");
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::PointerMoved(attach_rect.center())],
        Vec::new(),
    );
    // Advance past tooltip delay so the NF2 tooltip renders.
    t += 1.0;
    let out_tip = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
    assert!(
        has_ax_substring(&out_tip, "At most 5 images can be attached"),
        "Hovering disabled Attach image… at 5 attachments must display limit tooltip"
    );

    // 6. Click `Remove` on the second attachment row: removes ONLY that row (`Pasted image 1.png`).
    let removes = all_ax_bounds(&out_tip, "Remove");
    assert_eq!(removes.len(), 5);
    click_at(&mut editor, &ctx, &mut t, removes[1].center());
    assert_eq!(editor.beta_feedback.attachments.len(), 4);
    assert_eq!(editor.beta_feedback.attachments[0].filename, "first.png");
    assert_eq!(
        editor.beta_feedback.attachments[1].filename,
        "Pasted image 2.gif"
    );

    // 7. Invalid type (`notes.txt`) and oversized (`> 10 MB`) files are rejected with named errors.
    editor.beta_feedback.attach_paths(&[bad_path]);
    assert_eq!(editor.beta_feedback.attachments.len(), 4);
    assert!(
        editor
            .beta_feedback
            .error
            .as_deref()
            .is_some_and(|e| e.contains("notes.txt") && e.contains("PNG, JPEG, GIF or WebP"))
    );

    editor.beta_feedback.attach_paths(&[huge_path]);
    assert_eq!(editor.beta_feedback.attachments.len(), 4);
    assert!(
        editor
            .beta_feedback
            .error
            .as_deref()
            .is_some_and(|e| e.contains("oversized.png") && e.contains("10 MB"))
    );
}

#[test]
fn esc_cancel_and_close_preserve_draft_across_reopen_and_document_tab_switches() {
    let _guard = BetaFlagGuard::set(Some(true));
    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.open_beta_feedback();
    editor.beta_feedback.text = "Onion skin flickers when scrubbing backwards.".into();
    editor
        .beta_feedback
        .attach_bytes("capture-a.png", &valid_png());

    let mut t = 0.02;
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());

    // 1. `Esc` closes without sending and preserves text + attachments.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        Vec::new(),
    );
    assert!(!editor.beta_feedback.open);
    assert_eq!(
        editor.beta_feedback.text,
        "Onion skin flickers when scrubbing backwards."
    );
    assert_eq!(editor.beta_feedback.attachments.len(), 1);

    // 2. Reopen and click `Cancel`: still preserved.
    editor.open_beta_feedback();
    let out = warmup_dialog(&mut editor, &ctx, &mut t);
    let cancel_center = ax_bounds(&out, "Cancel").expect("Cancel button").center();
    click_at(&mut editor, &ctx, &mut t, cancel_center);
    assert!(!editor.beta_feedback.open);
    assert_eq!(editor.beta_feedback.attachments.len(), 1);

    // 3. Reopen and click header `Close` (`✕`): still preserved.
    editor.open_beta_feedback();
    let out_close = warmup_dialog(&mut editor, &ctx, &mut t);
    let close_center = ax_bounds(&out_close, "Close")
        .expect("DialogHeader Close button")
        .center();
    click_at(&mut editor, &ctx, &mut t, close_center);
    assert!(!editor.beta_feedback.open);

    // 4. Switch to a newly created document session tab and back: draft remains intact.
    editor.new_document_session(&ctx);
    editor.switch_session_tab(&ctx, 0);
    editor.open_beta_feedback();
    assert_eq!(
        editor.beta_feedback.text,
        "Onion skin flickers when scrubbing backwards."
    );
    assert_eq!(editor.beta_feedback.attachments.len(), 1);
}

#[test]
fn failed_send_including_formsubmit_activation_error_keeps_dialog_open_and_shows_inline_error() {
    let _guard = BetaFlagGuard::set(Some(true));
    let (url, server) = spawn_loopback_feedback_server(
        "200 OK",
        r#"{"success":"false","message":"This form needs Activation. Check developer@redinger.cc."}"#,
    );

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    editor.beta_feedback.endpoint_override = Some(url);
    editor.open_beta_feedback();
    editor.beta_feedback.text = "Timeline playhead snaps unexpectedly.".into();
    editor.beta_feedback.attach_bytes("snap.png", &valid_png());

    let mut t = 0.02;
    run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());

    // Submit with `⌘↵`.
    t += 0.02;
    run_editor_frame(
        &mut editor,
        &ctx,
        t,
        vec![Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        }],
        Vec::new(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut final_out = None;
    while Instant::now() < deadline {
        t += 0.02;
        let out = run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
        if !editor.beta_feedback.is_sending() {
            final_out = Some(out);
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = server.join();
    let out = final_out.expect("background send finished");

    assert!(editor.beta_feedback.open, "Failure must keep dialog open");
    assert_eq!(
        editor.beta_feedback.text,
        "Timeline playhead snaps unexpectedly."
    );
    assert_eq!(editor.beta_feedback.attachments.len(), 1);
    assert!(
        has_ax_substring(&out, "This form needs Activation"),
        "Error InlineMessage must display FormSubmit activation message"
    );
    assert!(
        editor.notifications.visible_toasts().is_empty(),
        "Failure must not emit Success toast"
    );
}

#[test]
fn successful_send_delivers_strict_multipart_payload_clears_draft_closes_and_shows_toast() {
    let _guard = BetaFlagGuard::set(Some(true));
    let (url, server) = spawn_loopback_feedback_server(
        "200 OK",
        r#"{"success":"true","message":"The form was submitted successfully."}"#,
    );

    let ctx = egui::Context::default();
    let mut project = Project::blank();
    project.name = "ConfidentialClientDoc".into();
    let mut editor = Editor::from_project(&ctx, project);
    editor.beta_feedback.endpoint_override = Some(url);
    editor.open_beta_feedback();
    editor.beta_feedback.text = "Brush stroke smoothing shifts endpoints at 200% zoom.".into();
    editor
        .beta_feedback
        .attach_bytes("zoom-200.png", &valid_png());
    editor
        .beta_feedback
        .attach_bytes("after.webp", &valid_webp());

    let mut t = 0.02;
    let out = warmup_dialog(&mut editor, &ctx, &mut t);
    let send_center = ax_bounds(&out, "Send").expect("Send button").center();
    click_at(&mut editor, &ctx, &mut t, send_center);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut final_out = None;
    while Instant::now() < deadline {
        t += 0.02;
        run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
        if !editor.beta_feedback.is_sending() && !editor.beta_feedback.open {
            // Run the scheduled `request_repaint()` pass after modal dismissal so
            // `top_modal_layer()` clears and `notification_host` promotes the pending Toast.
            t += 0.02;
            final_out = Some(run_editor_frame(
                &mut editor,
                &ctx,
                t,
                Vec::new(),
                Vec::new(),
            ));
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let (headers, body) = server.join().expect("server captured request");
    let out = final_out.expect("send completed and closed dialog");

    assert!(!editor.beta_feedback.open);
    assert!(editor.beta_feedback.text.is_empty());
    assert!(editor.beta_feedback.attachments.is_empty());
    assert!(editor.beta_feedback.error.is_none());
    assert!(
        editor
            .notifications
            .visible_toasts()
            .iter()
            .any(|(kind, msg)| *kind == c::ToastType::Success && msg == "Feedback sent"),
        "Success Toast 'Feedback sent' must be visible"
    );
    assert!(has_ax_substring(&out, "Feedback sent"));

    // Verify wire headers and strict privacy rule (`R-103` item 6).
    let lower_hdr = headers.to_ascii_lowercase();
    assert!(lower_hdr.starts_with("post /ajax/developer@redinger.cc http/1.1"));
    assert!(lower_hdr.contains("accept: application/json"));
    assert!(lower_hdr.contains("origin: https://redinger.cc\r\n"));
    assert!(lower_hdr.contains("referer: https://redinger.cc/\r\n"));

    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("name=\"message\""));
    assert!(body_str.contains("Brush stroke smoothing shifts endpoints at 200% zoom."));
    assert!(body_str.contains("name=\"attachment_1\"; filename=\"zoom-200.png\""));
    assert!(body_str.contains("name=\"attachment_2\"; filename=\"after.webp\""));
    assert!(
        !body_str.contains("ConfidentialClientDoc") && !headers.contains("ConfidentialClientDoc"),
        "Open document name/contents must never be transmitted"
    );
    assert!(
        !body_str.contains("0.1.0-beta") && !lower_hdr.contains("0.1.0-beta"),
        "Hidden version metadata must never be transmitted"
    );
}

#[test]
fn t311_closing_modal_during_in_flight_send_completes_cleanly_and_help_command_is_beta_gated() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let release_gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server_gate = std::sync::Arc::clone(&release_gate);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept feedback request");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 4096];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        server_gate.wait();
        let body = r#"{"success":"true"}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = std::io::Write::write_all(&mut stream, reply.as_bytes());
    });

    let ctx = egui::Context::default();
    let mut editor = Editor::from_project(&ctx, Project::blank());
    {
        let _guard = BetaFlagGuard::set(Some(true));
        editor.sessions.clear();
        assert!(
            editor
                .command_menu_state(Some(&ctx), crate::commands::CommandId::HelpSendBetaFeedback)
                .enabled,
            "HelpSendBetaFeedback must be enabled even with 0 open documents in beta builds"
        );

        editor.beta_feedback.endpoint_override =
            Some(format!("http://{addr}/ajax/developer@redinger.cc"));
        editor.execute_menu_command(&ctx, crate::commands::CommandId::HelpSendBetaFeedback);
        assert!(editor.beta_feedback.open);
        editor.beta_feedback.text = "Background send survives Cancel.".into();
        editor.beta_feedback.attach_bytes("shot.png", &valid_png());

        let mut t = 0.02;
        run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
        t += 0.02;
        run_editor_frame(
            &mut editor,
            &ctx,
            t,
            vec![Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }],
            Vec::new(),
        );
        assert!(editor.beta_feedback.is_sending());

        // Close modal while worker is still in flight; `in_flight` must NOT be orphaned.
        editor.close_beta_feedback();
        assert!(!editor.beta_feedback.open);
        assert!(editor.beta_feedback.is_sending());

        release_gate.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            t += 0.02;
            run_editor_frame(&mut editor, &ctx, t, Vec::new(), Vec::new());
            if !editor.beta_feedback.is_sending() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        server.join().unwrap();

        assert!(editor.beta_feedback.text.is_empty());
        assert!(editor.beta_feedback.attachments.is_empty());
        assert!(
            editor
                .notifications
                .visible_toasts()
                .iter()
                .any(|(kind, msg)| *kind == c::ToastType::Success && msg == "Feedback sent"),
            "closing Beta feedback while sending must still emit 'Feedback sent' and clear draft"
        );
    }

    {
        let _guard = BetaFlagGuard::set(Some(false));
        assert!(
            !editor
                .command_menu_state(Some(&ctx), crate::commands::CommandId::HelpSendBetaFeedback)
                .enabled,
            "HelpSendBetaFeedback must be disabled in non-beta builds"
        );
    }
}
