//! Beta feedback Stage chip (`Bug?`) and modal dialog (`Dialog / Beta feedback`, `R-103` variant 4, `T-317`).
//!
//! Beta 1 ships variant 4 — text and up to five image attachments (`PNG`, `JPEG`, `GIF`, `WebP`,
//! ≤ 10 MB each), with no `Email` or `Name` fields. Only the fields and images shown in the dialog
//! are sent (`R-103` item 6).

use crate::app::Editor;
use eframe::egui::{self, Color32, Rect};
use neoflash_export::{
    FeedbackAttachment, MAX_FEEDBACK_ATTACHMENT_BYTES, MAX_FEEDBACK_ATTACHMENTS,
    decode_feedback_image_data_url, detect_feedback_image_mime, format_attachment_size,
    send_beta_feedback_multipart, validate_feedback_attachment,
};
use neoflash_ui::{
    TextStyle, components as c,
    icons::{self, i16},
    palette,
    tokens::space,
    typography::{font, rich},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

const DIALOG_INNER_FRAME_WIDTH: f32 = 478.0;
const CHIP_BOTTOM_MARGIN: f32 = 10.0;
const LIMIT_TOOLTIP: &str = "At most 5 images can be attached";

#[cfg(test)]
thread_local! {
    static BETA_BUILD_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Override [`is_beta_build`] on the current test thread (`None` restores package-version detection).
#[cfg(test)]
pub(crate) fn set_beta_build_for_test(value: Option<bool>) {
    BETA_BUILD_OVERRIDE.with(|cell| cell.set(value));
}

/// Returns `true` when running a beta build (`CARGO_PKG_VERSION` contains `"beta"`).
/// Release builds return `false` and hide the `Bug?` Stage chip (`R-103` item 1).
pub(crate) fn is_beta_build() -> bool {
    #[cfg(test)]
    if let Some(override_val) = BETA_BUILD_OVERRIDE.with(|cell| cell.get()) {
        return override_val;
    }
    env!("CARGO_PKG_VERSION").contains("beta")
}

/// Session-scoped state for `"Beta feedback"`.
///
/// Lives on [`Editor`] (not on a `DocumentSession`) so typed text and attached images survive
/// closing the dialog (`Esc` / `Cancel` / `✕`) and switching document tabs until the app quits.
pub(crate) struct BetaFeedbackState {
    pub(crate) open: bool,
    pub(crate) focus_text_on_open: bool,
    pub(crate) text: String,
    pub(crate) attachments: Vec<FeedbackAttachment>,
    pub(crate) error: Option<String>,
    pub(crate) next_pasted_index: usize,
    pub(crate) endpoint_override: Option<String>,
    in_flight: Option<Receiver<Result<(), String>>>,
    #[cfg(test)]
    pub(crate) test_clipboard_image: Option<Vec<u8>>,
}

impl Default for BetaFeedbackState {
    fn default() -> Self {
        Self {
            open: false,
            focus_text_on_open: false,
            text: String::new(),
            attachments: Vec::new(),
            error: None,
            next_pasted_index: 1,
            endpoint_override: None,
            in_flight: None,
            #[cfg(test)]
            test_clipboard_image: None,
        }
    }
}

impl BetaFeedbackState {
    pub(crate) fn is_sending(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(crate) fn attach_bytes(&mut self, filename: &str, bytes: &[u8]) -> bool {
        let display = clean_label(filename);
        if self.attachments.len() >= MAX_FEEDBACK_ATTACHMENTS {
            self.error = Some(format!(
                "“{display}” was not added: at most {MAX_FEEDBACK_ATTACHMENTS} images can be attached."
            ));
            return false;
        }
        match validate_feedback_attachment(&display, bytes) {
            Ok(att) => {
                self.attachments.push(att);
                true
            }
            Err(err) => {
                self.error = Some(err);
                false
            }
        }
    }

    pub(crate) fn attach_paths(&mut self, paths: &[PathBuf]) {
        let mut any_added = false;
        let mut any_error = false;
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "image".to_owned());
            if self.attachments.len() >= MAX_FEEDBACK_ATTACHMENTS {
                self.error = Some(format!(
                    "“{}” was not added: at most {MAX_FEEDBACK_ATTACHMENTS} images can be attached.",
                    clean_label(&name)
                ));
                any_error = true;
                continue;
            }
            match std::fs::metadata(path) {
                Ok(meta) if meta.len() > MAX_FEEDBACK_ATTACHMENT_BYTES as u64 => {
                    let size_str =
                        format_attachment_size(meta.len().min(usize::MAX as u64) as usize);
                    self.error = Some(format!(
                        "“{}” exceeds the 10 MB limit ({size_str}).",
                        clean_label(&name)
                    ));
                    any_error = true;
                }
                Ok(_) => match std::fs::read(path) {
                    Ok(bytes) => {
                        if self.attach_bytes(&name, &bytes) {
                            any_added = true;
                        } else {
                            any_error = true;
                        }
                    }
                    Err(err) => {
                        self.error = Some(format!(
                            "“{}” could not be read: {err}.",
                            clean_label(&name)
                        ));
                        any_error = true;
                    }
                },
                Err(err) => {
                    self.error = Some(format!(
                        "“{}” could not be read: {err}.",
                        clean_label(&name)
                    ));
                    any_error = true;
                }
            }
        }
        if any_added && !any_error {
            self.error = None;
        }
    }

    pub(crate) fn attach_pasted_bytes(&mut self, bytes: &[u8]) -> bool {
        let ext = match detect_feedback_image_mime(bytes) {
            Some("image/jpeg") => "jpg",
            Some("image/gif") => "gif",
            Some("image/webp") => "webp",
            _ => "png",
        };
        let index = self.next_pasted_index.max(1);
        let filename = format!("Pasted image {index}.{ext}");
        if self.attach_bytes(&filename, bytes) {
            self.next_pasted_index = index + 1;
            self.error = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn remove_attachment(&mut self, index: usize) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            self.error = None;
        }
    }
}

fn clean_label(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let trimmed = base.trim();
    if trimmed.is_empty() {
        "image".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn middle_truncate_filename(ui: &egui::Ui, name: &str, max_width: f32) -> String {
    let body_font = font(TextStyle::Body);
    let measure = |candidate: &str| -> f32 {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(candidate.to_owned(), body_font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    if max_width <= 24.0 || measure(name) <= max_width {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= 4 {
        return name.to_owned();
    }
    let mut best = "…".to_owned();
    let mut lo = 2usize;
    let mut hi = chars.len() - 1;
    while lo <= hi {
        let keep = (lo + hi) / 2;
        let tail_len = (keep * 9 / 20).max(1);
        let head_len = keep.saturating_sub(tail_len).max(1);
        let head: String = chars[..head_len].iter().collect();
        let tail: String = chars[chars.len() - tail_len..].iter().collect();
        let candidate = format!("{head}…{tail}");
        if measure(&candidate) <= max_width {
            best = candidate;
            lo = keep + 1;
        } else {
            if keep == 0 {
                break;
            }
            hi = keep - 1;
        }
    }
    best
}

#[cfg(not(test))]
fn read_os_clipboard_image_png() -> Option<Vec<u8>> {
    let mut cb = arboard::Clipboard::new().ok()?;
    if cb.get_text().is_ok_and(|t| !t.trim().is_empty()) {
        return None;
    }
    let img = cb.get_image().ok()?;
    let rgba =
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    Some(cursor.into_inner())
}

pub(crate) fn chip_rect_temp_id() -> egui::Id {
    egui::Id::new("stage-beta-feedback-chip-rect")
}

impl Editor {
    /// Open `"Beta feedback"` and focus its `TextArea` (`R-103` item 2).
    pub(crate) fn open_beta_feedback(&mut self) {
        self.finish_edits();
        self.beta_feedback.open = true;
        self.beta_feedback.focus_text_on_open = true;
    }

    /// Close `"Beta feedback"` without discarding any in-flight send or typed draft (`R-103` item 5).
    ///
    /// If a submission is already in flight, `poll_beta_feedback` continues draining the worker so
    /// completion still clears the draft and shows `"Feedback sent"`, while an error re-opens the
    /// modal with the preserved draft and inline error message.
    pub(crate) fn close_beta_feedback(&mut self) {
        self.beta_feedback.open = false;
        self.beta_feedback.focus_text_on_open = false;
    }

    /// Floating `Bug?` chip at the bottom-left of the Stage canvas (`R-103` item 1).
    ///
    /// Shown in beta builds only; aligned `16 pt` right of the vertical ruler (or `canvas.left()`
    /// when rulers are hidden) and `10 pt` above `canvas.bottom()`. Steps aside during active
    /// canvas drags just like the floating Stage toolbar.
    pub(crate) fn beta_feedback_stage_chip(&mut self, ctx: &egui::Context, canvas: Rect) {
        let rect_id = chip_rect_temp_id();
        if !is_beta_build() || canvas.width() < 420.0 || canvas.height() < 160.0 {
            ctx.data_mut(|d| d.remove::<Rect>(rect_id));
            return;
        }
        let toolbar_rect: Option<Rect> =
            ctx.data(|d| d.get_temp(egui::Id::new("stage-toolbar-rect")));
        let last_chip_rect: Option<Rect> = ctx.data(|d| d.get_temp(rect_id));
        let gesture = ctx.input(|i| {
            i.pointer.primary_down()
                && i.pointer.is_decidedly_dragging()
                && i.pointer.press_origin().is_some_and(|p| {
                    canvas.contains(p)
                        && toolbar_rect.is_none_or(|r| !r.contains(p))
                        && last_chip_rect.is_none_or(|r| !r.contains(p))
                })
        });
        if gesture {
            ctx.data_mut(|d| d.remove::<Rect>(rect_id));
            return;
        }

        let ruler = if self.stage_view.prefs.rulers {
            neoflash_ui::components::canvas::RULER
        } else {
            0.0
        };
        let anchor = egui::pos2(
            canvas.left() + ruler + space::S4,
            canvas.bottom() - CHIP_BOTTOM_MARGIN,
        );
        let mut clicked = false;
        let area = egui::Area::new(egui::Id::new("stage-beta-feedback-chip"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(anchor)
            .constrain_to(canvas)
            .show(ctx, |ui| {
                let resp = ui.add(c::Button::new("Bug?").icon(i16::DEBUG));
                clicked = c::tooltip(resp, "Send beta feedback", None).clicked();
            });
        ctx.data_mut(|d| d.insert_temp(rect_id, area.response.rect));
        if clicked {
            self.open_beta_feedback();
        }
    }

    fn pick_beta_feedback_images(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
            .pick_files()
        {
            self.beta_feedback.attach_paths(&paths);
        }
    }

    pub(crate) fn start_beta_feedback_send(&mut self, ctx: &egui::Context) {
        if self.beta_feedback.is_sending() || self.beta_feedback.text.trim().is_empty() {
            return;
        }
        self.beta_feedback.error = None;
        let message = self.beta_feedback.text.clone();
        let attachments = self.beta_feedback.attachments.clone();
        let url_override = self.beta_feedback.endpoint_override.clone();
        let repaint_ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result =
                send_beta_feedback_multipart(&message, &attachments, url_override.as_deref())
                    .map_err(|e| e.to_string());
            let _ = tx.send(result);
            repaint_ctx.request_repaint();
        });
        self.beta_feedback.in_flight = Some(rx);
        ctx.request_repaint();
    }

    /// Poll any in-flight asynchronous feedback submission (`T-317`).
    pub(crate) fn poll_beta_feedback(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.beta_feedback.in_flight else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.beta_feedback.in_flight = None;
                self.beta_feedback.text.clear();
                self.beta_feedback.attachments.clear();
                self.beta_feedback.error = None;
                self.beta_feedback.open = false;
                self.beta_feedback.focus_text_on_open = false;
                self.notify(c::ToastType::Success, "Feedback sent");
                ctx.request_repaint();
            }
            Ok(Err(err)) => {
                self.beta_feedback.in_flight = None;
                self.beta_feedback.error = Some(err);
                self.beta_feedback.open = true;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            Err(TryRecvError::Disconnected) => {
                self.beta_feedback.in_flight = None;
                self.beta_feedback.error = Some(
                    "Could not send feedback: background worker stopped unexpectedly.".to_owned(),
                );
                self.beta_feedback.open = true;
                ctx.request_repaint();
            }
        }
    }

    fn ingest_beta_feedback_drops_and_paste(&mut self, ctx: &egui::Context) {
        if self.beta_feedback.is_sending() {
            return;
        }
        // 1. Drag-and-drop files onto the open dialog (`R-103` item 4).
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() {
            let mut any_added = false;
            let mut any_error = false;
            for file in dropped {
                let name = file
                    .path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| {
                        if file.name.trim().is_empty() {
                            "dropped-image".to_owned()
                        } else {
                            file.name.clone()
                        }
                    });
                if let Some(bytes) = &file.bytes {
                    if self.beta_feedback.attach_bytes(&name, bytes) {
                        any_added = true;
                    } else {
                        any_error = true;
                    }
                } else if let Some(path) = file.path {
                    let prev_len = self.beta_feedback.attachments.len();
                    self.beta_feedback.attach_paths(&[path]);
                    if self.beta_feedback.attachments.len() > prev_len {
                        any_added = true;
                    } else {
                        any_error = true;
                    }
                }
            }
            if any_added && !any_error {
                self.beta_feedback.error = None;
            }
        }

        // 2. Clipboard image paste (`⌘V` / `Ctrl+V` / `Event::Paste`; `R-103` item 4).
        let (has_paste_trigger, pasted_text) = ctx.input(|i| {
            let mut trigger = false;
            let mut text_payload = None;
            for ev in &i.events {
                match ev {
                    egui::Event::Paste(s) => {
                        trigger = true;
                        text_payload = Some(s.clone());
                    }
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.command || modifiers.ctrl => {
                        trigger = true;
                    }
                    egui::Event::Key {
                        key: egui::Key::Paste,
                        pressed: true,
                        ..
                    } => {
                        trigger = true;
                    }
                    _ => {}
                }
            }
            (trigger, text_payload)
        });
        if !has_paste_trigger {
            return;
        }

        #[cfg(test)]
        let mut clipboard_image_bytes = self.beta_feedback.test_clipboard_image.take();
        #[cfg(not(test))]
        let mut clipboard_image_bytes: Option<Vec<u8>> = None;
        if clipboard_image_bytes.is_none()
            && let Some(ref s) = pasted_text
        {
            clipboard_image_bytes = decode_feedback_image_data_url(s);
        }
        #[cfg(not(test))]
        if clipboard_image_bytes.is_none()
            && pasted_text.as_deref().is_none_or(|s| s.trim().is_empty())
        {
            clipboard_image_bytes = read_os_clipboard_image_png();
        }

        if let Some(img_bytes) = clipboard_image_bytes {
            self.beta_feedback.attach_pasted_bytes(&img_bytes);
            // Consume paste events so `TextEdit` does not alter the text selection.
            ctx.input_mut(|i| {
                i.events.retain(|ev| {
                    !matches!(
                        ev,
                        egui::Event::Paste(_)
                            | egui::Event::Key {
                                key: egui::Key::Paste,
                                ..
                            }
                    ) && !matches!(
                        ev,
                        egui::Event::Key {
                            key: egui::Key::V,
                            modifiers,
                            ..
                        } if modifiers.command || modifiers.ctrl
                    )
                });
            });
        }
    }

    /// Render the modal `"Beta feedback"` dialog (`R-103` variant 4).
    pub(crate) fn beta_feedback_dialog(&mut self, ctx: &egui::Context) {
        if !self.beta_feedback.open {
            return;
        }
        self.ingest_beta_feedback_drops_and_paste(ctx);

        let sending = self.beta_feedback.is_sending();
        let can_send_before = !self.beta_feedback.text.trim().is_empty() && !sending;
        let mut send_requested = false;
        let mut close_cancel = false;
        let mut pick_files_requested = false;
        let mut remove_idx: Option<usize> = None;

        // Consume `⌘↵` (`Cmd+Enter` / `Ctrl+Enter`) before `TextArea` runs so `TextEdit::multiline`
        // does not insert a trailing newline on the submit shortcut (`R-103` item 5).
        let cmd_enter = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::MAC_CMD, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)
        });
        if cmd_enter && can_send_before {
            send_requested = true;
        }

        let p = palette(ctx);
        let modal = egui::Modal::new(egui::Id::new("beta-feedback"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(DIALOG_INNER_FRAME_WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;

                close_cancel |= c::DialogHeader {
                    title: "Beta feedback",
                }
                .show(ui);

                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: space::S5 as i8,
                        right: space::S5 as i8,
                        top: 0,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        let body_w = DIALOG_INNER_FRAME_WIDTH - 2.0 * space::S5;
                        ui.set_width(body_w);
                        ui.spacing_mut().item_spacing.y = space::S3;

                        let ta_resp = ui.add_enabled(
                            !sending,
                            c::TextArea::new(&mut self.beta_feedback.text, "Beta feedback")
                                .rows(6)
                                .hint("What happened? What did you expect?"),
                        );
                        if self.beta_feedback.focus_text_on_open {
                            self.beta_feedback.focus_text_on_open = false;
                            ctx.memory_mut(|m| m.request_focus(ta_resp.id.with("edit")));
                        }

                        if !self.beta_feedback.attachments.is_empty() {
                            let row_h = neoflash_ui::metrics::control();
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (idx, att) in self.beta_feedback.attachments.iter().enumerate()
                                {
                                    let (row_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(body_w, row_h),
                                        egui::Sense::hover(),
                                    );
                                    let size_label = format_attachment_size(att.bytes.len());
                                    let size_w = ui.fonts_mut(|f| {
                                        f.layout_no_wrap(
                                            size_label.clone(),
                                            font(TextStyle::Small),
                                            Color32::WHITE,
                                        )
                                        .size()
                                        .x
                                    });
                                    let name_max_w = (body_w
                                        - 16.0
                                        - space::S2
                                        - size_w
                                        - space::S2 * 2.0
                                        - row_h)
                                        .max(48.0);
                                    let shown_name =
                                        middle_truncate_filename(ui, &att.filename, name_max_w);

                                    ui.scope_builder(
                                        egui::UiBuilder::new().max_rect(row_rect).layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                        ),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = space::S2;
                                            icons::show(ui, i16::BITMAP, p.text_secondary);
                                            let name_resp = ui.add(
                                                egui::Label::new(
                                                    rich(&shown_name, TextStyle::Body)
                                                        .color(p.text_primary),
                                                )
                                                .selectable(false),
                                            );
                                            if shown_name != att.filename {
                                                c::tooltip(name_resp, &att.filename, None);
                                            }
                                            ui.add(
                                                egui::Label::new(
                                                    rich(&size_label, TextStyle::Small)
                                                        .color(p.text_tertiary),
                                                )
                                                .selectable(false),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let rm = ui.add_enabled(
                                                        !sending,
                                                        c::IconButton::new(i16::X, "Remove"),
                                                    );
                                                    if rm.clicked() {
                                                        remove_idx = Some(idx);
                                                    }
                                                },
                                            );
                                        },
                                    );
                                }
                            });
                        }

                        if let Some(err) = &self.beta_feedback.error {
                            ui.add(c::InlineMessage::error(err));
                        }
                    });

                let at_limit = self.beta_feedback.attachments.len() >= MAX_FEEDBACK_ATTACHMENTS;
                let can_attach = !at_limit && !sending;
                let can_send = !self.beta_feedback.text.trim().is_empty() && !sending;

                c::surface::dialog_footer(
                    ui,
                    |ui| {
                        let attach_resp =
                            ui.add_enabled(can_attach, c::Button::ghost("Attach image…"));
                        if at_limit {
                            let hover_probe = ui.interact(
                                attach_resp.rect,
                                attach_resp.id.with("limit-hover"),
                                egui::Sense::hover(),
                            );
                            c::tooltip(hover_probe, LIMIT_TOOLTIP, None);
                        }
                        if attach_resp.clicked() {
                            pick_files_requested = true;
                        }
                    },
                    |ui| {
                        if ui
                            .add_enabled(can_send, c::Button::primary("Send").shortcut("⌘↵"))
                            .clicked()
                        {
                            send_requested = true;
                        }
                        if ui.add(c::Button::new("Cancel")).clicked() {
                            close_cancel = true;
                        }
                    },
                );
            });

        if !close_cancel && !send_requested && !super::dialog_helpers::is_any_popup_open(ctx) {
            let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if esc || modal.backdrop_response.clicked() {
                close_cancel = true;
            }
        }

        if let Some(idx) = remove_idx {
            self.beta_feedback.remove_attachment(idx);
        }
        if pick_files_requested {
            self.pick_beta_feedback_images();
        }
        if close_cancel {
            self.close_beta_feedback();
        } else if send_requested {
            self.start_beta_feedback_send(ctx);
        }
    }
}
