//! Timeline 2.0 (§5 of the reference): `NF2/TL Layer`, `NF2/TL Toggle`, `NF2/TL Layers Header`,
//! `NF2/TL Grid`, `NF2/TL Grid Lines`, `NF2/TL Span`, `NF2/TL Marker`, `NF2/TL Frame Label`, `NF2/TL Ruler`,
//! `NF2/TL Onion Range`, `NF2/TL Playhead`.
//!
//! The frame area is virtualised: callers paint only the visible frame range with these painters.
//! Frame `n` (1-based) starts at `origin + (n − 1) × frame_width` ([`Geometry::frame_x`]).
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{
    FrameView, LAYER_COLUMN, LAYER_INDENT, LAYER_MODE, LAYER_ROW, LAYER_TOGGLE, LAYERS_HEADER,
    TIMELINE_RULER,
};
use crate::theme::{cr, palette};
use crate::tokens::{Palette, TextStyle, radius, space};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui};

/// Horizontal mapping between frames and pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// x of the left edge of frame 1 (may be left of the visible area when scrolled).
    pub origin_x: f32,
    pub frame_width: f32,
    pub row_height: f32,
}

impl Geometry {
    pub fn new(origin_x: f32, view: FrameView, short_rows: bool) -> Self {
        Self {
            origin_x,
            frame_width: view.frame_width(),
            row_height: if short_rows {
                crate::metrics::LAYER_ROW_SHORT
            } else {
                LAYER_ROW
            },
        }
    }
    /// Left edge of 1-based frame `n`.
    pub fn frame_x(&self, n: u32) -> f32 {
        self.origin_x + (n.max(1) - 1) as f32 * self.frame_width
    }
    /// 1-based frame under `x`.
    pub fn frame_at(&self, x: f32) -> u32 {
        (((x - self.origin_x) / self.frame_width).floor().max(0.0) as u32) + 1
    }
    /// Rect of frame `n` inside a row rect.
    pub fn cell(&self, row: Rect, n: u32) -> Rect {
        Rect::from_min_size(
            egui::pos2(self.frame_x(n), row.top()),
            egui::vec2(self.frame_width, row.height()),
        )
    }
    /// Rect of frames `first..=last` inside a row rect.
    pub fn span(&self, row: Rect, first: u32, last: u32) -> Rect {
        Rect::from_min_max(
            egui::pos2(self.frame_x(first), row.top()),
            egui::pos2(self.frame_x(last + 1), row.bottom()),
        )
    }
    /// Visible 1-based frame range for a clip rect.
    pub fn visible(&self, clip: Rect) -> std::ops::RangeInclusive<u32> {
        self.frame_at(clip.left())..=self.frame_at(clip.right())
    }
}

/// `Type` of `NF2/TL Layer` = Flash layer kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerKind {
    #[default]
    Layer,
    Masked,
    FolderOpen,
    FolderClosed,
    Guide,
    MotionGuide,
    Mask,
    Sound,
    Video,
    Armature,
}

impl LayerKind {
    pub fn icon(self) -> Icon {
        match self {
            Self::Layer => i16::LAYER,
            Self::Masked => i16::MASKED,
            Self::FolderOpen => i16::FOLDEROPEN,
            Self::FolderClosed => i16::FOLDER,
            Self::Guide => i16::GUIDE,
            Self::MotionGuide => i16::MOTIONGUIDE,
            Self::Mask => i16::MASK,
            Self::Sound => i16::WAVEFORM,
            Self::Video => i16::VIDEO,
            Self::Armature => i16::ARMATURE,
        }
    }
    /// Icon colour when the row is not selected (folders and armatures brand orange, sound green).
    pub fn icon_color(self, p: &Palette) -> Color32 {
        match self {
            Self::FolderOpen | Self::FolderClosed | Self::Armature => p.brand_orange,
            Self::Sound => p.success_default,
            Self::Video => p.guide,
            _ => p.text_secondary,
        }
    }
    pub fn is_folder(self) -> bool {
        matches!(self, Self::FolderOpen | Self::FolderClosed)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Layer => "Layer",
            Self::Masked => "Masked layer",
            Self::FolderOpen | Self::FolderClosed => "Folder",
            Self::Guide => "Guide layer",
            Self::MotionGuide => "Motion guide",
            Self::Mask => "Mask layer",
            Self::Sound => "Sound layer",
            Self::Video => "Video layer",
            Self::Armature => "Armature layer",
        }
    }
}

/// `NF2/TL Layer` row (280 × 28): indent, chevron (folders), type icon, label, ✎ when `Editing`,
/// and the three `NF2/TL Toggle` columns (visibility, lock, outline).
pub struct LayerRow<'a> {
    pub label: &'a str,
    pub kind: LayerKind,
    pub selected: bool,
    /// 0, 1 (`Indent 1`) or 2 (`Indent 2`); deeper nesting keeps adding 16 px.
    pub indent: usize,
    /// `Editing` — pencil on the current layer.
    pub editing: bool,
    pub visible: bool,
    pub locked: bool,
    pub outline: bool,
    /// Outline colour of the layer (the swatch in the third column).
    pub color: Color32,
}

/// Interaction results of a layer row.
pub struct LayerRowResponse {
    pub row: Response,
    pub disclosure: bool,
    pub visibility: bool,
    pub lock: bool,
    pub outline: bool,
    /// Rect of the label, for an inline rename editor.
    pub label_rect: Rect,
    /// The `Mode` cell was clicked (R-040): open the blend-mode menu below [`Self::mode_rect`].
    pub mode: bool,
    /// Rect of the `Mode` cell when the column is shown.
    pub mode_rect: Option<Rect>,
    /// The Solo toggle was clicked (R-045). The caller reads ⌥ for “solo only this layer”.
    pub solo: bool,
}

