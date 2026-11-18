//! Tabs and headers: `NF2/Dock Tab`, `NF2/Panel Tab`, `NF2/Script Tab`, `NF2/Section Header`,
//! `NF2/Panel Header`, `NF2/Floating Panel Header`, `NF2/Dialog Header`.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};

use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, Ui, Widget};

/// `NF3/Tab` (dock) — `dock_tab()` high, `space::s4()` side padding, icon + label, 2 px accent
/// underline when `Active` (painted inside the tab, so the label never moves).
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
            space::s4(),
        )
    }
}

/// `NF3/Tab` (panel) — `panel_tab()` high, `space::s4()` side padding, text only, underline when
/// `Active` (inspector and floating panels). Width: [`panel_tab_width`].
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
            PANEL_TAB_STYLE,
            space::s4(),
        )
    }
}

const PANEL_TAB_STYLE: TextStyle = TextStyle::SmallStrong;

/// Width of a [`PanelTab`] with `label` at the current density (label + 2 × `space::s4()`), for
/// callers that hit-test the tabs of a [`FloatingPanelHeader`].
pub fn panel_tab_width(ui: &Ui, label: &str) -> f32 {
    galley(ui, label, PANEL_TAB_STYLE, egui::Color32::WHITE)
        .size()
        .x
        + 2.0 * space::s4()
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
        16.0 + space::s2()
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
    super::focus(ui, s, rect.shrink(2.0), radius::sm());
    a11y(
        &response,
        egui::WidgetType::SelectableLabel,
        label,
        Some(active),
    );
    response
}

/// `NF2/Script Tab` — 30 px tab at the bottom of the Actions editor, with optional interactive pin (`T-364`).
pub struct ScriptTab<'a> {
    pub label: &'a str,
    pub pinned: bool,
    pub active: bool,
}

/// Separate responses for clicking a [`ScriptTab`] body vs its `📌` (`i16::PIN`) toggle button.
pub struct ScriptTabResponse {
    /// Tab body response (primary click selects tab; secondary click opens context menu).
    pub tab: Response,
    /// Pin icon hit response when `pinnable || pinned`.
    pub pin: Option<Response>,
}

impl<'a> ScriptTab<'a> {
    /// Render the script tab with a custom leading `icon` and disjoint hit-testing for the
    /// trailing `i16::PIN` button (`pinned` tabs always show `📌`; unpinned `pinnable` tabs
    /// show `📌` in `text_tertiary` when active or hovered).
    pub fn show_interactive(self, ui: &mut Ui, icon: Icon, pinnable: bool) -> ScriptTabResponse {
        let p = palette(ui.ctx());
        let g = galley(ui, self.label, TextStyle::Small, p.text_primary);
        let has_pin_slot = pinnable || self.pinned;
        let w = space::s3()
            + 16.0
            + space::s2()
            + g.size().x
            + if has_pin_slot {
                space::s2() + 16.0
            } else {
                0.0
            }
            + space::s3();
        let (rect, full_resp) =
            ui.allocate_exact_size(egui::vec2(w, crate::metrics::script_tab()), Sense::hover());
        let (tab_resp, raw_pin_resp, pin_center_x) = if has_pin_slot {
            let pin_hit_left =
                (rect.right() - space::s3() - 16.0 - space::s1() * 0.5).max(rect.left());
            let body_rect = Rect::from_min_max(rect.min, egui::pos2(pin_hit_left, rect.bottom()));
            let pin_rect = Rect::from_min_max(egui::pos2(pin_hit_left, rect.top()), rect.max);
            let tab_r = ui.interact(body_rect, full_resp.id.with("body"), Sense::click());
            let pin_r = ui.interact(pin_rect, full_resp.id.with("pin"), Sense::click());
            (tab_r, Some(pin_r), rect.right() - space::s3() - 8.0)
        } else {
            let tab_r = ui.interact(rect, full_resp.id.with("body"), Sense::click());
            (tab_r, None, rect.right() - space::s3() - 8.0)
        };
        let pin_hovered = raw_pin_resp.as_ref().is_some_and(Response::hovered);
        let any_hovered = full_resp.hovered() || tab_resp.hovered() || pin_hovered;
        if any_hovered && !self.active {
            ui.painter().rect_filled(rect, cr(0.0), p.bg_hover);
        }
        let fg = if self.active || any_hovered {
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
        let mut x = rect.left() + space::s3();
        let icon_fg = if self.active { p.accent_default } else { fg };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            icon,
            icon_fg,
        );
        x += 16.0 + space::s2();
        text_at(
            ui,
            x,
            rect,
            galley(ui, self.label, TextStyle::Small, fg),
            fg,
        );
        let show_pin_icon = self.pinned || (pinnable && (self.active || any_hovered));
        if show_pin_icon {
            let pin_fg = if self.pinned {
                if pin_hovered {
                    p.accent_hover
                } else if self.active {
                    p.accent_default
                } else {
                    p.text_primary
                }
            } else if pin_hovered {
                p.text_primary
            } else {
                p.text_tertiary
            };
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(pin_center_x, rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                i16::PIN,
                pin_fg,
            );
        }
        a11y(
            &tab_resp,
            egui::WidgetType::SelectableLabel,
            self.label,
            Some(self.active),
        );
        let tab = tab_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let pin = raw_pin_resp.map(|pin_r| {
            let tip = if self.pinned {
                "Unpin script tab"
            } else {
                "Pin script tab"
            };
            let pin_label = format!("{tip}: {}", self.label);
            a11y(
                &pin_r,
                egui::WidgetType::Button,
                &pin_label,
                Some(self.pinned),
            );
            super::tooltip(
                pin_r.on_hover_cursor(egui::CursorIcon::PointingHand),
                tip,
                None,
            )
        });
        ScriptTabResponse { tab, pin }
    }
}

