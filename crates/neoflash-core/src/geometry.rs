//! Renderer-independent local-space path geometry. No renderer handles or caches
//! are serialized. Helpers return a limit error instead of truncated geometry.
use crate::{FillRule, PathCommand};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_PATH_COMMANDS: usize = 100_000;
const MAX_FLATTENED_POINTS: usize = 1_000_000;
const MAX_COORDINATE: f64 = 1e9;
type Point = [f64; 2];

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
