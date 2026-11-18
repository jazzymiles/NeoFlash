//! `NF3/Button`, `NF3/Icon Button`, `NF3/Tool Button` — heights, paddings and radii follow the density.
use super::{State, a11y, fill, focus, galley, text_at};
use crate::icons::{self, Icon};

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

/// `NF3/Button` — `control()` high, [`button_padding`] at the sides, optional 16 px icon with a
/// `space::s2()` gap, label in `NF2/Body`.
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
            16.0 + space::s2()
        } else {
            0.0
        };
        let hug = button_padding() * 2.0 + icon_w + text_probe.size().x;
        let width = self
            .width
            .unwrap_or(hug)
            .max(hug.min(self.width.unwrap_or(hug)));
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, crate::metrics::control()), Sense::click());
        let s = State::of(ui, &response);
        let (bg, border, fg) = match (self.kind, s.enabled, s.hovered) {
            (ButtonType::Primary, false, _) => (p.bg_hover, None, p.text_disabled),
            (ButtonType::Secondary, false, _) => {
                (p.bg_raised, Some(p.border_subtle), p.text_disabled)
            }
            (ButtonType::Ghost, false, _) => (Color32::TRANSPARENT, None, p.text_disabled),
            (ButtonType::Danger, false, _) => {
                (Color32::TRANSPARENT, Some(p.border_subtle), p.text_disabled)
            }
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
            (ButtonType::Danger, true, false) => (
                Color32::TRANSPARENT,
                Some(p.danger_default),
                p.danger_default,
            ),
            (ButtonType::Danger, true, true) => {
                (p.danger_subtle, Some(p.danger_default), p.danger_default)
            }
        };
        if ui.is_rect_visible(rect) {
            fill(ui, rect, radius::md(), bg, border);
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
            focus(ui, s, rect, radius::md());
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
    /// Secondary action that stands on its own beside a field (Edit symbol, profile ⋯): `bg/raised`
    /// fill with a 1 px `border/default` stroke.
    Outlined,
    /// Toggle that is on (loop, onion skin, rulers…): `accent/subtle` fill, accent icon.
    Selected,
    /// Accent button (main Play): `accent/default` fill, radius md like every other icon button. Square, not
    /// round, since owner decision 2026-11-07 (R-052): emphasis comes from colour, not shape or diameter.
    Primary,
}

