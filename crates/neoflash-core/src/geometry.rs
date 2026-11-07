//! Renderer-independent local-space path geometry. No renderer handles or caches
//! are serialized. Helpers return a limit error instead of truncated geometry.
use crate::{FillRule, PathCommand};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use thiserror::Error;

pub const MAX_PATH_COMMANDS: usize = 100_000;
const MAX_FLATTENED_POINTS: usize = 1_000_000;
const MAX_COORDINATE: f64 = 1e9;
type Point = [f64; 2];

/// Non-serialized identity for path geometry prepared during evaluation.
///
/// This is a derived cache hint only: renderers must remain correct when it is absent and document
/// equality/serialization must never depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFingerprint {
    pub hash_a: u64,
    pub hash_b: u64,
    pub has_curves: bool,
}

/// Fingerprint path content once so a renderer can share bounds/tessellation keys within a frame.
pub fn path_commands_fingerprint(commands: &[PathCommand]) -> PathFingerprint {
    let mut hash_a = std::collections::hash_map::DefaultHasher::new();
    let mut hash_b: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |tag: u8, values: &[f64]| {
        tag.hash(&mut hash_a);
        hash_b = (hash_b ^ u64::from(tag)).wrapping_mul(0x0100_0000_01b3);
        for &value in values {
            let bits = if value == 0.0 { 0 } else { value.to_bits() };
            bits.hash(&mut hash_a);
            hash_b = (hash_b ^ bits).wrapping_mul(0x0100_0000_01b3);
        }
    };
    let mut has_curves = false;
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => mix(1, &[x, y]),
            PathCommand::LineTo { x, y } => mix(2, &[x, y]),
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                has_curves = true;
                mix(3, &[cx, cy, x, y]);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                has_curves = true;
                mix(4, &[cx1, cy1, cx2, cy2, x, y]);
            }
            PathCommand::Close => mix(5, &[]),
        }
    }
    PathFingerprint {
        hash_a: hash_a.finish(),
        hash_b,
        has_curves,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FlattenOptions {
    /// Maximum control-hull distance to the chord, in local-coordinate units.
    pub tolerance: f64,
    pub max_depth: u8,
    pub max_points: usize,
}

impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.25,
            max_depth: 16,
            max_points: 65_536,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlattenedContour {
    pub points: Vec<Point>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathBounds {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

impl PathBounds {
    pub fn width(self) -> f64 {
        self.x_max - self.x_min
    }
    pub fn height(self) -> f64 {
        self.y_max - self.y_min
    }
    pub fn contains(self, point: Point) -> bool {
        point[0] >= self.x_min
            && point[0] <= self.x_max
            && point[1] >= self.y_min
            && point[1] <= self.y_max
    }
    /// Conservative bounds including a round stroke in local coordinates.
    pub fn expanded(self, amount: f64) -> Self {
        let amount = amount.max(0.);
        Self {
            x_min: self.x_min - amount,
            y_min: self.y_min - amount,
            x_max: self.x_max + amount,
            y_max: self.y_max + amount,
        }
    }
    fn include(&mut self, p: Point) {
        self.x_min = self.x_min.min(p[0]);
        self.y_min = self.y_min.min(p[1]);
        self.x_max = self.x_max.max(p[0]);
        self.y_max = self.y_max.max(p[1]);
    }
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum GeometryError {
    #[error("Invalid vector path: {0}")]
    InvalidPath(String),
    #[error(
        "Invalid flattening options: tolerance must be positive and finite, depth <=24, points in 1..=1000000"
    )]
    InvalidOptions,
    #[error(
        "Path flattening exceeded its point/depth budget; increase tolerance or simplify the path"
    )]
    ComplexityLimit,
}

pub fn validate_path_commands(commands: &[PathCommand]) -> Result<(), GeometryError> {
    if commands.len() > MAX_PATH_COMMANDS {
        return Err(GeometryError::InvalidPath(format!(
            "more than {MAX_PATH_COMMANDS} commands"
        )));
    }
    let mut open = false;
    for (index, command) in commands.iter().enumerate() {
        let coordinates: &[f64] = match command {
            PathCommand::MoveTo { x, y } => {
                open = true;
                &[*x, *y]
            }
            PathCommand::LineTo { x, y } => &[*x, *y],
            PathCommand::QuadraticTo { cx, cy, x, y } => &[*cx, *cy, *x, *y],
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => &[*cx1, *cy1, *cx2, *cy2, *x, *y],
            PathCommand::Close => &[],
        };
        if !open {
            return Err(GeometryError::InvalidPath(format!(
                "command {index} must follow MoveTo"
            )));
        }
        if coordinates
            .iter()
            .any(|v| !v.is_finite() || v.abs() > MAX_COORDINATE)
        {
            return Err(GeometryError::InvalidPath(format!(
                "command {index} has nonfinite coordinates or |coordinate| > 1e9"
            )));
        }
        if matches!(command, PathCommand::Close) {
            open = false;
        }
    }
    Ok(())
}

fn options_valid(options: FlattenOptions) -> bool {
    options.tolerance.is_finite()
        && options.tolerance > 0.
        && options.max_depth <= 24
        && options.max_points > 0
        && options.max_points <= MAX_FLATTENED_POINTS
}
fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn distance_squared(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
fn distance_to_segment_squared(p: Point, a: Point, b: Point) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let length = ab[0] * ab[0] + ab[1] * ab[1];
    if length == 0. {
        return distance_squared(p, a);
    }
    let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / length).clamp(0., 1.);
    distance_squared(p, [a[0] + t * ab[0], a[1] + t * ab[1]])
}

struct Flattener {
    options: FlattenOptions,
    points: usize,
}
impl Flattener {
    fn push(&mut self, out: &mut Vec<Point>, point: Point) -> Result<(), GeometryError> {
        if self.points >= self.options.max_points {
            return Err(GeometryError::ComplexityLimit);
        }
        self.points += 1;
        out.push(point);
        Ok(())
    }
    fn quadratic(
        &mut self,
        out: &mut Vec<Point>,
        p0: Point,
        p1: Point,
        p2: Point,
        depth: u8,
    ) -> Result<(), GeometryError> {
        if distance_to_segment_squared(p1, p0, p2) <= self.options.tolerance.powi(2) {
            return self.push(out, p2);
        }
        if depth >= self.options.max_depth {
            return Err(GeometryError::ComplexityLimit);
        }
        let a = midpoint(p0, p1);
        let b = midpoint(p1, p2);
        let m = midpoint(a, b);
        self.quadratic(out, p0, a, m, depth + 1)?;
        self.quadratic(out, m, b, p2, depth + 1)
    }
    fn cubic(
        &mut self,
        out: &mut Vec<Point>,
        points: [Point; 4],
        depth: u8,
    ) -> Result<(), GeometryError> {
        let [p0, p1, p2, p3] = points;
        if distance_to_segment_squared(p1, p0, p3).max(distance_to_segment_squared(p2, p0, p3))
            <= self.options.tolerance.powi(2)
        {
            return self.push(out, p3);
        }
        if depth >= self.options.max_depth {
            return Err(GeometryError::ComplexityLimit);
        }
        let a = midpoint(p0, p1);
        let b = midpoint(p1, p2);
        let c = midpoint(p2, p3);
        let d = midpoint(a, b);
        let e = midpoint(b, c);
        let m = midpoint(d, e);
        self.cubic(out, [p0, a, d, m], depth + 1)?;
        self.cubic(out, [m, e, c, p3], depth + 1)
    }
}

pub fn flatten_path(
    commands: &[PathCommand],
    options: FlattenOptions,
) -> Result<Vec<FlattenedContour>, GeometryError> {
    validate_path_commands(commands)?;
    if !options_valid(options) {
        return Err(GeometryError::InvalidOptions);
    }
    let mut flatten = Flattener { options, points: 0 };
    let mut contours = vec![];
    let mut contour: Option<FlattenedContour> = None;
    let mut current = [0., 0.];
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                if let Some(previous) = contour.take() {
                    contours.push(previous);
                }
                current = [x, y];
                let mut points = vec![];
                flatten.push(&mut points, current)?;
                contour = Some(FlattenedContour {
                    points,
                    closed: false,
                });
            }
            PathCommand::LineTo { x, y } => {
                current = [x, y];
                flatten.push(&mut contour.as_mut().unwrap().points, current)?;
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                flatten.quadratic(
                    &mut contour.as_mut().unwrap().points,
                    current,
                    [cx, cy],
                    [x, y],
                    0,
                )?;
                current = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                flatten.cubic(
                    &mut contour.as_mut().unwrap().points,
                    [current, [cx1, cy1], [cx2, cy2], [x, y]],
                    0,
                )?;
                current = [x, y];
            }
            PathCommand::Close => {
                let mut finished = contour.take().unwrap();
                finished.closed = true;
                contours.push(finished);
            }
        }
    }
    if let Some(last) = contour {
        contours.push(last);
    }
    Ok(contours)
}

