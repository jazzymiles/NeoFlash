//! Stage overlays and canvas editing chrome (shelves "Rulers, guides, grid" and "Canvas editing chrome"):
//! `NF2/Stage Ruler`, `NF2/Ruler Corner`, `NF2/Guide`, `NF2/Stage Grid`, `NF2/Selection Box`,
//! `NF2/Transform Box`, `NF2/Anchor Point`, `NF2/Bezier Handle`, `NF2/Marquee`, `NF2/Shape Selection`,
//! `NF2/Gradient Transform`, `NF2/Snap Ring`, `NF2/Motion Path`.
//!
//! All painters work in screen points. Stage ↔ screen mapping is the caller's: pass the screen
//! position of the stage origin and the zoom (screen points per stage pixel).
use crate::theme::{cr, palette};
use crate::tokens::TextStyle;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, vec2};

/// Ruler strip thickness (`NF2/Stage Ruler` 20, `NF2/Ruler Corner` 20 × 20).
pub const RULER: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Label step in stage units: the smallest of 10 / 50 / 100 / 500 (then ×10) whose screen distance is
/// at least 50 pt, so labels never overlap ("Labels: Auto" in View settings). `every` overrides it.
pub fn ruler_step(zoom: f32, every: Option<f32>) -> f32 {
    if let Some(step) = every.filter(|s| *s > 0.0) {
        return step;
    }
    let mut base = 1.0;
    loop {
        for s in [10.0, 50.0, 100.0, 500.0] {
            if s * base * zoom >= 50.0 {
                return s * base;
            }
        }
        base *= 10.0;
        if base > 1.0e6 {
            return 5.0e6;
        }
    }
}

/// Label step for rulers in other units (points, inches, centimetres, millimetres): the smallest of
/// 1 / 5 × 10ⁿ units at least 50 pt apart. `unit_zoom` = screen points per unit. Pixels use
/// [`ruler_step`] (never below 10 px).
pub fn ruler_step_units(unit_zoom: f32, every: Option<f32>) -> f32 {
    if let Some(step) = every.filter(|s| *s > 0.0) {
        return step;
    }
    for exponent in -3..8 {
        let base = 10.0_f32.powi(exponent);
        for s in [1.0, 5.0] {
            if s * base * unit_zoom >= 50.0 {
                return s * base;
            }
        }
    }
    5.0e7
}

/// Ruler label text: whole numbers, or up to three decimals for fractional unit steps.
pub fn ruler_label(value: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{}", value.round() as i64)
    } else {
        let decimals = (-step.log10()).ceil().clamp(1.0, 3.0) as usize;
        let text = format!("{value:.decimals$}");
        if text
            .trim_start_matches('-')
            .chars()
            .all(|c| c == '0' || c == '.')
        {
            "0".into()
        } else {
            text
        }
    }
}

/// `NF2/Stage Ruler`: strip on `bg/panel`, minor ticks every step/10, labels in `NF2/Mono Small`.
/// `origin` is the screen coordinate (x for horizontal, y for vertical) of stage 0.
pub fn paint_stage_ruler(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    orientation: Orientation,
    origin: f32,
    zoom: f32,
    every: Option<f32>,
) {
    paint_stage_ruler_units(painter, ctx, rect, orientation, origin, zoom, 1.0, every);
}