impl Widget for ScriptTab<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show_interactive(ui, i16::FILE_CODE, false).tab
    }
}

/// `NF3/Section Header` — collapsible `section_header()` overline row with a 12 px chevron.
/// Remembers its state per id. The row starts at the left edge of its Ui: the host panel supplies
/// the `space::s4()` side padding (`pad/panel`), so the chevron lines up with the fields below.
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

    /// Show the header and, when expanded, `body`. The body starts `metrics::row_gap()` below the
    /// header and its rows are `row_gap()` apart.
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
        icons::paint_sized(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(
                egui::pos2(rect.left() + SECTION_CHEVRON / 2.0, rect.center().y),
                egui::vec2(SECTION_CHEVRON, SECTION_CHEVRON),
            ),
            chev,
            SECTION_CHEVRON,
            if s.hovered {
                p.text_primary
            } else {
                p.text_tertiary
            },
        );
        text_at(
            ui,
            rect.left() + section_title_inset(),
            rect,
            galley(ui, self.label, TextStyle::Overline, fg),
            fg,
        );
        super::focus(ui, s, rect, radius::sm());
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, self.label);
            info.selected = Some(open);
            info
        });
        open.then(|| {
            let gap = crate::metrics::row_gap();
            // Whatever the host's row spacing is, the body starts exactly `gap/row` below.
            ui.add_space(gap - ui.spacing().item_spacing.y);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = gap;
                // No trailing space: the parent's gap separates sections.
                body(ui)
            })
            .inner
        })
    }
}

/// Side of the Section Header chevron (the 12 px master).
const SECTION_CHEVRON: f32 = 12.0;

/// Distance from the left edge of a [`SectionHeader`] to its title: chevron + tiny gap. Rows that
/// align with section titles (shortcut list) use it as their indent.
pub fn section_title_inset() -> f32 {
    SECTION_CHEVRON + space::s1()
}

/// What the user did with a panel header this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderActions {
    pub menu: bool,
    pub collapse: bool,
    pub close: bool,
}

/// `NF3/Panel Header` — docked panel title (`NF2/Title`), ⋯ menu, bottom separator; `panel_header()` high.
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
            rect.left() + space::s4(),
            rect,
            galley(ui, self.title, TextStyle::Title, p.text_primary),
            p.text_primary,
        );
        let more = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(space::s1(), 0.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = space::s1();
                    let more = ui.add(super::IconButton::new(i16::MORE, "Panel menu"));
                    trailing(ui);
                    more
                },
            )
            .inner;
        (response, more)
    }
}

/// `NF3/Floating Panel Header` — `panel_tab()` high: drag grip, up to 3 panel tabs, ⋯, collapse,
/// close (`control()` squares). Narrower than [`FloatingPanelHeader::NARROW_BELOW`] it shows only the
/// grip and ⋯ — no title to clip; the caller then offers Collapse and Close in the ⋯ menu
/// ([`FloatingHeaderResponse::narrow`]).
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
    /// The header was too narrow for tabs, collapse and close: only the grip and ⋯ were shown.
    pub narrow: bool,
}

