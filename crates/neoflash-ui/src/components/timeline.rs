//! Timeline 2.0 (§5 of the reference): `NF2/TL Layer`, `NF2/TL Toggle`, `NF2/TL Layers Header`,
//! `NF2/TL Grid`, `NF2/TL Grid Lines`, `NF2/TL Span`, `NF2/TL Marker`, `NF2/TL Frame Label`, `NF2/TL Ruler`,
//! `NF2/TL Onion Range`, `NF2/TL Playhead`.
//!
//! The frame area is virtualised: callers paint only the visible frame range with these painters.
//! Frame `n` (1-based) starts at `origin + (n − 1) × frame_width` ([`Geometry::frame_x`]).
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{
    FrameView, LAYER_COLUMN, LAYER_INDENT, LAYER_ROW, LAYER_TOGGLE, LAYERS_HEADER, TIMELINE_RULER,
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
}

impl LayerRow<'_> {
    pub fn show(self, ui: &mut Ui, width: f32, height: f32) -> LayerRowResponse {
        let (rect, row) =
            ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
        self.paint_at(ui, rect, row)
    }

    /// Paint into an already allocated rect (timelines that lay rows out themselves).
    pub fn paint_at(self, ui: &mut Ui, rect: Rect, row: Response) -> LayerRowResponse {
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
        let mut disclosure = false;
        if self.kind.is_folder() {
            let r = Rect::from_center_size(egui::pos2(x + 8.0, cy), egui::vec2(16.0, 16.0));
            let c = ui.interact(r.expand(4.0), row.id.with("disclosure"), Sense::click());
            disclosure = c.clicked();
            let open = matches!(self.kind, LayerKind::FolderOpen);
            icons::paint(
                ui.painter(),
                ui.ctx(),
                r,
                if open { i16::CHEVDOWN } else { i16::CHEVRIGHT },
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
        x += 16.0 + 2.0;
        let ic = if self.selected {
            p.accent_default
        } else {
            self.kind.icon_color(p)
        };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(egui::pos2(x + 8.0, cy), egui::vec2(16.0, 16.0)),
            self.kind.icon(),
            ic,
        );
        x += 16.0 + space::S2;
        let toggles_w = 3.0 * LAYER_TOGGLE + space::S2 + 8.0;
        let pencil_w = if self.editing { 20.0 } else { 0.0 };
        let label_right = rect.right() - toggles_w - pencil_w;
        let fg = if self.selected {
            p.text_primary
        } else {
            p.text_secondary
        };
        let label_rect = Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2(label_right, rect.bottom()),
        );
        let g = galley(ui, self.label, TextStyle::Body, fg);
        ui.painter()
            .with_clip_rect(label_rect)
            .galley(egui::pos2(x, cy - g.size().y / 2.0), g, fg);
        if self.editing {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(egui::pos2(label_right + 10.0, cy), egui::vec2(16.0, 16.0)),
                i16::PENCIL,
                p.accent_default,
            );
        }
        // Column toggles.
        let mut tx = rect.right() - space::S2 - 3.0 * LAYER_TOGGLE;
        let mut toggle = |ui: &mut Ui, kind: ToggleKind, on: bool| {
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
        }
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
            painter.line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)], s);
            painter.line_segment([c + egui::vec2(4.0, -4.0), c + egui::vec2(-4.0, 4.0)], s);
        }
        (ToggleKind::Lock, true) => icons::paint(painter, ctx, rect, i16::LOCK, p.text_primary),
        (ToggleKind::Outline, outline) => {
            let r = Rect::from_center_size(c, egui::vec2(10.0, 10.0));
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
}

/// `NF2/TL Layers Header` (280 × 32): "LAYERS" overline and the eye / lock / outline column headers
/// (click toggles all rows).
pub fn layers_header(ui: &mut Ui, width: f32) -> LayersHeaderResponse {
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
    let mut x = rect.right() - space::S2 - 3.0 * LAYER_TOGGLE;
    for (i, (icon, name)) in [
        (i16::EYE, "Show or hide all layers"),
        (i16::LOCK, "Lock or unlock all layers"),
        (i16::FRAME, "Show all layers as outlines"),
    ]
    .into_iter()
    .enumerate()
    {
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

/// `NF2/TL Grid` — empty-frame cells of one row, every 5th frame shaded. Paints only `row ∩ clip`.
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
        Stroke::new(1.0_f32, p.timeline_line),
    );
}

