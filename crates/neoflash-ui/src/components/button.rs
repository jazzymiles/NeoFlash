//! `NF2/Button`, `NF2/Icon Button`, `NF2/Tool Button`.
use super::{State, a11y, fill, focus, galley, text_at};
use crate::icons::{self, Icon};
use crate::metrics::{CONTROL, ICON_BUTTON_MD, TOOL_BUTTON};
use crate::theme::palette;
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Color32, Response, Sense, Ui, Widget};

/// `Type` of `NF2/Button`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonType {
    /// The one obvious action of a screen (Publish, Save, Apply, Convert).
    Primary,
    /// Bordered neutral action (Test Movie, Cancel, Publish settings…).
    #[default]
    Secondary,
    /// Text-only action inside dense rows and footers.
    Ghost,
    /// Destructive action (Discard, Delete). Red is reserved for this and for errors.
    Danger,
}

/// `NF2/Button` — 32 px high, 12 px side padding, optional 16 px icon, label in `NF2/Body`.
#[must_use = "add it with ui.add(…)"]
pub struct Button<'a> {
    label: &'a str,
    icon: Option<Icon>,
    kind: ButtonType,
    width: Option<f32>,
    shortcut: Option<&'a str>,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            kind: ButtonType::Secondary,
            width: None,
            shortcut: None,
        }
    }
    pub fn primary(label: &'a str) -> Self {
        Self::new(label).kind(ButtonType::Primary)
    }
    pub fn ghost(label: &'a str) -> Self {
        Self::new(label).kind(ButtonType::Ghost)
    }
    pub fn danger(label: &'a str) -> Self {
        Self::new(label).kind(ButtonType::Danger)
    }
    pub fn kind(mut self, kind: ButtonType) -> Self {
        self.kind = kind;
        self
    }
    /// `Icon` boolean of the Figma component + the swapped icon.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Fixed width (dialog footers); otherwise the button hugs its content.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Shortcut shown in the tooltip (e.g. "⌘↵").
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let text_probe = galley(ui, self.label, TextStyle::Body, Color32::WHITE);
        let icon_w = if self.icon.is_some() {
            16.0 + space::S2
        } else {
            0.0
        };
        let hug = space::S3 * 2.0 + icon_w + text_probe.size().x;
        let width = self
            .width
            .unwrap_or(hug)
            .max(hug.min(self.width.unwrap_or(hug)));
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, CONTROL), Sense::click());
        let s = State::of(ui, &response);
        let (bg, border, fg) = match (self.kind, s.enabled, s.hovered) {
            (_, false, _) => (p.bg_hover, None, p.text_disabled),
            (ButtonType::Primary, true, false) => (p.accent_default, None, p.text_on_accent),
            (ButtonType::Primary, true, true) => (p.accent_hover, None, p.text_on_accent),
            (ButtonType::Secondary, true, false) => {
                (p.bg_raised, Some(p.border_default), p.text_primary)
            }
            (ButtonType::Secondary, true, true) => {
                (p.bg_hover, Some(p.border_strong), p.text_primary)
            }
            (ButtonType::Ghost, true, false) => (Color32::TRANSPARENT, None, p.text_secondary),
            (ButtonType::Ghost, true, true) => (p.bg_hover, None, p.text_primary),
            (ButtonType::Danger, true, false) => (p.danger_subtle, None, p.danger_default),
            (ButtonType::Danger, true, true) => {
                (p.danger_subtle, Some(p.danger_default), p.danger_default)
            }
        };
        if ui.is_rect_visible(rect) {
            fill(ui, rect, radius::MD, bg, border);
            let content_w = icon_w + text_probe.size().x;
            let mut x = rect.center().x - content_w / 2.0;
            if let Some(icon) = self.icon {
                let r = egui::Rect::from_center_size(
                    egui::pos2(x + 8.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                );
                icons::paint(ui.painter(), ui.ctx(), r, icon, fg);
                x += icon_w;
            }
            text_at(ui, x, rect, galley(ui, self.label, TextStyle::Body, fg), fg);
            focus(ui, s, rect, radius::MD);
        }
        a11y(&response, egui::WidgetType::Button, self.label, None);
        match self.shortcut {
            Some(k) => super::display::tooltip(response, self.label, Some(k)),
            None => response,
        }
    }
}

/// `Type` of `NF2/Icon Button`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconButtonType {
    /// Transparent until hover.
    #[default]
    Ghost,
    /// Toggle that is on (loop, onion skin, rulers…): `accent/subtle` fill, accent icon.
    Selected,
    /// Round accent button (main Play).
    Primary,
}

/// `Size` of `NF2/Icon Button`: SM 32, MD 40.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconButtonSize {
    #[default]
    SM,
    MD,
}

