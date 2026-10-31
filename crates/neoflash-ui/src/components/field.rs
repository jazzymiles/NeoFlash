//! `NF2/Field`, `NF2/Search`, `NF2/Select`, `NF2/Stepper Field`, `NF2/Combo Field`, `NF2/Color Field`,
//! `NF2/Line Style Select`.
//!
//! Field interaction rules (§13.4): drag the prefix to scrub, ↑/↓ step, ⇧ = ×10, Enter commits,
//! Escape restores the value from before editing, units are shown in the value ("5.00 s", "100 %").
use super::{State, a11y, fill, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::CONTROL;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget};
use std::ops::RangeInclusive;

/// Run `content` in a child Ui over `rect` **without moving the parent's cursor** (the field rect is
/// already allocated). `scope_builder` would re-allocate the inner rect and pull the row cursor back,
/// leaving ~2 pt instead of the 8 pt gap before the next control.
fn inside<R>(ui: &mut Ui, rect: Rect, content: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    content(&mut child)
}

/// Shared frame of every field: `bg/field`, 1 px border, 2 px accent border when focused.
fn frame(ui: &Ui, rect: Rect, s: State, editing: bool) {
    let p = palette(ui.ctx());
    let (bg, border, w) = if !s.enabled {
        (p.bg_panel, p.border_subtle, 1.0)
    } else if editing {
        (p.bg_field, p.border_focus, 2.0)
    } else if s.hovered {
        (p.bg_field, p.border_strong, 1.0)
    } else {
        (p.bg_field, p.border_default, 1.0_f32)
    };
    ui.painter().rect_filled(rect, cr(radius::MD), bg);
    ui.painter().rect_stroke(
        rect,
        cr(radius::MD),
        Stroke::new(w, border),
        StrokeKind::Inside,
    );
}

fn edit_style(ui: &mut Ui, fg: Color32) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(fg);
    v.extreme_bg_color = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_stroke = Stroke::NONE;
    // egui paints selected text in `selection.stroke.color`; keep it visible, without a border.
    v.selection.stroke = Stroke::new(0.0_f32, fg);
}

/// `NF2/Field` (numeric) — optional prefix ("W", "FPS", "α"), value, unit.
pub struct Field<'a> {
    value: &'a mut f64,
    prefix: &'a str,
    unit: &'a str,
    range: RangeInclusive<f64>,
    step: f64,
    decimals: usize,
    width: Option<f32>,
    label: &'a str,
    stepper: bool,
    presets: &'a [f64],
    frameless: bool,
}

impl<'a> Field<'a> {
    /// `label` is the accessible name ("Width"); `prefix` is the short visible caption ("W").
    pub fn new(value: &'a mut f64, label: &'a str) -> Self {
        Self {
            value,
            prefix: "",
            unit: "",
            range: f64::MIN..=f64::MAX,
            step: 1.0,
            decimals: 0,
            width: None,
            label,
            stepper: false,
            presets: &[],
            frameless: false,
        }
    }
    /// `NF2/Stepper Field`: ▲▼ buttons on the right (Size, Sides, Repeat count).
    pub fn stepper(mut self) -> Self {
        self.stepper = true;
        self
    }
    /// `NF2/Combo Field`: ▾ preset menu on the right (stroke size, angles, inner radius).
    pub fn presets(mut self, presets: &'a [f64]) -> Self {
        self.presets = presets;
        self
    }
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = range;
        self
    }
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

#[derive(Clone)]
struct Draft {
    text: String,
    before: f64,
}