fn quadratic_point(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    std::array::from_fn(|i| (1. - t).powi(2) * p0[i] + 2. * (1. - t) * t * p1[i] + t * t * p2[i])
}
fn cubic_point(p: [Point; 4], t: f64) -> Point {
    std::array::from_fn(|i| {
        (1. - t).powi(3) * p[0][i]
            + 3. * (1. - t).powi(2) * t * p[1][i]
            + 3. * (1. - t) * t * t * p[2][i]
            + t.powi(3) * p[3][i]
    })
}

/// Tight axis-aligned bounds of drawable path segments, excluding stroke.
/// Move-only/empty paths have no drawable bounds.
pub fn path_bounds(commands: &[PathCommand]) -> Result<Option<PathBounds>, GeometryError> {
    validate_path_commands(commands)?;
    let mut bounds: Option<PathBounds> = None;
    let mut current = [0., 0.];
    let mut start = current;
    let mut has_segment = false;
    let mut include = |point: Point| {
        if let Some(bounds) = &mut bounds {
            bounds.include(point);
        } else {
            bounds = Some(PathBounds {
                x_min: point[0],
                y_min: point[1],
                x_max: point[0],
                y_max: point[1],
            });
        }
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                current = [x, y];
                start = current;
                has_segment = false;
            }
            PathCommand::LineTo { x, y } => {
                has_segment = true;
                include(current);
                current = [x, y];
                include(current);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                has_segment = true;
                let end = [x, y];
                let control = [cx, cy];
                include(current);
                include(end);
                for axis in 0..2 {
                    let denominator = current[axis] - 2. * control[axis] + end[axis];
                    if denominator != 0. {
                        let t = (current[axis] - control[axis]) / denominator;
                        if t > 0. && t < 1. {
                            include(quadratic_point(current, control, end, t));
                        }
                    }
                }
                current = end;
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                has_segment = true;
                let p = [current, [cx1, cy1], [cx2, cy2], [x, y]];
                include(current);
                include(p[3]);
                for axis in 0..2 {
                    let a = -p[0][axis] + 3. * p[1][axis] - 3. * p[2][axis] + p[3][axis];
                    let b = 2. * (p[0][axis] - 2. * p[1][axis] + p[2][axis]);
                    let c = p[1][axis] - p[0][axis];
                    let mut roots = [f64::NAN; 2];
                    if a.abs() <= f64::EPSILON * (b.abs() + c.abs()).max(1.) {
                        if b != 0. {
                            roots[0] = -c / b;
                        }
                    } else {
                        let discriminant = b * b - 4. * a * c;
                        if discriminant >= 0. {
                            let d = discriminant.sqrt();
                            let q = -0.5 * (b + d.copysign(b));
                            if q == 0. {
                                roots[0] = -b / (2. * a);
                            } else {
                                roots = [q / a, c / q];
                            }
                        }
                    }
                    for t in roots {
                        if t > 0. && t < 1. {
                            include(cubic_point(p, t));
                        }
                    }
                }
                current = p[3];
            }
            PathCommand::Close => {
                if has_segment {
                    include(current);
                    include(start);
                }
                current = start;
            }
        }
    }
    Ok(bounds)
}

/// Local-space picking. Fill closes every contour implicitly. Stroke tests only
/// explicit segments plus Close, using round caps/joins. Curve tolerance is the
/// geometric approximation bound; convert screen tolerance to local units first.
pub fn hit_test_path(
    commands: &[PathCommand],
    rule: FillRule,
    point: Point,
    fill_enabled: bool,
    stroke_width: f64,
    options: FlattenOptions,
) -> Result<bool, GeometryError> {
    if !point.iter().all(|v| v.is_finite()) || !stroke_width.is_finite() || stroke_width < 0. {
        return Err(GeometryError::InvalidPath(
            "nonfinite hit-test point or invalid stroke width".into(),
        ));
    }
    let contours = flatten_path(commands, options)?;
    let mut winding = 0i64;
    let mut crossings = 0u64;
    let stroke_distance = (stroke_width * 0.5).powi(2);
    for contour in &contours {
        if contour.points.len() < 2 {
            continue;
        }
        for edge in contour.points.windows(2) {
            if stroke_width > 0.
                && distance_to_segment_squared(point, edge[0], edge[1]) <= stroke_distance
            {
                return Ok(true);
            }
        }
        if contour.closed
            && stroke_width > 0.
            && distance_to_segment_squared(
                point,
                *contour.points.last().unwrap(),
                contour.points[0],
            ) <= stroke_distance
        {
            return Ok(true);
        }
        if !fill_enabled {
            continue;
        }
        for i in 0..contour.points.len() {
            let a = contour.points[i];
            let b = contour.points[(i + 1) % contour.points.len()];
            // A point exactly on a nondegenerate fill boundary belongs to the fill.
            if contour.points.len() >= 3
                && distance_squared(a, b) > 0.
                && distance_to_segment_squared(point, a, b) <= 1e-20
            {
                return Ok(true);
            }
            if (a[1] > point[1]) != (b[1] > point[1]) {
                let x = a[0] + (point[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x > point[0] {
                    crossings += 1;
                    winding += if b[1] > a[1] { 1 } else { -1 };
                }
            }
        }
    }
    Ok(fill_enabled
        && match rule {
            FillRule::NonZero => winding != 0,
            FillRule::EvenOdd => crossings % 2 == 1,
        })
}

/// Only equal command variants/counts have a defined point-to-point morph.
pub fn paths_have_same_topology(a: &[PathCommand], b: &[PathCommand]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b))
}

pub(crate) fn interpolate_commands(
    a: &[PathCommand],
    b: &[PathCommand],
    t: f64,
) -> Vec<PathCommand> {
    let mix = |a: f64, b: f64| a + (b - a) * t;
    a.iter()
        .zip(b)
        .map(|(a, b)| match (*a, *b) {
            (PathCommand::MoveTo { x: a, y: b }, PathCommand::MoveTo { x: c, y: d }) => {
                PathCommand::MoveTo {
                    x: mix(a, c),
                    y: mix(b, d),
                }
            }
            (PathCommand::LineTo { x: a, y: b }, PathCommand::LineTo { x: c, y: d }) => {
                PathCommand::LineTo {
                    x: mix(a, c),
                    y: mix(b, d),
                }
            }
            (
                PathCommand::QuadraticTo {
                    cx: a,
                    cy: b,
                    x: c,
                    y: d,
                },
                PathCommand::QuadraticTo {
                    cx: e,
                    cy: f,
                    x: g,
                    y: h,
                },
            ) => PathCommand::QuadraticTo {
                cx: mix(a, e),
                cy: mix(b, f),
                x: mix(c, g),
                y: mix(d, h),
            },
            (
                PathCommand::CubicTo {
                    cx1: a,
                    cy1: b,
                    cx2: c,
                    cy2: d,
                    x: e,
                    y: f,
                },
                PathCommand::CubicTo {
                    cx1: g,
                    cy1: h,
                    cx2: i,
                    cy2: j,
                    x: k,
                    y: l,
                },
            ) => PathCommand::CubicTo {
                cx1: mix(a, g),
                cy1: mix(b, h),
                cx2: mix(c, i),
                cy2: mix(d, j),
                x: mix(e, k),
                y: mix(f, l),
            },
            _ => *a,
        })
        .collect()
}

