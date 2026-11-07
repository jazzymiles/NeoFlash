//! Tabs and headers: `NF2/Dock Tab`, `NF2/Panel Tab`, `NF2/Script Tab`, `NF2/Section Header`,
//! `NF2/Panel Header`, `NF2/Floating Panel Header`, `NF2/Dialog Header`.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};

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
            crate::metrics::dock_tab(),
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
            crate::metrics::panel_tab(),
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
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(w, crate::metrics::script_tab()), Sense::click());
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
            egui::vec2(ui.available_width(), crate::metrics::section_header()),
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
            egui::vec2(ui.available_width(), crate::metrics::panel_header()),
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
            egui::vec2(ui.available_width(), crate::metrics::panel_tab()),
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
            egui::vec2(ui.available_width(), crate::metrics::dialog_header()),
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

/// Width limits of `NF2/Document Tab` (R-035): 96–220 pt, the label ends with "…" beyond that.
pub const DOCUMENT_TAB_MIN: f32 = 96.0;
pub const DOCUMENT_TAB_MAX: f32 = 220.0;
/// Height of `NF2/Document Tab` (same as controls in the top bar) at the current density.
pub fn document_tab_height() -> f32 {
    crate::metrics::control()
}

/// What the 16 pt trailing slot of a document tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentTabTrailing {
    /// Clean, inactive and not hovered.
    Empty,
    /// Unsaved changes (6 pt dot).
    Dirty,
    /// × close button (on hover, and on the clean active tab).
    Close,
}

impl DocumentTabTrailing {
    /// The Figma variant rule: × on hover and on the clean active tab, otherwise the dirty dot.
    pub fn of(active: bool, hovered: bool, dirty: bool) -> Self {
        if hovered || (active && !dirty) {
            Self::Close
        } else if dirty {
            Self::Dirty
        } else {
            Self::Empty
        }
    }
}

/// Outer width of a document tab whose label (plus optional folder text) is `content` points wide:
/// 12 left padding, 16 file icon, 8 gap, content, 8 gap, 16 trailing slot, 8 right padding,
/// clamped to [`DOCUMENT_TAB_MIN`]…[`DOCUMENT_TAB_MAX`].
pub fn document_tab_width(content: f32) -> f32 {
    (space::S3 + 16.0 + space::S2 + content + space::S2 + 16.0 + space::S2)
        .clamp(DOCUMENT_TAB_MIN, DOCUMENT_TAB_MAX)
}

/// `NF2/Document Tab` (`126:18291`, R-035) — one open document in the top-bar strip.
/// `State` Active / Inactive / Hover × `Dirty`: Active = `bg/selected`, Hover = `bg/hover`; file icon,
/// `NF2/Small` label (primary when active or hovered, secondary otherwise), 16 pt trailing slot with
/// the dirty dot or ×. Tooltip = full path.
#[must_use = "call .show(ui)"]
pub struct DocumentTab<'a> {
    label: &'a str,
    active: bool,
    dirty: bool,
    folder: Option<&'a str>,
    path: Option<&'a str>,
}

/// Result of [`DocumentTab::show`]: the tab (click = activate) and the × button when it was shown.
pub struct DocumentTabResponse {
    pub response: Response,
    pub close: Option<Response>,
}

impl DocumentTabResponse {
    /// The × was clicked this frame.
    pub fn close_clicked(&self) -> bool {
        self.close.as_ref().is_some_and(Response::clicked)
    }
}