impl Widget for Field<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = self
            .width
            .unwrap_or(ui.available_width().clamp(64.0, 160.0));
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::hover());
        let id = response.id;
        let edit_id = id.with("edit");
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let clamp = |v: f64| v.clamp(lo, hi);
        let fmt = |v: f64| {
            let n = format!("{v:.*}", self.decimals);
            if self.unit.is_empty() {
                n
            } else {
                format!("{n} {}", self.unit)
            }
        };
        let mut changed = false;
        let focused = ui.memory(|m| m.has_focus(edit_id));
        let s = State::of(ui, &response);
        if !self.frameless {
            frame(ui, rect, s, focused);
        }

        // Prefix: drag to scrub.
        let mut x = rect.left() + space::S3;
        if !self.prefix.is_empty() {
            let g = galley(ui, self.prefix, TextStyle::Small, p.text_tertiary);
            let prefix_rect = Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(x + g.size().x + space::S1, rect.bottom()),
            );
            let scrub = ui.interact(prefix_rect, id.with("scrub"), Sense::drag());
            if scrub.hovered() || scrub.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if scrub.dragged() {
                let k = if ui.input(|i| i.modifiers.shift) {
                    10.0
                } else {
                    1.0
                };
                let d = scrub.drag_delta().x as f64 * self.step * k;
                if d != 0.0 {
                    *self.value = clamp(*self.value + d);
                    changed = true;
                }
            }
            let fg = if ui.is_enabled() {
                p.text_tertiary
            } else {
                p.text_disabled
            };
            text_at(ui, x, rect, g, fg);
            x = prefix_rect.right() + space::S1;
        }

        // Value: a frameless TextEdit holding a draft while focused.
        let mut draft = ui
            .data(|d| d.get_temp::<Draft>(id))
            .unwrap_or_else(|| Draft {
                text: fmt(*self.value),
                before: *self.value,
            });
        if !focused {
            draft = Draft {
                text: fmt(*self.value),
                before: *self.value,
            };
        }
        let trailing = if !self.presets.is_empty() {
            32.0
        } else if self.stepper {
            20.0
        } else {
            0.0
        };
        let value_rect = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(rect.right() - space::S2 - trailing, rect.bottom()),
        );
        if self.stepper && s.enabled {
            let arrows = Rect::from_min_max(
                egui::pos2(rect.right() - 22.0, rect.top() + 4.0),
                egui::pos2(rect.right() - 4.0, rect.bottom() - 4.0),
            );
            let halves = [
                (
                    Rect::from_min_max(arrows.min, egui::pos2(arrows.right(), arrows.center().y)),
                    1.0,
                    i16::CHEVUP,
                    "Increase",
                ),
                (
                    Rect::from_min_max(egui::pos2(arrows.left(), arrows.center().y), arrows.max),
                    -1.0,
                    i16::CHEVDOWN,
                    "Decrease",
                ),
            ];
            for (r, dir, icon, name) in halves {
                let b = ui.interact(r, id.with(name), Sense::click());
                let c = if b.hovered() {
                    p.text_primary
                } else {
                    p.text_tertiary
                };
                icons::paint_sized(ui.painter(), ui.ctx(), r, icon, 12.0, c);
                a11y(&b, egui::WidgetType::Button, name, None);
                if b.clicked() {
                    let k = if ui.input(|i| i.modifiers.shift) {
                        10.0
                    } else {
                        1.0
                    };
                    *self.value = clamp(*self.value + dir * self.step * k);
                    changed = true;
                }
            }
        }
        if !self.presets.is_empty() {
            let btn = Rect::from_min_max(egui::pos2(rect.right() - 32.0, rect.top()), rect.max);
            ui.painter().vline(
                btn.left(),
                btn.y_range().shrink(1.0),
                Stroke::new(1.0_f32, p.border_default),
            );
            let b = ui.interact(btn, id.with("presets"), Sense::click());
            let c = if !s.enabled {
                p.text_disabled
            } else if b.hovered() {
                p.text_primary
            } else {
                p.text_secondary
            };
            icons::paint(ui.painter(), ui.ctx(), btn, i16::CHEVDOWN, c);
            a11y(&b, egui::WidgetType::ComboBox, "Presets", None);
            let picked = super::menu::popup_below(&b, 120.0, |ui| {
                let mut picked = None;
                for v in self.presets {
                    if ui.add(super::MenuItem::new(fmt(*v).as_str())).clicked() {
                        picked = Some(*v);
                    }
                }
                picked
            });
            if let Some(Some(v)) = picked {
                *self.value = clamp(v);
                changed = true;
            }
        }
        let fg = if ui.is_enabled() {
            p.text_primary
        } else {
            p.text_disabled
        };
        let edit = inside(ui, value_rect, |ui| {
            edit_style(ui, fg);
            ui.add(
                egui::TextEdit::singleline(&mut draft.text)
                    .id(edit_id)
                    .frame(false)
                    .font(crate::typography::font(TextStyle::Body))
                    .margin(egui::Margin::ZERO)
                    .desired_width(value_rect.width())
                    .min_size(value_rect.size())
                    .vertical_align(egui::Align::Center),
            )
        });
        if edit.has_focus() {
            let (up, down, shift, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::Escape),
                )
            });
            let k = if shift { 10.0 } else { 1.0 };
            if up || down {
                let base = parse_number(&draft.text).unwrap_or(*self.value);
                *self.value = clamp(base + if up { self.step * k } else { -self.step * k });
                draft.text = fmt(*self.value);
                changed = true;
            }
            if esc {
                *self.value = draft.before;
                draft.text = fmt(draft.before);
                changed = true;
                edit.surrender_focus();
            }
        }
        if edit.lost_focus() {
            if let Some(v) = parse_number(&draft.text) {
                let v = clamp(v);
                if v != *self.value {
                    *self.value = v;
                    changed = true;
                }
            }
            draft.text = fmt(*self.value);
        }
        ui.data_mut(|d| d.insert_temp(id, draft));
        let mut response = response | edit;
        if changed {
            response.mark_changed();
        }
        let value = *self.value;
        let enabled = response.enabled();
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, enabled, self.label);
            info.value = Some(value);
            info
        });
        response
    }
}

