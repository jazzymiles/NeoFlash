//! Shared, presentation-only and keyboard-transaction helpers used by editor dialogs.

use eframe::egui::{self, Ui};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

/// Normalized outcome of modal keyboard / backdrop handling (`T-224`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogKeyboardOutcome {
    None,
    Cancel,
    Confirm,
}

/// Snapshot of focus and popup state captured **before** painting an `egui::Modal` body.
///
/// Sampling `wants_keyboard_input()` and `Popup::is_any_open()` before the modal body runs ensures
/// that `Escape` or `Enter` pressed while a popup menu is open or while a `Field` / `TextField`
/// has keyboard focus closes the popup or commits/reverts that field first (surrendering focus)
/// instead of discarding or submitting the entire modal dialog on the same keystroke.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DialogKeyboardState {
    pub(crate) was_typing: bool,
    pub(crate) was_popup_open: bool,
}

pub(crate) fn prev_wants_keyboard_input(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-wants-kb")))
        .unwrap_or(false)
}

pub(crate) fn prev_popup_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-popup-open")))
        .unwrap_or(false)
}

/// Returns `true` when `Escape` on this pass is dismissing a focused text/number field or open popup
/// from the previous frame (`egui::Focus::begin_pass` clears focus on `Escape` before widgets run).
pub(crate) fn escape_dismissed_focus_or_popup(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
        && (prev_wants_keyboard_input(ctx) || prev_popup_open(ctx))
}

/// Records end-of-frame keyboard focus and popup state so the next pass can distinguish
/// field-level `Escape` (surrender focus) from dialog/Stage-level `Escape` (cancel/deselect).
pub(crate) fn record_end_of_frame_focus(ctx: &egui::Context) {
    let wants_kb = ctx.wants_keyboard_input();
    let popup_open = egui::Popup::is_any_open(ctx);
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new("nf2-prev-wants-kb"), wants_kb);
        d.insert_temp(egui::Id::new("nf2-prev-popup-open"), popup_open);
    });
}

impl DialogKeyboardState {
    pub(crate) fn capture(ctx: &egui::Context) -> Self {
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        Self {
            was_typing: ctx.wants_keyboard_input() || (esc && prev_wants_keyboard_input(ctx)),
            was_popup_open: egui::Popup::is_any_open(ctx) || (esc && prev_popup_open(ctx)),
        }
    }

    pub(crate) fn resolve<R>(
        self,
        ctx: &egui::Context,
        modal: &egui::ModalResponse<R>,
        allow_confirm: bool,
    ) -> DialogKeyboardOutcome {
        let popup_open = self.was_popup_open || egui::Popup::is_any_open(ctx);
        if popup_open {
            return DialogKeyboardOutcome::None;
        }
        if self.was_typing {
            let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if (esc || enter)
                && let Some(focused) = ctx.memory(|m| m.focused())
            {
                ctx.memory_mut(|m| m.surrender_focus(focused));
            }
            if modal.backdrop_response.clicked() {
                return DialogKeyboardOutcome::Cancel;
            }
            return DialogKeyboardOutcome::None;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            DialogKeyboardOutcome::Cancel
        } else if allow_confirm
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            DialogKeyboardOutcome::Confirm
        } else if modal.should_close() {
            DialogKeyboardOutcome::Cancel
        } else {
            DialogKeyboardOutcome::None
        }
    }
}

/// Overline caption of a dialog group ("OUTPUT", "RANGE").
pub(super) fn group(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    ui.label(rich(s, TextStyle::Overline).color(nf.text_tertiary));
}

/// Stage-preview placeholder (export / import dialogs): `bg/canvas` pasteboard with a `bg/stage` page.
pub(super) fn preview_box(ui: &mut Ui, width: f32, height: f32, label: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::LG as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
    ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
    ui.painter().text(
        stage.center(),
        egui::Align2::CENTER_CENTER,
        label,
        neoflash_ui::typography::font(TextStyle::Small),
        nf.text_tertiary,
    );
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
}