/// `Mode` cell of a layer row (R-040, owner-approved 5 Nov 2026, Figma `143:22609`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModeCell<'a> {
    /// No Mode column in this timeline (the classic 280 pt layout).
    #[default]
    Hidden,
    /// Column shown, but this row has no blend mode (guide, motion guide, sound).
    Empty,
    /// Column shown with the layer's blend mode name; `"Normal"` is drawn quietly.
    Mode(&'a str),
}

/// Optional layer-row columns added by R-040 (Mode) and R-045 (Solo). The default keeps the
/// classic row. Every row of one timeline must use the same column set; the layer column is then
/// [`crate::metrics::LAYER_COLUMN_MODE_SOLO`] wide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerColumns<'a> {
    pub mode: ModeCell<'a>,
    /// `Some(on)` shows the Solo toggle right after the eye; `None` hides the column.
    pub solo: Option<bool>,
}

/// Blend modes in menu order and grouping (R-040): separators between the groups. Labels match the
/// layer context menu `Blending ▸`.
pub const BLEND_MODE_GROUPS: [&[&str]; 6] = [
    &["Normal", "Layer"],
    &["Darken", "Multiply"],
    &["Lighten", "Screen", "Add"],
    &["Overlay", "Hard light"],
    &["Difference", "Subtract", "Invert"],
    &["Alpha", "Erase"],
];

const MODE_TOOLTIP: &str = "Blend mode — click to change";
const SOLO_TOOLTIP: &str = "Solo — show only soloed layers (⌥-click: only this one)";

impl LayerRow<'_> {
    pub fn show(self, ui: &mut Ui, width: f32, height: f32) -> LayerRowResponse {
        let (rect, row) =
            ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
        self.paint_at(ui, rect, row)
    }

    /// [`Self::show`] with the optional Mode / Solo columns.
    pub fn show_with(
        self,
        ui: &mut Ui,
        width: f32,
        height: f32,
        columns: LayerColumns<'_>,
    ) -> LayerRowResponse {
        let (rect, row) =
            ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
        self.paint_at_with(ui, rect, row, columns)
    }

    /// Paint into an already allocated rect (timelines that lay rows out themselves).
    pub fn paint_at(self, ui: &mut Ui, rect: Rect, row: Response) -> LayerRowResponse {
        self.paint_at_with(ui, rect, row, LayerColumns::default())
    }

    /// [`Self::paint_at`] with the optional Mode / Solo columns.
    pub fn paint_at_with(
        self,
        ui: &mut Ui,
        rect: Rect,
        row: Response,
        columns: LayerColumns<'_>,
    ) -> LayerRowResponse {
        let p = palette(ui.ctx());
        let s = State::of(ui, &row);
        let bg = if self.selected {
            p.timeline_row_selected
        } else if s.hovered {
            p.bg_hover
        } else {
            p.bg_panel
        };
        ui.painter().rect_filled(rect, cr(0.0), bg);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.timeline_line),
        );
        let mut x = rect.left() + space::S2 + self.indent as f32 * LAYER_INDENT;
        let cy = rect.center().y;
        // `Size=Compact` (Figma 166:1038…): Short 18 pt / Compact 14 pt rows use 12 pt icons, 4 pt gaps
        // and NF2/Small labels; toggle columns keep their 20 pt width so the header icons line up.
        let compact = layer_row_is_compact(rect.height());
        let icon = layer_row_icon(rect.height());
        let mut disclosure = false;
        if self.kind.is_folder() {
            let r = Rect::from_center_size(egui::pos2(x + icon / 2.0, cy), egui::vec2(icon, icon));
            let c = ui.interact(r.expand(4.0), row.id.with("disclosure"), Sense::click());
            disclosure = c.clicked();
            let open = matches!(self.kind, LayerKind::FolderOpen);
            icons::paint_sized(
                ui.painter(),
                ui.ctx(),
                r,
                if open { i16::CHEVDOWN } else { i16::CHEVRIGHT },
                icon,
                p.text_secondary,
            );
            a11y(
                &c,
                egui::WidgetType::Button,
                if open {
                    "Collapse folder"
                } else {
                    "Expand folder"
                },
                None,
            );
        }
        x += icon + if compact { space::S1 } else { 2.0 };
        let ic = if self.selected {
            p.accent_default
        } else {
            self.kind.icon_color(p)
        };
        icons::paint_sized(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + icon / 2.0, cy), egui::vec2(icon, icon)),
            self.kind.icon(),
            icon,
            ic,
        );
        x += icon + if compact { space::S1 } else { space::S2 };
        let toggle_count = if columns.solo.is_some() { 4.0 } else { 3.0 };
        let toggles_left = rect.right() - space::S2 - toggle_count * LAYER_TOGGLE;
        let toggles_w = toggle_count * LAYER_TOGGLE + space::S2 + 8.0;
        let mode_w = if columns.mode == ModeCell::Hidden {
            0.0
        } else {
            // Cell + 4 pt to the toggles + 4 pt to the label: 92 pt with the Solo column (Figma 143:22609).
            LAYER_MODE + space::S2
        };
        let pencil_w = if self.editing { icon + 4.0 } else { 0.0 };
        let label_right = rect.right() - toggles_w - mode_w - pencil_w;
        let fg = if self.selected {
            p.text_primary
        } else {
            p.text_secondary
        };
        let label_rect = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(label_right, rect.bottom()),
        );
        let label_style = if compact {
            TextStyle::Small
        } else {
            TextStyle::Body
        };
        let g = galley(ui, self.label, label_style, fg);
        ui.painter()
            .with_clip_rect(label_rect)
            .galley(egui::pos2(x, cy - g.size().y / 2.0), g, fg);
        if self.editing {
            icons::paint_sized(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(label_right + pencil_w / 2.0, cy),
                    egui::vec2(icon, icon),
                ),
                i16::PENCIL,
                icon,
                p.accent_default,
            );
        }
        // Mode cell (R-040).
        let mut mode = false;
        let mut mode_rect = None;
        if columns.mode != ModeCell::Hidden {
            let mode_h = 20.0_f32.min(rect.height());
            let r = Rect::from_min_size(
                egui::pos2(toggles_left - space::S1 - LAYER_MODE, cy - mode_h / 2.0),
                egui::vec2(LAYER_MODE, mode_h),
            );
            mode_rect = Some(r);
            if let ModeCell::Mode(name) = columns.mode {
                let resp = ui.interact(r, row.id.with("mode"), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, cr(radius::SM), p.bg_hover);
                }
                let color = if name == "Normal" {
                    p.text_tertiary
                } else {
                    p.text_primary
                };
                let g = galley(ui, name, TextStyle::Small, color);
                ui.painter().with_clip_rect(r).galley(
                    egui::pos2(r.left() + space::S1, cy - g.size().y / 2.0),
                    g,
                    color,
                );
                a11y(
                    &resp,
                    egui::WidgetType::Button,
                    &format!("Blend mode: {name}"),
                    None,
                );
                mode = super::display::tooltip(resp, MODE_TOOLTIP, None).clicked();
            }
        }
        // Column toggles: eye, [solo], lock, outline.
        let mut tx = toggles_left;
        let mut solo = false;
        if let Some(on) = columns.solo {
            let eye = Rect::from_min_size(
                egui::pos2(tx, rect.top()),
                egui::vec2(LAYER_TOGGLE, rect.height()),
            );
            let r = eye.translate(egui::vec2(LAYER_TOGGLE, 0.0));
            let resp = ui.interact(r, row.id.with("solo"), Sense::click());
            paint_solo_toggle(ui.painter(), ui.ctx(), r, on, resp.hovered());
            a11y(&resp, egui::WidgetType::Checkbox, "Solo layer", Some(on));
            solo = super::display::tooltip(resp, SOLO_TOOLTIP, None).clicked();
        }
        let solo_slot = columns.solo.is_some();
        let mut toggle = |ui: &mut Ui, kind: ToggleKind, on: bool| {
            if solo_slot && kind == ToggleKind::Lock {
                tx += LAYER_TOGGLE;
            }
            let r = Rect::from_min_size(
                egui::pos2(tx, rect.top()),
                egui::vec2(LAYER_TOGGLE, rect.height()),
            );
            tx += LAYER_TOGGLE;
            let resp = ui.interact(r, row.id.with(kind as u8), Sense::click());
            paint_toggle(
                ui.painter(),
                ui.ctx(),
                r,
                kind,
                on,
                self.color,
                resp.hovered(),
            );
            let (name, key) = kind.describe(on);
            a11y(&resp, egui::WidgetType::Checkbox, name, Some(on));
            super::display::tooltip(resp, name, key).clicked()
        };
        let visibility = toggle(ui, ToggleKind::Visibility, !self.visible);
        let lock = toggle(ui, ToggleKind::Lock, self.locked);
        let outline = toggle(ui, ToggleKind::Outline, self.outline);
        a11y(
            &row,
            egui::WidgetType::SelectableLabel,
            &format!("{} ({})", self.label, self.kind.label()),
            Some(self.selected),
        );
        LayerRowResponse {
            row,
            disclosure,
            visibility,
            lock,
            outline,
            label_rect,
            mode,
            mode_rect,
            solo,
        }
    }
}