/// Parse "960", "5.00 s", "100 %", "-12,5". Ignores units.
pub fn parse_number(text: &str) -> Option<f64> {
    let t: String = text
        .trim()
        .replace(',', ".")
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect();
    t.parse().ok()
}

/// `NF2/Field` (text) — prefix + free text ("Name  Ideas in motion").
pub struct TextField<'a> {
    text: &'a mut String,
    prefix: &'a str,
    hint: &'a str,
    width: Option<f32>,
    label: &'a str,
    mono: bool,
}

impl<'a> TextField<'a> {
    pub fn new(text: &'a mut String, label: &'a str) -> Self {
        Self {
            text,
            prefix: "",
            hint: "",
            width: None,
            label,
            mono: false,
        }
    }
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Roboto Mono value (hex, IDs, paths).
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    /// Id of the inner text editor for a `TextField` response (to request focus or read its cursor).
    pub fn edit_id(response_id: egui::Id) -> egui::Id {
        response_id.with("edit")
    }
}

impl Widget for TextField<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = self.width.unwrap_or(ui.available_width());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::hover());
        let edit_id = response.id.with("edit");
        let focused = ui.memory(|m| m.has_focus(edit_id));
        frame(ui, rect, State::of(ui, &response), focused);
        let mut x = rect.left() + space::S3;
        if !self.prefix.is_empty() {
            let g = galley(ui, self.prefix, TextStyle::Small, p.text_tertiary);
            let w = g.size().x;
            text_at(ui, x, rect, g, p.text_tertiary);
            x += w + space::S2;
        }
        let r = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(rect.right() - space::S2, rect.bottom()),
        );
        let fg = if ui.is_enabled() {
            p.text_primary
        } else {
            p.text_disabled
        };
        let style = if self.mono {
            TextStyle::Mono
        } else {
            TextStyle::Body
        };
        let edit = inside(ui, r, |ui| {
            edit_style(ui, fg);
            ui.add(
                egui::TextEdit::singleline(self.text)
                    .id(edit_id)
                    .frame(false)
                    .hint_text(crate::typography::rich(self.hint, style).color(p.text_tertiary))
                    .font(crate::typography::font(style))
                    .margin(egui::Margin::ZERO)
                    .desired_width(r.width())
                    .min_size(r.size())
                    .vertical_align(egui::Align::Center),
            )
        });
        let response = response | edit;
        a11y(&response, egui::WidgetType::TextEdit, self.label, None);
        response
    }
}