/// Exact clockwise vector contour for a `Rectangle` with optional per-corner radii.
/// Corner radii `[top_left, top_right, bottom_left, bottom_right]` are proportionally
/// clamped so adjacent corners never overlap.
pub fn rectangle_path_commands(
    width: f64,
    height: f64,
    options: crate::RectangleOptions,
) -> Vec<PathCommand> {
    const KAPPA: f64 = 0.552_284_749_830_793_6;
    let w = width.max(0.0);
    let h = height.max(0.0);
    let [mut tl, mut tr, mut bl, mut br] = options
        .corner_radii
        .map(|r| if r.is_finite() { r.max(0.0) } else { 0.0 });
    let mut scale = 1.0_f64;
    for (sum, limit) in [(tl + tr, w), (bl + br, w), (tl + bl, h), (tr + br, h)] {
        if sum > 1e-9 && sum > limit {
            scale = scale.min(limit / sum);
        }
    }
    tl *= scale;
    tr *= scale;
    bl *= scale;
    br *= scale;

    if tl <= 1e-9 && tr <= 1e-9 && bl <= 1e-9 && br <= 1e-9 {
        return vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: w, y: 0.0 },
            PathCommand::LineTo { x: w, y: h },
            PathCommand::LineTo { x: 0.0, y: h },
            PathCommand::Close,
        ];
    }

    let mut commands = Vec::with_capacity(10);
    commands.push(PathCommand::MoveTo { x: tl, y: 0.0 });
    if (w - tr - tl) > 1e-9 {
        commands.push(PathCommand::LineTo { x: w - tr, y: 0.0 });
    }
    if tr > 1e-9 {
        commands.push(PathCommand::CubicTo {
            cx1: w - tr + tr * KAPPA,
            cy1: 0.0,
            cx2: w,
            cy2: tr - tr * KAPPA,
            x: w,
            y: tr,
        });
    }
    if (h - br - tr) > 1e-9 {
        commands.push(PathCommand::LineTo { x: w, y: h - br });
    }
    if br > 1e-9 {
        commands.push(PathCommand::CubicTo {
            cx1: w,
            cy1: h - br + br * KAPPA,
            cx2: w - br + br * KAPPA,
            cy2: h,
            x: w - br,
            y: h,
        });
    }
    if (w - bl - br) > 1e-9 {
        commands.push(PathCommand::LineTo { x: bl, y: h });
    }
    if bl > 1e-9 {
        commands.push(PathCommand::CubicTo {
            cx1: bl - bl * KAPPA,
            cy1: h,
            cx2: 0.0,
            cy2: h - bl + bl * KAPPA,
            x: 0.0,
            y: h - bl,
        });
    }
    if (h - tl - bl) > 1e-9 {
        commands.push(PathCommand::LineTo { x: 0.0, y: tl });
    }
    if tl > 1e-9 {
        commands.push(PathCommand::CubicTo {
            cx1: 0.0,
            cy1: tl - tl * KAPPA,
            cx2: tl - tl * KAPPA,
            cy2: 0.0,
            x: tl,
            y: 0.0,
        });
    }
    commands.push(PathCommand::Close);
    commands
}

#[derive(Clone, Copy)]
enum ArcStart {
    Move,
    Line,
}

#[allow(clippy::too_many_arguments)]
fn append_elliptic_arc(
    commands: &mut Vec<PathCommand>,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    start_rad: f64,
    sweep_rad: f64,
    start_mode: ArcStart,
) {
    let sx = cx + rx * start_rad.cos();
    let sy = cy + ry * start_rad.sin();
    match start_mode {
        ArcStart::Move => commands.push(PathCommand::MoveTo { x: sx, y: sy }),
        ArcStart::Line => commands.push(PathCommand::LineTo { x: sx, y: sy }),
    }
    let steps = ((sweep_rad.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let delta = sweep_rad / (steps as f64);
    let alpha = (4.0 / 3.0) * (delta * 0.25).tan();
    for step in 0..steps {
        let a0 = start_rad + (step as f64) * delta;
        let a1 = a0 + delta;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        commands.push(PathCommand::CubicTo {
            cx1: cx + rx * (c0 - alpha * s0),
            cy1: cy + ry * (s0 + alpha * c0),
            cx2: cx + rx * (c1 + alpha * s1),
            cy2: cy + ry * (s1 - alpha * c1),
            x: cx + rx * c1,
            y: cy + ry * s1,
        });
    }
}

/// Exact vector contour for an `Ellipse` (Oval) with optional start/end sweep angles,
/// inner hole radius percentage (`0..=99`), and open/closed path flag.
pub fn oval_path_commands(
    width: f64,
    height: f64,
    options: crate::OvalOptions,
) -> Vec<PathCommand> {
    let w = width.max(0.0);
    let h = height.max(0.0);
    let (cx, cy) = (w * 0.5, h * 0.5);
    let (rx, ry) = (w * 0.5, h * 0.5);
    let inner_ratio = if options.inner_radius.is_finite() {
        options.inner_radius.clamp(0.0, 99.0) / 100.0
    } else {
        0.0
    };
    let (irx, iry) = (rx * inner_ratio, ry * inner_ratio);
    let a0 = options.start_angle.to_radians();
    let mut commands = Vec::with_capacity(12);

    if !options.is_partial_sweep() {
        append_elliptic_arc(
            &mut commands,
            cx,
            cy,
            rx,
            ry,
            a0,
            std::f64::consts::TAU,
            ArcStart::Move,
        );
        if options.close_path {
            commands.push(PathCommand::Close);
        }
        if inner_ratio > 1e-6 {
            append_elliptic_arc(
                &mut commands,
                cx,
                cy,
                irx,
                iry,
                a0,
                -std::f64::consts::TAU,
                ArcStart::Move,
            );
            if options.close_path {
                commands.push(PathCommand::Close);
            }
        }
    } else {
        let sweep_deg = (options.end_angle - options.start_angle).rem_euclid(360.0);
        let sweep = sweep_deg.to_radians();
        let a1 = a0 + sweep;
        append_elliptic_arc(&mut commands, cx, cy, rx, ry, a0, sweep, ArcStart::Move);
        if options.close_path {
            if inner_ratio > 1e-6 {
                append_elliptic_arc(&mut commands, cx, cy, irx, iry, a1, -sweep, ArcStart::Line);
                commands.push(PathCommand::Close);
            } else {
                commands.push(PathCommand::LineTo { x: cx, y: cy });
                commands.push(PathCommand::Close);
            }
        } else if inner_ratio > 1e-6 {
            append_elliptic_arc(&mut commands, cx, cy, irx, iry, a0, sweep, ArcStart::Move);
        }
    }
    commands
}

/// Generate a closed polygon or star contour centered at `center` with outer `radius` and
/// first-vertex `angle_radians`. Returns `(local_commands, [origin_x, origin_y])` with the
/// local bounding box shifted to `(0, 0)`.
pub fn polystar_path_commands(
    center: [f64; 2],
    radius: f64,
    angle_radians: f64,
    sides: u32,
    star: bool,
    point_size: f64,
) -> (Vec<PathCommand>, [f64; 2]) {
    let sides = sides.clamp(3, 32);
    let r = radius.max(0.0);
    let inner_r = r * point_size.clamp(0.05, 0.95);
    let count = if star { sides * 2 } else { sides };
    let step = std::f64::consts::TAU / (count as f64);
    let mut points = Vec::with_capacity(count as usize);
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    for i in 0..count {
        let rad = if star && i % 2 == 1 { inner_r } else { r };
        let a = angle_radians + (i as f64) * step;
        let x = center[0] + rad * a.cos();
        let y = center[1] + rad * a.sin();
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        points.push([x, y]);
    }
    let mut commands = Vec::with_capacity(points.len() + 1);
    for (i, [x, y]) in points.into_iter().enumerate() {
        let (lx, ly) = (x - min_x, y - min_y);
        if i == 0 {
            commands.push(PathCommand::MoveTo { x: lx, y: ly });
        } else {
            commands.push(PathCommand::LineTo { x: lx, y: ly });
        }
    }
    commands.push(PathCommand::Close);
    (commands, [min_x, min_y])
}

fn lerp_pt(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Subdivide the path segment at `command_index` at parameter `t` (`0 < t < 1`) using exact
/// De Casteljau subdivision. Returns `(new_commands, inserted_anchor_command_index)`.
pub fn subdivide_path_at(
    commands: &[PathCommand],
    command_index: usize,
    t: f64,
) -> Option<(Vec<PathCommand>, usize)> {
    if command_index >= commands.len() || !t.is_finite() {
        return None;
    }
    let t = t.clamp(1e-4, 1.0 - 1e-4);
    let mut subpath_start: Option<Point> = None;
    let mut cursor: Option<Point> = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if idx == command_index {
            let p0 = cursor?;
            let mut out = Vec::with_capacity(commands.len() + 1);
            out.extend_from_slice(&commands[..command_index]);
            match *cmd {
                PathCommand::MoveTo { .. } => return None,
                PathCommand::LineTo { x, y } => {
                    let mid = lerp_pt(p0, [x, y], t);
                    out.push(PathCommand::LineTo {
                        x: mid[0],
                        y: mid[1],
                    });
                    out.push(PathCommand::LineTo { x, y });
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let c = [cx, cy];
                    let p2 = [x, y];
                    let q0 = lerp_pt(p0, c, t);
                    let q1 = lerp_pt(c, p2, t);
                    let mid = lerp_pt(q0, q1, t);
                    out.push(PathCommand::QuadraticTo {
                        cx: q0[0],
                        cy: q0[1],
                        x: mid[0],
                        y: mid[1],
                    });
                    out.push(PathCommand::QuadraticTo {
                        cx: q1[0],
                        cy: q1[1],
                        x,
                        y,
                    });
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let c1 = [cx1, cy1];
                    let c2 = [cx2, cy2];
                    let p3 = [x, y];
                    let a0 = lerp_pt(p0, c1, t);
                    let a1 = lerp_pt(c1, c2, t);
                    let a2 = lerp_pt(c2, p3, t);
                    let b0 = lerp_pt(a0, a1, t);
                    let b1 = lerp_pt(a1, a2, t);
                    let mid = lerp_pt(b0, b1, t);
                    out.push(PathCommand::CubicTo {
                        cx1: a0[0],
                        cy1: a0[1],
                        cx2: b0[0],
                        cy2: b0[1],
                        x: mid[0],
                        y: mid[1],
                    });
                    out.push(PathCommand::CubicTo {
                        cx1: b1[0],
                        cy1: b1[1],
                        cx2: a2[0],
                        cy2: a2[1],
                        x,
                        y,
                    });
                }
                PathCommand::Close => {
                    let start = subpath_start?;
                    if (start[0] - p0[0]).hypot(start[1] - p0[1]) <= 1e-6 {
                        return None;
                    }
                    let mid = lerp_pt(p0, start, t);
                    out.push(PathCommand::LineTo {
                        x: mid[0],
                        y: mid[1],
                    });
                    out.push(PathCommand::Close);
                }
            }
            out.extend_from_slice(&commands[command_index + 1..]);
            return Some((out, command_index));
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                cursor = subpath_start;
            }
        }
    }
    None
}

fn eval_quad(p0: Point, c: Point, p2: Point, t: f64) -> Point {
    let u = 1.0 - t;
    [
        u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p2[0],
        u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p2[1],
    ]
}

fn eval_cubic(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let (u2, t2) = (u * u, t * t);
    [
        u2 * u * p0[0] + 3.0 * u2 * t * c1[0] + 3.0 * u * t2 * c2[0] + t2 * t * p3[0],
        u2 * u * p0[1] + 3.0 * u2 * t * c1[1] + 3.0 * u * t2 * c2[1] + t2 * t * p3[1],
    ]
}

/// Expand a stroked path into a closed filled outline contour using `base_width` and a
/// variable-width `StrokeWidthProfile` (`Modify ▸ Shape ▸ Expand Stroke` / width profiles).
pub fn expand_variable_width_stroke(
    commands: &[PathCommand],
    base_width: f64,
    profile: &crate::StrokeWidthProfile,
) -> Vec<PathCommand> {
    if base_width <= 0.0 || !base_width.is_finite() {
        return Vec::new();
    }
    let Ok(contours) = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.25,
            ..FlattenOptions::default()
        },
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for contour in contours {
        let mut pts = contour.points;
        if contour.closed && pts.len() >= 2 && pts.first() != pts.last() {
            pts.push(pts[0]);
        }
        pts.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6);
        if pts.len() < 2 {
            continue;
        }
        let mut cum = Vec::with_capacity(pts.len());
        cum.push(0.0);
        for pair in pts.windows(2) {
            let d = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            cum.push(cum.last().copied().unwrap_or(0.0) + d);
        }
        let total = cum.last().copied().unwrap_or(0.0);
        if total <= 1e-6 {
            continue;
        }
        let mut left = Vec::with_capacity(pts.len());
        let mut right = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let t = (cum[i] / total).clamp(0.0, 1.0);
            let hw = (0.5 * base_width * profile.sample_scale(t)).max(0.05);
            let dir_prev = if i > 0 {
                let dx = pts[i][0] - pts[i - 1][0];
                let dy = pts[i][1] - pts[i - 1][1];
                let len = dx.hypot(dy).max(1e-9);
                [dx / len, dy / len]
            } else {
                let dx = pts[1][0] - pts[0][0];
                let dy = pts[1][1] - pts[0][1];
                let len = dx.hypot(dy).max(1e-9);
                [dx / len, dy / len]
            };
            let dir_next = if i + 1 < pts.len() {
                let dx = pts[i + 1][0] - pts[i][0];
                let dy = pts[i + 1][1] - pts[i][1];
                let len = dx.hypot(dy).max(1e-9);
                [dx / len, dy / len]
            } else {
                dir_prev
            };
            let n_prev = [-dir_prev[1], dir_prev[0]];
            let n_next = [-dir_next[1], dir_next[0]];
            let mut nx = n_prev[0] + n_next[0];
            let mut ny = n_prev[1] + n_next[1];
            let n_len = nx.hypot(ny);
            if n_len > 1e-6 {
                nx /= n_len;
                ny /= n_len;
            } else {
                nx = n_prev[0];
                ny = n_prev[1];
            }
            left.push([pts[i][0] + nx * hw, pts[i][1] + ny * hw]);
            right.push([pts[i][0] - nx * hw, pts[i][1] - ny * hw]);
        }
        if let Some(&first) = left.first() {
            out.push(PathCommand::MoveTo {
                x: first[0],
                y: first[1],
            });
            for &[x, y] in &left[1..] {
                out.push(PathCommand::LineTo { x, y });
            }
            for &[x, y] in right.iter().rev() {
                out.push(PathCommand::LineTo { x, y });
            }
            out.push(PathCommand::Close);
        }
    }
    out
}