/// `NF2/TL Layer` `Size=Compact` applies to rows shorter than the 28 pt default (Short 18, Compact 14).
pub fn layer_row_is_compact(height: f32) -> bool {
    height < LAYER_ROW - 0.5
}

/// Icon side in a layer row: 16 pt (Default) or 12 pt (Compact).
pub fn layer_row_icon(height: f32) -> f32 {
    if layer_row_is_compact(height) {
        12.0
    } else {
        16.0
    }
}

/// Paint the Solo toggle (R-045): 8 pt dot — off = 1.5 pt ring (`text/tertiary`, `text/primary` on
/// hover), on = `accent/default` fill.
pub fn paint_solo_toggle(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    on: bool,
    hovered: bool,
) {
    let p = palette(ctx);
    let c = rect.center();
    if on {
        painter.circle_filled(c, 4.0, p.accent_default);
    } else {
        let ring = if hovered {
            p.text_primary
        } else {
            p.text_tertiary
        };
        painter.circle_stroke(c, 3.25, Stroke::new(1.5_f32, ring));
    }
}

/// `Kind` of `NF2/TL Toggle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleKind {
    Visibility = 1,
    Lock = 2,
    Outline = 3,
}

impl ToggleKind {
    fn describe(self, on: bool) -> (&'static str, Option<&'static str>) {
        match (self, on) {
            (Self::Visibility, false) => ("Hide layer", None),
            (Self::Visibility, true) => ("Show layer", None),
            (Self::Lock, false) => ("Lock layer", None),
            (Self::Lock, true) => ("Unlock layer", None),
            (Self::Outline, false) => ("View layer as outlines", None),
            (Self::Outline, true) => ("View layer as solid", None),
        }
    }
}