/// `NF2/Icon Button`. Every icon-only control has a tooltip with its name and shortcut (accessibility rule).
#[must_use = "add it with ui.add(…)"]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    shortcut: Option<&'a str>,
    kind: IconButtonType,
    size: IconButtonSize,
    side: Option<f32>,
}

impl<'a> IconButton<'a> {
    /// `label` is the accessible name and tooltip text.
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            shortcut: None,
            kind: IconButtonType::Ghost,
            size: IconButtonSize::SM,
            side: None,
        }
    }
    pub fn kind(mut self, kind: IconButtonType) -> Self {
        self.kind = kind;
        self
    }
    /// Shorthand for toggles: `Selected` when `on`, `Ghost` otherwise.
    pub fn selected(self, on: bool) -> Self {
        self.kind(if on {
            IconButtonType::Selected
        } else {
            IconButtonType::Ghost
        })
    }
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }
    /// Smaller square for dense rows (layer column 20, table rows 24). Never below 20.
    pub fn side(mut self, side: f32) -> Self {
        self.side = Some(side.max(20.0));
        self
    }
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let side = self.side.unwrap_or(match self.size {
            IconButtonSize::SM => CONTROL,
            IconButtonSize::MD => ICON_BUTTON_MD,
        });
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
        let s = State::of(ui, &response);
        let round = matches!(self.kind, IconButtonType::Primary);
        let r = if round { side / 2.0 } else { radius::MD };
        let (bg, fg) = match (self.kind, s.enabled, s.hovered) {
            (_, false, _) => (Color32::TRANSPARENT, p.text_disabled),
            (IconButtonType::Ghost, true, false) => (Color32::TRANSPARENT, p.text_secondary),
            (IconButtonType::Ghost, true, true) => (p.bg_hover, p.text_primary),
            (IconButtonType::Selected, true, _) => (p.accent_subtle, p.accent_default),
            (IconButtonType::Primary, true, false) => (p.accent_default, p.text_on_accent),
            (IconButtonType::Primary, true, true) => (p.accent_hover, p.text_on_accent),
        };
        if ui.is_rect_visible(rect) {
            fill(ui, rect, r, bg, None);
            let icon = if matches!(self.size, IconButtonSize::MD) {
                crate::icons::Icon::by_path(&format!("20/{}", self.icon.name)).unwrap_or(self.icon)
            } else {
                self.icon
            };
            icons::paint(ui.painter(), ui.ctx(), rect, icon, fg);
            focus(ui, s, rect, r);
        }
        let selected = matches!(self.kind, IconButtonType::Selected).then_some(true);
        a11y(&response, egui::WidgetType::Button, self.label, selected);
        super::display::tooltip(response, self.label, self.shortcut)
    }
}

/// `NF2/Tool Button` — 32 × 32, 20 px icon, `Flyout` corner mark, tooltip "Name  key".
#[must_use = "add it with ui.add(…)"]
pub struct ToolButton<'a> {
    icon: Icon,
    name: &'a str,
    key: Option<&'a str>,
    selected: bool,
    flyout: bool,
}

impl<'a> ToolButton<'a> {
    pub fn new(icon: Icon, name: &'a str) -> Self {
        Self {
            icon,
            name,
            key: None,
            selected: false,
            flyout: false,
        }
    }
    pub fn key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// The tool is the visible member of a group; long-press or right-click opens the group.
    pub fn flyout(mut self, flyout: bool) -> Self {
        self.flyout = flyout;
        self
    }
}

impl Widget for ToolButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(TOOL_BUTTON, TOOL_BUTTON), Sense::click());
        let s = State::of(ui, &response);
        let (bg, fg) = match (self.selected, s.enabled, s.hovered) {
            (_, false, _) => (Color32::TRANSPARENT, p.text_disabled),
            (true, _, _) => (p.accent_subtle, p.accent_default),
            (false, _, true) => (p.bg_hover, p.text_primary),
            (false, _, false) => (Color32::TRANSPARENT, p.text_secondary),
        };
        if ui.is_rect_visible(rect) {
            fill(ui, rect, radius::MD, bg, None);
            icons::paint(ui.painter(), ui.ctx(), rect, self.icon, fg);
            if self.flyout {
                let c = rect.right_bottom() + egui::vec2(-4.0, -4.0);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c, c + egui::vec2(-4.0, 0.0), c + egui::vec2(0.0, -4.0)],
                    fg,
                    egui::Stroke::NONE,
                ));
            }
            focus(ui, s, rect, radius::MD);
        }
        a11y(
            &response,
            egui::WidgetType::Button,
            self.name,
            Some(self.selected),
        );
        super::display::tooltip(response, self.name, self.key)
    }
}