fn closest_on_curve(point: Point, eval: impl Fn(f64) -> Point) -> (f64, Point, f64) {
    const SAMPLES: usize = 32;
    let mut best_t = 0.5;
    let mut best_pt = eval(0.5);
    let mut best_d2 = f64::INFINITY;
    for i in 0..=SAMPLES {
        let t = (i as f64) / (SAMPLES as f64);
        let p = eval(t);
        let d2 = (p[0] - point[0]).powi(2) + (p[1] - point[1]).powi(2);
        if d2 < best_d2 {
            best_d2 = d2;
            best_t = t;
            best_pt = p;
        }
    }
    let mut step = 0.5 / (SAMPLES as f64);
    for _ in 0..14 {
        for candidate in [
            (best_t - step).clamp(0.0, 1.0),
            (best_t + step).clamp(0.0, 1.0),
        ] {
            let p = eval(candidate);
            let d2 = (p[0] - point[0]).powi(2) + (p[1] - point[1]).powi(2);
            if d2 < best_d2 {
                best_d2 = d2;
                best_t = candidate;
                best_pt = p;
            }
        }
        step *= 0.5;
    }
    (best_t, best_pt, best_d2.sqrt())
}

/// Find the closest drawable segment of `commands` to `point` in local coordinates.
/// Returns `(command_index, t, closest_point, distance)`.
pub fn nearest_path_segment(
    commands: &[PathCommand],
    point: [f64; 2],
) -> Option<(usize, f64, [f64; 2], f64)> {
    let mut subpath_start: Option<Point> = None;
    let mut cursor: Option<Point> = None;
    let mut best: Option<(usize, f64, Point, f64)> = None;

    let mut consider = |idx: usize, t: f64, pt: Point, dist: f64| {
        if best.is_none_or(|(_, _, _, d)| dist < d) {
            best = Some((idx, t, pt, dist));
        }
    };

    for (idx, cmd) in commands.iter().enumerate() {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y } => {
                if let Some(p0) = cursor {
                    let (dx, dy) = (x - p0[0], y - p0[1]);
                    let len2 = dx * dx + dy * dy;
                    if len2 > 1e-12 {
                        let t = (((point[0] - p0[0]) * dx + (point[1] - p0[1]) * dy) / len2)
                            .clamp(0.0, 1.0);
                        let pt = [p0[0] + dx * t, p0[1] + dy * t];
                        let dist = (pt[0] - point[0]).hypot(pt[1] - point[1]);
                        consider(idx, t, pt, dist);
                    }
                }
                cursor = Some([x, y]);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(p0) = cursor {
                    let (t, pt, dist) =
                        closest_on_curve(point, |t| eval_quad(p0, [cx, cy], [x, y], t));
                    consider(idx, t, pt, dist);
                }
                cursor = Some([x, y]);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let Some(p0) = cursor {
                    let (t, pt, dist) = closest_on_curve(point, |t| {
                        eval_cubic(p0, [cx1, cy1], [cx2, cy2], [x, y], t)
                    });
                    consider(idx, t, pt, dist);
                }
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                if let (Some(p0), Some(start)) = (cursor, subpath_start) {
                    let (dx, dy) = (start[0] - p0[0], start[1] - p0[1]);
                    let len2 = dx * dx + dy * dy;
                    if len2 > 1e-12 {
                        let t = (((point[0] - p0[0]) * dx + (point[1] - p0[1]) * dy) / len2)
                            .clamp(0.0, 1.0);
                        let pt = [p0[0] + dx * t, p0[1] + dy * t];
                        let dist = (pt[0] - point[0]).hypot(pt[1] - point[1]);
                        consider(idx, t, pt, dist);
                    }
                }
                cursor = subpath_start;
            }
        }
    }
    best
}

/// Invert a 2D affine matrix `[a, b, c, d, tx, ty]`. Returns `None` if singular.
pub fn invert_affine_matrix(m: [f64; 6]) -> Option<[f64; 6]> {
    let [a, b, c, d, tx, ty] = m;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let ia = d * inv_det;
    let ib = -b * inv_det;
    let ic = -c * inv_det;
    let id = a * inv_det;
    let itx = -(ia * tx + ic * ty);
    let ity = -(ib * tx + id * ty);
    Some([ia, ib, ic, id, itx, ity])
}