/// Paint one `NF2/TL Toggle`: `on` = Hidden (red ✕) / Locked (lock) / Outline (hollow square).
/// Off states are a quiet dot, so a column of normal layers stays calm.
pub fn paint_toggle(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    kind: ToggleKind,
    on: bool,
    layer_color: Color32,
    hovered: bool,
) {
    let p = palette(ctx);
    let c = rect.center();
    let compact = layer_row_is_compact(rect.height());
    let dot = if hovered {
        p.text_primary
    } else {
        p.text_tertiary
    };
    match (kind, on) {
        (ToggleKind::Visibility, false) | (ToggleKind::Lock, false) => {
            painter.circle_filled(c, 2.0, dot);
        }
        (ToggleKind::Visibility, true) => {
            let s = Stroke::new(1.5_f32, p.danger_default);
            let d = if compact { 3.0 } else { 4.0 };
            painter.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], s);
            painter.line_segment([c + egui::vec2(d, -d), c + egui::vec2(-d, d)], s);
        }
        (ToggleKind::Lock, true) => icons::paint_sized(
            painter,
            ctx,
            rect,
            i16::LOCK,
            layer_row_icon(rect.height()),
            p.text_primary,
        ),
        (ToggleKind::Outline, outline) => {
            let side = if compact { 9.0 } else { 10.0 };
            let r = Rect::from_center_size(c, egui::vec2(side, side));
            if outline {
                painter.rect_stroke(
                    r,
                    cr(2.0),
                    Stroke::new(1.5_f32, layer_color),
                    StrokeKind::Inside,
                );
            } else {
                painter.rect_filled(r, cr(2.0), layer_color);
            }
        }
    }
}

/// What was clicked in the layers header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayersHeaderResponse {
    pub all_visibility: bool,
    pub all_lock: bool,
    pub all_outline: bool,
    /// The Solo column header was clicked: clear every Solo (R-045).
    pub clear_solo: bool,
}

/// Optional header columns matching [`LayerColumns`]: `mode` shows the “Mode” label, `solo` the
/// Solo ring after the eye.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayersHeaderColumns {
    pub mode: bool,
    pub solo: bool,
}

/// `NF2/TL Layers Header` (280 × 32): "LAYERS" overline and the eye / lock / outline column headers
/// (click toggles all rows).
pub fn layers_header(ui: &mut Ui, width: f32) -> LayersHeaderResponse {
    layers_header_with(ui, width, LayersHeaderColumns::default())
}

/// [`layers_header`] with the optional Mode / Solo columns (R-040, R-045).
pub fn layers_header_with(
    ui: &mut Ui,
    width: f32,
    columns: LayersHeaderColumns,
) -> LayersHeaderResponse {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, LAYERS_HEADER), Sense::hover());
    ui.painter().rect_filled(rect, cr(0.0), p.bg_panel);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, p.border_subtle),
    );
    text_at(
        ui,
        rect.left() + space::S3,
        rect,
        galley(ui, "Layers", TextStyle::Overline, p.text_tertiary),
        p.text_tertiary,
    );
    let mut out = LayersHeaderResponse::default();
    let toggle_count = if columns.solo { 4.0 } else { 3.0 };
    let mut x = rect.right() - space::S2 - toggle_count * LAYER_TOGGLE;
    if columns.mode {
        let left = x - LAYER_MODE;
        text_at(
            ui,
            left,
            rect,
            galley(ui, "Mode", TextStyle::Small, p.text_tertiary),
            p.text_tertiary,
        );
    }
    if columns.solo {
        let r = Rect::from_min_size(
            egui::pos2(x + LAYER_TOGGLE, rect.top()),
            egui::vec2(LAYER_TOGGLE, rect.height()),
        );
        let resp = ui.interact(r, ui.id().with(("layers-header", "solo")), Sense::click());
        let ring = if resp.hovered() {
            p.text_primary
        } else {
            p.text_secondary
        };
        ui.painter()
            .circle_stroke(r.center(), 3.25, Stroke::new(1.5_f32, ring));
        let name = "Clear solo on all layers";
        a11y(&resp, egui::WidgetType::Button, name, None);
        out.clear_solo = super::display::tooltip(resp, name, None).clicked();
    }
    for (i, (icon, name)) in [
        (i16::EYE, "Show or hide all layers"),
        (i16::LOCK, "Lock or unlock all layers"),
        (i16::FRAME, "Show all layers as outlines"),
    ]
    .into_iter()
    .enumerate()
    {
        if columns.solo && i == 1 {
            x += LAYER_TOGGLE;
        }
        let r = Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(LAYER_TOGGLE, rect.height()),
        );
        x += LAYER_TOGGLE;
        let resp = ui.interact(r, ui.id().with(("layers-header", i)), Sense::click());
        icons::paint(
            &ui.painter().with_clip_rect(r),
            ui.ctx(),
            r,
            icon,
            if resp.hovered() {
                p.text_primary
            } else {
                p.text_secondary
            },
        );
        a11y(&resp, egui::WidgetType::Button, name, None);
        if super::display::tooltip(resp, name, None).clicked() {
            match i {
                0 => out.all_visibility = true,
                1 => out.all_lock = true,
                _ => out.all_outline = true,
            }
        }
    }
    out
}

/// `Kind` of `NF2/TL Grid`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridKind {
    #[default]
    Layer,
    /// Collapsed / folder row: flat strip.
    Folder,
}

