//! Tabs and headers: `NF2/Dock Tab`, `NF2/Panel Tab`, `NF2/Script Tab`, `NF2/Section Header`,
//! `NF2/Panel Header`, `NF2/Floating Panel Header`, `NF2/Dialog Header`.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{
    DIALOG_HEADER, DOCK_TAB, PANEL_HEADER, PANEL_TAB, SCRIPT_TAB, SECTION_HEADER,
};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, Ui, Widget};

/// `NF2/Dock Tab` — 40 px, icon + label, 2 px accent underline when `Active`.
pub struct DockTab<'a> {
    pub label: &'a str,
    pub icon: Option<Icon>,
    pub active: bool,
}

impl Widget for DockTab<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        tab(
            ui,
            self.label,
            self.icon,
            self.active,
            DOCK_TAB,
            TextStyle::Body,
            space::S4,
        )
    }
}

/// `NF2/Panel Tab` — 36 px, text only, underline when `Active` (inspector and floating panels).
pub struct PanelTab<'a> {
    pub label: &'a str,
    pub active: bool,
}

impl Widget for PanelTab<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        tab(
            ui,
            self.label,
            None,
            self.active,
            PANEL_TAB,
            TextStyle::SmallStrong,
            space::S3,
        )
    }
}

fn tab(
    ui: &mut Ui,
    label: &str,
    icon: Option<Icon>,
    active: bool,
    height: f32,
    style: TextStyle,
    pad: f32,
) -> Response {
    let p = palette(ui.ctx());
    let g = galley(ui, label, style, p.text_primary);
    let icon_w = if icon.is_some() {
        16.0 + space::S2
    } else {
        0.0
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(g.size().x + icon_w + 2.0 * pad, height),
        Sense::click(),
    );
    let s = State::of(ui, &response);
    if s.hovered && !active {
        ui.painter().rect_filled(rect, cr(0.0), p.bg_hover);
    }
    let fg = if !s.enabled {
        p.text_disabled
    } else if active || s.hovered {
        p.text_primary
    } else {
        p.text_secondary
    };
    let mut x = rect.left() + pad;
    if let Some(icon) = icon {
        let ic = if active { p.accent_default } else { fg };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            icon,
            ic,
        );
        x += icon_w;
    }
    text_at(ui, x, rect, galley(ui, label, style, fg), fg);
    if active {
        let line = Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - 2.0),
            rect.right_bottom(),
        );
        ui.painter().rect_filled(line, cr(1.0), p.accent_default);
    }
    super::focus(ui, s, rect.shrink(2.0), radius::SM);
    a11y(
        &response,
        egui::WidgetType::SelectableLabel,
        label,
        Some(active),
    );
    response
}

/// `NF2/Script Tab` — 30 px tab at the bottom of the Actions editor, with optional pin.
pub struct ScriptTab<'a> {
    pub label: &'a str,
    pub pinned: bool,
    pub active: bool,
}

impl Widget for ScriptTab<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let g = galley(ui, self.label, TextStyle::Small, p.text_primary);
        let w = space::S3
            + 16.0
            + space::S2
            + g.size().x
            + if self.pinned { space::S2 + 16.0 } else { 0.0 }
            + space::S3;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, SCRIPT_TAB), Sense::click());
        let s = State::of(ui, &response);
        let fg = if self.active || s.hovered {
            p.text_primary
        } else {
            p.text_secondary
        };
        if self.active {
            ui.painter().hline(
                rect.x_range(),
                rect.top() + 1.0,
                Stroke::new(2.0_f32, p.accent_default),
            );
        }
        let mut x = rect.left() + space::S3;
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            i16::FILE_CODE,
            fg,
        );
        x += 16.0 + space::S2;
        let gw = g.size().x;
        text_at(
            ui,
            x,
            rect,
            galley(ui, self.label, TextStyle::Small, fg),
            fg,
        );
        if self.pinned {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(x + gw + space::S2 + 8.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                i16::PIN,
                p.text_tertiary,
            );
        }
        a11y(
            &response,
            egui::WidgetType::SelectableLabel,
            self.label,
            Some(self.active),
        );
        response
    }
}

/// `NF2/Section Header` — collapsible 32 px overline row. Remembers its state per id.
pub struct SectionHeader<'a> {
    label: &'a str,
    id: egui::Id,
    default_open: bool,
}