/// Deform a local 2D point `p` using the pins of an `AssetWarpRig` (T-112).
/// `Fixed` pins anchor geometry near their `rest_pos` with zero displacement/rotation;
/// `Open` pins apply inverse-distance-weighted rigid translation and rotation around `rest_pos`.
pub fn deform_point_with_pins(p: [f64; 2], pins: &[crate::WarpPin]) -> [f64; 2] {
    if pins.is_empty() {
        return p;
    }
    let mut weight_sum = 0.0;
    let mut accum_x = 0.0;
    let mut accum_y = 0.0;
    for pin in pins {
        let target = match pin.kind {
            crate::WarpPinKind::Fixed => pin.rest_pos,
            crate::WarpPinKind::Open => pin.pos,
        };
        let rot_rad = match pin.kind {
            crate::WarpPinKind::Fixed => 0.0,
            crate::WarpPinKind::Open => pin.rotation.to_radians(),
        };
        let dx = p[0] - pin.rest_pos[0];
        let dy = p[1] - pin.rest_pos[1];
        let dist2 = dx * dx + dy * dy;
        if dist2 < 1e-10 {
            return target;
        }
        let power = 1.0 + pin.stiffness.clamp(0.0, 1.0);
        let w = 1.0 / (dist2.powf(power) + 1e-6);
        let (s, c) = rot_rad.sin_cos();
        let rx = c * dx - s * dy;
        let ry = s * dx + c * dy;
        accum_x += w * (target[0] + rx);
        accum_y += w * (target[1] + ry);
        weight_sum += w;
    }
    if weight_sum <= 1e-12 {
        p
    } else {
        [accum_x / weight_sum, accum_y / weight_sum]
    }
}

fn subdivide_commands_uniform(commands: &[PathCommand], subdivisions: u8) -> Vec<PathCommand> {
    let subs = subdivisions.clamp(1, 16) as usize;
    if subs <= 1 {
        return commands.to_vec();
    }
    let mut out = Vec::with_capacity(commands.len() * subs);
    let mut subpath_start = [0.0, 0.0];
    let mut cursor = [0.0, 0.0];
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = [x, y];
                cursor = [x, y];
                out.push(PathCommand::MoveTo { x, y });
            }
            PathCommand::LineTo { x, y } => {
                for i in 1..=subs {
                    let t = (i as f64) / (subs as f64);
                    let pt = lerp_pt(cursor, [x, y], t);
                    out.push(PathCommand::LineTo { x: pt[0], y: pt[1] });
                }
                cursor = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                out.push(PathCommand::QuadraticTo { cx, cy, x, y });
                cursor = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                out.push(PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                });
                cursor = [x, y];
            }
            PathCommand::Close => {
                if (cursor[0] - subpath_start[0]).hypot(cursor[1] - subpath_start[1]) > 1e-6 {
                    for i in 1..subs {
                        let t = (i as f64) / (subs as f64);
                        let pt = lerp_pt(cursor, subpath_start, t);
                        out.push(PathCommand::LineTo { x: pt[0], y: pt[1] });
                    }
                }
                out.push(PathCommand::Close);
                cursor = subpath_start;
            }
        }
    }
    out
}

/// Apply an `AssetWarpRig` to an `Element`, deforming vector contours (or fitting a raster/instance transform) (T-112).
pub fn apply_asset_warp_to_element(
    element: &crate::Element,
    rig: &crate::AssetWarpRig,
) -> crate::Element {
    if rig.pins.is_empty() {
        return element.clone();
    }
    let is_identity = rig.pins.iter().all(|pin| {
        pin.kind == crate::WarpPinKind::Fixed
            || ((pin.pos[0] - pin.rest_pos[0]).hypot(pin.pos[1] - pin.rest_pos[1]) <= 1e-9
                && pin.rotation.abs() <= 1e-9)
    });
    if is_identity {
        return element.clone();
    }
    let mut out = element.clone();
    if let Some((base_commands, fill_rule)) = element.to_path_commands() {
        let subdivided = subdivide_commands_uniform(&base_commands, rig.envelop_subdivisions);
        let deformed: Vec<PathCommand> = subdivided
            .into_iter()
            .map(|cmd| match cmd {
                PathCommand::MoveTo { x, y } => {
                    let [nx, ny] = deform_point_with_pins([x, y], &rig.pins);
                    PathCommand::MoveTo { x: nx, y: ny }
                }
                PathCommand::LineTo { x, y } => {
                    let [nx, ny] = deform_point_with_pins([x, y], &rig.pins);
                    PathCommand::LineTo { x: nx, y: ny }
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let [ncx, ncy] = deform_point_with_pins([cx, cy], &rig.pins);
                    let [nx, ny] = deform_point_with_pins([x, y], &rig.pins);
                    PathCommand::QuadraticTo {
                        cx: ncx,
                        cy: ncy,
                        x: nx,
                        y: ny,
                    }
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let [ncx1, ncy1] = deform_point_with_pins([cx1, cy1], &rig.pins);
                    let [ncx2, ncy2] = deform_point_with_pins([cx2, cy2], &rig.pins);
                    let [nx, ny] = deform_point_with_pins([x, y], &rig.pins);
                    PathCommand::CubicTo {
                        cx1: ncx1,
                        cy1: ncy1,
                        cx2: ncx2,
                        cy2: ncy2,
                        x: nx,
                        y: ny,
                    }
                }
                PathCommand::Close => PathCommand::Close,
            })
            .collect();
        out.transform.primitive = None;
        match &element.kind {
            crate::ElementKind::Rectangle {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | crate::ElementKind::Ellipse {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | crate::ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                out.kind = crate::ElementKind::Path {
                    commands: deformed,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    stroke_width: *stroke_width,
                };
            }
            crate::ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                stroke_width,
                ..
            } => {
                out.kind = crate::ElementKind::PaintedPath {
                    commands: deformed,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: fill_gradient.clone(),
                    stroke_gradient: stroke_gradient.clone(),
                    stroke_width: *stroke_width,
                };
            }
            _ => {}
        }
        return out;
    }
    // For Bitmap or Instance elements, apply average pin translation and rotation to the element transform.
    let n = rig.pins.len() as f64;
    let mut dx = 0.0;
    let mut dy = 0.0;
    let mut drot = 0.0;
    for pin in &rig.pins {
        if pin.kind == crate::WarpPinKind::Open {
            dx += pin.pos[0] - pin.rest_pos[0];
            dy += pin.pos[1] - pin.rest_pos[1];
            drot += pin.rotation;
        }
    }
    out.transform.x += dx / n;
    out.transform.y += dy / n;
    out.transform.rotation += drot / n;
    out
}

/// Solve a bounded Inverse Kinematics (`IkArmature`) chain deterministically via Cyclic Coordinate Descent (CCD) (T-113).
pub fn solve_ik_armature(elements: &mut [crate::Element], armature: &crate::IkArmature) {
    if armature.bones.is_empty() {
        return;
    }
    // Build chain from root bone down to effector_bone_id.
    let mut chain_indices = Vec::new();
    let mut current_id = Some(armature.effector_bone_id.as_str());
    let mut guard = 0usize;
    while let Some(bid) = current_id {
        if guard >= crate::IkBone::MAX_BONES {
            break;
        }
        let Some(idx) = armature.bones.iter().position(|b| b.id == bid) else {
            break;
        };
        if chain_indices.contains(&idx) {
            break;
        }
        chain_indices.push(idx);
        current_id = armature.bones[idx].parent_bone_id.as_deref();
        guard += 1;
    }
    chain_indices.reverse();
    if chain_indices.is_empty() {
        return;
    }

    // Extract current world positions and world angles of each bone in the chain.
    let n = chain_indices.len();
    let mut joint_pos = vec![[0.0, 0.0]; n + 1];
    let mut world_angles = vec![0.0_f64; n];
    let mut lengths = vec![0.0_f64; n];

    let root_el_id = &armature.bones[chain_indices[0]].element_id;
    let root_origin = elements
        .iter()
        .find(|e| &e.id == root_el_id)
        .map(|e| [e.transform.x, e.transform.y])
        .unwrap_or([0.0, 0.0]);
    joint_pos[0] = root_origin;

    for (k, &b_idx) in chain_indices.iter().enumerate() {
        let bone = &armature.bones[b_idx];
        let rot = elements
            .iter()
            .find(|e| e.id == bone.element_id)
            .map(|e| e.transform.rotation)
            .unwrap_or(0.0);
        world_angles[k] = rot;
        lengths[k] = bone.length.max(1.0);
        let rad = rot.to_radians();
        joint_pos[k + 1] = [
            joint_pos[k][0] + lengths[k] * rad.cos(),
            joint_pos[k][1] + lengths[k] * rad.sin(),
        ];
    }

    // CCD iterations (up to 24 passes).
    let target = armature.target_pos;
    for _iter in 0..24 {
        let eff = joint_pos[n];
        if (eff[0] - target[0]).hypot(eff[1] - target[1]) <= 1e-3 {
            break;
        }
        for k in (0..n).rev() {
            let bone = &armature.bones[chain_indices[k]];
            let pivot = joint_pos[k];
            let eff = joint_pos[n];
            let to_eff = [eff[0] - pivot[0], eff[1] - pivot[1]];
            let to_target = [target[0] - pivot[0], target[1] - pivot[1]];
            if to_eff[0].hypot(to_eff[1]) <= 1e-6 || to_target[0].hypot(to_target[1]) <= 1e-6 {
                continue;
            }
            let ang_eff = to_eff[1].atan2(to_eff[0]).to_degrees();
            let ang_target = to_target[1].atan2(to_target[0]).to_degrees();
            let mut delta = (ang_target - ang_eff + 540.0).rem_euclid(360.0) - 180.0;
            let weight = (1.0 - bone.stiffness.clamp(0.0, 0.95)).max(0.05);
            delta *= weight;

            let parent_angle = if k > 0 { world_angles[k - 1] } else { 0.0 };
            let mut rel_angle = world_angles[k] + delta - parent_angle;
            if let Some(min_a) = bone.min_angle {
                rel_angle = rel_angle.max(min_a);
            }
            if let Some(max_a) = bone.max_angle {
                rel_angle = rel_angle.min(max_a);
            }
            let applied = (parent_angle + rel_angle) - world_angles[k];
            for j in k..n {
                world_angles[j] += applied;
                let rad = world_angles[j].to_radians();
                joint_pos[j + 1] = [
                    joint_pos[j][0] + lengths[j] * rad.cos(),
                    joint_pos[j][1] + lengths[j] * rad.sin(),
                ];
            }
        }
    }

    // Write solved positions and rotations back to bone elements.
    for (k, &b_idx) in chain_indices.iter().enumerate() {
        let bone = &armature.bones[b_idx];
        if let Some(el) = elements.iter_mut().find(|e| e.id == bone.element_id) {
            el.transform.x = joint_pos[k][0];
            el.transform.y = joint_pos[k][1];
            el.transform.rotation = world_angles[k];
        }
    }
}