/// Distance from the bottom of a timeline row to the NF2 glyph line: `y 19` of the 28 pt row
/// (27 pt cell + 1 pt row line). Keyframe glyphs, tween lines and arrowheads share this line in every
/// Frame View row height (Normal, Short, Compact, Preview, Layer height 200/300 %).
pub const GLYPH_LINE_FROM_BOTTOM: f32 = 9.0;
/// `NF2/TL Grid` frame tick: 1 × 4 pt at the bottom of the cell, directly above the row line.
pub const FRAME_TICK: f32 = 4.0;
/// `NF2/TL Marker` Keyframe / Blank Keyframe / Action dot diameter (fixed, independent of zoom).
pub const MARKER_DOT: f32 = 6.0;
/// `NF2/TL Marker` Span End rectangle.
pub const SPAN_END: egui::Vec2 = egui::vec2(5.0, 8.0);
/// `NF2/TL Marker` Property Keyframe diamond (bounding square).
pub const PROPERTY_DIAMOND: f32 = 7.0;
/// `NF2/TL Span` tween line: starts 11 pt after the span's left edge, arrowhead (4 × 5) ends 9 pt
/// before its right edge (the next keyframe).
pub const TWEEN_LINE_START: f32 = 11.0;
pub const TWEEN_ARROW_END: f32 = 9.0;
pub const TWEEN_ARROW: egui::Vec2 = egui::vec2(4.0, 5.0);

/// y of the glyph line inside a row (or a cell of that row).
pub fn glyph_line_y(row: Rect) -> f32 {
    row.bottom() - GLYPH_LINE_FROM_BOTTOM
}

/// `NF2/TL Grid` — empty-frame cells of one row, every 5th frame shaded, 1 × 4 frame ticks at the
/// bottom (Flash style) and the row line. Paints only `row ∩ clip`.
pub fn paint_grid(
    painter: &Painter,
    ctx: &egui::Context,
    row: Rect,
    geo: &Geometry,
    kind: GridKind,
) {
    let p = palette(ctx);
    let clip = row.intersect(painter.clip_rect());
    match kind {
        GridKind::Folder => {
            painter.rect_filled(clip, 0.0, p.timeline_folder);
        }
        GridKind::Layer => {
            painter.rect_filled(clip, 0.0, p.timeline_empty);
            if geo.frame_width >= 4.0 {
                for n in geo.visible(clip) {
                    if n % 5 == 0 {
                        painter.rect_filled(geo.cell(row, n), 0.0, p.timeline_empty_5th);
                    }
                }
                paint_grid_lines(painter, ctx, row, geo, p.timeline_grid);
            }
        }
    }
    painter.hline(
        clip.x_range(),
        row.bottom() - 0.5,
        Stroke::new(1.0_f32, p.timeline_grid),
    );
}

/// `NF2/TL Grid Lines` — the frame ticks only (1 × 4 pt at the bottom of each frame's right edge).
/// Paint it directly above [`paint_grid`] and **below** spans, so static and tween spans stay
/// continuous; never paint it over spans.
pub fn paint_grid_lines(
    painter: &Painter,
    ctx: &egui::Context,
    row: Rect,
    geo: &Geometry,
    color: Color32,
) {
    let _ = ctx;
    if geo.frame_width < 4.0 {
        return;
    }
    let clip = row.intersect(painter.clip_rect());
    let bottom = row.bottom() - 1.0;
    let ticks = (bottom - FRAME_TICK)..=bottom;
    for n in geo.visible(clip) {
        let x = geo.frame_x(n + 1) - 0.5;
        painter.vline(x, ticks.clone(), Stroke::new(1.0_f32, color));
    }
}

/// `Type` of `NF2/TL Span`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanType {
    /// Frames holding content after a keyframe (grey).
    Static,
    ClassicTween,
    ShapeTween,
    MotionTween,
    /// Tween with a missing end keyframe (dashed arrow).
    BrokenTween,
    /// Frame selection.
    Selected,
    Sound,
    Video,
}

/// Decoded peaks of one sound channel: per screen column, the lowest and highest sample in it (−1…1).
pub type SoundPeaks<'a> = &'a [(f32, f32)];

/// `NF2/TL Span · Sound — decoded peaks` (R-010): paints real waveform peaks over a Sound span painted
/// with [`paint_span`]. One 1 pt `success/default` column per screen point, mirrored around the lane
/// centre, height = peak × (lane height / 2 − 2). `channels` holds one slice (mono) or two (L, R: two lanes,
/// 1 pt apart); column `i` of a channel is drawn at `body.left() + i + 0.5`. An empty `channels` (not decoded
/// yet) draws a flat 1 pt centre line at 50 %. Cache peaks per resource and zoom; never decode while scrubbing.
pub fn paint_sound_peaks(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    channels: &[SoundPeaks<'_>],
) {
    let p = palette(ctx);
    let body = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - 1.0));
    let painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    if channels.is_empty() {
        painter.hline(
            body.x_range(),
            body.center().y,
            Stroke::new(1.0_f32, p.success_default.gamma_multiply(0.5)),
        );
        return;
    }
    let lanes = channels.len().min(2) as f32;
    let lane_h = (body.height() - (lanes - 1.0)) / lanes;
    let stroke = Stroke::new(1.0_f32, p.success_default);
    for (lane, peaks) in channels.iter().take(2).enumerate() {
        let top = body.top() + lane as f32 * (lane_h + 1.0);
        let centre = top + lane_h / 2.0;
        let scale = (lane_h / 2.0 - 2.0).max(0.5);
        for (i, &(lo, hi)) in peaks.iter().enumerate() {
            let x = body.left() + i as f32 + 0.5;
            if x > body.right() {
                break;
            }
            let (lo, hi) = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
            // Mirrored: the larger excursion sets both halves, so quiet passages stay readable.
            let a = lo.abs().max(hi.abs()) * scale;
            painter.vline(x, (centre - a.max(0.5))..=(centre + a.max(0.5)), stroke);
        }
    }
}