/// [`paint_stage_ruler`] in other units: `unit` = stage pixels per unit (1 for pixels and points,
/// 72 for inches …). `every` is in units. Vertical labels are rotated a quarter turn
/// counter-clockwise and end just above their tick, as in the Figma ruler.
#[allow(clippy::too_many_arguments)]
pub fn paint_stage_ruler_units(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    orientation: Orientation,
    origin: f32,
    zoom: f32,
    unit: f32,
    every: Option<f32>,
) {
    let p = palette(ctx);
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, p.bg_panel);
    let horizontal = orientation == Orientation::Horizontal;
    let edge = Stroke::new(1.0_f32, p.border_subtle);
    if horizontal {
        painter.hline(rect.x_range(), rect.bottom() - 0.5, edge);
    } else {
        painter.vline(rect.right() - 0.5, rect.y_range(), edge);
    }
    let unit = if unit.is_finite() && unit > 0.0 {
        unit
    } else {
        1.0
    };
    let zoom = zoom.max(1.0e-3) * unit;
    let step = if unit == 1.0 {
        ruler_step(zoom, every)
    } else {
        ruler_step_units(zoom, every)
    };
    let minor = step / 10.0;
    let (lo, hi) = if horizontal {
        (rect.left(), rect.right())
    } else {
        (rect.top(), rect.bottom())
    };
    let first = ((lo - origin) / zoom / minor).floor() as i64 - 10;
    let last = ((hi - origin) / zoom / minor).ceil() as i64;
    let font = crate::typography::font(TextStyle::MonoSmall);
    let tick = Stroke::new(1.0_f32, p.border_strong);
    for i in first..=last {
        let units = i as f32 * minor;
        let at = origin + units * zoom;
        let major = i.rem_euclid(10) == 0;
        let half = i.rem_euclid(5) == 0;
        let len = if major {
            RULER * 0.6
        } else if half {
            RULER * 0.35
        } else {
            RULER * 0.2
        };
        let label = major.then(|| ruler_label(units, step));
        if horizontal {
            let x = at.round() + 0.5;
            painter.vline(x, (rect.bottom() - len)..=rect.bottom(), tick);
            if let Some(label) = label {
                painter.text(
                    egui::pos2(x + 2.0, rect.top() + 1.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    font.clone(),
                    p.text_tertiary,
                );
            }
        } else {
            let y = at.round() + 0.5;
            painter.hline((rect.right() - len)..=rect.right(), y, tick);
            if let Some(label) = label {
                let galley = painter.layout_no_wrap(label, font.clone(), p.text_tertiary);
                let pos = egui::pos2(rect.left() + 1.0, y - 2.0);
                painter.add(
                    egui::epaint::TextShape::new(pos, galley, p.text_tertiary)
                        .with_angle(-std::f32::consts::FRAC_PI_2),
                );
            }
        }
    }
}

/// Where the rulers sit on a canvas: `NF2/Ruler Corner` top-left, the horizontal ruler along the top
/// and the vertical ruler down the left edge, each [`RULER`] thick. Content keeps the full canvas;
/// the rulers overlay it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerLayout {
    pub corner: Rect,
    pub horizontal: Rect,
    pub vertical: Rect,
}

impl RulerLayout {
    pub fn new(canvas: Rect) -> Self {
        let corner = Rect::from_min_size(canvas.min, vec2(RULER, RULER));
        Self {
            corner,
            horizontal: Rect::from_min_max(
                egui::pos2(corner.right(), canvas.top()),
                egui::pos2(canvas.right(), corner.bottom()),
            ),
            vertical: Rect::from_min_max(
                egui::pos2(canvas.left(), corner.bottom()),
                egui::pos2(corner.right(), canvas.bottom()),
            ),
        }
    }

    /// Ruler strip a guide of `orientation` is dragged out of (and deleted by dropping back on).
    pub fn source(&self, orientation: Orientation) -> Rect {
        match orientation {
            Orientation::Horizontal => self.horizontal,
            Orientation::Vertical => self.vertical,
        }
    }

    /// Whether `pos` is on either ruler or the corner.
    pub fn contains(&self, pos: Pos2) -> bool {
        self.corner.contains(pos) || self.horizontal.contains(pos) || self.vertical.contains(pos)
    }
}

/// Half the `NF2/Guide` hit band (the component is 9 pt across its line).
pub const GUIDE_HIT: f32 = 4.5;

/// Whether `pointer` is on a guide at screen coordinate `at` (y for horizontal, x for vertical).
pub fn guide_hit(orientation: Orientation, at: f32, pointer: Pos2) -> bool {
    let d = match orientation {
        Orientation::Horizontal => pointer.y - at,
        Orientation::Vertical => pointer.x - at,
    };
    d.abs() <= GUIDE_HIT
}

/// Area a guide spans: the canvas minus the ruler strip the guide runs along (the guide starts
/// where the perpendicular ruler ends).
pub fn guide_extent(canvas: Rect, orientation: Orientation, rulers: bool) -> Rect {
    if !rulers {
        return canvas;
    }
    let mut r = canvas;
    match orientation {
        Orientation::Horizontal => r.min.x += RULER,
        Orientation::Vertical => r.min.y += RULER,
    }
    r
}

/// `NF2/Ruler Corner`: origin box where both rulers meet (drag to move zero, double-click to reset).
pub fn paint_ruler_corner(painter: &Painter, ctx: &egui::Context, rect: Rect) {
    let p = palette(ctx);
    painter.rect_filled(rect, 0.0, p.bg_panel);
    let s = Stroke::new(1.0_f32, p.border_strong);
    let c = rect.shrink(6.0);
    painter.hline(c.left()..=c.right(), c.top(), s);
    painter.vline(c.left(), c.top()..=c.bottom(), s);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, p.border_subtle),
    );
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0_f32, p.border_subtle),
    );
}