/// Apply one non-destructive `VectorModifier` to a path contour (T-114).
pub fn apply_vector_modifier(
    commands: &[PathCommand],
    modifier: &crate::VectorModifier,
) -> Vec<PathCommand> {
    apply_vector_modifier_checked(commands, modifier).unwrap_or_else(|| commands.to_vec())
}

/// Apply one modifier without ever materializing a result above the shared path-command budget.
/// `None` means the modifier would exceed the budget; callers can then keep the unmodified path.
fn apply_vector_modifier_checked(
    commands: &[PathCommand],
    modifier: &crate::VectorModifier,
) -> Option<Vec<PathCommand>> {
    if commands.len() > MAX_PATH_COMMANDS {
        return None;
    }
    // Mirror and Repeater have exact multiplicative sizes. Check these before either branch
    // clones a command so a chained appearance cannot briefly allocate an enormous intermediate.
    let exact_len = match modifier {
        crate::VectorModifier::Mirror { axis_x, axis_y } => commands
            .len()
            .checked_mul(if axis_x.is_some() { 2 } else { 1 })?
            .checked_mul(if axis_y.is_some() { 2 } else { 1 })?,
        crate::VectorModifier::Repeater { copies, .. } => commands
            .len()
            .checked_mul((*copies).clamp(1, 64) as usize)?,
        crate::VectorModifier::OffsetContour { .. } | crate::VectorModifier::LiveCorners { .. } => {
            commands.len()
        }
    };
    if exact_len > MAX_PATH_COMMANDS {
        return None;
    }

    let output = match modifier {
        crate::VectorModifier::OffsetContour { delta } => {
            if delta.abs() <= 1e-6 || !delta.is_finite() {
                return Some(commands.to_vec());
            }
            let Ok(contours) = flatten_path(commands, FlattenOptions::default()) else {
                return Some(commands.to_vec());
            };
            let mut out = Vec::new();
            for contour in contours {
                let n = contour.points.len();
                if n < 2 {
                    continue;
                }
                let mut cx = 0.0;
                let mut cy = 0.0;
                for p in &contour.points {
                    cx += p[0];
                    cy += p[1];
                }
                cx /= n as f64;
                cy /= n as f64;
                for (i, p) in contour.points.iter().enumerate() {
                    let dx = p[0] - cx;
                    let dy = p[1] - cy;
                    let len = dx.hypot(dy).max(1e-6);
                    let nx = p[0] + (dx / len) * delta;
                    let ny = p[1] + (dy / len) * delta;
                    if i == 0 {
                        out.push(PathCommand::MoveTo { x: nx, y: ny });
                    } else {
                        out.push(PathCommand::LineTo { x: nx, y: ny });
                    }
                }
                if contour.closed {
                    out.push(PathCommand::Close);
                }
            }
            out
        }
        crate::VectorModifier::LiveCorners { radius } => {
            let r = radius.clamp(0.0, 1e6);
            if r <= 1e-6 {
                return Some(commands.to_vec());
            }
            let Ok(contours) = flatten_path(commands, FlattenOptions::default()) else {
                return Some(commands.to_vec());
            };
            let mut out = Vec::new();
            for contour in contours {
                let mut pts = contour.points;
                if pts.len() >= 2 && pts.first() == pts.last() {
                    pts.pop();
                }
                let n = pts.len();
                if n < 3 {
                    continue;
                }
                for i in 0..n {
                    if !contour.closed && (i == 0 || i + 1 == n) {
                        if i == 0 {
                            out.push(PathCommand::MoveTo {
                                x: pts[i][0],
                                y: pts[i][1],
                            });
                        } else {
                            out.push(PathCommand::LineTo {
                                x: pts[i][0],
                                y: pts[i][1],
                            });
                        }
                        continue;
                    }
                    let prev = pts[(i + n - 1) % n];
                    let curr = pts[i];
                    let next = pts[(i + 1) % n];
                    let v1 = [prev[0] - curr[0], prev[1] - curr[1]];
                    let v2 = [next[0] - curr[0], next[1] - curr[1]];
                    let l1 = v1[0].hypot(v1[1]);
                    let l2 = v2[0].hypot(v2[1]);
                    let d = r.min(l1 * 0.45).min(l2 * 0.45);
                    if d <= 1e-4 {
                        if i == 0 {
                            out.push(PathCommand::MoveTo {
                                x: curr[0],
                                y: curr[1],
                            });
                        } else {
                            out.push(PathCommand::LineTo {
                                x: curr[0],
                                y: curr[1],
                            });
                        }
                    } else {
                        let p_enter = [curr[0] + (v1[0] / l1) * d, curr[1] + (v1[1] / l1) * d];
                        let p_exit = [curr[0] + (v2[0] / l2) * d, curr[1] + (v2[1] / l2) * d];
                        if i == 0 {
                            out.push(PathCommand::MoveTo {
                                x: p_enter[0],
                                y: p_enter[1],
                            });
                        } else {
                            out.push(PathCommand::LineTo {
                                x: p_enter[0],
                                y: p_enter[1],
                            });
                        }
                        out.push(PathCommand::QuadraticTo {
                            cx: curr[0],
                            cy: curr[1],
                            x: p_exit[0],
                            y: p_exit[1],
                        });
                    }
                }
                if contour.closed {
                    out.push(PathCommand::Close);
                }
            }
            out
        }
        crate::VectorModifier::Mirror { axis_x, axis_y } => {
            let mut out = commands.to_vec();
            if let Some(ax) = *axis_x {
                let mirrored: Vec<PathCommand> = out
                    .iter()
                    .map(|cmd| match *cmd {
                        PathCommand::MoveTo { x, y } => PathCommand::MoveTo { x: 2.0 * ax - x, y },
                        PathCommand::LineTo { x, y } => PathCommand::LineTo { x: 2.0 * ax - x, y },
                        PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                            cx: 2.0 * ax - cx,
                            cy,
                            x: 2.0 * ax - x,
                            y,
                        },
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => PathCommand::CubicTo {
                            cx1: 2.0 * ax - cx1,
                            cy1,
                            cx2: 2.0 * ax - cx2,
                            cy2,
                            x: 2.0 * ax - x,
                            y,
                        },
                        PathCommand::Close => PathCommand::Close,
                    })
                    .collect();
                out.extend(mirrored);
            }
            if let Some(ay) = *axis_y {
                let mirrored: Vec<PathCommand> = out
                    .iter()
                    .map(|cmd| match *cmd {
                        PathCommand::MoveTo { x, y } => PathCommand::MoveTo { x, y: 2.0 * ay - y },
                        PathCommand::LineTo { x, y } => PathCommand::LineTo { x, y: 2.0 * ay - y },
                        PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                            cx,
                            cy: 2.0 * ay - cy,
                            x,
                            y: 2.0 * ay - y,
                        },
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => PathCommand::CubicTo {
                            cx1,
                            cy1: 2.0 * ay - cy1,
                            cx2,
                            cy2: 2.0 * ay - cy2,
                            x,
                            y: 2.0 * ay - y,
                        },
                        PathCommand::Close => PathCommand::Close,
                    })
                    .collect();
                out.extend(mirrored);
            }
            out
        }
        crate::VectorModifier::Repeater {
            copies,
            offset_x,
            offset_y,
            scale_step,
            rotation_step,
        } => {
            let count = (*copies).clamp(1, 64);
            let mut out = Vec::with_capacity(commands.len() * (count as usize));
            for i in 0..count {
                let fi = i as f64;
                let scale = scale_step.clamp(0.05, 10.0).powf(fi);
                let (s, c) = (rotation_step * fi).to_radians().sin_cos();
                let tx = offset_x * fi;
                let ty = offset_y * fi;
                let map_pt = |x: f64, y: f64| -> [f64; 2] {
                    let sx = x * scale;
                    let sy = y * scale;
                    [c * sx - s * sy + tx, s * sx + c * sy + ty]
                };
                for cmd in commands {
                    out.push(match *cmd {
                        PathCommand::MoveTo { x, y } => {
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::MoveTo { x: nx, y: ny }
                        }
                        PathCommand::LineTo { x, y } => {
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::LineTo { x: nx, y: ny }
                        }
                        PathCommand::QuadraticTo { cx, cy, x, y } => {
                            let [ncx, ncy] = map_pt(cx, cy);
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::QuadraticTo {
                                cx: ncx,
                                cy: ncy,
                                x: nx,
                                y: ny,
                            }
                        }
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => {
                            let [ncx1, ncy1] = map_pt(cx1, cy1);
                            let [ncx2, ncy2] = map_pt(cx2, cy2);
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::CubicTo {
                                cx1: ncx1,
                                cy1: ncy1,
                                cx2: ncx2,
                                cy2: ncy2,
                                x: nx,
                                y: ny,
                            }
                        }
                        PathCommand::Close => PathCommand::Close,
                    });
                }
            }
            out
        }
    };
    (output.len() <= MAX_PATH_COMMANDS).then_some(output)
}

