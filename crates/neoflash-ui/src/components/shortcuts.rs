//! Preferences ▸ Shortcuts (R-046, owner-approved 5 November 2026, Figma `146:28332`): the grouped
//! command row with its shortcut cell, the recorder row (record → Accept / Cancel) and the conflict
//! strip with “Go to …”. Key capture and conflict detection stay in the editor; these widgets only
//! draw and report clicks.
use super::{Button, a11y, fill, galley, galley_truncated, text_at};
use crate::icons::{self, i16};
use crate::metrics::KBD;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, Ui};

/// Row height of the shortcut list.
pub const SHORTCUT_ROW: f32 = 32.0;
/// Left inset of command titles (under the group's Section Header chevron).
pub const SHORTCUT_INDENT: f32 = 28.0;
/// Width of the right-aligned shortcut cell.
pub const SHORTCUT_CELL: f32 = 140.0;
/// Width of the recorder field.
pub const SHORTCUT_RECORDER: f32 = 150.0;

const CHIP_GAP: f32 = 4.0;

fn chip_width(ui: &Ui, key: &str) -> f32 {
    let p = palette(ui.ctx());
    (galley(ui, key, TextStyle::Small, p.text_secondary).size().x + 2.0 * 6.0).max(20.0)
}

/// Width of the chips of `chords`, joined by “or”.
fn chords_width(ui: &Ui, chords: &[&[&str]]) -> f32 {
    let p = palette(ui.ctx());
    let or = galley(ui, "or", TextStyle::Small, p.text_tertiary).size().x;
    let mut w = 0.0;
    for (i, chord) in chords.iter().enumerate() {
        if i > 0 {
            w += CHIP_GAP + or + CHIP_GAP;
        }
        for (j, key) in chord.iter().enumerate() {
            if j > 0 {
                w += CHIP_GAP;
            }
            w += chip_width(ui, key);
        }
    }
    w
}

/// Paint `chords` as `NF2/Kbd` chips ending at `right`, vertically centred in `rect`.
fn paint_chords(ui: &Ui, rect: Rect, right: f32, chords: &[&[&str]]) {
    let p = palette(ui.ctx());
    let mut x = right - chords_width(ui, chords);
    for (i, chord) in chords.iter().enumerate() {
        if i > 0 {
            x += CHIP_GAP;
            let g = galley(ui, "or", TextStyle::Small, p.text_tertiary);
            let w = g.size().x;
            text_at(ui, x, rect, g, p.text_tertiary);
            x += w + CHIP_GAP;
        }
        for (j, key) in chord.iter().enumerate() {
            if j > 0 {
                x += CHIP_GAP;
            }
            let w = chip_width(ui, key);
            let chip = Rect::from_min_size(
                egui::pos2(x, rect.center().y - KBD / 2.0),
                egui::vec2(w, KBD),
            );
            fill(
                ui,
                chip,
                radius::SM,
                egui::Color32::TRANSPARENT,
                Some(p.border_default),
            );
            let g = galley(ui, key, TextStyle::Small, p.text_secondary);
            let gx = chip.center().x - g.size().x / 2.0;
            text_at(ui, gx, chip, g, p.text_secondary);
            x += w;
        }
    }
}

/// One command of the shortcut list: title, optional “Also on …” note and the shortcut cell.
/// `chords` holds the key labels of each chord (`[["⇧", "F6"]]`); empty = unassigned (“—”, on
/// hover “Add shortcut”). Several chords are joined by “or”.
pub struct ShortcutRow<'a> {
    title: &'a str,
    chords: &'a [&'a [&'a str]],
    conflict: Option<&'a str>,
    selected: bool,
}

/// Clicks of a [`ShortcutRow`].
pub struct ShortcutRowResponse {
    /// The whole row (select; secondary click opens the row menu).
    pub row: Response,
    /// The shortcut cell: start recording.
    pub cell: Response,
}

