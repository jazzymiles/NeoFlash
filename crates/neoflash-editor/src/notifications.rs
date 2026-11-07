//! Window-owned R-044 notifications. Every notification is retained in Output;
//! the transient queue never belongs to a document session or Undo history.
use crate::app::Editor;
use eframe::egui::{self, Pos2, Rect};
use neoflash_ui::{
    components as nf,
    tokens::{TextStyle, space},
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotificationAction {
    ShowOutput,
}

impl NotificationAction {
    fn label(self) -> &'static str {
        match self {
            Self::ShowOutput => "Show",
        }
    }
}

struct Notification {
    id: u64,
    kind: nf::ToastType,
    message: String,
    action: Option<NotificationAction>,
    count: u32,
    remaining: Option<Duration>,
    rect: Rect,
    restarted: bool,
}

fn lifetime(kind: nf::ToastType) -> Option<Duration> {
    match kind {
        nf::ToastType::Info | nf::ToastType::Success => Some(Duration::from_secs(4)),
        nf::ToastType::Warning => Some(Duration::from_secs(6)),
        nf::ToastType::Error => None,
    }
}

#[derive(Default)]
pub(crate) struct NotificationCenter {
    visible: VecDeque<Notification>,
    pending: VecDeque<Notification>,
    next_id: u64,
    last_tick: Option<Instant>,
}

impl NotificationCenter {
    fn push(&mut self, kind: nf::ToastType, message: String, action: Option<NotificationAction>) {
        if let Some(old) = self
            .visible
            .iter_mut()
            .chain(self.pending.iter_mut())
            .find(|toast| toast.kind == kind && toast.message == message && toast.action == action)
        {
            old.count = old.count.saturating_add(1);
            old.remaining = lifetime(kind);
            old.restarted = true;
            return;
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.pending.push_back(Notification {
            id: self.next_id,
            kind,
            message,
            action,
            count: 1,
            remaining: lifetime(kind),
            rect: Rect::NOTHING,
            restarted: false,
        });
        // Modal operations may report many errors. Output retains them all; bound the transient queue.
        while self.pending.len() > 32 {
            self.pending.pop_front();
        }
    }

    fn advance(&mut self, elapsed: Duration, modal: bool, pointer: Option<Pos2>) {
        if modal {
            return;
        }
        self.visible.retain_mut(|toast| {
            if let Some(remaining) = &mut toast.remaining {
                if !toast.restarted
                    && !pointer.is_some_and(|position| toast.rect.contains(position))
                {
                    *remaining = remaining.saturating_sub(elapsed);
                }
                toast.restarted = false;
                !remaining.is_zero()
            } else {
                true
            }
        });
        self.visible.extend(self.pending.drain(..));
        while self.visible.len() > 3 {
            self.visible.pop_front();
        }
    }
}

impl Editor {
    pub(crate) fn notify(&mut self, kind: nf::ToastType, message: impl Into<String>) {
        let message = message.into();
        self.message(message.clone());
        self.notifications.push(kind, message, None);
    }

    pub(crate) fn notify_with_output(&mut self, kind: nf::ToastType, message: impl Into<String>) {
        let message = message.into();
        self.message(message.clone());
        self.notifications
            .push(kind, message, Some(NotificationAction::ShowOutput));
    }