/// `NF2/TL Grid Lines` — per-frame dividers drawn over spans so filled frames stay countable.
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
    let y = row.y_range();
    for n in geo.visible(clip) {
        let x = geo.frame_x(n + 1) - 0.5;
        painter.vline(x, y, Stroke::new(1.0_f32, color));
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
    let y = body.center().y;
    let arrow = Stroke::new(1.0_f32, p.timeline_marker);
    match kind {
        SpanType::ClassicTween | SpanType::ShapeTween if body.width() > 24.0 => {
            let (a, b) = (body.left() + 10.0, body.right() - 8.0);
            painter.hline(a..=b, y, arrow);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(b + 3.0, y),
                    egui::pos2(b - 2.0, y - 3.0),
                    egui::pos2(b - 2.0, y + 3.0),
                ],
                p.timeline_marker,
                Stroke::NONE,
            ));
        }
        SpanType::BrokenTween if body.width() > 24.0 => {
            painter.extend(egui::Shape::dashed_line(
                &[
                    egui::pos2(body.left() + 10.0, y),
                    egui::pos2(body.right() - 6.0, y),
                ],
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
    let r = (cell.width().min(cell.height()) * 0.25).clamp(1.5, 3.5);
    let action = matches!(
        kind,
        MarkerKind::ActionKeyframe | MarkerKind::ActionBlankKeyframe
    );
    // Markers sit on the row centre (legend); an action keyframe drops its dot below the "a".
    let c = if action {
        egui::pos2(cell.center().x, cell.center().y + r + 1.0)
    } else {
        cell.center()
    };
    match kind {
        MarkerKind::Keyframe | MarkerKind::ActionKeyframe => {
            painter.circle_filled(c, r, fg);
        }
        MarkerKind::BlankKeyframe | MarkerKind::ActionBlankKeyframe => {
            painter.circle(c, r - 0.5, bg, Stroke::new(1.0_f32, fg));
        }
        MarkerKind::SpanEnd => {
            let rr = Rect::from_center_size(c, egui::vec2(r * 1.6, r * 2.6));
            painter.rect(
                rr,
                cr(0.5),
                bg,
                Stroke::new(1.0_f32, fg),
                StrokeKind::Inside,
            );
        }
        MarkerKind::PropertyKeyframe => {
            let d = r + 0.5;
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
    if action && cell.width() >= 8.0 {
        painter.text(
            egui::pos2(c.x, c.y - r - 1.0),
            egui::Align2::CENTER_BOTTOM,
            "a",
            crate::typography::font(TextStyle::MonoSmall),
            fg,
        );
    }
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
    let (color, icon) = match kind {
        FrameLabelKind::Label => (p.timeline_label, Some(i16::FLAG)),
        FrameLabelKind::Comment => (p.timeline_comment, None),
        FrameLabelKind::Anchor => (p.text_primary, Some(i16::ANCHOR)),
    };
    let clip = Rect::from_min_max(
        egui::pos2(cell.left() + 2.0, cell.top()),
        egui::pos2(max_right, cell.bottom()),
    );
    let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    // Vertically centred on the row, starting right after the keyframe marker.
    let y = cell.center().y;
    let mut x = cell.right();
    if let Some(icon) = icon {
        icons::paint_sized(
            &painter,
            ctx,
            Rect::from_min_size(egui::pos2(x - 2.0, y - 6.0), egui::vec2(12.0, 12.0)),
            icon,
            12.0,
            color,
        );
        x += 11.0;
    } else {
        painter.text(
            egui::pos2(x + 1.0, y),
            egui::Align2::LEFT_CENTER,
            "//",
            crate::typography::font(TextStyle::MonoSmall),
            color,
        );
        x += 15.0;
    }
    painter.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
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
    let seconds_h = if show_seconds {
        rect.height() * 0.45
    } else {
        0.0
    };
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
                egui::pos2(x + 1.0, rect.top() + seconds_h + 1.0),
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
    painter.vline(
        cx,
        (ruler.bottom() - 4.0)..=full.bottom(),
        Stroke::new(1.0_f32, p.timeline_playhead),
    );
    let mono = crate::typography::font(TextStyle::MonoSmall);
    let label = frame.to_string();
    let w = (label.len() as f32 * 7.0 + 8.0).max(geo.frame_width + 4.0);
    let chip = Rect::from_center_size(egui::pos2(cx, ruler.bottom() - 8.0), egui::vec2(w, 14.0));
    painter.rect_filled(chip, cr(radius::SM), p.timeline_playhead);
    painter.text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        label,
        mono.clone(),
        p.timeline_marker_inverse,
    );
    if let Some(t) = time {
        let tw = t.len() as f32 * 6.6 + 6.0;
        let tchip = Rect::from_center_size(egui::pos2(cx, ruler.top() + 6.0), egui::vec2(tw, 12.0));
        painter.rect_filled(tchip, cr(radius::SM), p.bg_panel);
        painter.rect_stroke(
            tchip,
            cr(radius::SM),
            Stroke::new(1.0_f32, p.timeline_playhead),
            StrokeKind::Inside,
        );
        painter.text(
            tchip.center(),
            egui::Align2::CENTER_CENTER,
            t,
            mono,
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
