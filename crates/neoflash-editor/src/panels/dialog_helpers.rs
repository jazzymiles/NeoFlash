//! Shared, presentation-only and keyboard-transaction helpers used by editor dialogs.

use crate::app::Editor;
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

pub(crate) fn mark_inline_edit_active(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("nf2-inline-edit-this-frame"), true));
}

pub(crate) fn prev_inline_edit_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("nf2-prev-inline-edit")))
        .unwrap_or(false)
}

/// Returns `true` when `Escape` on this pass is dismissing a focused text/number field, open popup,
/// or active inline panel rename/edit form from the previous frame (`egui::Focus::begin_pass` clears
/// focus on `Escape` before widgets run, and `Editor::shortcuts` runs before docked panels).
pub(crate) fn escape_dismissed_focus_or_popup(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
        && (prev_wants_keyboard_input(ctx) || prev_popup_open(ctx) || prev_inline_edit_active(ctx))
}

const TRACKED_RGBA_POPUPS_ID: &str = "nf2-tracked-rgba-popups";

/// Registers an RGBA colour popover (`paint_controls::show_color32_field_popup` or
/// `show_rgba_color_popup`) so [`is_any_popup_open`] and [`DialogKeyboardState`] treat an open
/// colour popover as a first-step `Escape` target and block unmodified Stage shortcuts.
pub(crate) fn track_rgba_popup(ctx: &egui::Context, response_id: egui::Id) {
    let open_id = response_id.with("nf2-rgba-popup-open");
    ctx.data_mut(|d| {
        let key = egui::Id::new(TRACKED_RGBA_POPUPS_ID);
        let mut list = d.get_temp::<Vec<egui::Id>>(key).unwrap_or_default();
        if !list.contains(&open_id) {
            list.push(open_id);
            d.insert_temp(key, list);
        }
    });
}

pub(crate) fn is_any_tracked_rgba_popup_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<Vec<egui::Id>>(egui::Id::new(TRACKED_RGBA_POPUPS_ID))
            .is_some_and(|list| {
                list.into_iter()
                    .any(|open_id| d.get_temp::<bool>(open_id).unwrap_or(false))
            })
    })
}

/// Whether any `egui::Popup` (Select, MenuBar, context menu, Timeline menu) or editor popover
/// (`Tools` flyout/popover, `Color` popover, RGBA colour popover, `Symbols & scenes`, `Help search`) is open.
pub(crate) fn is_any_popup_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || super::tools::is_rail_popover_open(ctx)
        || crate::authoring_modify::is_color_popover_open(ctx)
        || super::symbols_popover::is_symbols_popover_open(ctx)
        || super::function_search::is_function_search_open(ctx)
        || is_any_tracked_rgba_popup_open(ctx)
}

/// Editor-aware popup check used by global shortcut and Stage input gating. This includes custom
/// document-scoped areas which cannot be discovered through `egui::Popup::is_any_open`.
pub(crate) fn is_any_editor_popup_open(editor: &Editor, ctx: &egui::Context) -> bool {
    is_any_popup_open(ctx)
        || editor.is_layer_outline_color_popover_open(ctx)
        || editor.timeline_shape_hints.context_menu.is_some()
}

/// Whether unmodified `Enter` inside a local modal should be ignored because a control holds
/// keyboard focus or a popup/popover is open.
pub(crate) fn blocks_local_dialog_enter(ctx: &egui::Context) -> bool {
    ctx.wants_keyboard_input() || is_any_popup_open(ctx)
}

/// Records end-of-frame keyboard focus and popup state so the next pass can distinguish
/// field-level `Escape` (surrender focus) from dialog/Stage-level `Escape` (cancel/deselect).
pub(crate) fn record_end_of_frame_focus(ctx: &egui::Context) {
    let wants_kb = ctx.wants_keyboard_input();
    let popup_open = is_any_popup_open(ctx);
    ctx.data_mut(|d| {
        let inline_edit = d
            .get_temp::<bool>(egui::Id::new("nf2-inline-edit-this-frame"))
            .unwrap_or(false);
        d.insert_temp(egui::Id::new("nf2-inline-edit-this-frame"), false);
        d.insert_temp(egui::Id::new("nf2-prev-inline-edit"), inline_edit);
        d.insert_temp(egui::Id::new("nf2-prev-wants-kb"), wants_kb);
        d.insert_temp(egui::Id::new("nf2-prev-popup-open"), popup_open);

        // Controls register their stable popup ID every frame while they are painted. Retaining
        // closed IDs forever makes the context grow with every document/object/filter ever
        // inspected. Keep only popovers which must still gate shortcuts on the next frame; a
        // closed control registers itself again before it can reopen.
        let key = egui::Id::new(TRACKED_RGBA_POPUPS_ID);
        if let Some(mut list) = d.get_temp::<Vec<egui::Id>>(key) {
            list.retain(|open_id| d.get_temp::<bool>(*open_id).unwrap_or(false));
            if list.is_empty() {
                d.remove::<Vec<egui::Id>>(key);
            } else {
                d.insert_temp(key, list);
            }
        }
    });
}

impl DialogKeyboardState {
    pub(crate) fn capture(ctx: &egui::Context) -> Self {
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        Self {
            was_typing: ctx.wants_keyboard_input() || (esc && prev_wants_keyboard_input(ctx)),
            was_popup_open: is_any_popup_open(ctx) || (esc && prev_popup_open(ctx)),
        }
    }

    pub(crate) fn resolve<R>(
        self,
        ctx: &egui::Context,
        modal: &egui::ModalResponse<R>,
        allow_confirm: bool,
    ) -> DialogKeyboardOutcome {
        let popup_open = self.was_popup_open || is_any_popup_open(ctx);
        if popup_open {
            let _ = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked_count(ctx: &egui::Context) -> usize {
        ctx.data(|d| {
            d.get_temp::<Vec<egui::Id>>(egui::Id::new(TRACKED_RGBA_POPUPS_ID))
                .map_or(0, |ids| ids.len())
        })
    }

    #[test]
    fn t322_closed_rgba_popup_ids_are_pruned_but_open_popup_survives() {
        let ctx = egui::Context::default();
        let closed = egui::Id::new("closed-colour");
        let open = egui::Id::new("open-colour");
        track_rgba_popup(&ctx, closed);
        track_rgba_popup(&ctx, open);
        ctx.data_mut(|d| {
            d.insert_temp(closed.with("nf2-rgba-popup-open"), false);
            d.insert_temp(open.with("nf2-rgba-popup-open"), true);
        });

        record_end_of_frame_focus(&ctx);
        assert_eq!(tracked_count(&ctx), 1);
        assert!(is_any_tracked_rgba_popup_open(&ctx));

        ctx.data_mut(|d| d.insert_temp(open.with("nf2-rgba-popup-open"), false));
        record_end_of_frame_focus(&ctx);
        assert_eq!(tracked_count(&ctx), 0);
        assert!(!is_any_tracked_rgba_popup_open(&ctx));
    }
}