impl FloatingPanelHeader<'_> {
    /// Below this width the header shows only the grip and the ⋯ button.
    pub const NARROW_BELOW: f32 = 120.0;
    /// Gap between the ⋯ / collapse / close buttons.
    const ACTION_GAP: f32 = 2.0;

    /// Distance from the left edge to the first tab: gap, grip, tiny gap.
    pub fn tab_strip_start() -> f32 {
        space::s2() + i16::GRIP.side() + space::s1()
    }
    /// Width taken by ⋯, collapse and close at the right edge, including the edge inset.
    pub fn actions_width() -> f32 {
        space::s1() + 3.0 * crate::metrics::control() + 2.0 * Self::ACTION_GAP
    }
    /// Smallest width of the full header without tabs: grip + the three buttons.
    pub fn chrome_width() -> f32 {
        Self::tab_strip_start() + space::s1() + Self::actions_width()
    }

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
            narrow: rect.width() < Self::NARROW_BELOW,
        };
        if out.narrow {
            // Grip centred in the space left of ⋯; nothing else fits without clipping.
            let side = crate::metrics::control().min(rect.width());
            let more = Rect::from_center_size(
                egui::pos2(rect.right() - space::s1() - side / 2.0, rect.center().y),
                egui::vec2(side, side),
            );
            let grip = Rect::from_min_max(rect.min, egui::pos2(more.left(), rect.bottom()));
            if grip.width() >= i16::GRIP_H.side() {
                icons::paint(ui.painter(), ui.ctx(), grip, i16::GRIP_H, p.text_tertiary);
            }
            out.menu = ui.put(
                more,
                super::IconButton::new(i16::MORE, "Panel menu").side(side),
            );
            return out;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(space::s2());
                icons::show(ui, i16::GRIP, p.text_tertiary);
                ui.add_space(space::s1());
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
                    ui.add_space(space::s1());
                    ui.spacing_mut().item_spacing.x = Self::ACTION_GAP;
                    out.close = ui
                        .add(super::IconButton::new(i16::X, "Close panel").shortcut("⌘W"))
                        .clicked();
                    let (icon, name) = if self.collapsed {
                        (i16::CHEVDOWN, "Expand panel")
                    } else {
                        (i16::CHEVUP, "Collapse panel")
                    };
                    out.collapse = ui.add(super::IconButton::new(icon, name)).clicked();
                    out.menu = ui.add(super::IconButton::new(i16::MORE, "Panel menu"));
                });
            });
        });
        out
    }
}

/// `NF3/Dialog Header` — `dialog_header()` high: `NF2/Dialog Title` after `space::s5()`, close ✕ (Esc).
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
            rect.left() + space::s5(),
            rect,
            galley(ui, self.title, TextStyle::DialogTitle, p.text_primary),
            p.text_primary,
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(space::s4(), 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.add(super::IconButton::new(i16::X, "Close").shortcut("Esc"))
                    .clicked()
            },
        )
        .inner
    }
}

/// Normal-density width limits of `NF2/Document Tab` (R-035); layout uses [`document_tab_min`] and
/// [`document_tab_max`].
pub const DOCUMENT_TAB_MIN: f32 = crate::metrics::FIELD_S;
pub const DOCUMENT_TAB_MAX: f32 = crate::metrics::FIELD_L;
/// Width limits of a document tab at the current density (`width/field-s` … `width/field-l`); the
/// label ends with "…" beyond the maximum.
pub fn document_tab_min() -> f32 {
    crate::metrics::field_s()
}
pub fn document_tab_max() -> f32 {
    crate::metrics::field_l()
}
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
/// field padding, 16 file icon, gap, content, gap, 16 trailing slot, gap,
/// clamped to [`document_tab_min`]…[`document_tab_max`].
pub fn document_tab_width(content: f32) -> f32 {
    (space::s3() + 16.0 + space::s2() + content + space::s2() + 16.0 + space::s2())
        .clamp(document_tab_min(), document_tab_max())
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
            egui::pos2(rect.right() - space::s2() - 16.0, rect.center().y - 8.0),
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
                ui.painter().rect_filled(rect, cr(radius::md()), bg);
            }
            let fg = if !enabled {
                p.text_disabled
            } else if self.active || hovered {
                p.text_primary
            } else {
                p.text_secondary
            };
            let icon_rect = Rect::from_min_size(
                egui::pos2(rect.left() + space::s3(), rect.center().y - 8.0),
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
            let x = icon_rect.right() + space::s2();
            let room = (slot.left() - space::s2() - x).max(0.0);
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
                        ui.painter().rect_filled(slot, cr(radius::sm()), p.bg_hover);
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
            super::focus(ui, s, rect, radius::md());
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
