//! `NF2/Breadcrumb Item`, `NF2/Breadcrumb Separator`, `NF2/Breadcrumb Overflow` — the editing path
//! Scene ▸ symbol ▸ nested symbol. Clicking an item goes up; the optional ▾ lists siblings.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, i16};
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
        let w = space::s2()
            + 16.0
            + space::s2()
            + g.size().x
            + space::s2()
            + if self.menu { 12.0 + space::s1() } else { 0.0 };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(w, crate::metrics::row()), Sense::click());
        let s = State::of(ui, &response);
        if self.current || s.hovered {
            ui.painter().rect_filled(rect, cr(radius::md()), p.bg_hover);
        }
        let (fg, ic) = if self.current {
            (p.text_primary, p.accent_default)
        } else if s.hovered {
            (p.text_primary, p.text_secondary)
        } else {
            (p.text_secondary, p.text_secondary)
        };
        let x = rect.left() + space::s2();
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            self.kind.icon(),
            ic,
        );
        text_at(
            ui,
            x + 16.0 + space::s2(),
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        if self.menu {
            icons::paint_sized(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(rect.right() - space::s2() - 6.0, rect.center().y),
                    egui::vec2(12.0, 12.0),
                ),
                i16::CHEVDOWN,
                12.0,
                p.text_tertiary,
            );
        }
        super::focus(ui, s, rect, radius::md());
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
    // The 12 px chevron master in a 12 pt cell.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, crate::metrics::row()), Sense::hover());
    icons::paint_sized(
        ui.painter(),
        ui.ctx(),
        rect,
        i16::CHEVRIGHT,
        12.0,
        p.text_tertiary,
    );
}

/// `NF2/Breadcrumb Overflow` — "…" button that lists the hidden middle of a long path.
pub fn breadcrumb_overflow(ui: &mut Ui) -> Response {
    ui.add(super::IconButton::new(i16::MORE, "Show full path").side(crate::metrics::row()))
}