impl<'a> SectionHeader<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            id: egui::Id::new(("nf2-section", label)),
            default_open: true,
        }
    }
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.id = egui::Id::new(("nf2-section", salt));
        self
    }
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Show the header and, when expanded, `body`. Body gets 8 px gaps between rows.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let p = palette(ui.ctx());
        let id = ui.id().with(self.id);
        let mut open = ui
            .data_mut(|d| d.get_persisted::<bool>(id))
            .unwrap_or(self.default_open);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), SECTION_HEADER),
            Sense::click(),
        );
        if response.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(id, open));
        }
        let s = State::of(ui, &response);
        let fg = if s.hovered {
            p.text_primary
        } else {
            p.text_secondary
        };
        let chev = if open { i16::CHEVDOWN } else { i16::CHEVRIGHT };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(
                egui::pos2(rect.left() + 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            chev,
            fg,
        );
        text_at(
            ui,
            rect.left() + 16.0 + space::S1 + 2.0,
            rect,
            galley(ui, self.label, TextStyle::Overline, fg),
            fg,
        );
        super::focus(ui, s, rect, radius::SM);
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, self.label);
            info.selected = Some(open);
            info
        });
        open.then(|| {
            ui.scope(|ui| {
                // Figma inspector body: 12 pt between section header, fields and rows.
                ui.spacing_mut().item_spacing.y = space::S3;
                // No trailing space: the parent's 12 pt gap separates sections (Figma body V gap 12).
                body(ui)
            })
            .inner
        })
    }
}

/// What the user did with a panel header this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderActions {
    pub menu: bool,
    pub collapse: bool,
    pub close: bool,
}

/// `NF2/Panel Header` — docked panel title (`NF2/Title` 16 semibold), ⋯ menu, bottom separator. 40 px.
/// Returns the title response (drag handle) and the ⋯ button response (anchor for the panel menu).
pub struct PanelHeader<'a> {
    pub title: &'a str,
}

impl PanelHeader<'_> {
    pub fn show(self, ui: &mut Ui, trailing: impl FnOnce(&mut Ui)) -> (Response, Response) {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), PANEL_HEADER),
            Sense::click_and_drag(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        text_at(
            ui,
            rect.left() + space::S3,
            rect,
            galley(ui, self.title, TextStyle::Title, p.text_primary),
            p.text_primary,
        );
        let more = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(space::S2, 4.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = space::S1;
                    let more = ui.add(super::IconButton::new(i16::MORE, "Panel menu"));
                    trailing(ui);
                    more
                },
            )
            .inner;
        (response, more)
    }
}

/// `NF2/Floating Panel Header` — 36 px: drag grip, up to 3 panel tabs, ⋯, collapse, close.
pub struct FloatingPanelHeader<'a> {
    pub tabs: &'a [&'a str],
    pub active: usize,
    pub collapsed: bool,
}

pub struct FloatingHeaderResponse {
    /// Drag handle (grip and empty header space).
    pub drag: Response,
    /// Clicked tab index.
    pub tab: Option<usize>,
    /// ⋯ button, to anchor the panel menu.
    pub menu: Response,
    pub collapse: bool,
    pub close: bool,
}

impl FloatingPanelHeader<'_> {
    pub fn show(self, ui: &mut Ui) -> FloatingHeaderResponse {
        let p = palette(ui.ctx());
        let (rect, drag) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), PANEL_TAB),
            Sense::click_and_drag(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        if drag.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let mut out = FloatingHeaderResponse {
            menu: drag.clone(),
            drag,
            tab: None,
            collapse: false,
            close: false,
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(space::S2);
                icons::show(ui, i16::GRIP, p.text_tertiary);
                ui.add_space(space::S1);
                for (i, t) in self.tabs.iter().enumerate() {
                    if ui
                        .add(PanelTab {
                            label: t,
                            active: i == self.active,
                        })
                        .clicked()
                    {
                        out.tab = Some(i);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(space::S1);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    out.close = ui
                        .add(
                            super::IconButton::new(i16::X, "Close panel")
                                .shortcut("⌘W")
                                .side(28.0),
                        )
                        .clicked();
                    let (icon, name) = if self.collapsed {
                        (i16::CHEVDOWN, "Expand panel")
                    } else {
                        (i16::CHEVUP, "Collapse panel")
                    };
                    out.collapse = ui
                        .add(super::IconButton::new(icon, name).side(28.0))
                        .clicked();
                    out.menu = ui.add(super::IconButton::new(i16::MORE, "Panel menu").side(28.0));
                });
            });
        });
        out
    }
}

/// `NF2/Dialog Header` — 60 px: `NF2/Dialog Title`, close ✕ (Esc).
pub struct DialogHeader<'a> {
    pub title: &'a str,
}

impl DialogHeader<'_> {
    /// Returns `true` when the close button was clicked.
    pub fn show(self, ui: &mut Ui) -> bool {
        let p = palette(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), DIALOG_HEADER),
            Sense::hover(),
        );
        text_at(
            ui,
            rect.left() + space::S5,
            rect,
            galley(ui, self.title, TextStyle::DialogTitle, p.text_primary),
            p.text_primary,
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(space::S4, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.add(super::IconButton::new(i16::X, "Close").shortcut("Esc"))
                    .clicked()
            },
        )
        .inner
    }
}
