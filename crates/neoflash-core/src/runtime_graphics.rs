//! Bounded runtime drawing state. Paint batches are owned by one display object.
use crate::{
    Color, ColorTransform, ElementKind, FillRule, PathCommand, PixelRef, RuntimeDrawKind,
    validate_path_commands,
};
use serde::{Deserialize, Serialize};

pub const MAX_GRAPHICS_COMMANDS: usize = 100_000;
pub const MAX_GRAPHICS_OPS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GradientKind {
    Linear,
    Radial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub ratio: u8,
    pub color: Color,
}

/// Canonical gradient coordinates are ±819.2 for linear endpoints and radius
/// 819.2 for radial paints. The matrix maps that space into local path coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientPaint {
    pub kind: GradientKind,
    pub matrix: [f64; 6],
    pub stops: Vec<GradientStop>,
    pub focal_point_ratio: f64,
}

impl GradientPaint {
    pub fn validate(&self) -> Result<(), &'static str> {
        let spec = GradientSpec {
            kind: self.kind,
            colors: self
                .stops
                .iter()
                .map(|s| {
                    (u32::from(s.color[0]) << 16)
                        | (u32::from(s.color[1]) << 8)
                        | u32::from(s.color[2])
                })
                .collect(),
            alphas: self
                .stops
                .iter()
                .map(|s| f64::from(s.color[3]) / 255.)
                .collect(),
            ratios: self.stops.iter().map(|s| f64::from(s.ratio)).collect(),
            matrix: self.matrix,
            spread_method: "pad".into(),
            interpolation_method: "rgb".into(),
            focal_point_ratio: self.focal_point_ratio,
        };
        spec.validate()
    }
    /// Shared native paint sampling; the browser uses the equivalent Canvas brush.
    pub fn sample(&self, x: f64, y: f64) -> Color {
        let [a, b, c, d, tx, ty] = self.matrix;
        let det = a * d - b * c;
        if det == 0. || self.stops.is_empty() {
            return [0; 4];
        }
        let x = x - tx;
        let y = y - ty;
        let gx = (d * x - c * y) / det / 819.2;
        let gy = (-b * x + a * y) / det / 819.2;
        let t = match self.kind {
            GradientKind::Linear => (gx + 1.) * 0.5,
            GradientKind::Radial => {
                let f = self.focal_point_ratio;
                let dx = gx - f;
                let qa = f * f - 1.;
                let qb = 2. * f * dx;
                let qc = dx * dx + gy * gy;
                if qc == 0. {
                    0.
                } else if qa.abs() < 1e-12 {
                    if qb == 0. { 1. } else { -qc / qb }
                } else {
                    (-qb - (qb * qb - 4. * qa * qc).max(0.).sqrt()) / (2. * qa)
                }
            }
        }
        .clamp(0., 1.)
            * 255.;
        let first = &self.stops[0];
        if t <= f64::from(first.ratio) {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            if t <= f64::from(right.ratio) {
                let span = f64::from(right.ratio - left.ratio);
                if span == 0. {
                    return right.color;
                }
                let f = (t - f64::from(left.ratio)) / span;
                return std::array::from_fn(|i| {
                    (f64::from(left.color[i])
                        + (f64::from(right.color[i]) - f64::from(left.color[i])) * f)
                        .round()
                        .clamp(0., 255.) as u8
                });
            }
        }
        self.stops.last().unwrap().color
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientSpec {
    pub kind: GradientKind,
    pub colors: Vec<u32>,
    pub alphas: Vec<f64>,
    pub ratios: Vec<f64>,
    pub matrix: [f64; 6],
    pub spread_method: String,
    pub interpolation_method: String,
    pub focal_point_ratio: f64,
}
impl GradientSpec {
    fn validate(&self) -> Result<(), &'static str> {
        let count = self.colors.len();
        if !(1..=15).contains(&count) || self.alphas.len() != count || self.ratios.len() != count {
            return Err("Graphics gradients require 1..15 equally sized colors, alphas and ratios");
        }
        if self.spread_method != "pad" || self.interpolation_method != "rgb" {
            return Err(
                "Graphics gradients currently support only pad spread and rgb interpolation",
            );
        }
        if self.colors.iter().any(|c| *c > 0xff_ffff)
            || self
                .alphas
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Gradient colors must be uint24 and alpha in 0..=1");
        }
        if self
            .ratios
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=255.0).contains(v) || v.fract() != 0.)
            || self.ratios.windows(2).any(|p| p[0] > p[1])
        {
            return Err("Gradient ratios must be nondecreasing integers in 0..=255");
        }
        let [a, b, c, d, _, _] = self.matrix;
        let det = a * d - b * c;
        if self
            .matrix
            .iter()
            .enumerate()
            .any(|(i, v)| !v.is_finite() || v.abs() > if i < 4 { 1e4 } else { 1e9 })
            || det == 0.
            || !det.is_finite()
            || [a / det, b / det, c / det, d / det]
                .iter()
                .any(|v| !v.is_finite())
        {
            return Err(
                "Gradient matrix must be finite, invertible, with linear values within ±10000 and translation within ±1e9",
            );
        }
        if !self.focal_point_ratio.is_finite() || !(-1.0..=1.0).contains(&self.focal_point_ratio) {
            return Err("Gradient focalPointRatio must be finite in -1..=1");
        }
        Ok(())
    }
    fn paint(&self) -> GradientPaint {
        GradientPaint {
            kind: self.kind,
            matrix: self.matrix,
            focal_point_ratio: self.focal_point_ratio,
            stops: self
                .colors
                .iter()
                .zip(&self.alphas)
                .zip(&self.ratios)
                .map(|((&c, &a), &ratio)| GradientStop {
                    ratio: ratio as u8,
                    color: color(c, a),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitmapFillSpec {
    pub pixel_ref: PixelRef,
    pub matrix: [f64; 6],
    pub repeat: bool,
    pub smoothing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitmapPaint {
    pub pixel_ref: PixelRef,
    pub width: u32,
    pub height: u32,
    pub revision: String,
    pub matrix: [f64; 6],
    pub repeat: bool,
    pub smoothing: bool,
    pub color_transform: ColorTransform,
}

fn validate_paint_matrix(matrix: &[f64; 6], err: &'static str) -> Result<(), &'static str> {
    let [a, b, c, d, _, _] = *matrix;
    let det = a * d - b * c;
    if matrix
        .iter()
        .enumerate()
        .any(|(i, v)| !v.is_finite() || v.abs() > if i < 4 { 1e4 } else { 1e9 })
        || det == 0.
        || !det.is_finite()
        || [a / det, b / det, c / det, d / det]
            .iter()
            .any(|v| !v.is_finite())
    {
        return Err(err);
    }
    Ok(())
}

impl BitmapFillSpec {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_paint_matrix(
            &self.matrix,
            "Bitmap fill matrix must be finite, invertible, with linear values within ±10000 and translation within ±1e9",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphicsOp {
    Clear,
    BeginFill {
        color: u32,
        alpha: f64,
    },
    BeginBitmapFill {
        bitmap: BitmapFillSpec,
    },
    BeginGradientFill {
        gradient: GradientSpec,
    },
    LineGradientStyle {
        gradient: GradientSpec,
    },
    EndFill,
    LineStyle {
        width: Option<f64>,
        color: u32,
        alpha: f64,
    },
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    CurveTo {
        cx: f64,
        cy: f64,
        x: f64,
        y: f64,
    },
    CubicCurveTo {
        cx1: f64,
        cy1: f64,
        cx2: f64,
        cy2: f64,
        x: f64,
        y: f64,
    },
    DrawPath {
        commands: Vec<PathCommand>,
        fill_rule: FillRule,
    },
    DrawRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    DrawEllipse {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    DrawRoundRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ellipse_width: f64,
        ellipse_height: f64,
    },
    DrawRoundRectComplex {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        top_left_radius: f64,
        top_right_radius: f64,
        bottom_left_radius: f64,
        bottom_right_radius: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Paint {
    Solid(Color),
    Gradient(GradientPaint),
    Bitmap(BitmapFillSpec),
}
impl Paint {
    fn cost(&self) -> usize {
        match self {
            Self::Solid(_) => 0,
            Self::Gradient(p) => p.stops.len(),
            Self::Bitmap(_) => 1,
        }
    }
    fn draw(
        &self,
        pixels: &crate::pixels::PixelArena,
        commands: Vec<PathCommand>,
        fill_rule: FillRule,
        width: f64,
        transform: crate::ColorTransform,
    ) -> RuntimeDrawKind {
        match self {
            Self::Solid(color) => RuntimeDrawKind::Element(ElementKind::Path {
                commands,
                fill_rule,
                fill: if width == 0. {
                    transform.apply(*color)
                } else {
                    [0; 4]
                },
                stroke: if width > 0. {
                    transform.apply(*color)
                } else {
                    [0; 4]
                },
                stroke_width: width,
            }),
            Self::Gradient(paint) => {
                let mut gradient = paint.clone();
                for stop in &mut gradient.stops {
                    stop.color = transform.apply(stop.color);
                }
                RuntimeDrawKind::GradientPath {
                    commands,
                    fill_rule,
                    fill: [0; 4],
                    stroke: [0; 4],
                    fill_gradient: if width == 0. {
                        Some(gradient.clone())
                    } else {
                        None
                    },
                    stroke_gradient: if width > 0. { Some(gradient) } else { None },
                    fill_bitmap: None,
                    stroke_width: width,
                }
            }
            Self::Bitmap(spec) => {
                let fill_bitmap = if width == 0.
                    && let Ok(info) = pixels.info(&spec.pixel_ref)
                    && !info.disposed
                {
                    Some(BitmapPaint {
                        pixel_ref: spec.pixel_ref.clone(),
                        width: info.width,
                        height: info.height,
                        revision: info.revision,
                        matrix: spec.matrix,
                        repeat: spec.repeat,
                        smoothing: spec.smoothing,
                        color_transform: transform,
                    })
                } else {
                    None
                };
                RuntimeDrawKind::GradientPath {
                    commands,
                    fill_rule,
                    fill: [0; 4],
                    stroke: [0; 4],
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill_bitmap,
                    stroke_width: width,
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Stroke {
    width: f64,
    paint: Paint,
}
#[derive(Clone, Debug)]
struct StrokeRun {
    style: Stroke,
    commands: Vec<PathCommand>,
    open: bool,
}
#[derive(Clone, Debug, Default)]
struct Group {
    fill: Option<Paint>,
    fill_rule: FillRule,
    commands: Vec<PathCommand>,
    open: bool,
    strokes: Vec<StrokeRun>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct GraphicsState {
    groups: Vec<Group>,
    stroke: Option<Stroke>,
    pen: [f64; 2],
    /// Counts stored commands and paint runs, including invisible pending paths.
    cost: usize,
}

fn color(value: u32, alpha: f64) -> Color {
    [
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
        (alpha * 255.).round() as u8,
    ]
}
fn valid_number(value: f64) -> bool {
    value.is_finite() && value.abs() <= 1e9
}

impl GraphicsOp {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        use GraphicsOp::*;
        let coordinates: Vec<f64> = match self {
            Clear | EndFill => vec![],
            BeginBitmapFill { bitmap } => {
                bitmap.validate()?;
                vec![]
            }
            BeginGradientFill { gradient } | LineGradientStyle { gradient } => {
                gradient.validate()?;
                vec![]
            }
            BeginFill { color, alpha } | LineStyle { color, alpha, .. } => {
                if *color > 0xff_ffff || !alpha.is_finite() || !(0.0..=1.0).contains(alpha) {
                    return Err("Graphics color must be uint24 and alpha in 0..=1");
                }
                if let LineStyle {
                    width: Some(width), ..
                } = self
                    && (!width.is_finite() || *width <= 0. || *width > 255.)
                {
                    return Err(
                        "Graphics supports positive round strokes up to 255; hairlines are unsupported",
                    );
                }
                vec![]
            }
            MoveTo { x, y } | LineTo { x, y } => vec![*x, *y],
            CurveTo { cx, cy, x, y } => vec![*cx, *cy, *x, *y],
            CubicCurveTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => vec![*cx1, *cy1, *cx2, *cy2, *x, *y],
            DrawPath { commands, .. } => {
                if commands.len() > MAX_GRAPHICS_COMMANDS {
                    return Err("Graphics drawPath exceeds 100000 commands");
                }
                let mut checked = Vec::with_capacity(commands.len() + 1);
                checked.push(PathCommand::MoveTo { x: 0., y: 0. });
                checked.extend_from_slice(commands);
                validate_path_commands(&checked).map_err(
                    |_| "Graphics drawPath requires finite bounded commands and a move after close",
                )?;
                vec![]
            }
            DrawRoundRect {
                x,
                y,
                width,
                height,
                ellipse_width,
                ellipse_height,
            } => {
                if *width < 0. || *height < 0. || *ellipse_width < 0. || *ellipse_height < 0. {
                    return Err("Graphics round rectangle dimensions must be nonnegative");
                }
                vec![
                    *x,
                    *y,
                    *width,
                    *height,
                    *x + *width,
                    *y + *height,
                    *ellipse_width,
                    *ellipse_height,
                ]
            }
            DrawRoundRectComplex {
                x,
                y,
                width,
                height,
                top_left_radius,
                top_right_radius,
                bottom_left_radius,
                bottom_right_radius,
            } => {
                let [tl, tr, bl, br] = [
                    *top_left_radius,
                    *top_right_radius,
                    *bottom_left_radius,
                    *bottom_right_radius,
                ];
                if *width < 0. || *height < 0. || [tl, tr, bl, br].iter().any(|v| *v < 0.) {
                    return Err(
                        "Graphics complex round rectangle dimensions/radii must be nonnegative",
                    );
                }
                if tl + tr > *width || bl + br > *width || tl + bl > *height || tr + br > *height {
                    return Err("Complex round rectangle corner radii must fit without overlap");
                }
                vec![
                    *x,
                    *y,
                    *width,
                    *height,
                    *x + *width,
                    *y + *height,
                    tl,
                    tr,
                    bl,
                    br,
                ]
            }
            DrawRect {
                x,
                y,
                width,
                height,
            }
            | DrawEllipse {
                x,
                y,
                width,
                height,
            } => {
                if *width < 0. || *height < 0. {
                    return Err("Graphics dimensions must be nonnegative");
                }
                vec![*x, *y, *width, *height, *x + *width, *y + *height]
            }
        };
        if coordinates.into_iter().all(valid_number) {
            Ok(())
        } else {
            Err("Graphics coordinates must be finite and within ±1e9")
        }
    }
    /// A conservative constant-time reservation prevents quadratic path validation.
    pub(crate) fn reservation(&self) -> usize {
        match self {
            Self::Clear | Self::MoveTo { .. } | Self::LineStyle { .. } => 0,
            Self::LineGradientStyle { gradient } => gradient.colors.len(),
            Self::BeginFill { .. } | Self::EndFill => 1,
            Self::BeginBitmapFill { .. } => 2,
            Self::BeginGradientFill { gradient } => 1 + gradient.colors.len(),
            Self::DrawEllipse { .. } => 32,
            Self::DrawRect { .. } => 30,
            Self::DrawRoundRect { .. } | Self::DrawRoundRectComplex { .. } => 40,
            Self::DrawPath { commands, .. } => commands.len().saturating_mul(2).saturating_add(36),
            _ => 22,
        }
    }
}

impl GraphicsState {
    pub(crate) fn cost(&self) -> usize {
        self.cost + self.stroke.as_ref().map_or(0, |s| s.paint.cost())
    }
    pub(crate) fn has_stroke(&self) -> bool {
        self.stroke.is_some()
    }
    fn group(&mut self) -> &mut Group {
        if self.groups.is_empty() {
            self.groups.push(Group {
                fill_rule: FillRule::EvenOdd,
                ..Group::default()
            });
            self.cost += 1;
        }
        self.groups.last_mut().expect("group created")
    }
    fn new_group(&mut self, fill: Option<Paint>) {
        self.cost += 1 + fill.as_ref().map_or(0, Paint::cost);
        self.groups.push(Group {
            fill,
            fill_rule: FillRule::EvenOdd,
            ..Group::default()
        });
    }
    fn append(&mut self, commands: &[PathCommand], separate: bool) {
        let pen = self.pen;
        let stroke = self.stroke.clone();
        let mut added = 0;
        let group = self.group();
        if group.fill.is_some() {
            if !separate && !group.open {
                group.commands.push(PathCommand::MoveTo {
                    x: pen[0],
                    y: pen[1],
                });
                added += 1;
            }
            group.commands.extend_from_slice(commands);
            added += commands.len();
            group.open = !separate;
        }
        if let Some(style) = stroke {
            if group.strokes.last().is_none_or(|run| run.style != style) {
                added += 1 + style.paint.cost();
                group.strokes.push(StrokeRun {
                    style,
                    commands: vec![],
                    open: false,
                });
            }
            let run = group.strokes.last_mut().expect("stroke created");
            if !separate && !run.open {
                run.commands.push(PathCommand::MoveTo {
                    x: pen[0],
                    y: pen[1],
                });
                added += 1;
            }
            run.commands.extend_from_slice(commands);
            added += commands.len();
            run.open = !separate;
        }
        self.cost += added;
    }
    /// Caller validates/reserves the complete operation batch before invoking this.
    pub(crate) fn apply(&mut self, operation: &GraphicsOp) {
        use GraphicsOp::*;
        match *operation {
            Clear => *self = Self::default(),
            BeginFill {
                color: value,
                alpha,
            } => self.new_group(Some(Paint::Solid(color(value, alpha)))),
            BeginBitmapFill { ref bitmap } => self.new_group(Some(Paint::Bitmap(bitmap.clone()))),
            BeginGradientFill { ref gradient } => {
                self.new_group(Some(Paint::Gradient(gradient.paint())))
            }
            LineGradientStyle { ref gradient } => {
                if let Some(stroke) = &mut self.stroke {
                    stroke.paint = Paint::Gradient(gradient.paint());
                }
                if let Some(group) = self.groups.last_mut()
                    && let Some(run) = group.strokes.last_mut()
                {
                    run.open = false;
                }
            }
            EndFill => self.new_group(None),
            LineStyle {
                width,
                color: value,
                alpha,
            } => {
                self.stroke = width.map(|width| Stroke {
                    width,
                    paint: Paint::Solid(color(value, alpha)),
                });
                // Even an identical style begins a new stroke run at the current pen.
                if let Some(group) = self.groups.last_mut()
                    && let Some(run) = group.strokes.last_mut()
                {
                    run.open = false;
                }
            }
            MoveTo { x, y } => {
                self.pen = [x, y];
                if let Some(group) = self.groups.last_mut() {
                    group.open = false;
                    if let Some(run) = group.strokes.last_mut() {
                        run.open = false;
                    }
                }
            }
            LineTo { x, y } => {
                self.append(&[PathCommand::LineTo { x, y }], false);
                self.pen = [x, y];
            }
            CurveTo { cx, cy, x, y } => {
                self.append(&[PathCommand::QuadraticTo { cx, cy, x, y }], false);
                self.pen = [x, y];
            }
            CubicCurveTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                self.append(
                    &[PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    }],
                    false,
                );
                self.pen = [x, y];
            }
            DrawPath {
                ref commands,
                fill_rule,
            } => {
                if !commands.is_empty() {
                    let fill = self.groups.last().and_then(|g| g.fill.clone());
                    self.new_group(fill);
                    self.group().fill_rule = fill_rule;
                    self.append(commands, false);
                    let mut start = self.pen;
                    for command in commands {
                        match *command {
                            PathCommand::MoveTo { x, y } => {
                                start = [x, y];
                                self.pen = [x, y];
                            }
                            PathCommand::LineTo { x, y }
                            | PathCommand::QuadraticTo { x, y, .. }
                            | PathCommand::CubicTo { x, y, .. } => self.pen = [x, y],
                            PathCommand::Close => self.pen = start,
                        }
                    }
                    if matches!(commands.last(), Some(PathCommand::Close)) {
                        let group = self.group();
                        group.open = false;
                        if let Some(run) = group.strokes.last_mut() {
                            run.open = false;
                        }
                    }
                }
            }
            DrawRoundRect {
                x,
                y,
                width,
                height,
                ellipse_width,
                ellipse_height,
            } => {
                let rx = ellipse_width.min(width) / 2.;
                let ry = ellipse_height.min(height) / 2.;
                self.append(&round_rect(x, y, width, height, [[rx, ry]; 4]), true);
            }
            DrawRoundRectComplex {
                x,
                y,
                width,
                height,
                top_left_radius,
                top_right_radius,
                bottom_left_radius,
                bottom_right_radius,
            } => {
                self.append(
                    &round_rect(
                        x,
                        y,
                        width,
                        height,
                        [
                            [top_left_radius; 2],
                            [top_right_radius; 2],
                            [bottom_right_radius; 2],
                            [bottom_left_radius; 2],
                        ],
                    ),
                    true,
                );
            }
            DrawRect {
                x,
                y,
                width,
                height,
            } => {
                self.append(
                    &[
                        PathCommand::MoveTo { x, y },
                        PathCommand::LineTo { x: x + width, y },
                        PathCommand::LineTo {
                            x: x + width,
                            y: y + height,
                        },
                        PathCommand::LineTo { x, y: y + height },
                        PathCommand::Close,
                    ],
                    true,
                );
            }
            DrawEllipse {
                x,
                y,
                width,
                height,
            } => {
                // Shared cubic approximation: the same path is rendered and picked
                // natively and in Canvas (no separate browser-only ellipse geometry).
                let k = 0.552_284_749_830_793_6;
                let rx = width / 2.;
                let ry = height / 2.;
                let cx = x + rx;
                let cy = y + ry;
                self.append(
                    &[
                        PathCommand::MoveTo {
                            x: x + width,
                            y: cy,
                        },
                        PathCommand::CubicTo {
                            cx1: x + width,
                            cy1: cy + k * ry,
                            cx2: cx + k * rx,
                            cy2: y + height,
                            x: cx,
                            y: y + height,
                        },
                        PathCommand::CubicTo {
                            cx1: cx - k * rx,
                            cy1: y + height,
                            cx2: x,
                            cy2: cy + k * ry,
                            x,
                            y: cy,
                        },
                        PathCommand::CubicTo {
                            cx1: x,
                            cy1: cy - k * ry,
                            cx2: cx - k * rx,
                            cy2: y,
                            x: cx,
                            y,
                        },
                        PathCommand::CubicTo {
                            cx1: cx + k * rx,
                            cy1: y,
                            cx2: x + width,
                            cy2: cy - k * ry,
                            x: x + width,
                            y: cy,
                        },
                        PathCommand::Close,
                    ],
                    true,
                );
            }
        }
    }
    /// Fill presence is independent of color alpha; stroke-only runs do not mask.
    pub(crate) fn mask_paths(&self) -> impl Iterator<Item = (&[PathCommand], FillRule)> {
        self.groups
            .iter()
            .filter(|group| group.fill.is_some() && group.commands.len() > 1)
            .map(|group| (group.commands.as_slice(), group.fill_rule))
    }
    pub(crate) fn paints(&self, pixels: &crate::pixels::PixelArena) -> Vec<RuntimeDrawKind> {
        self.paints_with_color(pixels, crate::ColorTransform::default())
    }
    pub(crate) fn paints_with_color(
        &self,
        pixels: &crate::pixels::PixelArena,
        color: crate::ColorTransform,
    ) -> Vec<RuntimeDrawKind> {
        let mut paints = vec![];
        for group in &self.groups {
            if let Some(fill) = &group.fill
                && group.commands.len() > 1
                && !matches!(
                    fill,
                    Paint::Bitmap(spec)
                        if !pixels
                            .info(&spec.pixel_ref)
                            .is_ok_and(|info| !info.disposed)
                )
            {
                paints.push(fill.draw(pixels, group.commands.clone(), group.fill_rule, 0., color));
            }
            for run in &group.strokes {
                if run.commands.len() > 1 {
                    paints.push(run.style.paint.draw(
                        pixels,
                        run.commands.clone(),
                        group.fill_rule,
                        run.style.width,
                        color,
                    ));
                }
            }
        }
        paints
    }
}

fn round_rect(x: f64, y: f64, width: f64, height: f64, corners: [[f64; 2]; 4]) -> Vec<PathCommand> {
    let k = 0.552_284_749_830_793_6;
    let [[tlx, tly], [trx, try_], [brx, bry], [blx, bly]] = corners;
    let right = x + width;
    let bottom = y + height;
    vec![
        PathCommand::MoveTo { x: x + tlx, y },
        PathCommand::LineTo { x: right - trx, y },
        PathCommand::CubicTo {
            cx1: right - trx + k * trx,
            cy1: y,
            cx2: right,
            cy2: y + try_ - k * try_,
            x: right,
            y: y + try_,
        },
        PathCommand::LineTo {
            x: right,
            y: bottom - bry,
        },
        PathCommand::CubicTo {
            cx1: right,
            cy1: bottom - bry + k * bry,
            cx2: right - brx + k * brx,
            cy2: bottom,
            x: right - brx,
            y: bottom,
        },
        PathCommand::LineTo {
            x: x + blx,
            y: bottom,
        },
        PathCommand::CubicTo {
            cx1: x + blx - k * blx,
            cy1: bottom,
            cx2: x,
            cy2: bottom - bly + k * bly,
            x,
            y: bottom - bly,
        },
        PathCommand::LineTo { x, y: y + tly },
        PathCommand::CubicTo {
            cx1: x,
            cy1: y + tly - k * tly,
            cx2: x + tlx - k * tlx,
            cy2: y,
            x: x + tlx,
            y,
        },
        PathCommand::Close,
    ]
}