/// `NF2/Search` — magnifier, text, clear ✕ when `Filled`.
pub struct Search<'a> {
    text: &'a mut String,
    hint: &'a str,
    width: Option<f32>,
}

impl<'a> Search<'a> {
    pub fn new(text: &'a mut String, hint: &'a str) -> Self {
        Self {
            text,
            hint,
            width: None,
        }
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl Widget for Search<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = self.width.unwrap_or(ui.available_width());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::hover());
        let edit_id = response.id.with("edit");
        let focused = ui.memory(|m| m.has_focus(edit_id));
        frame(ui, rect, State::of(ui, &response), focused);
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + space::S3 + 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            icon_rect,
            i16::SEARCH,
            p.text_tertiary,
        );
        let clear_w = if self.text.is_empty() { 0.0 } else { 24.0 };
        let r = Rect::from_min_max(
            egui::pos2(icon_rect.right() + space::S2, rect.top()),
            egui::pos2(rect.right() - space::S2 - clear_w, rect.bottom()),
        );
        let edit = inside(ui, r, |ui| {
            edit_style(ui, p.text_primary);
            ui.add(
                egui::TextEdit::singleline(self.text)
                    .id(edit_id)
                    .frame(false)
                    .hint_text(
                        crate::typography::rich(self.hint, TextStyle::Body).color(p.text_tertiary),
                    )
                    .font(crate::typography::font(TextStyle::Body))
                    .margin(egui::Margin::ZERO)
                    .desired_width(r.width())
                    .min_size(r.size())
                    .vertical_align(egui::Align::Center),
            )
        });
        let mut response = response | edit;
        if !self.text.is_empty() {
            let cr_ = Rect::from_center_size(
                egui::pos2(rect.right() - space::S2 - 12.0, rect.center().y),
                egui::vec2(24.0, 24.0),
            );
            let clear = ui.interact(cr_, response.id.with("clear"), Sense::click());
            let c = if clear.hovered() {
                p.text_primary
            } else {
                p.text_tertiary
            };
            icons::paint(ui.painter(), ui.ctx(), cr_, i16::X, c);
            let clear = super::display::tooltip(clear, "Clear search", Some("Esc"));
            let esc = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
            if clear.clicked() || esc {
                self.text.clear();
                response.mark_changed();
            }
        }
        a11y(&response, egui::WidgetType::TextEdit, self.hint, None);
        response
    }
}

/// `NF2/Select` — closed state looks like a field with ▾; the popup holds `NF2/Menu Item`s.
pub struct Select<'a> {
    id_salt: egui::Id,
    label: &'a str,
    width: Option<f32>,
    icon: Option<Icon>,
}

impl<'a> Select<'a> {
    pub fn new(id_salt: impl std::hash::Hash, label: &'a str) -> Self {
        Self {
            id_salt: egui::Id::new(id_salt),
            label,
            width: None,
            icon: None,
        }
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Show the closed select; `menu` fills the popup (use [`super::MenuItem`]).
    pub fn show<R>(self, ui: &mut Ui, menu: impl FnOnce(&mut Ui) -> R) -> (Response, Option<R>) {
        let p = palette(ui.ctx());
        let width = self.width.unwrap_or(ui.available_width().min(200.0));
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::click());
        let response = ui.interact(rect, ui.id().with(self.id_salt), Sense::click()) | response;
        let s = State::of(ui, &response);
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        let (bg, border) = if !s.enabled {
            (p.bg_panel, p.border_subtle)
        } else if s.hovered || open {
            (p.bg_hover, p.border_strong)
        } else {
            (p.bg_raised, p.border_default)
        };
        fill(ui, rect, radius::MD, bg, Some(border));
        let fg = if s.enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        let mut x = rect.left() + space::S3;
        if let Some(icon) = self.icon {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(x + 8.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                icon,
                p.text_secondary,
            );
            x += 16.0 + space::S2;
        }
        let max = rect.right() - 32.0 - x;
        let g = galley(ui, self.label, TextStyle::Body, fg);
        let clip = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(x + max, rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).galley(
            egui::pos2(x, rect.center().y - g.size().y / 2.0),
            g,
            fg,
        );
        let chev = Rect::from_center_size(
            egui::pos2(rect.right() - space::S3 - 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            chev,
            i16::CHEVDOWN,
            if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            },
        );
        super::focus(ui, s, rect, radius::MD);
        a11y(&response, egui::WidgetType::ComboBox, self.label, None);
        let inner = super::menu::popup_below(&response, width, menu);
        (response, inner)
    }