/// `State` of `NF2/Guide`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuideState {
    #[default]
    Default,
    /// Shows the drag handle.
    Selected,
    /// Dashed; cannot be moved.
    Locked,
}

/// `NF2/Guide`: a line across `extent` at screen coordinate `at`. `color` = the guide colour from
/// View settings (default `color/guide`).
pub fn paint_guide(
    painter: &Painter,
    ctx: &egui::Context,
    orientation: Orientation,
    at: f32,
    extent: Rect,
    state: GuideState,
    color: Option<Color32>,
) {
    let p = palette(ctx);
    let base = color.unwrap_or(p.guide);
    let c = if state == GuideState::Selected {
        p.guide_selected
    } else {
        base
    };
    let stroke = Stroke::new(1.0_f32, c);
    let (a, b) = match orientation {
        Orientation::Horizontal => (
            egui::pos2(extent.left(), at),
            egui::pos2(extent.right(), at),
        ),
        Orientation::Vertical => (
            egui::pos2(at, extent.top()),
            egui::pos2(at, extent.bottom()),
        ),
    };
    if state == GuideState::Locked {
        painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
    } else {
        painter.line_segment([a, b], stroke);
    }
    if state == GuideState::Selected {
        let handle = match orientation {
            Orientation::Horizontal => vec2(12.0, 0.0),
            Orientation::Vertical => vec2(0.0, 12.0),
        };
        painter.circle_filled(a + handle, 3.5, c);
    }
}

/// `NF2/Stage Grid`: lines every `spacing` stage units over `page` (screen rect of the stage), every
/// 5th line stronger. `color` = grid colour from View settings (default `color/stage-grid` at 14 %):
/// every 5th line uses it, the lines between use 42 % of it (editor-main.png: minor ≈ 6 %, major
/// 14 % white over the stage).
pub fn paint_stage_grid(
    painter: &Painter,
    ctx: &egui::Context,
    page: Rect,
    zoom: f32,
    spacing: egui::Vec2,
    color: Option<Color32>,
) {
    let p = palette(ctx);
    let strong = color.unwrap_or_else(|| p.stage_grid.gamma_multiply(0.14));
    let base = strong.gamma_multiply(0.42);
    let (sx, sy) = (spacing.x.max(1.0) * zoom, spacing.y.max(1.0) * zoom);
    if sx < 3.0 || sy < 3.0 {
        return;
    }
    let clip = page.intersect(painter.clip_rect());
    let mut i = ((clip.left() - page.left()) / sx).floor().max(0.0) as i64;
    loop {
        let x = page.left() + i as f32 * sx;
        if x > clip.right() {
            break;
        }
        let c = if i % 5 == 0 { strong } else { base };
        painter.vline(x.round() + 0.5, clip.y_range(), Stroke::new(1.0_f32, c));
        i += 1;
    }
    let mut j = ((clip.top() - page.top()) / sy).floor().max(0.0) as i64;
    loop {
        let y = page.top() + j as f32 * sy;
        if y > clip.bottom() {
            break;
        }
        let c = if j % 5 == 0 { strong } else { base };
        painter.hline(clip.x_range(), y.round() + 0.5, Stroke::new(1.0_f32, c));
        j += 1;
    }
}

/// `Kind` of `NF2/Selection Box`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Thin box only.
    Group,
    /// Registration cross (top-left) + white transformation point (centre).
    Symbol,
    /// Round handle top-right = auto width.
    TextStatic,
    /// Square handle bottom-right = fixed size.
    TextDynamic,
    /// Extra control points for angles / inner radius.
    Primitive,
}