fn apply_appearance_modifiers_checked(
    mut commands: Vec<PathCommand>,
    modifiers: &[crate::VectorModifier],
) -> Result<Vec<PathCommand>, usize> {
    for (index, modifier) in modifiers.iter().enumerate() {
        commands = apply_vector_modifier_checked(&commands, modifier).ok_or(index)?;
    }
    Ok(commands)
}

/// Validate the cumulative evaluated path size of one appearance, returning the first modifier
/// whose output would exceed [`MAX_PATH_COMMANDS`]. Orphan/non-path metadata is handled elsewhere.
pub(crate) fn validate_element_appearance_complexity(
    element: &crate::Element,
    appearance: &crate::ElementAppearance,
) -> Result<(), usize> {
    let Some((commands, _)) = element.to_path_commands() else {
        return Ok(());
    };
    apply_appearance_modifiers_checked(commands, &appearance.modifiers).map(|_| ())
}

/// Expand a `PatternBrushKind` stroke along `commands` into concrete path geometry (T-114).
pub fn expand_pattern_brush_stroke(
    commands: &[PathCommand],
    width: f64,
    brush: &crate::PatternBrushKind,
) -> Vec<PathCommand> {
    let Ok(contours) = flatten_path(commands, FlattenOptions::default()) else {
        return commands.to_vec();
    };
    let mut out = Vec::new();
    let hw = (width * 0.5).max(0.5);
    for contour in contours {
        if contour.points.len() < 2 {
            continue;
        }
        let path = crate::evaluate::motion_path_polyline(&{
            let mut cmds = Vec::with_capacity(contour.points.len());
            for (i, &[x, y]) in contour.points.iter().enumerate() {
                if i == 0 {
                    cmds.push(PathCommand::MoveTo { x, y });
                } else {
                    cmds.push(PathCommand::LineTo { x, y });
                }
            }
            cmds
        });
        let Some(guide) = path else {
            continue;
        };
        let total = guide.length();
        if total <= 1e-6 {
            continue;
        }
        match brush {
            crate::PatternBrushKind::DashPattern { dash, gap } => {
                let d = dash.max(1.0);
                let g = gap.max(0.5);
                let mut s = 0.0;
                while s < total {
                    let s_end = (s + d).min(total);
                    let p0 = guide.point_at(s);
                    let p1 = guide.point_at(s_end);
                    let ang = guide.tangent_at(s).to_radians();
                    let (sin, cos) = ang.sin_cos();
                    let nx = -sin * hw;
                    let ny = cos * hw;
                    out.push(PathCommand::MoveTo {
                        x: p0[0] + nx,
                        y: p0[1] + ny,
                    });
                    out.push(PathCommand::LineTo {
                        x: p1[0] + nx,
                        y: p1[1] + ny,
                    });
                    out.push(PathCommand::LineTo {
                        x: p1[0] - nx,
                        y: p1[1] - ny,
                    });
                    out.push(PathCommand::LineTo {
                        x: p0[0] - nx,
                        y: p0[1] - ny,
                    });
                    out.push(PathCommand::Close);
                    s += d + g;
                }
            }
            crate::PatternBrushKind::ArtStretch {
                stamp_commands,
                nominal_length,
            } => {
                let nom = nominal_length.max(1.0);
                let map_pt = |x: f64, y: f64| -> [f64; 2] {
                    let s = (x / nom).clamp(0.0, 1.0) * total;
                    let base = guide.point_at(s);
                    let ang = guide.tangent_at(s).to_radians();
                    let (sin, cos) = ang.sin_cos();
                    [base[0] - sin * y, base[1] + cos * y]
                };
                for cmd in stamp_commands {
                    out.push(match *cmd {
                        PathCommand::MoveTo { x, y } => {
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::MoveTo { x: nx, y: ny }
                        }
                        PathCommand::LineTo { x, y } => {
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::LineTo { x: nx, y: ny }
                        }
                        PathCommand::QuadraticTo { cx, cy, x, y } => {
                            let [ncx, ncy] = map_pt(cx, cy);
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::QuadraticTo {
                                cx: ncx,
                                cy: ncy,
                                x: nx,
                                y: ny,
                            }
                        }
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => {
                            let [ncx1, ncy1] = map_pt(cx1, cy1);
                            let [ncx2, ncy2] = map_pt(cx2, cy2);
                            let [nx, ny] = map_pt(x, y);
                            PathCommand::CubicTo {
                                cx1: ncx1,
                                cy1: ncy1,
                                cx2: ncx2,
                                cy2: ncy2,
                                x: nx,
                                y: ny,
                            }
                        }
                        PathCommand::Close => PathCommand::Close,
                    });
                }
            }
            crate::PatternBrushKind::PatternRepeat {
                tile_commands,
                spacing,
            } => {
                let step = spacing.max(2.0);
                let mut s = 0.0;
                let mut count = 0usize;
                while s < total && count < 256 {
                    let base = guide.point_at(s);
                    let ang = guide.tangent_at(s).to_radians();
                    let (sin, cos) = ang.sin_cos();
                    let map_pt = |x: f64, y: f64| -> [f64; 2] {
                        [base[0] + cos * x - sin * y, base[1] + sin * x + cos * y]
                    };
                    for cmd in tile_commands {
                        out.push(match *cmd {
                            PathCommand::MoveTo { x, y } => {
                                let [nx, ny] = map_pt(x, y);
                                PathCommand::MoveTo { x: nx, y: ny }
                            }
                            PathCommand::LineTo { x, y } => {
                                let [nx, ny] = map_pt(x, y);
                                PathCommand::LineTo { x: nx, y: ny }
                            }
                            PathCommand::QuadraticTo { cx, cy, x, y } => {
                                let [ncx, ncy] = map_pt(cx, cy);
                                let [nx, ny] = map_pt(x, y);
                                PathCommand::QuadraticTo {
                                    cx: ncx,
                                    cy: ncy,
                                    x: nx,
                                    y: ny,
                                }
                            }
                            PathCommand::CubicTo {
                                cx1,
                                cy1,
                                cx2,
                                cy2,
                                x,
                                y,
                            } => {
                                let [ncx1, ncy1] = map_pt(cx1, cy1);
                                let [ncx2, ncy2] = map_pt(cx2, cy2);
                                let [nx, ny] = map_pt(x, y);
                                PathCommand::CubicTo {
                                    cx1: ncx1,
                                    cy1: ncy1,
                                    cx2: ncx2,
                                    cy2: ncy2,
                                    x: nx,
                                    y: ny,
                                }
                            }
                            PathCommand::Close => PathCommand::Close,
                        });
                    }
                    s += step;
                    count += 1;
                }
            }
        }
    }
    out
}