    /// Convenience: choose one of `options`; returns `true` when the value changed.
    pub fn options<T: PartialEq + Copy>(
        ui: &mut Ui,
        id_salt: impl std::hash::Hash,
        value: &mut T,
        options: &[(T, &str)],
        width: f32,
    ) -> bool {
        let current = options
            .iter()
            .find(|(v, _)| v == value)
            .map_or("—", |(_, l)| *l);
        let mut changed = false;
        Select::new(id_salt, current).width(width).show(ui, |ui| {
            for (v, l) in options {
                if ui
                    .add(super::MenuItem::new(l).checked(*v == *value))
                    .clicked()
                {
                    *value = *v;
                    changed = true;
                }
            }
        });
        changed
    }
}

/// `NF2/Color Field` — swatch, hex (mono), alpha %. Clicking the swatch reports `swatch_clicked`.
pub struct ColorField<'a> {
    color: &'a mut Color32,
    label: &'a str,
    width: Option<f32>,
}

pub struct ColorFieldResponse {
    pub response: Response,
    pub swatch_clicked: bool,
}

impl<'a> ColorField<'a> {
    pub fn new(color: &'a mut Color32, label: &'a str) -> Self {
        Self {
            color,
            label,
            width: None,
        }
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn show(self, ui: &mut Ui) -> ColorFieldResponse {
        let p = palette(ui.ctx());
        let width = self.width.unwrap_or(ui.available_width().min(200.0));
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::hover());
        let id = response.id;
        frame(ui, rect, State::of(ui, &response), false);
        let sw = Rect::from_center_size(
            egui::pos2(rect.left() + 6.0 + 9.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        super::display::paint_swatch(ui, sw, *self.color, radius::SM);
        let swatch_clicked = super::display::tooltip(
            ui.interact(sw, id.with("swatch"), Sense::click()),
            "Open colour picker",
            None,
        )
        .clicked();
        let alpha_w = 64.0;
        let hex_rect = Rect::from_min_max(
            egui::pos2(sw.right() + space::S2, rect.top()),
            egui::pos2(rect.right() - alpha_w, rect.bottom()),
        );
        let [r, g, b, a] = self.color.to_srgba_unmultiplied();
        let mut hex = ui
            .data(|d| d.get_temp::<String>(id.with("hex")))
            .unwrap_or_else(|| format!("{r:02X}{g:02X}{b:02X}"));
        let hex_id = id.with("hex-edit");
        if !ui.memory(|m| m.has_focus(hex_id)) {
            hex = format!("{r:02X}{g:02X}{b:02X}");
        }
        let mut changed = false;
        let hex_resp = inside(ui, hex_rect, |ui| {
            edit_style(ui, p.text_primary);
            ui.add(
                egui::TextEdit::singleline(&mut hex)
                    .id(hex_id)
                    .frame(false)
                    .font(crate::typography::font(TextStyle::Mono))
                    .margin(egui::Margin::ZERO)
                    .char_limit(7)
                    .desired_width(hex_rect.width())
                    .min_size(hex_rect.size())
                    .vertical_align(egui::Align::Center),
            )
        });
        if hex_resp.lost_focus()
            && let Some(c) = parse_hex(&hex)
        {
            *self.color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a);
            changed = true;
        }
        ui.data_mut(|d| d.insert_temp(id.with("hex"), hex));
        ui.painter().vline(
            rect.right() - alpha_w,
            rect.y_range().shrink(8.0),
            Stroke::new(1.0_f32, p.border_default),
        );
        let mut alpha = (a as f64 / 255.0 * 100.0).round();
        let alpha_rect = Rect::from_min_max(
            egui::pos2(rect.right() - alpha_w + 1.0, rect.top()),
            rect.max,
        );
        let alpha_resp = inside(ui, alpha_rect, |ui| {
            ui.add(Field {
                frameless: true,
                ..Field::new(&mut alpha, "Alpha")
                    .unit("%")
                    .range(0.0..=100.0)
                    .width(alpha_w - 1.0)
            })
        });
        if alpha_resp.changed() {
            let [r, g, b, _] = self.color.to_srgba_unmultiplied();
            *self.color =
                Color32::from_rgba_unmultiplied(r, g, b, (alpha / 100.0 * 255.0).round() as u8);
            changed = true;
        }
        let mut response = response | hex_resp | alpha_resp;
        if changed {
            response.mark_changed();
        }
        a11y(&response, egui::WidgetType::ColorButton, self.label, None);
        ColorFieldResponse {
            response,
            swatch_clicked,
        }
    }
}

pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let h = text.trim().trim_start_matches('#');
    let h = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_owned(),
        _ => return None,
    };
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// `Style` of `NF2/Line Style Select`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Ragged,
    Stipple,
    Hatched,
}