impl<'a> ShortcutRow<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            chords: &[],
            conflict: None,
            selected: false,
        }
    }
    pub fn chords(mut self, chords: &'a [&'a [&'a str]]) -> Self {
        self.chords = chords;
        self
    }
    /// Warning note before the cell, e.g. “Also on Insert ▸ Keyframe”.
    pub fn conflict(mut self, note: &'a str) -> Self {
        self.conflict = Some(note);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn show(self, ui: &mut Ui, width: f32) -> ShortcutRowResponse {
        let (rect, row) = ui.allocate_exact_size(egui::vec2(width, SHORTCUT_ROW), Sense::click());
        let p = palette(ui.ctx());
        let cell_rect = Rect::from_min_max(
            egui::pos2(rect.right() - space::S3 - SHORTCUT_CELL, rect.top()),
            egui::pos2(rect.right() - space::S3, rect.bottom()),
        );
        let cell = ui.interact(cell_rect, row.id.with("cell"), Sense::click());
        if self.selected {
            ui.painter().rect_filled(rect, cr(0.0), p.bg_selected);
        } else if row.hovered() || cell.hovered() {
            ui.painter().rect_filled(rect, cr(0.0), p.bg_hover);
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        let assigned = !self.chords.is_empty();
        // Cell.
        if assigned {
            paint_chords(ui, cell_rect, cell_rect.right(), self.chords);
        } else {
            let (label, color) = if cell.hovered() {
                ("Add shortcut", p.accent_default)
            } else {
                ("—", p.text_tertiary)
            };
            let g = galley(ui, label, TextStyle::Small, color);
            let x = cell_rect.right() - g.size().x;
            text_at(ui, x, cell_rect, g, color);
        }
        // Conflict note, right-aligned before the chips.
        let used = if assigned {
            chords_width(ui, self.chords)
        } else {
            0.0
        };
        let mut title_right = cell_rect.right() - used.max(SHORTCUT_CELL) - space::S2;
        if let Some(note) = self.conflict {
            let g = galley(ui, note, TextStyle::Small, p.warning_default);
            let right = cell_rect.right() - used - space::S3;
            let x = right - g.size().x;
            let icon = Rect::from_center_size(
                egui::pos2(x - space::S1 - 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            );
            icons::paint(
                ui.painter(),
                ui.ctx(),
                icon,
                i16::WARNING,
                p.warning_default,
            );
            text_at(ui, x, rect, g, p.warning_default);
            title_right = title_right.min(icon.left() - space::S2);
        }
        // Title.
        let fg = if assigned {
            p.text_primary
        } else {
            p.text_secondary
        };
        let x = rect.left() + SHORTCUT_INDENT;
        let g = galley_truncated(ui, self.title, TextStyle::Body, fg, title_right - x);
        text_at(ui, x, rect, g, fg);
        a11y(
            &row,
            egui::WidgetType::SelectableLabel,
            self.title,
            Some(self.selected),
        );
        let cell_name = if assigned {
            format!("Change shortcut for {}", self.title)
        } else {
            format!("Add shortcut for {}", self.title)
        };
        a11y(&cell, egui::WidgetType::Button, &cell_name, None);
        ShortcutRowResponse { row, cell }
    }
}

/// The row while recording: selected fill, title, recorder field (“Press keys…” or the captured
/// chips) and Cancel / Accept. `captured` = the chord pressed so far. With `conflict_with` (menu
/// path of the other command) a warning strip with “Go to …” follows the row.
pub struct ShortcutRecorder<'a> {
    title: &'a str,
    captured: Option<&'a [&'a str]>,
    conflict_with: Option<&'a str>,
}

/// Clicks of a [`ShortcutRecorder`]. Esc / ⌫ handling stays with the caller's key capture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShortcutRecorderResponse {
    pub accept: bool,
    pub cancel: bool,
    /// “Go to <command>” in the conflict strip.
    pub go_to: bool,
}