impl<'a> DocumentTab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            active: false,
            dirty: false,
            folder: None,
            path: None,
        }
    }
    /// `State=Active` — the document being edited.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// `Dirty=True` — unsaved changes.
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }
    /// Folder name shown in `text/tertiary` after the label when two open documents share a name.
    pub fn folder(mut self, folder: &'a str) -> Self {
        self.folder = Some(folder);
        self
    }
    /// Full path, shown as the tooltip.
    pub fn path(mut self, path: &'a str) -> Self {
        self.path = Some(path);
        self
    }

    pub fn show(self, ui: &mut Ui) -> DocumentTabResponse {
        let p = palette(ui.ctx());
        let folder_text = self.folder.map(|f| format!(" — {f}"));
        let probe = |text: &str| galley(ui, text, TextStyle::Small, p.text_primary).size().x;
        let label_w = probe(self.label);
        let folder_w = folder_text.as_deref().map_or(0.0, probe);
        let width = document_tab_width(label_w + folder_w);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, document_tab_height()), Sense::click());
        let enabled = ui.is_enabled();
        // Pointer anywhere over the tab, including its × (which takes the hover hit itself).
        let hovered = enabled && ui.rect_contains_pointer(rect);
        let trailing = DocumentTabTrailing::of(self.active, hovered, self.dirty);
        let slot = Rect::from_min_size(
            egui::pos2(rect.right() - space::S2 - 16.0, rect.center().y - 8.0),
            egui::vec2(16.0, 16.0),
        );
        let close = (enabled && trailing == DocumentTabTrailing::Close)
            .then(|| ui.interact(slot, response.id.with("close"), Sense::click()));
        if ui.is_rect_visible(rect) {
            let bg = if self.active {
                Some(p.bg_selected)
            } else if hovered {
                Some(p.bg_hover)
            } else {
                None
            };
            if let Some(bg) = bg {
                ui.painter().rect_filled(rect, cr(radius::MD), bg);
            }
            let fg = if !enabled {
                p.text_disabled
            } else if self.active || hovered {
                p.text_primary
            } else {
                p.text_secondary
            };
            let icon_rect = Rect::from_min_size(
                egui::pos2(rect.left() + space::S3, rect.center().y - 8.0),
                egui::vec2(16.0, 16.0),
            );
            icons::paint(
                ui.painter(),
                ui.ctx(),
                icon_rect,
                i16::NEW_FILE,
                if enabled {
                    p.text_secondary
                } else {
                    p.text_disabled
                },
            );
            let x = icon_rect.right() + space::S2;
            let room = (slot.left() - space::S2 - x).max(0.0);
            let label = super::galley_truncated(ui, self.label, TextStyle::Small, fg, room);
            let label_w = label.size().x;
            text_at(ui, x, rect, label, fg);
            if let Some(folder) = folder_text.as_deref() {
                let rest = room - label_w;
                if rest > 12.0 {
                    let g = super::galley_truncated(
                        ui,
                        folder,
                        TextStyle::Small,
                        p.text_tertiary,
                        rest,
                    );
                    text_at(ui, x + label_w, rect, g, p.text_tertiary);
                }
            }
            match trailing {
                DocumentTabTrailing::Empty => {}
                DocumentTabTrailing::Dirty => {
                    let dot = if self.active {
                        p.text_primary
                    } else {
                        p.text_secondary
                    };
                    ui.painter().circle_filled(slot.center(), 3.0, dot);
                }
                DocumentTabTrailing::Close => {
                    let over = close.as_ref().is_some_and(Response::hovered);
                    if over {
                        ui.painter().rect_filled(slot, cr(radius::SM), p.bg_hover);
                    }
                    let c = if over {
                        p.text_primary
                    } else {
                        p.text_secondary
                    };
                    icons::paint(ui.painter(), ui.ctx(), slot, i16::X, c);
                }
            }
            let s = State::of(ui, &response);
            super::focus(ui, s, rect, radius::MD);
        }
        let name = if self.dirty {
            format!("{} — unsaved", self.label)
        } else {
            self.label.to_owned()
        };
        a11y(
            &response,
            egui::WidgetType::SelectableLabel,
            &name,
            Some(self.active),
        );
        if let Some(close) = &close {
            a11y(close, egui::WidgetType::Button, "Close document", None);
        }
        let response = match self.path {
            Some(path) => super::display::tooltip(response, path, None),
            None => response,
        };
        DocumentTabResponse { response, close }
    }
}
