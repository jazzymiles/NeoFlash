//! `NF2/Breadcrumb Item`, `NF2/Breadcrumb Separator`, `NF2/Breadcrumb Overflow` — the editing path
//! Scene ▸ symbol ▸ nested symbol. Clicking an item goes up; the optional ▾ lists siblings.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, i16};
use crate::metrics::ROW;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Ui, Widget};

/// `Type` of `NF2/Breadcrumb Item`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreadcrumbType {
    Scene,
    MovieClip,
    Graphic,
    Button,
}

impl BreadcrumbType {
    pub fn icon(self) -> crate::Icon {
        match self {
            Self::Scene => i16::SCENE,
            Self::MovieClip => i16::MOVIECLIP,
            Self::Graphic => i16::GRAPHIC,
            Self::Button => i16::BUTTON_SYMBOL,
        }
    }
}

/// `NF2/Breadcrumb Item` — `State=Current` for the last item, `Link` for the others.
pub struct BreadcrumbItem<'a> {
    pub label: &'a str,
    pub kind: BreadcrumbType,
    pub current: bool,
    /// `Menu` boolean: shows ▾ (siblings).
    pub menu: bool,
}

impl Widget for BreadcrumbItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let g = galley(ui, self.label, TextStyle::Body, p.text_primary);
        let w = space::S2
            + 16.0
            + space::S1
            + 2.0
            + g.size().x
            + space::S2
            + if self.menu { 16.0 } else { 0.0 };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, ROW), Sense::click());
        let s = State::of(ui, &response);
        if self.current || s.hovered {
            ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
        }
        let (fg, ic) = if self.current {
            (p.text_primary, p.accent_default)
        } else if s.hovered {
            (p.text_primary, p.text_secondary)
        } else {
            (p.text_secondary, p.text_secondary)
        };
        let x = rect.left() + space::S2;
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            self.kind.icon(),
            ic,
        );
        text_at(
            ui,
            x + 16.0 + 6.0,
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        if self.menu {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(rect.right() - 12.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                i16::CHEVDOWN,
                p.text_tertiary,
            );
        }
        super::focus(ui, s, rect, radius::MD);
        a11y(
            &response,
            egui::WidgetType::Button,
            self.label,
            Some(self.current),
        );
        response
    }
}

/// `NF2/Breadcrumb Separator` — ›.
pub fn breadcrumb_separator(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, ROW), Sense::hover());
    icons::paint(
        &ui.painter().with_clip_rect(rect.expand(4.0)),
        ui.ctx(),
        rect,
        i16::CHEVRIGHT,
        p.text_tertiary,
    );
}

/// `NF2/Breadcrumb Overflow` — "…" button that lists the hidden middle of a long path.
pub fn breadcrumb_overflow(ui: &mut Ui) -> Response {
    ui.add(super::IconButton::new(i16::MORE, "Show full path").side(28.0))
}