/// `NF2/TL Span` — one element per span, `frames × frame_width` wide. `tinted = false`
/// (Frame view ▸ Tinted frames off) draws spans neutral with arrows only.
pub fn paint_span(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    kind: SpanType,
    tinted: bool,
) {
    let p = palette(ctx);
    let body = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.bottom() - 1.0));
    let fill = match (kind, tinted) {
        (SpanType::Selected, _) => p.timeline_selected,
        (_, false) => p.timeline_static,
        (SpanType::Static | SpanType::Sound | SpanType::Video, true) => p.timeline_static,
        (SpanType::ClassicTween | SpanType::BrokenTween, true) => p.timeline_classic_tween,
        (SpanType::ShapeTween, true) => p.timeline_shape_tween,
        (SpanType::MotionTween, true) => p.timeline_motion_tween,
    };
    painter.rect_filled(body, 0.0, fill);
    // Static, motion, selected, sound and video spans close with 1 pt `timeline/grid` edges; classic,
    // shape and broken tweens are one undivided band.
    if !matches!(
        kind,
        SpanType::ClassicTween | SpanType::ShapeTween | SpanType::BrokenTween
    ) && body.width() >= 3.0
    {
        let edge = Stroke::new(1.0_f32, p.timeline_grid);
        painter.vline(body.left() + 0.5, body.y_range(), edge);
        painter.vline(body.right() - 0.5, body.y_range(), edge);
    }
    let y = body.center().y;
    // Tween line on the glyph line: 1 pt stroke whose pixel row ends at the glyph centre.
    let glyph = glyph_line_y(rect);
    let line_y = glyph - 0.5;
    let arrow = Stroke::new(1.0_f32, p.timeline_marker);
    let (a, tip) = (
        body.left() + TWEEN_LINE_START,
        body.right() - TWEEN_ARROW_END,
    );
    match kind {
        SpanType::ClassicTween | SpanType::ShapeTween if tip - TWEEN_ARROW.x > a => {
            let base = tip - TWEEN_ARROW.x;
            painter.hline(a..=base, line_y, arrow);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(base, glyph - TWEEN_ARROW.y / 2.0),
                    egui::pos2(tip, glyph),
                    egui::pos2(base, glyph + TWEEN_ARROW.y / 2.0),
                ],
                p.timeline_marker,
                Stroke::NONE,
            ));
        }
        SpanType::BrokenTween if tip > a => {
            painter.extend(egui::Shape::dashed_line(
                &[egui::pos2(a, line_y), egui::pos2(tip, line_y)],
                arrow,
                3.0,
                3.0,
            ));
        }
        SpanType::Sound => {
            // Deterministic waveform so scrubbing does not flicker.
            let mut x = body.left() + 1.0;
            let mut seed = 0x9e37_79b9_u32;
            while x < body.right() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let h = 2.0 + (seed % 100) as f32 / 100.0 * (body.height() / 2.0 - 3.0);
                painter.vline(
                    x,
                    (y - h)..=(y + h),
                    Stroke::new(1.0_f32, p.success_default),
                );
                x += 2.0;
            }
        }
        SpanType::Video => {
            let s = Stroke::new(1.0_f32, p.timeline_marker.gamma_multiply(0.6));
            painter.extend(egui::Shape::dashed_line(
                &[
                    egui::pos2(body.left(), body.top() + 2.5),
                    egui::pos2(body.right(), body.top() + 2.5),
                ],
                s,
                3.0,
                3.0,
            ));
            painter.extend(egui::Shape::dashed_line(
                &[
                    egui::pos2(body.left(), body.bottom() - 2.5),
                    egui::pos2(body.right(), body.bottom() - 2.5),
                ],
                s,
                3.0,
                3.0,
            ));
        }
        _ => {}
    }
}

/// `Kind` of `NF2/TL Marker`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    /// ● filled.
    Keyframe,
    /// ○ hollow.
    BlankKeyframe,
    /// ▯ end of a span.
    SpanEnd,
    /// ◆ property keyframe (motion tween / armature).
    PropertyKeyframe,
    /// "a" over ● — keyframe with a frame script.
    ActionKeyframe,
    /// "a" over ○ — blank keyframe with a frame script.
    ActionBlankKeyframe,
}

/// `NF2/TL Marker` in the cell of frame `n`. `on_selection` = `Tone=On Selection` (inverted).
pub fn paint_marker(
    painter: &Painter,
    ctx: &egui::Context,
    cell: Rect,
    kind: MarkerKind,
    on_selection: bool,
) {
    let p = palette(ctx);
    let fg = if on_selection {
        p.timeline_marker_inverse
    } else {
        p.timeline_marker
    };
    let bg = if on_selection {
        p.timeline_selected
    } else {
        p.timeline_empty
    };
    let action = matches!(
        kind,
        MarkerKind::ActionKeyframe | MarkerKind::ActionBlankKeyframe
    );
    // Locked 12 × 27 master: every glyph is centred on the glyph line (x 6, y 19) with a fixed size,
    // whatever the frame width or row height; the action "a" sits at the top of the cell.
    let c = egui::pos2(cell.center().x, glyph_line_y(cell));
    let r = MARKER_DOT / 2.0;
    match kind {
        MarkerKind::Keyframe | MarkerKind::ActionKeyframe => {
            painter.circle_filled(c, r, fg);
        }
        MarkerKind::BlankKeyframe | MarkerKind::ActionBlankKeyframe => {
            // 1.2 pt ring whose outer edge is the 6 pt dot.
            painter.circle(c, r - 0.6, bg, Stroke::new(1.2_f32, fg));
        }
        MarkerKind::SpanEnd => {
            painter.rect(
                Rect::from_center_size(c, SPAN_END),
                0.0,
                bg,
                Stroke::new(1.0_f32, fg),
                StrokeKind::Inside,
            );
        }
        MarkerKind::PropertyKeyframe => {
            let d = PROPERTY_DIAMOND / 2.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(0.0, -d),
                    c + egui::vec2(d, 0.0),
                    c + egui::vec2(0.0, d),
                    c + egui::vec2(-d, 0.0),
                ],
                fg,
                Stroke::NONE,
            ));
        }
    }
    // "a" (Inter Semi Bold 9/9) at y 1; Compact rows have no room above the dot.
    if action && cell.width() >= 6.0 && cell.height() >= 17.0 {
        let top = if cell.height() >= 24.0 { 1.0 } else { 0.0 };
        painter.text(
            egui::pos2(c.x, cell.top() + top),
            egui::Align2::CENTER_TOP,
            "a",
            action_font(),
            fg,
        );
    }
}