/// `NF2/Selection Box` around object bounds `rect` (screen). Accent 1 pt outline.
pub fn paint_selection_box(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    kind: SelectionKind,
) {
    let p = palette(ctx);
    let line = Stroke::new(1.0_f32, p.accent_default);
    painter.rect_stroke(rect, 0.0, line, StrokeKind::Middle);
    match kind {
        SelectionKind::Group => {}
        SelectionKind::Symbol => {
            let o = rect.left_top();
            painter.line_segment(
                [o + vec2(-5.0, 0.0), o + vec2(5.0, 0.0)],
                Stroke::new(1.0_f32, p.text_primary),
            );
            painter.line_segment(
                [o + vec2(0.0, -5.0), o + vec2(0.0, 5.0)],
                Stroke::new(1.0_f32, p.text_primary),
            );
            transformation_point(painter, ctx, rect.center());
        }
        SelectionKind::TextStatic => {
            painter.circle(rect.right_top(), 3.5, p.bg_canvas, line);
        }
        SelectionKind::TextDynamic => {
            painter.rect(
                Rect::from_center_size(rect.right_bottom(), vec2(6.0, 6.0)),
                0.0,
                p.bg_canvas,
                line,
                StrokeKind::Middle,
            );
        }
        SelectionKind::Primitive => {
            painter.circle(
                rect.center(),
                3.5,
                p.accent_default,
                Stroke::new(1.0_f32, p.text_primary),
            );
            painter.circle(
                egui::pos2(rect.right(), rect.center().y),
                3.5,
                p.accent_default,
                Stroke::new(1.0_f32, p.text_primary),
            );
            painter.circle(
                rect.right_top() + vec2(-rect.width() * 0.09, rect.height() * 0.3),
                3.5,
                p.accent_default,
                Stroke::new(1.0_f32, p.text_primary),
            );
        }
    }
}

/// White transformation point used by symbols and the transform box.
pub fn transformation_point(painter: &Painter, ctx: &egui::Context, at: Pos2) {
    let p = palette(ctx);
    painter.circle(at, 3.5, Color32::WHITE, Stroke::new(1.0_f32, p.bg_canvas));
}

/// `Mode` of `NF2/Transform Box`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformMode {
    /// 8 square handles (⇧ proportional, ⌥ from centre).
    Scale,
    /// Round corner handles rotate, edge bars skew.
    RotateSkew,
}

/// `NF2/Transform Box`. Returns the 8 handle rects (corners clockwise from top-left, then edges
/// top, right, bottom, left) for hit-testing.
pub fn paint_transform_box(
    painter: &Painter,
    ctx: &egui::Context,
    rect: Rect,
    mode: TransformMode,
) -> [Rect; 8] {
    let p = palette(ctx);
    let line = Stroke::new(1.0_f32, p.accent_default);
    painter.rect_stroke(rect, 0.0, line, StrokeKind::Middle);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let edges = [
        rect.center_top(),
        rect.right_center(),
        rect.center_bottom(),
        rect.left_center(),
    ];
    let mut handles = [Rect::NOTHING; 8];
    for (i, c) in corners.iter().enumerate() {
        let r = Rect::from_center_size(*c, vec2(7.0, 7.0));
        handles[i] = r;
        match mode {
            TransformMode::Scale => {
                painter.rect(r, 0.0, p.bg_canvas, line, StrokeKind::Middle);
            }
            TransformMode::RotateSkew => {
                painter.circle(*c, 4.0, p.bg_canvas, line);
            }
        }
    }
    for (i, e) in edges.iter().enumerate() {
        let horizontal_edge = i % 2 == 0;
        let r = match mode {
            TransformMode::Scale => Rect::from_center_size(*e, vec2(7.0, 7.0)),
            TransformMode::RotateSkew if horizontal_edge => {
                Rect::from_center_size(*e, vec2(14.0, 3.0))
            }
            TransformMode::RotateSkew => Rect::from_center_size(*e, vec2(3.0, 14.0)),
        };
        handles[4 + i] = r;
        match mode {
            TransformMode::Scale => {
                painter.rect(r, 0.0, p.bg_canvas, line, StrokeKind::Middle);
            }
            TransformMode::RotateSkew => {
                painter.rect_filled(r, cr(1.0), p.accent_default);
            }
        }
    }
    transformation_point(painter, ctx, rect.center());
    handles
}

/// `State` of `NF2/Anchor Point`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorState {
    #[default]
    Unselected,
    Selected,
    Hover,
}

/// `NF2/Anchor Point` (path editing): hollow square, filled when selected, double square on hover.
pub fn paint_anchor(painter: &Painter, ctx: &egui::Context, at: Pos2, state: AnchorState) {
    let p = palette(ctx);
    let line = Stroke::new(1.0_f32, p.accent_default);
    let r = Rect::from_center_size(at, vec2(7.0, 7.0));
    match state {
        AnchorState::Unselected => {
            painter.rect(r, 0.0, p.bg_canvas, line, StrokeKind::Middle);
        }
        AnchorState::Selected => {
            painter.rect_filled(r, 0.0, p.accent_default);
        }
        AnchorState::Hover => {
            painter.rect(r, 0.0, p.bg_canvas, line, StrokeKind::Middle);
            painter.rect_stroke(r.expand(3.0), cr(1.0), line, StrokeKind::Middle);
        }
    }
}

