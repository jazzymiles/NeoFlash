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