/// `NF2/TL Marker` action "a": Inter Semi Bold 9/9.
fn action_font() -> egui::FontId {
    egui::FontId::new(
        9.0,
        egui::FontFamily::Name(crate::typography::INTER_SEMIBOLD.into()),
    )
}

/// `Kind` of `NF2/TL Frame Label`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameLabelKind {
    /// Red flag (`timeline/label`).
    Label,
    /// Green `//` (`timeline/comment`).
    Comment,
    /// ⚓ named anchor.
    Anchor,
}

/// `NF2/TL Frame Label` — sits on top of the span that starts at the keyframe, clipped to `max_right`.
pub fn paint_frame_label(
    painter: &Painter,
    ctx: &egui::Context,
    cell: Rect,
    kind: FrameLabelKind,
    text: &str,
    max_right: f32,
) {
    let p = palette(ctx);
    let color = match kind {
        FrameLabelKind::Label => p.timeline_label,
        FrameLabelKind::Comment => p.timeline_comment,
        FrameLabelKind::Anchor => p.timeline_marker,
    };
    // The 27 pt label starts 1 pt after the keyframe cell and sits at the top of the row: icon at
    // (1, 3), text (Mono Small 11/14) at (11, 1), Anchor text at (12, 1).
    let o = egui::pos2(cell.right() + 1.0, cell.top());
    let clip = Rect::from_min_max(o, egui::pos2(max_right, cell.bottom()));
    let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    let at = |x: f32, y: f32| o + egui::vec2(1.0 + x, 3.0 + y);
    let line = |w: f32| Stroke::new(w, color);
    match kind {
        FrameLabelKind::Label => {
            // Flag: 1 pt pole + pennant with a notch (two convex halves).
            painter.vline(o.x + 1.5, (o.y + 3.0)..=(o.y + 13.0), line(1.0));
            for half in [
                [at(1.0, 0.5), at(7.0, 0.5), at(5.5, 2.75), at(1.0, 2.75)],
                [at(1.0, 2.75), at(5.5, 2.75), at(7.0, 5.0), at(1.0, 5.0)],
            ] {
                painter.add(egui::Shape::convex_polygon(
                    half.to_vec(),
                    color,
                    Stroke::NONE,
                ));
            }
        }
        FrameLabelKind::Comment => {
            painter.line_segment([at(3.0, 1.0), at(1.0, 9.0)], line(1.1));
            painter.line_segment([at(6.5, 1.0), at(4.5, 9.0)], line(1.1));
        }
        FrameLabelKind::Anchor => {
            painter.circle_stroke(at(4.0, 1.5), 1.2, line(1.0));
            painter.line_segment([at(4.0, 2.7), at(4.0, 9.5)], line(1.0));
            for (c1, c2, end) in [
                ((2.6, 9.5), (1.3, 8.5), (1.0, 6.5)),
                ((5.4, 9.5), (6.7, 8.5), (7.0, 6.5)),
            ] {
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        at(4.0, 9.5),
                        at(c1.0, c1.1),
                        at(c2.0, c2.1),
                        at(end.0, end.1),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    line(1.0),
                ));
            }
        }
    }
    let text_x = if kind == FrameLabelKind::Anchor {
        12.0
    } else {
        11.0
    };
    painter.text(
        o + egui::vec2(text_x, 1.0),
        egui::Align2::LEFT_TOP,
        text,
        crate::typography::font(TextStyle::MonoSmall),
        color,
    );
}

/// `NF2/TL Ruler` (32 px): seconds row on top (`0s 1s 2s …` at `fps`), frame numbers (1, 5, 10, …) with ticks.
pub fn paint_ruler(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    geo: &Geometry,
    fps: f32,
    show_seconds: bool,
) {
    let p = palette(ctx);
    painter.rect_filled(rect, 0.0, p.bg_panel);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, p.border_subtle),
    );
    let mono = crate::typography::font(TextStyle::MonoSmall);
    let digits_top = ruler_digits_top(rect, show_seconds);
    let seconds_h = digits_top - 1.0 - rect.top();
    let clip = rect.intersect(painter.clip_rect());
    // Label step adapts to the frame width so numbers never overlap.
    let step = match geo.frame_width {
        w if w >= 9.0 => 5,
        w if w >= 6.0 => 10,
        _ => 20,
    };
    let fps_i = fps.round().max(1.0) as u32;
    for n in geo.visible(clip) {
        let x = geo.frame_x(n);
        let tick_top = if n == 1 || n % step == 0 {
            rect.bottom() - 7.0
        } else {
            rect.bottom() - 4.0
        };
        painter.vline(
            x + 0.5,
            tick_top..=rect.bottom(),
            Stroke::new(1.0_f32, p.border_default),
        );
        if n == 1 || n % step == 0 {
            painter.text(
                egui::pos2(x + 1.0, digits_top),
                egui::Align2::LEFT_TOP,
                n.to_string(),
                mono.clone(),
                p.text_tertiary,
            );
        }
        if show_seconds && (n - 1) % fps_i == 0 {
            painter.vline(
                x + 0.5,
                rect.top()..=rect.top() + seconds_h,
                Stroke::new(1.0_f32, p.border_default),
            );
            painter.text(
                egui::pos2(x + 3.0, rect.top() + 1.0),
                egui::Align2::LEFT_TOP,
                format!("{}s", (n - 1) / fps_i),
                mono.clone(),
                p.text_tertiary,
            );
        }
    }
}