/// `NF2/Bezier Handle`: direction line from an anchor to its control point.
pub fn paint_bezier_handle(painter: &Painter, ctx: &egui::Context, anchor: Pos2, control: Pos2) {
    let p = palette(ctx);
    painter.line_segment([anchor, control], Stroke::new(1.0_f32, p.accent_default));
    painter.circle_filled(control, 3.5, p.accent_default);
}

/// `NF2/Marquee`: dashed rectangle with a faint fill while drag-selecting.
pub fn paint_marquee(painter: &Painter, ctx: &egui::Context, rect: Rect) {
    let p = palette(ctx);
    painter.rect_filled(rect, 0.0, p.accent_subtle.gamma_multiply(0.35));
    let s = Stroke::new(1.0_f32, p.text_primary);
    let pts = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(&pts, s, 4.0, 3.0));
}

/// `NF2/Shape Selection`: hatch mesh over a selected raw fill / stroke. Clip the painter to the shape.
pub fn paint_shape_selection(painter: &Painter, ctx: &egui::Context, rect: Rect) {
    let p = palette(ctx);
    let s = Stroke::new(1.0_f32, p.text_tertiary.gamma_multiply(0.8));
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x + rect.height(), rect.top()),
            ],
            s,
        );
        x += 4.0;
    }
}

/// `NF2/Gradient Transform`: dashed bounds circle, centre (move), focal triangle, width square,
/// size circle and rotation dot. Returns handle centres: [centre, focal, width, size, rotation].
pub fn paint_gradient_transform(
    painter: &Painter,
    ctx: &egui::Context,
    center: Pos2,
    radius: f32,
    rotation: f32,
) -> [Pos2; 5] {
    let p = palette(ctx);
    let white = Stroke::new(1.0_f32, p.text_primary);
    let pts: Vec<Pos2> = (0..=64)
        .map(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    painter.extend(egui::Shape::dashed_line(&pts, white, 4.0, 4.0));
    let dir = vec2(rotation.cos(), rotation.sin());
    let focal = center - dir * (radius * 0.28);
    let width = center + dir * radius;
    let size_dir = vec2((rotation + 0.8).cos(), (rotation + 0.8).sin());
    let size = center + size_dir * radius;
    let rot_dir = vec2((rotation - 0.8).cos(), (rotation - 0.8).sin());
    let rot = center + rot_dir * radius;
    painter.circle_filled(center, 3.5, p.text_primary);
    painter.add(egui::Shape::convex_polygon(
        vec![
            focal + vec2(0.0, -4.0),
            focal + vec2(4.0, 3.0),
            focal + vec2(-4.0, 3.0),
        ],
        p.text_primary,
        Stroke::NONE,
    ));
    painter.rect(
        Rect::from_center_size(width, vec2(8.0, 8.0)),
        0.0,
        p.bg_canvas,
        white,
        StrokeKind::Middle,
    );
    painter.circle(size, 4.5, p.bg_canvas, white);
    painter.circle_filled(rot, 5.0, p.accent_default);
    [center, focal, width, size, rot]
}

/// `NF2/Snap Ring`: ring at the registration point while dragging; larger and thicker when snapped.
pub fn paint_snap_ring(painter: &Painter, ctx: &egui::Context, at: Pos2, snapped: bool) {
    let p = palette(ctx);
    let (r, w): (f32, f32) = if snapped { (7.0, 2.5) } else { (5.0, 2.0) };
    painter.circle_stroke(at, r, Stroke::new(w, p.text_primary));
}

/// `State` of a `NF2/Motion Path` key point (◆).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionKeyState {
    #[default]
    Default,
    /// Pointer over the point: ring around the ◆ (it can be dragged).
    Hover,
    /// Point being dragged.
    Selected,
}

/// Half diagonal of the ◆ key point and its pointer hit radius.
const MOTION_KEY: f32 = 4.5;
const MOTION_KEY_HIT: f32 = 7.0;

fn diamond(c: Pos2, d: f32) -> Vec<Pos2> {
    vec![
        c + vec2(0.0, -d),
        c + vec2(d, 0.0),
        c + vec2(0.0, d),
        c + vec2(-d, 0.0),
    ]
}