impl<'a> ShortcutRecorder<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            captured: None,
            conflict_with: None,
        }
    }
    pub fn captured(mut self, chord: &'a [&'a str]) -> Self {
        self.captured = Some(chord);
        self
    }
    pub fn conflict_with(mut self, menu_path: &'a str) -> Self {
        self.conflict_with = Some(menu_path);
        self
    }

    pub fn show(self, ui: &mut Ui, width: f32) -> ShortcutRecorderResponse {
        let p = palette(ui.ctx());
        let mut out = ShortcutRecorderResponse::default();
        let (rect, row) =
            ui.allocate_exact_size(egui::vec2(width, SHORTCUT_ROW + space::S2), Sense::hover());
        fill(ui, rect, radius::MD, p.bg_selected, None);
        // Buttons, right to left.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(row.id.with("recorder-buttons"))
                .max_rect(rect.shrink2(egui::vec2(space::S2, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = space::S2;
        if self.captured.is_some() {
            out.accept = child.add(Button::primary("Accept")).clicked();
            out.cancel = child.add(Button::new("Cancel")).clicked();
        } else {
            out.cancel = child.add(Button::ghost("Cancel")).clicked();
        }
        let buttons_left = child.min_rect().left();
        // Recorder field.
        let field = Rect::from_min_size(
            egui::pos2(
                buttons_left - space::S2 - SHORTCUT_RECORDER,
                rect.center().y - 14.0,
            ),
            egui::vec2(SHORTCUT_RECORDER, 28.0),
        );
        fill(ui, field, radius::MD, p.bg_field, Some(p.border_focus));
        match self.captured {
            Some(chord) => paint_chords(ui, field, field.right() - space::S2, &[chord]),
            None => {
                let g = galley(ui, "Press keys…", TextStyle::Small, p.text_tertiary);
                let x = field.right() - space::S2 - g.size().x;
                text_at(ui, x, field, g, p.text_tertiary);
            }
        }
        let x = rect.left() + SHORTCUT_INDENT;
        let g = galley_truncated(
            ui,
            self.title,
            TextStyle::Body,
            p.text_primary,
            field.left() - space::S2 - x,
        );
        text_at(ui, x, rect, g, p.text_primary);
        a11y(
            &row,
            egui::WidgetType::TextEdit,
            &format!("Recording shortcut for {}", self.title),
            None,
        );
        if let (Some(chord), Some(other)) = (self.captured, self.conflict_with) {
            ui.add_space(space::S1);
            let text = format!(
                "{} is already used by {other}. Accept keeps both — only the first in menu order runs.",
                chord.concat()
            );
            out.go_to = conflict_strip(ui, width, &text, other);
        }
        out
    }
}

/// Warning strip under a recorder row (`warning/subtle` fill, warning icon and text, Ghost
/// “Go to <menu path>”). Returns `true` when Go to was clicked.
pub fn conflict_strip(ui: &mut Ui, width: f32, text: &str, path: &str) -> bool {
    let p = palette(ui.ctx());
    let go_label = format!("Go to {path}");
    let button_w = galley(ui, &go_label, TextStyle::BodyStrong, p.text_primary)
        .size()
        .x
        + 2.0 * space::S3;
    let text_w = width - SHORTCUT_INDENT - 16.0 - space::S2 - button_w - 2.0 * space::S2;
    let g = super::galley_wrapped(ui, text, TextStyle::Small, p.warning_default, text_w);
    let height = (g.size().y + 2.0 * 6.0).max(SHORTCUT_ROW);
    let (rect, strip) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    fill(ui, rect, radius::MD, p.warning_subtle, None);
    let icon = Rect::from_min_size(
        egui::pos2(rect.left() + SHORTCUT_INDENT, rect.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    icons::paint(
        ui.painter(),
        ui.ctx(),
        icon,
        i16::WARNING,
        p.warning_default,
    );
    let tx = icon.right() + space::S2;
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - g.size().y / 2.0),
        g,
        p.warning_default,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(strip.id.with("go-to"))
            .max_rect(rect.shrink2(egui::vec2(space::S2, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.add(Button::ghost(&go_label)).clicked()
}