/// Top line of the ruler's frame digits (Mono Small 11/14). The playhead's frame chip shares it.
pub fn ruler_digits_top(ruler: Rect, show_seconds: bool) -> f32 {
    let seconds_h = if show_seconds {
        ruler.height() * 0.45
    } else {
        0.0
    };
    ruler.top() + seconds_h + 1.0
}

/// `NF2/TL Onion Range` on the ruler: accent band with draggable handles at both ends.
/// Returns the handle rects (before, after) for hit-testing.
pub fn paint_onion_range(
    painter: &Painter,
    ctx: &egui::Context,
    ruler: Rect,
    x0: f32,
    x1: f32,
) -> (Rect, Rect) {
    let p = palette(ctx);
    let band = Rect::from_min_max(
        egui::pos2(x0, ruler.top() + 6.0),
        egui::pos2(x1, ruler.bottom() - 2.0),
    );
    painter.rect_filled(band, cr(radius::SM), p.accent_subtle);
    let h0 = Rect::from_min_max(
        egui::pos2(x0, band.top()),
        egui::pos2(x0 + 3.0, band.bottom()),
    );
    let h1 = Rect::from_min_max(
        egui::pos2(x1 - 3.0, band.top()),
        egui::pos2(x1, band.bottom()),
    );
    painter.rect_filled(h0, cr(1.5), p.timeline_onion);
    painter.rect_filled(h1, cr(1.5), p.timeline_onion);
    (
        h0.expand2(egui::vec2(3.0, 0.0)),
        h1.expand2(egui::vec2(3.0, 0.0)),
    )
}

/// `NF2/TL Playhead`: line through all rows, frame chip on the frame row, quiet time chip on the
/// seconds row. `x` = left edge of the current frame; `full` = ruler top to the bottom of the rows.
#[allow(clippy::too_many_arguments)] // Mirrors the Figma component's inputs; painters take no struct.
pub fn paint_playhead(
    painter: &Painter,
    ctx: &egui::Context,
    full: Rect,
    ruler: Rect,
    geo: &Geometry,
    x: f32,
    frame: u32,
    time: Option<&str>,
) {
    let p = palette(ctx);
    let cx = x + geo.frame_width / 2.0;
    // Frame chip: Mono 11/14 on the ruler digits' top line (1 pt padding), 3 pt sides, min width 20,
    // radius 4. Time chip: quiet 13 pt chip at the top of the seconds row.
    let mono = crate::typography::font(TextStyle::MonoSmall);
    let label = frame.to_string();
    let text_w = painter
        .layout_no_wrap(label.clone(), mono.clone(), p.timeline_marker_inverse)
        .size()
        .x;
    let digits_top = ruler_digits_top(ruler, time.is_some());
    let chip = Rect::from_min_size(
        egui::pos2(cx - (text_w + 6.0).max(20.0) / 2.0, digits_top - 1.0),
        egui::vec2((text_w + 6.0).max(20.0), 16.0),
    );
    painter.vline(
        cx,
        chip.bottom()..=full.bottom(),
        Stroke::new(1.5_f32, p.timeline_playhead),
    );
    painter.rect_filled(chip, cr(4.0), p.timeline_playhead);
    painter.text(
        egui::pos2(cx, chip.top() + 1.0),
        egui::Align2::CENTER_TOP,
        label,
        mono,
        p.timeline_marker_inverse,
    );
    if let Some(t) = time {
        let small = egui::FontId::new(10.0, egui::FontFamily::Monospace);
        let tw = painter
            .layout_no_wrap(t.to_owned(), small.clone(), p.timeline_playhead)
            .size()
            .x;
        let tchip = Rect::from_center_size(
            egui::pos2(cx, ruler.top() + 6.5),
            egui::vec2(tw + 8.0, 13.0),
        );
        painter.rect_filled(tchip, cr(3.0), p.bg_panel);
        painter.rect_stroke(
            tchip,
            cr(3.0),
            Stroke::new(1.0_f32, p.timeline_playhead),
            StrokeKind::Inside,
        );
        painter.text(
            tchip.center(),
            egui::Align2::CENTER_CENTER,
            t,
            small,
            p.timeline_playhead,
        );
    }
}

/// Default sizes for callers.
pub const RULER: f32 = TIMELINE_RULER;
pub const LAYERS: f32 = LAYER_COLUMN;

/// Small helper so callers can position a marker for a 1-based frame in a row.
pub fn marker_cell(geo: &Geometry, row: Rect, frame: u32) -> Rect {
    geo.cell(row, frame)
}

/// Pos helper used by tests and the gallery.
pub fn frame_center(geo: &Geometry, row: Rect, frame: u32) -> Pos2 {
    geo.cell(row, frame).center()
}