/// `NF2/Motion Path` (R-005): the path of a motion tween on the stage — a 1 pt `color/guide` polyline
/// through the per-frame registration points with a small dot at every frame. Key points are painted
/// over it with [`paint_motion_key`].
pub fn paint_motion_path(painter: &Painter, ctx: &egui::Context, frames: &[Pos2]) {
    let p = palette(ctx);
    if frames.len() >= 2 {
        painter.add(egui::Shape::line(
            frames.to_vec(),
            Stroke::new(1.0_f32, p.guide),
        ));
    }
    for &at in frames {
        painter.circle_filled(at, 1.5, p.guide);
    }
}

/// `NF2/Motion Path` key point: the `TL Marker` ◆ glyph in `color/guide` with a `bg/canvas` outline so
/// it reads over artwork; Hover adds a ring, Selected (dragging) uses `color/guide-selected`.
pub fn paint_motion_key(painter: &Painter, ctx: &egui::Context, at: Pos2, state: MotionKeyState) {
    let p = palette(ctx);
    let fill = if state == MotionKeyState::Selected {
        p.guide_selected
    } else {
        p.guide
    };
    painter.add(egui::Shape::convex_polygon(
        diamond(at, MOTION_KEY),
        fill,
        Stroke::new(1.0_f32, p.bg_canvas),
    ));
    if state == MotionKeyState::Hover {
        painter.add(egui::Shape::closed_line(
            diamond(at, MOTION_KEY + 3.0),
            Stroke::new(1.0_f32, p.guide),
        ));
    }
}

/// Whether `pointer` is on the motion path key point at `at`.
pub fn motion_key_hit(at: Pos2, pointer: Pos2) -> bool {
    at.distance(pointer) <= MOTION_KEY_HIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_key_hit_covers_the_diamond_and_its_ring() {
        let at = egui::pos2(10.0, 10.0);
        assert!(motion_key_hit(at, egui::pos2(10.0 + MOTION_KEY, 10.0)));
        assert!(motion_key_hit(at, egui::pos2(15.0, 14.0)));
        assert!(!motion_key_hit(at, egui::pos2(20.0, 10.0)));
    }

    #[test]
    fn pixel_steps_keep_labels_fifty_points_apart() {
        assert_eq!(ruler_step(1.0, None), 50.0);
        assert_eq!(ruler_step(0.75, None), 100.0);
        assert_eq!(ruler_step(0.25, None), 500.0);
        assert_eq!(ruler_step(0.1, None), 500.0);
        assert_eq!(ruler_step(0.05, None), 1000.0);
        assert_eq!(ruler_step(4.0, None), 50.0);
        assert_eq!(ruler_step(5.0, None), 10.0);
        assert_eq!(ruler_step(1.0, Some(25.0)), 25.0);
        for zoom in [0.1, 0.25, 0.33, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0] {
            assert!(ruler_step(zoom, None) * zoom >= 50.0, "{zoom}");
        }
    }

    #[test]
    fn unit_steps_go_below_one_unit() {
        // Inches at 100 %: 72 pt per inch.
        assert_eq!(ruler_step_units(72.0, None), 1.0);
        // Centimetres at 100 %: 28.35 pt per cm.
        assert_eq!(ruler_step_units(72.0 / 2.54, None), 5.0);
        // Inches at 400 %: 288 pt per inch → half inches.
        assert_eq!(ruler_step_units(288.0, None), 0.5);
        assert_eq!(ruler_label(0.5, 0.5), "0.5");
        assert_eq!(ruler_label(-0.0001, 0.5), "0");
        assert_eq!(ruler_label(150.0, 50.0), "150");
    }

    #[test]
    fn ruler_layout_and_guide_hits() {
        let canvas = Rect::from_min_size(egui::pos2(56.0, 48.0), vec2(1544.0, 588.0));
        let r = RulerLayout::new(canvas);
        assert_eq!(r.corner, Rect::from_min_size(canvas.min, vec2(20.0, 20.0)));
        assert_eq!(r.horizontal.left(), 76.0);
        assert_eq!(r.horizontal.height(), 20.0);
        assert_eq!(r.vertical.top(), 68.0);
        assert_eq!(r.vertical.width(), 20.0);
        assert!(r.contains(egui::pos2(60.0, 300.0)));
        assert!(!r.contains(egui::pos2(100.0, 100.0)));
        assert!(guide_hit(
            Orientation::Vertical,
            300.0,
            egui::pos2(304.0, 0.0)
        ));
        assert!(!guide_hit(
            Orientation::Vertical,
            300.0,
            egui::pos2(305.0, 0.0)
        ));
        assert_eq!(
            guide_extent(canvas, Orientation::Horizontal, true).left(),
            76.0
        );
    }
}
