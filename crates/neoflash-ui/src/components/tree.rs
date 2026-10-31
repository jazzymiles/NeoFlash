//! `NF2/Tree Item` (Library, Project, Toolbox, Movie Explorer) and `NF2/Table Row` (filters, cue points,
//! variables, compiler errors).
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{CONTROL, LAYER_INDENT, ROW};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Ui, Widget};

/// `Type` of `NF2/Tree Item`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeItemType {
    FolderOpen,
    FolderClosed,
    /// Any leaf; pass its icon (movie clip, graphic, button, bitmap, sound, font, script…).
    Symbol(Icon),
}

/// `NF2/Tree Item` — 32 px row (Figma 288×32), chevron for folders, 16 px type icon, label, optional trailing meta.
pub struct TreeItem<'a> {
    label: &'a str,
    kind: TreeItemType,
    selected: bool,
    depth: usize,
    meta: &'a str,
    icon: Option<Icon>,
}

/// Responses of a tree row: the row and its disclosure chevron (folders only).
pub struct TreeItemResponse {
    pub row: Response,
    pub toggle: bool,
}

impl<'a> TreeItem<'a> {
    pub fn new(label: &'a str, kind: TreeItemType) -> Self {
        Self {
            label,
            kind,
            selected: false,
            depth: 0,
            meta: "",
            icon: None,
        }
    }
    /// Type icon of a folder row (Figma instance override): package, class, scene, target…
    /// Leaves pass their icon in [`TreeItemType::Symbol`].
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }
    /// Right-aligned secondary text (linkage class, use count, file size).
    pub fn meta(mut self, meta: &'a str) -> Self {
        self.meta = meta;
        self
    }

    pub fn show(self, ui: &mut Ui) -> TreeItemResponse {
        let p = palette(ui.ctx());
        let (rect, row) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), CONTROL),
            Sense::click_and_drag(),
        );
        let s = State::of(ui, &row);
        if self.selected {
            ui.painter()
                .rect_filled(rect, cr(radius::MD), p.bg_selected);
        } else if s.hovered {
            ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
        }
        let mut x = rect.left() + space::S2 + self.depth as f32 * LAYER_INDENT;
        let mut toggle = false;
        let chev_rect =
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0));
        let icon = match self.kind {
            TreeItemType::FolderOpen | TreeItemType::FolderClosed => {
                let open = matches!(self.kind, TreeItemType::FolderOpen);
                let c = ui.interact(
                    chev_rect.expand(4.0),
                    row.id.with("disclosure"),
                    Sense::click(),
                );
                toggle = c.clicked()
                    || (row.has_focus()
                        && ui.input(|i| {
                            (open && i.key_pressed(egui::Key::ArrowLeft))
                                || (!open && i.key_pressed(egui::Key::ArrowRight))
                        }));
                icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    chev_rect,
                    if open { i16::CHEVDOWN } else { i16::CHEVRIGHT },
                    p.text_secondary,
                );
                self.icon
                    .unwrap_or(if open { i16::FOLDEROPEN } else { i16::FOLDER })
            }
            TreeItemType::Symbol(icon) => icon,
        };
        x += 16.0 + space::S1;
        let ic = if self.selected {
            p.accent_default
        } else {
            p.text_secondary
        };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
            icon,
            ic,
        );
        x += 16.0 + space::S2;
        let fg = if !s.enabled {
            p.text_disabled
        } else if self.selected {
            p.text_primary
        } else {
            p.text_secondary
        };
        let mut right = rect.right() - space::S2;
        if !self.meta.is_empty() {
            let g = galley(ui, self.meta, TextStyle::Small, p.text_tertiary);
            right -= g.size().x;
            text_at(ui, right, rect, g, p.text_tertiary);
            right -= space::S2;
        }
        let clip = Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(right, rect.bottom()));
        let g = galley(ui, self.label, TextStyle::Body, fg);
        ui.painter().with_clip_rect(clip).galley(
            egui::pos2(x, rect.center().y - g.size().y / 2.0),
            g,
            fg,
        );
        super::focus(ui, s, rect, radius::MD);
        a11y(
            &row,
            egui::WidgetType::SelectableLabel,
            self.label,
            Some(self.selected),
        );
        TreeItemResponse { row, toggle }
    }
}

/// `Kind` of `NF2/Table Row`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableRowKind {
    /// Column captions (overline).
    Header,
    #[default]
    Row,
    Selected,
}

/// `NF2/Table Row` — 28 px, two or more columns at fixed fractions of the width.
pub struct TableRow<'a> {
    pub cells: &'a [&'a str],
    pub kind: TableRowKind,
    /// Column start fractions, e.g. `&[0.0, 0.5]`.
    pub columns: &'a [f32],
}

impl Widget for TableRow<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let sense = if matches!(self.kind, TableRowKind::Header) {
            Sense::hover()
        } else {
            Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), sense);
        let s = State::of(ui, &response);
        match self.kind {
            TableRowKind::Header => ui.painter().rect_filled(rect, cr(0.0), p.bg_raised),
            TableRowKind::Selected => ui.painter().rect_filled(rect, cr(0.0), p.bg_selected),
            TableRowKind::Row if s.hovered => ui.painter().rect_filled(rect, cr(0.0), p.bg_hover),
            TableRowKind::Row => ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                egui::Stroke::new(1.0_f32, p.border_subtle),
            ),
        };
        let style = if matches!(self.kind, TableRowKind::Header) {
            TextStyle::Overline
        } else {
            TextStyle::Small
        };
        for (i, cell) in self.cells.iter().enumerate() {
            let start = self
                .columns
                .get(i)
                .copied()
                .unwrap_or(i as f32 / self.cells.len() as f32);
            let end = self.columns.get(i + 1).copied().unwrap_or(1.0);
            let fg = match (self.kind, i) {
                (TableRowKind::Header, _) => p.text_tertiary,
                (_, 0) => p.text_primary,
                _ => p.text_secondary,
            };
            let x0 = rect.left() + rect.width() * start + space::S3;
            let clip = Rect::from_min_max(
                egui::pos2(x0, rect.top()),
                egui::pos2(rect.left() + rect.width() * end - space::S1, rect.bottom()),
            );
            let g = galley(ui, cell, style, fg);
            ui.painter().with_clip_rect(clip).galley(
                egui::pos2(x0, rect.center().y - g.size().y / 2.0),
                g,
                fg,
            );
        }
        response
    }
}