/// `Size` of `NF3/Icon Button`: SM = `control()`, MD = `icon_button_md()`.
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
    cell: Option<egui::Vec2>,
    outlined: bool,
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
            cell: None,
            outlined: false,
        }
    }
    /// `Outlined` button (`bg/raised` + 1 px border).
    pub fn outlined(icon: Icon, label: &'a str) -> Self {
        Self::new(icon, label).kind(IconButtonType::Outlined)
    }
    pub fn kind(mut self, kind: IconButtonType) -> Self {
        self.kind = kind;
        self
    }
    /// Shorthand for toggles: `Selected` when `on`, otherwise the kind it had (`Ghost` or
    /// `Outlined`). An outlined toggle keeps its border while selected.
    pub fn selected(mut self, on: bool) -> Self {
        if matches!(self.kind, IconButtonType::Outlined) {
            self.outlined = true;
        }
        if on {
            self.kind = IconButtonType::Selected;
        } else if matches!(self.kind, IconButtonType::Selected) {
            self.kind = if self.outlined {
                IconButtonType::Outlined
            } else {
                IconButtonType::Ghost
            };
        }
        self
    }
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }
    /// Explicit square for dense rows (layer column, icon groups). Never below the 16 px icon.
    pub fn side(mut self, side: f32) -> Self {
        self.side = Some(side.max(crate::metrics::ICON_UI));
        self
    }
    /// Explicit `width` × `height` cell for a header or list row: an icon-wide hit area as high as
    /// the row (the ⋯ of a section header). Never narrower or lower than the 16 px icon.
    pub fn cell(mut self, width: f32, height: f32) -> Self {
        let icon = crate::metrics::ICON_UI;
        self.cell = Some(egui::vec2(width.max(icon), height.max(icon)));
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
            IconButtonSize::SM => crate::metrics::control(),
            IconButtonSize::MD => crate::metrics::icon_button_md(),
        });
        let size = self.cell.unwrap_or(egui::vec2(side, side));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let s = State::of(ui, &response);
        let r = radius::md();
        let outlined = self.outlined || matches!(self.kind, IconButtonType::Outlined);
        let (bg, fg) = match (self.kind, s.enabled, s.hovered) {
            (_, false, _) if outlined => (p.bg_raised, p.text_disabled),
            (_, false, _) => (Color32::TRANSPARENT, p.text_disabled),
            (IconButtonType::Ghost, true, false) => (Color32::TRANSPARENT, p.text_secondary),
            (IconButtonType::Ghost, true, true) => (p.bg_hover, p.text_primary),
            (IconButtonType::Outlined, true, false) => (p.bg_raised, p.text_secondary),
            (IconButtonType::Outlined, true, true) => (p.bg_hover, p.text_primary),
            (IconButtonType::Selected, true, _) => (p.bg_selected, p.accent_default),
            (IconButtonType::Primary, true, false) => (p.accent_default, p.text_on_accent),
            (IconButtonType::Primary, true, true) => (p.accent_hover, p.text_on_accent),
        };
        let border = outlined.then_some(if s.enabled {
            p.border_default
        } else {
            p.border_subtle
        });
        if ui.is_rect_visible(rect) {
            fill(ui, rect, r, bg, border);
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

/// `NF3/Tool Button` — `tool_button()` square, tool glyph no larger than `icon_tool()`, `Flyout`
/// corner mark, tooltip "Name  key".
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
    /// Hit area of the `Flyout` corner mark (R-038): the bottom-right 12 × 12 pt of the button
    /// `rect` at Normal ([`corner_hit_side`] at other densities). A click there opens the group's
    /// flyout without switching the tool.
    pub fn corner_hit(rect: egui::Rect) -> egui::Rect {
        let side = corner_hit_side();
        egui::Rect::from_min_max(rect.max - egui::vec2(side, side), rect.max)
    }
}

/// Normal-density side of the [`ToolButton::corner_hit`] square; layout uses [`corner_hit_side`].
pub const CORNER_HIT: f32 = 12.0;

/// Side of the [`ToolButton::corner_hit`] square at the current density: 3/7 of the tool button
/// (12 / 10 / 9 pt).
pub fn corner_hit_side() -> f32 {
    (crate::metrics::tool_button() * 3.0 / 7.0).round()
}

/// Horizontal padding of [`Button`] at the current density (`pad/button`: 10 / 8 / 7 pt).
pub fn button_padding() -> f32 {
    space::s3() + 2.0
}

impl Widget for ToolButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(crate::metrics::tool_button(), crate::metrics::tool_button()),
            Sense::click(),
        );
        let s = State::of(ui, &response);
        let (bg, fg) = match (self.selected, s.enabled, s.hovered) {
            (_, false, _) => (Color32::TRANSPARENT, p.text_disabled),
            (true, _, _) => (p.bg_selected, p.accent_default),
            (false, _, true) => (p.bg_hover, p.text_primary),
            (false, _, false) => (Color32::TRANSPARENT, p.text_secondary),
        };
        if ui.is_rect_visible(rect) {
            fill(ui, rect, radius::md(), bg, None);
            let glyph = self.icon.side().min(crate::metrics::icon_tool());
            icons::paint_sized(ui.painter(), ui.ctx(), rect, self.icon, glyph, fg);
            if self.flyout {
                let c = rect.right_bottom() + egui::vec2(-2.0, -2.0);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c, c + egui::vec2(-4.0, 0.0), c + egui::vec2(0.0, -4.0)],
                    fg,
                    egui::Stroke::NONE,
                ));
            }
            focus(ui, s, rect, radius::md());
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