impl LineStyle {
    pub const ALL: [Self; 6] = [
        Self::Solid,
        Self::Dashed,
        Self::Dotted,
        Self::Ragged,
        Self::Stipple,
        Self::Hatched,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
            Self::Ragged => "Ragged",
            Self::Stipple => "Stipple",
            Self::Hatched => "Hatched",
        }
    }
    /// Paint the stroke preview into `rect`.
    pub fn paint(self, painter: &egui::Painter, rect: Rect, color: Color32) {
        let y = rect.center().y;
        let st = Stroke::new(1.5_f32, color);
        match self {
            Self::Solid => {
                painter.hline(rect.x_range(), y, st);
            }
            Self::Dashed | Self::Dotted | Self::Stipple => {
                let (dash, gap) = match self {
                    Self::Dashed => (6.0, 4.0),
                    Self::Dotted => (1.5, 3.0),
                    _ => (1.0, 1.5),
                };
                painter.extend(egui::Shape::dashed_line(
                    &[egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    st,
                    dash,
                    gap,
                ));
            }
            Self::Ragged => {
                let pts: Vec<_> = (0..=((rect.width() / 4.0) as usize))
                    .map(|i| {
                        egui::pos2(
                            rect.left() + i as f32 * 4.0,
                            y + if i % 2 == 0 { -2.0 } else { 2.0 },
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, st));
            }
            Self::Hatched => {
                let mut x = rect.left();
                while x < rect.right() {
                    painter.line_segment(
                        [egui::pos2(x, y - 5.0), egui::pos2(x + 1.5, y + 5.0)],
                        Stroke::new(1.0_f32, color),
                    );
                    x += 3.0;
                }
            }
        }
    }
}

/// `NF2/Line Style Select` — select whose label is followed by the stroke preview.
pub struct LineStyleSelect;

impl LineStyleSelect {
    pub fn show(
        ui: &mut Ui,
        id_salt: impl std::hash::Hash,
        value: &mut LineStyle,
        width: f32,
    ) -> bool {
        let p = palette(ui.ctx());
        let mut changed = false;
        let (resp, _) = Select::new(id_salt, value.label())
            .width(width)
            .show(ui, |ui| {
                for s in LineStyle::ALL {
                    if ui
                        .add(super::MenuItem::new(s.label()).checked(s == *value))
                        .clicked()
                    {
                        *value = s;
                        changed = true;
                    }
                }
            });
        let r = resp.rect;
        let preview = Rect::from_min_max(
            egui::pos2(r.left() + 84.0, r.top()),
            egui::pos2(r.right() - 36.0, r.bottom()),
        );
        if preview.width() > 16.0 {
            value.paint(ui.painter(), preview, p.text_primary);
        }
        changed
    }
}