    /// Call once for the root viewport, after modals, with the actual canvas viewport if visible.
    pub(crate) fn notification_host(&mut self, ctx: &egui::Context, canvas: Option<Rect>) {
        let now = Instant::now();
        let elapsed = self
            .notifications
            .last_tick
            .replace(now)
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or_default();
        let modal = self.has_modal_dialog_in(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some());
        let pointer = ctx.pointer_hover_pos();
        self.notifications.advance(elapsed, modal, pointer);
        if modal || self.notifications.visible.is_empty() {
            return;
        }
        let viewport = canvas
            .filter(|rect| rect.is_finite() && rect.is_positive())
            .unwrap_or_else(|| {
                let mut window = ctx.content_rect();
                window.max.y -= neoflash_ui::metrics::status_bar();
                window
            });
        let available = viewport.width() - 2. * space::S4;
        if available <= 0. {
            return;
        }
        let mut bottom = viewport.bottom() - space::S4;
        let mut remove = Vec::new();
        let mut action = None;
        let mut action_message = None;
        let text_color = neoflash_ui::palette(ctx).text_primary;
        for toast in self.notifications.visible.iter_mut().rev() {
            if bottom <= viewport.top() {
                break;
            }
            let message = if toast.count > 1 {
                format!("{} ×{}", toast.message, toast.count)
            } else {
                toast.message.clone()
            };
            let text_width = ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(
                        message.clone(),
                        neoflash_ui::typography::font(TextStyle::Body),
                        text_color,
                    )
                    .size()
                    .x
            });
            let action_width = toast.action.map_or(0., |action| {
                ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            action.label().into(),
                            neoflash_ui::typography::font(TextStyle::Body),
                            text_color,
                        )
                        .size()
                        .x
                }) + 2. * space::S3
            });
            // Shared Toast uses 24 pt close, 16 pt icon, 3 pt stripe, padding and four row gaps.
            let controls_width = action_width + 24. + 16. + 3. + 2. * space::S3 + 4. * space::S2;
            let width = (text_width + controls_width)
                .clamp(320., 480.)
                .min(available);
            // Toast's horizontal label precedes its trailing controls. Reserve their space before
            // passing text to the shared component: plain Wrap would consume the entire row and
            // leave the action and close button on top of the message. Keep the full original in
            // Output/dedup; these line breaks are presentation only, using the same NF2 body font.
            let message = ctx.fonts_mut(|fonts| {
                fonts
                    .layout(
                        message,
                        neoflash_ui::typography::font(TextStyle::Body),
                        text_color,
                        (width - controls_width).max(1.),
                    )
                    .rows
                    .iter()
                    .map(|row| row.text())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            let shown = egui::Area::new(egui::Id::new(("window-toast", toast.id)))
                .order(egui::Order::Foreground)
                .movable(false)
                .pivot(egui::Align2::CENTER_BOTTOM)
                .fixed_pos(egui::pos2(viewport.center().x, bottom))
                .constrain_to(viewport)
                .show(ctx, |ui| {
                    ui.set_clip_rect(viewport);
                    ui.set_max_width(width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    nf::Toast {
                        kind: toast.kind,
                        message: &message,
                        action: toast.action.map(NotificationAction::label),
                    }
                    .show(ui, width)
                });
            toast.rect = shown.response.rect;
            bottom = toast.rect.top() - space::S2;
            if shown.inner.close || shown.inner.action {
                remove.push(toast.id);
            }
            if shown.inner.action {
                action = toast.action;
                action_message = Some(toast.message.clone());
            }
        }
        self.notifications
            .visible
            .retain(|toast| !remove.contains(&toast.id));
        if action == Some(NotificationAction::ShowOutput) {
            // The toast is window-owned and may outlive its originating tab. Show the relevant
            // diagnostic in the current Output too, rather than revealing an unrelated empty log.
            if let Some(message) = action_message
                && !self.output.contains(&message)
            {
                self.output.push(message);
            }
            self.show_panel(crate::workspace::PanelId::Output);
        }
        if let Some(next) = self
            .notifications
            .visible
            .iter()
            .filter(|toast| !pointer.is_some_and(|position| toast.rect.contains(position)))
            .filter_map(|toast| toast.remaining)
            .min()
        {
            ctx.request_repaint_after(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_host_uses_canvas_bounds_keeps_focus_and_supports_output_and_dismiss() {
        fn frame(
            editor: &mut Editor,
            ctx: &egui::Context,
            canvas: Rect,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 800.))),
                    events,
                    ..Default::default()
                },
                |ctx| editor.notification_host(ctx, Some(canvas)),
            )
        }
        fn button(output: &egui::FullOutput, label: &str) -> Rect {
            let bounds = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap()
                .1
                .bounds()
                .unwrap();
            Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        }
        fn assert_message_bounds(
            output: &egui::FullOutput,
            message: &str,
            toast: Rect,
            action: bool,
        ) {
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text.split_whitespace().collect::<Vec<_>>()
                            == message.split_whitespace().collect::<Vec<_>>() =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("the complete message remains visible, with presentation-only line breaks");
            let text_rect = text.galley.rect.translate(text.pos.to_vec2());
            let close = button(output, "Dismiss");
            assert!(toast.contains_rect(text_rect));
            assert!(toast.contains_rect(close));
            assert!(
                text_rect.right() <= close.left(),
                "message overlaps Dismiss"
            );
            if action {
                let action = button(output, "Show");
                assert!(toast.contains_rect(action));
                assert!(text_rect.right() <= action.left(), "message overlaps Show");
                assert!(action.right() <= close.left(), "Show overlaps Dismiss");
            }
        }
        fn click(editor: &mut Editor, ctx: &egui::Context, canvas: Rect, pos: Pos2) {
            for pressed in [true, false] {
                frame(
                    editor,
                    ctx,
                    canvas,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        for theme in [
            crate::theme::ThemeMode::Dark,
            crate::theme::ThemeMode::Light,
        ] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut editor = Editor::from_project(&ctx, neoflash_core::Project::demo());
            editor.set_theme(&ctx, theme);
            let canvas = Rect::from_min_max(egui::pos2(80., 70.), egui::pos2(680., 550.));
            let project = editor.project.clone();
            let focus = egui::Id::new("notification-keeps-authoring-focus");
            ctx.memory_mut(|memory| memory.request_focus(focus));
            let error = "Save failed: read-only destination. Choose a writable folder for this document and try saving again; your current changes remain open.";
            editor.notify_with_output(nf::ToastType::Error, error);
            frame(&mut editor, &ctx, canvas, vec![]);
            let output = frame(&mut editor, &ctx, canvas, vec![]);
            let rect = editor.notifications.visible[0].rect;
            assert!(canvas.contains_rect(rect));
            assert!((rect.center().x - canvas.center().x).abs() < 1.);
            assert!((rect.bottom() - (canvas.bottom() - space::S4)).abs() < 1.);
            assert!((320. ..=482.).contains(&rect.width()));
            assert_message_bounds(&output, error, rect, true);
            assert!(
                editor
                    .output
                    .iter()
                    .any(|message| message.contains("read-only destination"))
            );
            // The host makes no focus request: any surviving authoring focus keeps its identity.
            assert!(
                ctx.memory(|memory| memory.focused())
                    .is_none_or(|id| id == focus)
            );
            let at = button(&output, "Show").center();
            click(&mut editor, &ctx, canvas, at);
            assert!(editor.notifications.visible.is_empty());
            assert!(
                editor
                    .workspace
                    .is_panel_visible(crate::workspace::PanelId::Output)
            );
            let warning = "Partial stroke erasing on masked artwork is not available yet; no artwork was changed.";
            editor.notify(nf::ToastType::Warning, warning);
            frame(&mut editor, &ctx, canvas, vec![]);
            let output = frame(&mut editor, &ctx, canvas, vec![]);
            let rect = editor.notifications.visible[0].rect;
            assert!((320. ..=482.).contains(&rect.width()));
            assert_message_bounds(&output, warning, rect, false);
            click(
                &mut editor,
                &ctx,
                canvas,
                button(&output, "Dismiss").center(),
            );
            assert!(editor.notifications.visible.is_empty());
            assert_eq!(editor.project, project);
            assert!(!editor.history.can_undo());
        }
    }

    #[test]
    fn notifications_delay_coalesce_pause_expire_and_bound_the_visible_stack() {
        let mut center = NotificationCenter::default();
        center.push(
            nf::ToastType::Warning,
            "Cannot cut masked artwork".into(),
            None,
        );
        center.advance(Duration::from_secs(90), true, None);
        assert!(center.visible.is_empty());
        center.advance(Duration::from_secs(90), false, None);
        assert_eq!(center.visible[0].remaining, Some(Duration::from_secs(6)));
        center.visible[0].rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400., 46.));
        center.advance(Duration::from_secs(5), false, Some(egui::pos2(30., 20.)));
        assert_eq!(center.visible[0].remaining, Some(Duration::from_secs(6)));
        center.advance(Duration::from_secs(5), false, None);
        center.push(
            nf::ToastType::Warning,
            "Cannot cut masked artwork".into(),
            None,
        );
        assert_eq!(center.visible.len(), 1);
        assert_eq!(center.visible[0].count, 2);
        assert_eq!(center.visible[0].remaining, Some(Duration::from_secs(6)));
        center.advance(Duration::from_secs(5), false, None);
        assert_eq!(
            center.visible[0].remaining,
            Some(Duration::from_secs(6)),
            "dedup restarts at this frame, not the previous timer tick"
        );
        center.advance(Duration::from_secs(6), false, None);
        assert!(center.visible.is_empty());
        for kind in [
            nf::ToastType::Info,
            nf::ToastType::Success,
            nf::ToastType::Warning,
            nf::ToastType::Error,
        ] {
            center.push(kind, format!("{kind:?}"), None);
        }
        center.advance(Duration::ZERO, false, None);
        assert_eq!(center.visible.len(), 3);
        assert_eq!(center.visible.front().unwrap().kind, nf::ToastType::Success);
        assert_eq!(center.visible.back().unwrap().kind, nf::ToastType::Error);
        center.advance(Duration::from_secs(5), false, None);
        assert_eq!(center.visible.len(), 2);
        center.advance(Duration::from_secs(10), false, None);
        assert_eq!(center.visible.len(), 1);
        assert_eq!(center.visible[0].kind, nf::ToastType::Error);
    }
}