/// Evaluate an `ElementAppearance` (modifiers, multiple fills/strokes, pattern brushes, or text bindings) into concrete `Element`s (T-114).
pub fn evaluate_element_appearance(
    element: &crate::Element,
    appearance: &crate::ElementAppearance,
) -> Vec<crate::Element> {
    // Handle Text on Path or Area Text bindings first if present on a Text/TextBox element.
    if let Some(binding) = &appearance.text_binding {
        match (&element.kind, binding) {
            (
                crate::ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                },
                crate::TextPathBinding::PathText {
                    path_commands,
                    start_offset,
                },
            ) => {
                if let Some(guide) = crate::evaluate::motion_path_polyline(path_commands) {
                    let advance = (font_size * 0.6).max(2.0);
                    let mut out = Vec::new();
                    for (idx, ch) in text.chars().enumerate() {
                        if ch.is_whitespace() {
                            continue;
                        }
                        let s = (start_offset + (idx as f64) * advance).min(guide.length());
                        let [px, py] = guide.point_at(s);
                        let rot = guide.tangent_at(s);
                        let mut glyph_el = element.clone();
                        glyph_el.id = format!("{}-glyph-{idx}", element.id);
                        glyph_el.transform.x = element.transform.x + px;
                        glyph_el.transform.y = element.transform.y + py;
                        glyph_el.transform.rotation = element.transform.rotation + rot;
                        glyph_el.kind = crate::ElementKind::Text {
                            text: ch.to_string(),
                            font_family: font_family.clone(),
                            font_size: *font_size,
                            color: *color,
                        };
                        out.push(glyph_el);
                    }
                    if !out.is_empty() {
                        return out;
                    }
                }
            }
            (
                crate::ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                },
                crate::TextPathBinding::AreaText {
                    bounds,
                    columns,
                    gutter,
                },
            ) => {
                let cols = (*columns).clamp(1, 8) as usize;
                let total_w = bounds[2].max(20.0);
                let col_w = ((total_w - (*gutter) * ((cols.saturating_sub(1)) as f64))
                    / (cols as f64))
                    .max(10.0);
                let max_chars_per_line = ((col_w / (font_size * 0.55)).floor() as usize).max(1);
                let line_h = (font_size * 1.25).max(4.0);
                let max_lines_per_col = ((bounds[3].max(line_h) / line_h).floor() as usize).max(1);
                let mut lines: Vec<String> = Vec::new();
                let mut current = String::new();
                for word in text.split_whitespace() {
                    if current.is_empty() {
                        current.push_str(word);
                    } else if current.chars().count() + 1 + word.chars().count()
                        <= max_chars_per_line
                    {
                        current.push(' ');
                        current.push_str(word);
                    } else {
                        lines.push(std::mem::take(&mut current));
                        current.push_str(word);
                    }
                }
                if !current.is_empty() {
                    lines.push(current);
                }
                let mut out = Vec::new();
                for (line_idx, line) in lines.into_iter().enumerate() {
                    let col = (line_idx / max_lines_per_col).min(cols - 1);
                    let row = line_idx % max_lines_per_col;
                    let mut line_el = element.clone();
                    line_el.id = format!("{}-line-{line_idx}", element.id);
                    line_el.transform.x =
                        element.transform.x + bounds[0] + (col as f64) * (col_w + *gutter);
                    line_el.transform.y = element.transform.y + bounds[1] + (row as f64) * line_h;
                    line_el.kind = crate::ElementKind::Text {
                        text: line,
                        font_family: font_family.clone(),
                        font_size: *font_size,
                        color: *color,
                    };
                    out.push(line_el);
                }
                if !out.is_empty() {
                    return out;
                }
            }
            _ => {}
        }
    }

    let Some((commands, fill_rule)) = element.to_path_commands() else {
        return vec![element.clone()];
    };
    // Reject the entire synthetic expansion if any modifier would cross the path budget. Keeping
    // the authored element is deterministic and avoids exposing a silently partial modifier stack.
    let Ok(commands) = apply_appearance_modifiers_checked(commands, &appearance.modifiers) else {
        return vec![element.clone()];
    };

    if appearance.entries.is_empty() {
        let mut modified = element.clone();
        modified.transform.primitive = None;
        match &element.kind {
            crate::ElementKind::Rectangle {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | crate::ElementKind::Ellipse {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | crate::ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                modified.kind = crate::ElementKind::Path {
                    commands,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    stroke_width: *stroke_width,
                };
            }
            crate::ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                stroke_width,
                ..
            } => {
                modified.kind = crate::ElementKind::PaintedPath {
                    commands,
                    fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: fill_gradient.clone(),
                    stroke_gradient: stroke_gradient.clone(),
                    stroke_width: *stroke_width,
                };
            }
            _ => {}
        }
        return vec![modified];
    }

    let mut out = Vec::with_capacity(appearance.entries.len());
    for (idx, entry) in appearance.entries.iter().enumerate() {
        let mut item = element.clone();
        item.id = if idx == 0 {
            element.id.clone()
        } else {
            format!("{}-app-{idx}", element.id)
        };
        item.transform.primitive = None;
        match entry {
            crate::AppearancePaintEntry::Fill {
                color,
                gradient,
                offset,
                opacity,
                ..
            } => {
                item.transform.x += offset[0];
                item.transform.y += offset[1];
                item.transform.alpha = (item.transform.alpha * opacity).clamp(0.0, 1.0);
                if gradient.is_some() {
                    item.kind = crate::ElementKind::PaintedPath {
                        commands: commands.clone(),
                        fill_rule,
                        fill: *color,
                        stroke: [0, 0, 0, 0],
                        fill_gradient: gradient.clone(),
                        stroke_gradient: None,
                        stroke_width: 0.0,
                    };
                } else {
                    item.kind = crate::ElementKind::Path {
                        commands: commands.clone(),
                        fill_rule,
                        fill: *color,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    };
                }
            }
            crate::AppearancePaintEntry::Stroke {
                color,
                gradient,
                width,
                offset_distance,
                pattern_brush,
                opacity,
                ..
            } => {
                item.transform.alpha = (item.transform.alpha * opacity).clamp(0.0, 1.0);
                let stroke_cmds = if offset_distance.abs() > 1e-6 {
                    apply_vector_modifier(
                        &commands,
                        &crate::VectorModifier::OffsetContour {
                            delta: *offset_distance,
                        },
                    )
                } else {
                    commands.clone()
                };
                if let Some(brush) = pattern_brush {
                    let expanded = expand_pattern_brush_stroke(&stroke_cmds, *width, brush);
                    item.kind = crate::ElementKind::Path {
                        commands: expanded,
                        fill_rule: crate::FillRule::NonZero,
                        fill: *color,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    };
                } else if gradient.is_some() {
                    item.kind = crate::ElementKind::PaintedPath {
                        commands: stroke_cmds,
                        fill_rule,
                        fill: [0, 0, 0, 0],
                        stroke: *color,
                        fill_gradient: None,
                        stroke_gradient: gradient.clone(),
                        stroke_width: *width,
                    };
                } else {
                    item.kind = crate::ElementKind::Path {
                        commands: stroke_cmds,
                        fill_rule,
                        fill: [0, 0, 0, 0],
                        stroke: *color,
                        stroke_width: *width,
                    };
                }
            }
        }
        out.push(item);
    }
    out
}

#[cfg(test)]
mod appearance_complexity_tests {
    use super::*;
    use crate::{Element, ElementAppearance, ElementKind, Project, VectorModifier};

    fn rectangle() -> Element {
        Element {
            id: "shape".into(),
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }
    }

    fn repeater(copies: u32) -> VectorModifier {
        VectorModifier::Repeater {
            copies,
            offset_x: 1.0,
            offset_y: 0.0,
            scale_step: 1.0,
            rotation_step: 0.0,
        }
    }

    #[test]
    fn appearance_repeaters_are_bounded_without_changing_ordinary_stacks() {
        let element = rectangle();
        let ordinary = ElementAppearance {
            element_id: element.id.clone(),
            modifiers: vec![repeater(2)],
            ..ElementAppearance::default()
        };
        let ordinary_result = evaluate_element_appearance(&element, &ordinary);
        let ElementKind::Path { commands, .. } = &ordinary_result[0].kind else {
            panic!("ordinary modifier should still evaluate to a path");
        };
        assert_eq!(commands.len(), 10);

        let oversized = ElementAppearance {
            element_id: element.id.clone(),
            modifiers: vec![repeater(64), repeater(64), repeater(64)],
            ..ElementAppearance::default()
        };
        assert_eq!(
            evaluate_element_appearance(&element, &oversized),
            vec![element.clone()],
            "an over-budget stack must keep the authored element, not a partial expansion"
        );

        let mut project = Project::blank();
        project.format_version = 9;
        project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![element];
        project.symbols[0].timeline.layers[0].keyframes[0].appearances = vec![oversized];
        let issues = crate::validate(&project);
        assert!(issues.iter().any(|issue| {
            issue.path == "symbols[0].timeline.layers[0].keyframes[0].appearances[0].modifiers[2]"
                && issue.message.contains("100000 path-command budget")
        }));
    }
}
