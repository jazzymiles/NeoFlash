//! Pure geometry for authoring gestures. Input paths are never silently truncated.
//! Gap thresholds and eraser sweeps are measured after the supplied affine transform.
use crate::{
    FlattenOptions, GeometryError, MAX_PATH_COMMANDS, PathCommand, flatten_path,
    validate_path_commands,
};

type Point = [f64; 2];
const MAX_GESTURE_POINTS: usize = 4096;
const MAX_INTERSECTION_CHECKS: usize = 8_000_000;
const MAX_OUTPUT_POINTS: usize = 65_536;

#[derive(Clone, Debug, PartialEq)]
pub struct GapCloseResult {
    pub commands: Vec<PathCommand>,
    pub closed_contours: usize,
    pub remaining_open_contours: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeEraseResult {
    pub commands: Vec<PathCommand>,
    pub changed: bool,
    /// Only curves on a contour touched by the eraser are flattened.
    pub approximated_curves: bool,
}

fn invalid(message: &str) -> GeometryError {
    GeometryError::InvalidPath(message.into())
}

fn check_matrix(matrix: [f64; 6]) -> Result<(), GeometryError> {
    if matrix.iter().any(|n| !n.is_finite()) {
        return Err(invalid("authoring transform must be finite"));
    }
    Ok(())
}

fn map(matrix: [f64; 6], point: Point) -> Point {
    [
        matrix[0] * point[0] + matrix[2] * point[1] + matrix[4],
        matrix[1] * point[0] + matrix[3] * point[1] + matrix[5],
    ]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn contour_ranges(commands: &[PathCommand]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, command) in commands.iter().enumerate() {
        if index > start && matches!(command, PathCommand::MoveTo { .. }) {
            ranges.push(start..index);
            start = index;
        }
        if matches!(command, PathCommand::Close) {
            ranges.push(start..index + 1);
            start = index + 1;
        }
    }
    if start < commands.len() {
        ranges.push(start..commands.len());
    }
    ranges
}

/// Close each eligible open contour independently, leaving its authored curves intact.
/// `local_to_metric` may include the view zoom to express `max_gap` in screen points.
/// An existing closed contour, or a lone MoveTo, is never changed.
pub fn close_path_gaps(
    commands: &[PathCommand],
    local_to_metric: [f64; 6],
    max_gap: f64,
) -> Result<GapCloseResult, GeometryError> {
    validate_path_commands(commands)?;
    check_matrix(local_to_metric)?;
    if !max_gap.is_finite() || max_gap < 0. {
        return Err(invalid("gap tolerance must be finite and nonnegative"));
    }
    let mut result = GapCloseResult {
        commands: Vec::with_capacity(commands.len()),
        closed_contours: 0,
        remaining_open_contours: 0,
    };
    for range in contour_ranges(commands) {
        let contour = &commands[range];
        result.commands.extend_from_slice(contour);
        if matches!(contour.last(), Some(PathCommand::Close)) {
            continue;
        }
        let PathCommand::MoveTo { x, y } = contour[0] else {
            unreachable!()
        };
        let start = map(local_to_metric, [x, y]);
        let end = match contour.last().unwrap() {
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => Some(map(local_to_metric, [*x, *y])),
            _ => None,
        };
        if start
            .iter()
            .chain(end.iter().flatten())
            .any(|n| !n.is_finite())
        {
            return Err(invalid("transformed gap endpoints must be finite"));
        }
        if end.is_some_and(|end| (end[0] - start[0]).hypot(end[1] - start[1]) <= max_gap) {
            result.commands.push(PathCommand::Close);
            result.closed_contours += 1;
        } else {
            result.remaining_open_contours += 1;
        }
    }
    validate_path_commands(&result.commands)?;
    Ok(result)
}

fn inverse(matrix: [f64; 6]) -> Result<[f64; 6], GeometryError> {
    check_matrix(matrix)?;
    let determinant = matrix[0] * matrix[3] - matrix[1] * matrix[2];
    if !determinant.is_finite() || determinant == 0. {
        return Err(invalid("stroke erasing requires an invertible transform"));
    }
    let a = matrix[3] / determinant;
    let b = -matrix[1] / determinant;
    let c = -matrix[2] / determinant;
    let d = matrix[0] / determinant;
    let inverted = [
        a,
        b,
        c,
        d,
        -(a * matrix[4] + c * matrix[5]),
        -(b * matrix[4] + d * matrix[5]),
    ];
    check_matrix(inverted)?;
    Ok(inverted)
}

fn transform_commands(commands: &[PathCommand], matrix: [f64; 6]) -> Vec<PathCommand> {
    commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { x, y } => {
                let [x, y] = map(matrix, [x, y]);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let [x, y] = map(matrix, [x, y]);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [cx, cy] = map(matrix, [cx, cy]);
                let [x, y] = map(matrix, [x, y]);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [cx1, cy1] = map(matrix, [cx1, cy1]);
                let [cx2, cy2] = map(matrix, [cx2, cy2]);
                let [x, y] = map(matrix, [x, y]);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn interval(from: f64, to: f64) -> Option<(f64, f64)> {
    let (from, to) = (from.max(0.), to.min(1.));
    (from < to).then_some((from, to))
}

fn disk_interval(a: Point, b: Point, center: Point, radius: f64) -> Option<(f64, f64)> {
    let direction = sub(b, a);
    let length_squared = dot(direction, direction);
    if length_squared == 0. {
        return None;
    }
    let delta = sub(center, a);
    let perpendicular_squared = cross(delta, direction).powi(2) / length_squared;
    let remaining = radius * radius - perpendicular_squared;
    if remaining <= 0. {
        return None;
    }
    let center_t = dot(delta, direction) / length_squared;
    let half = (remaining / length_squared).sqrt();
    interval(center_t - half, center_t + half)
}

fn constrain_linear(value: f64, delta: f64, min: f64, max: f64, range: &mut (f64, f64)) -> bool {
    if delta == 0. {
        return value >= min && value <= max;
    }
    let (a, b) = ((min - value) / delta, (max - value) / delta);
    range.0 = range.0.max(a.min(b));
    range.1 = range.1.min(a.max(b));
    range.0 < range.1
}

/// Intersection of a line segment and the rectangular part of a round capsule.
fn strip_interval(a: Point, b: Point, from: Point, to: Point, radius: f64) -> Option<(f64, f64)> {
    let sweep = sub(to, from);
    let length = sweep[0].hypot(sweep[1]);
    if length == 0. {
        return None;
    }
    let along = [sweep[0] / length, sweep[1] / length];
    let delta = sub(b, a);
    let start = sub(a, from);
    let mut range = (0., 1.);
    (constrain_linear(dot(start, along), dot(delta, along), 0., length, &mut range)
        && constrain_linear(
            cross(start, along),
            cross(delta, along),
            -radius,
            radius,
            &mut range,
        ))
    .then_some(range)
}

fn overlaps_bounds(a: Point, b: Point, from: Point, to: Point, radius: f64) -> bool {
    (0..2).all(|axis| {
        a[axis].min(b[axis]) <= from[axis].max(to[axis]) + radius
            && a[axis].max(b[axis]) >= from[axis].min(to[axis]) - radius
    })
}

fn append_run(runs: &mut Vec<Vec<Point>>, current: &mut Vec<Point>) {
    if current.len() > 1 {
        runs.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn push_output_point(
    current: &mut Vec<Point>,
    point: Point,
    total: &mut usize,
) -> Result<(), GeometryError> {
    *total += 1;
    if *total > MAX_PATH_COMMANDS {
        return Err(GeometryError::ComplexityLimit);
    }
    current.push(point);
    Ok(())
}

/// Remove the stroke centreline inside the union of the gesture's round capsules.
/// Radius and flattening tolerance are world-space units. Untouched contours retain
/// their exact original commands; only touched curves become bounded polylines.
/// Paint, stroke width and fill geometry belong to the caller and are not changed.
pub fn erase_path_stroke(
    commands: &[PathCommand],
    local_to_world: [f64; 6],
    eraser_points: &[Point],
    radius: f64,
    tolerance: f64,
) -> Result<StrokeEraseResult, GeometryError> {
    validate_path_commands(commands)?;
    let world_to_local = inverse(local_to_world)?;
    if !radius.is_finite() || radius <= 0. || !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid(
            "eraser radius and tolerance must be positive and finite",
        ));
    }
    if eraser_points.len() > MAX_GESTURE_POINTS {
        return Err(GeometryError::ComplexityLimit);
    }
    if eraser_points
        .iter()
        .flatten()
        .any(|n| !n.is_finite() || n.abs() > 1e9)
    {
        return Err(invalid(
            "eraser points must be finite and within coordinate limits",
        ));
    }
    let unchanged = || StrokeEraseResult {
        commands: commands.to_vec(),
        changed: false,
        approximated_curves: false,
    };
    if eraser_points.is_empty() || commands.is_empty() {
        return Ok(unchanged());
    }
    let world_commands = transform_commands(commands, local_to_world);
    let contours = flatten_path(
        &world_commands,
        FlattenOptions {
            tolerance,
            max_depth: 20,
            max_points: MAX_OUTPUT_POINTS,
        },
    )?;
    let sweeps: Vec<_> = if eraser_points.len() == 1 {
        vec![(eraser_points[0], eraser_points[0])]
    } else {
        eraser_points
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect()
    };
    let mut result = StrokeEraseResult {
        commands: Vec::new(),
        changed: false,
        approximated_curves: false,
    };
    let mut checks = 0usize;
    let mut output_points = 0usize;
    for (contour, range) in contours.iter().zip(contour_ranges(commands)) {
        let original = &commands[range];
        let mut runs = Vec::new();
        let mut current = Vec::new();
        let mut touched = false;
        let mut points = contour.points.clone();
        if contour.closed && points.len() > 1 {
            points.push(points[0]);
        }
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a == b {
                continue;
            }
            let mut cuts = Vec::new();
            for &(from, to) in &sweeps {
                checks += 1;
                if checks > MAX_INTERSECTION_CHECKS {
                    return Err(GeometryError::ComplexityLimit);
                }
                if !overlaps_bounds(a, b, from, to, radius) {
                    continue;
                }
                cuts.extend(disk_interval(a, b, from, radius));
                if from != to {
                    cuts.extend(disk_interval(a, b, to, radius));
                    cuts.extend(strip_interval(a, b, from, to, radius));
                }
            }
            cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut cursor = 0.;
            for (start, end) in cuts {
                if end <= cursor {
                    continue;
                }
                if start > cursor {
                    if current.is_empty() {
                        push_output_point(&mut current, lerp(a, b, cursor), &mut output_points)?;
                    }
                    push_output_point(&mut current, lerp(a, b, start), &mut output_points)?;
                }
                append_run(&mut runs, &mut current);
                touched = true;
                cursor = end;
            }
            if cursor < 1. {
                if current.is_empty() {
                    push_output_point(&mut current, lerp(a, b, cursor), &mut output_points)?;
                }
                push_output_point(&mut current, b, &mut output_points)?;
            }
        }
        append_run(&mut runs, &mut current);
        if !touched {
            result.commands.extend_from_slice(original);
            continue;
        }
        result.changed = true;
        result.approximated_curves |= original.iter().any(|command| {
            matches!(
                command,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        if contour.closed
            && runs.len() > 1
            && runs.first().unwrap().first() == runs.last().unwrap().last()
        {
            let mut tail = runs.pop().unwrap();
            tail.extend(runs.remove(0).into_iter().skip(1));
            runs.insert(0, tail);
        }
        for run in runs {
            for (index, point) in run.into_iter().enumerate() {
                let [x, y] = map(world_to_local, point);
                result.commands.push(if index == 0 {
                    PathCommand::MoveTo { x, y }
                } else {
                    PathCommand::LineTo { x, y }
                });
                if result.commands.len() > MAX_PATH_COMMANDS {
                    return Err(GeometryError::ComplexityLimit);
                }
            }
        }
    }
    validate_path_commands(&result.commands)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];
    fn line(a: Point, b: Point) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x: a[0], y: a[1] },
            PathCommand::LineTo { x: b[0], y: b[1] },
        ]
    }

    #[test]
    fn gaps_close_independent_contours_in_transformed_metric() {
        let mut commands = vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: 50., y: 30. },
            PathCommand::QuadraticTo {
                cx: 20.,
                cy: 40.,
                x: 2.,
                y: 0.,
            },
        ];
        commands.extend(line([100., 0.], [104., 0.]));
        commands.extend(line([200., 0.], [202., 0.]));
        commands.push(PathCommand::Close);
        let result = close_path_gaps(&commands, [2., 0., 0., 3., 10., 20.], 5.).unwrap();
        assert_eq!(
            (result.closed_contours, result.remaining_open_contours),
            (1, 1)
        );
        assert_eq!(result.commands[3], PathCommand::Close);
        assert_eq!(&result.commands[4..], &commands[3..]);
        assert_eq!(&result.commands[..3], &commands[..3]);
        assert_eq!(
            close_path_gaps(&commands, IDENTITY, 1.).unwrap().commands,
            commands
        );
    }

    #[test]
    fn eraser_sweeps_sparse_motion_and_retains_exact_untouched_contours() {
        let mut commands = line([0., 0.], [100., 0.]);
        let curve = vec![
            PathCommand::MoveTo { x: 0., y: 100. },
            PathCommand::CubicTo {
                cx1: 20.,
                cy1: 80.,
                cx2: 80.,
                cy2: 120.,
                x: 100.,
                y: 100.,
            },
        ];
        commands.extend(curve.clone());
        let cut =
            erase_path_stroke(&commands, IDENTITY, &[[25., -20.], [25., 20.]], 5., 0.1).unwrap();
        assert!(cut.changed);
        assert!(!cut.approximated_curves);
        assert_eq!(
            &cut.commands[..4],
            &[
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 20., y: 0. },
                PathCommand::MoveTo { x: 30., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. }
            ]
        );
        assert_eq!(&cut.commands[4..], curve);
        let miss = erase_path_stroke(&commands, IDENTITY, &[[500., 500.]], 5., 0.1).unwrap();
        assert_eq!(miss.commands, commands);
        assert!(!miss.changed);
    }

    #[test]
    fn eraser_clips_curves_and_maps_affine_paths_back_to_local_space() {
        let curve = vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::QuadraticTo {
                cx: 50.,
                cy: 100.,
                x: 100.,
                y: 0.,
            },
        ];
        let cut = erase_path_stroke(&curve, IDENTITY, &[[50., 50.]], 5., 0.05).unwrap();
        assert!(cut.changed && cut.approximated_curves);
        assert_eq!(
            cut.commands
                .iter()
                .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
                .count(),
            2
        );
        let transformed = erase_path_stroke(
            &line([0., 0.], [100., 0.]),
            [0., 2., -3., 0., 10., 20.],
            &[[10., 70.]],
            10.,
            0.1,
        )
        .unwrap();
        assert_eq!(
            transformed.commands,
            vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 20., y: 0. },
                PathCommand::MoveTo { x: 30., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. }
            ]
        );
    }

    #[test]
    fn eraser_preserves_closed_seam_and_returns_empty_for_fully_erased_path() {
        let mut square = line([0., 0.], [100., 0.]);
        square.extend([
            PathCommand::LineTo { x: 100., y: 100. },
            PathCommand::LineTo { x: 0., y: 100. },
            PathCommand::Close,
        ]);
        let cut = erase_path_stroke(&square, IDENTITY, &[[100., 50.]], 5., 0.1).unwrap();
        assert_eq!(
            cut.commands
                .iter()
                .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
                .count(),
            1
        );
        assert!(!cut.commands.iter().any(|c| matches!(c, PathCommand::Close)));
        let all =
            erase_path_stroke(&line([0., 0.], [10., 0.]), IDENTITY, &[[5., 0.]], 20., 0.1).unwrap();
        assert!(all.changed && all.commands.is_empty());
    }

    #[test]
    fn eraser_unions_overlapping_capsules_and_ignores_a_tangent_point() {
        let commands = line([0., 0.], [100., 0.]);
        let cut = erase_path_stroke(
            &commands,
            IDENTITY,
            &[[30., 0.], [40., 0.], [35., 0.]],
            10.,
            0.1,
        )
        .unwrap();
        assert_eq!(
            cut.commands,
            vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 20., y: 0. },
                PathCommand::MoveTo { x: 50., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
            ]
        );
        let tangent = erase_path_stroke(&commands, IDENTITY, &[[25., 5.]], 5., 0.1).unwrap();
        assert!(!tangent.changed);
        assert_eq!(tangent.commands, commands);
    }

    #[test]
    fn authoring_geometry_rejects_invalid_input_and_budgets() {
        let commands = line([0., 0.], [100., 0.]);
        assert!(close_path_gaps(&commands, IDENTITY, f64::NAN).is_err());
        assert!(erase_path_stroke(&commands, [0.; 6], &[[0., 0.]], 5., 0.1).is_err());
        assert!(erase_path_stroke(&commands, IDENTITY, &[[f64::NAN, 0.]], 5., 0.1).is_err());
        assert!(matches!(
            erase_path_stroke(
                &commands,
                IDENTITY,
                &vec![[0., 0.]; MAX_GESTURE_POINTS + 1],
                5.,
                0.1
            ),
            Err(GeometryError::ComplexityLimit)
        ));
    }
}
