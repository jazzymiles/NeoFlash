use super::*;

pub(super) fn invalid_merge_geometry(message: impl Into<String>) -> GeometryError {
    GeometryError::InvalidPath(message.into())
}

pub(super) fn ensure_fragmentable_merge(element: &Element) -> Result<(), GeometryError> {
    let protected = if element.cache_as_bitmap {
        Some("Cache as bitmap")
    } else if !element.blend_mode.is_normal() {
        Some("a non-Normal blend mode")
    } else if !element.filters.is_empty() {
        Some("filters")
    } else {
        None
    };
    if let Some(feature) = protected {
        return Err(invalid_merge_geometry(format!(
            "partial Merge editing cannot split element '{}' while it uses {feature}; disable the effect or use whole-object editing",
            element.id
        )));
    }
    Ok(())
}

/// Validate and flatten a path without the silent-empty fallback used by the older convenience
/// helpers. Fill paths may use the usual implicit closing edge; selection polygons must contain
/// only explicitly closed contours.
pub(super) fn checked_rings(
    commands: &[PathCommand],
    require_closed: bool,
) -> Result<Vec<Vec<[f64; 2]>>, GeometryError> {
    validate_path_commands(commands)?;
    let contours = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.35,
            ..FlattenOptions::default()
        },
    )?;
    let mut rings = Vec::new();
    for contour in contours {
        if require_closed && !contour.closed {
            return Err(invalid_merge_geometry(
                "selection polygon contours must be closed",
            ));
        }
        let mut points = Vec::with_capacity(contour.points.len());
        for point in contour.points {
            if points
                .last()
                .is_none_or(|previous| dist_sq(*previous, point) > SNAP_EPS * SNAP_EPS)
            {
                points.push(point);
            }
        }
        if points.len() >= 2 && dist_sq(points[0], *points.last().unwrap()) <= SNAP_EPS * SNAP_EPS {
            points.pop();
        }
        if points.len() >= 3 && signed_ring_area(&points).abs() > AREA_EPS {
            rings.push(points);
        }
    }
    Ok(rings)
}

pub(super) fn validate_generated_pieces(pieces: &[Vec<PathCommand>]) -> Result<(), GeometryError> {
    let mut total = 0usize;
    for commands in pieces {
        validate_path_commands(commands)?;
        total = total
            .checked_add(commands.len())
            .ok_or(GeometryError::ComplexityLimit)?;
        if total > MAX_PATH_COMMANDS {
            return Err(GeometryError::ComplexityLimit);
        }
    }
    Ok(())
}

/// Replace `Close` with its explicit seam segment before cutting a stroke. A closed stroke that
/// does not cross the polygon is retained verbatim by the caller, so its authored join is lost
/// only when the seam really is cut into open fragments.
pub(super) fn explicit_close_segments(commands: &[PathCommand]) -> Vec<PathCommand> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cursor = [0.0, 0.0];
    let mut start = [0.0, 0.0];
    for &command in commands {
        match command {
            PathCommand::MoveTo { x, y } => {
                cursor = [x, y];
                start = cursor;
                result.push(command);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = [x, y];
                result.push(command);
            }
            PathCommand::Close => {
                if dist_sq(cursor, start) > SNAP_EPS * SNAP_EPS {
                    result.push(PathCommand::LineTo {
                        x: start[0],
                        y: start[1],
                    });
                }
                cursor = start;
            }
        }
    }
    result
}

/// Split stroke commands at polygon edges, classify every exact Bézier sub-arc by its midpoint,
/// then join adjacent sub-arcs with the same classification back into the fewest path runs.
pub(super) fn classified_stroke_runs(
    commands: &[PathCommand],
    cutters: &[([f64; 2], [f64; 2])],
    polygon_rings: &[Vec<[f64; 2]>],
) -> Vec<(bool, Vec<PathCommand>)> {
    let explicit = explicit_close_segments(commands);
    let pieces = split_bezier_path_with_segments(&explicit, cutters);
    let mut runs = Vec::new();
    let mut current_commands: Vec<PathCommand> = Vec::new();
    let mut current_class = None;
    let mut current_end: Option<[f64; 2]> = None;

    for piece in pieces {
        let mut cursor = [0.0, 0.0];
        let mut have_cursor = false;
        for command in piece {
            match command {
                PathCommand::MoveTo { x, y } => {
                    cursor = [x, y];
                    have_cursor = true;
                }
                PathCommand::LineTo { .. }
                | PathCommand::QuadraticTo { .. }
                | PathCommand::CubicTo { .. }
                    if have_cursor =>
                {
                    let midpoint = eval_command_at(cursor, command, 0.5);
                    let inside = point_in_rings(midpoint, polygon_rings, FillRule::EvenOdd);
                    let connected =
                        current_end.is_some_and(|end| dist_sq(end, cursor) <= SNAP_EPS * SNAP_EPS);
                    if current_class != Some(inside) || !connected {
                        if current_commands.len() >= 2 {
                            runs.push((
                                current_class.expect("a path run has a classification"),
                                std::mem::take(&mut current_commands),
                            ));
                        } else {
                            current_commands.clear();
                        }
                        current_commands.push(PathCommand::MoveTo {
                            x: cursor[0],
                            y: cursor[1],
                        });
                        current_class = Some(inside);
                    }
                    current_commands.push(command);
                    cursor = eval_command_at(cursor, command, 1.0);
                    current_end = Some(cursor);
                }
                PathCommand::Close
                | PathCommand::LineTo { .. }
                | PathCommand::QuadraticTo { .. }
                | PathCommand::CubicTo { .. } => {}
            }
        }
    }
    if current_commands.len() >= 2 {
        runs.push((
            current_class.expect("a path run has a classification"),
            current_commands,
        ));
    }
    runs
}

pub(super) fn split_check_budget(
    commands: &[PathCommand],
    cutter_count: usize,
) -> Result<(), GeometryError> {
    let drawable = commands
        .iter()
        .filter(|command| !matches!(command, PathCommand::MoveTo { .. } | PathCommand::Close))
        .count();
    let checks = drawable
        .checked_mul(cutter_count)
        .and_then(|value| value.checked_mul(32))
        .ok_or(GeometryError::ComplexityLimit)?;
    if checks > MAX_SPLIT_INTERSECTION_CHECKS {
        return Err(GeometryError::ComplexityLimit);
    }
    Ok(())
}

pub(super) enum FragmentSelection {
    Existing(Id),
    Pending(usize),
}

/// Split every raw Merge fill and stroke along `polygon`, returning the IDs of the pieces inside
/// it. Fills use planar intersection/difference; strokes keep exact quadratic/cubic sub-arcs.
/// Object Drawing and temporarily protected grouped elements (`merge == false`) are untouched.
///
/// The operation is atomic: invalid input or any path/complexity failure returns an error without
/// changing `elements`. The source ID/name/scripts/accessibility stay on the first outside piece;
/// every additional fragment receives a fresh ID and no duplicated identity metadata.
pub fn split_merge_geometry_by_polygon(
    elements: &mut Vec<Element>,
    polygon: &[PathCommand],
) -> Result<Vec<Id>, GeometryError> {
    let polygon_rings = checked_rings(polygon, true)?;
    if polygon_rings.is_empty() {
        return Err(invalid_merge_geometry(
            "selection polygon must contain a nondegenerate closed contour",
        ));
    }
    let cutters = fill_boundary_segments(&polygon_rings, FillRule::EvenOdd);
    if cutters.is_empty() {
        return Err(invalid_merge_geometry(
            "selection polygon has no usable boundary",
        ));
    }
    if cutters.len() > MAX_PATH_COMMANDS {
        return Err(GeometryError::ComplexityLimit);
    }
    let polygon_bounds = rings_bounds(&polygon_rings).expect("validated polygon has bounds");
    let mut selections = Vec::new();
    let mut pending_ids = Vec::new();
    let mut next_elements = Vec::with_capacity(elements.len() + 2);

    for existing in elements.iter() {
        if !is_merge_fill(existing) && !is_merge_stroke(existing) {
            next_elements.push(existing.clone());
            continue;
        }
        let Some(existing_bounds) = element_fast_world_bounds(existing) else {
            next_elements.push(existing.clone());
            continue;
        };
        if !bounds_overlap(existing_bounds, polygon_bounds, SNAP_EPS * 2.0) {
            next_elements.push(existing.clone());
            continue;
        }
        ensure_fragmentable_merge(existing)?;
        let Some((commands, fill_rule)) = element_world_commands(existing) else {
            next_elements.push(existing.clone());
            continue;
        };
        validate_path_commands(&commands)?;

        if is_merge_fill(existing) {
            let existing_rings = checked_rings(&commands, false)?;
            if existing_rings.is_empty() {
                next_elements.push(existing.clone());
                continue;
            }
            let inside_pieces = boolean_rings(
                &existing_rings,
                fill_rule,
                &polygon_rings,
                FillRule::EvenOdd,
                BooleanOp::Intersection,
            );
            if inside_pieces.is_empty() {
                next_elements.push(existing.clone());
                continue;
            }
            let outside_pieces = boolean_rings(
                &existing_rings,
                fill_rule,
                &polygon_rings,
                FillRule::EvenOdd,
                BooleanOp::Difference,
            );
            if outside_pieces.is_empty() {
                selections.push(FragmentSelection::Existing(existing.id.clone()));
                next_elements.push(existing.clone());
                continue;
            }
            validate_generated_pieces(&outside_pieces)?;
            validate_generated_pieces(&inside_pieces)?;
            let paint =
                extract_fill_paint(&existing.kind).expect("a Merge fill always has fill paint");
            for (index, piece) in outside_pieces.into_iter().enumerate() {
                let keep_identity = index == 0;
                let mut fragment = build_fill_element(
                    existing,
                    if keep_identity {
                        existing.id.clone()
                    } else {
                        String::new()
                    },
                    piece,
                    FillRule::NonZero,
                    &paint,
                    true,
                );
                if !keep_identity {
                    clear_fragment_identity(&mut fragment);
                    pending_ids.push(next_elements.len());
                }
                next_elements.push(fragment);
            }
            for piece in inside_pieces {
                let mut fragment = build_fill_element(
                    existing,
                    String::new(),
                    piece,
                    FillRule::NonZero,
                    &paint,
                    true,
                );
                clear_fragment_identity(&mut fragment);
                let pending_index = next_elements.len();
                pending_ids.push(pending_index);
                selections.push(FragmentSelection::Pending(pending_index));
                next_elements.push(fragment);
            }
            continue;
        }

        split_check_budget(&commands, cutters.len())?;
        let runs = classified_stroke_runs(&commands, &cutters, &polygon_rings);
        let has_inside = runs.iter().any(|(inside, _)| *inside);
        let has_outside = runs.iter().any(|(inside, _)| !*inside);
        if !has_inside {
            next_elements.push(existing.clone());
            continue;
        }
        if !has_outside {
            selections.push(FragmentSelection::Existing(existing.id.clone()));
            next_elements.push(existing.clone());
            continue;
        }
        let run_commands: Vec<_> = runs.iter().map(|(_, commands)| commands.clone()).collect();
        validate_generated_pieces(&run_commands)?;
        let mut paint =
            extract_stroke_paint(&existing.kind).expect("a Merge stroke always has stroke paint");
        paint.width *= transform_scale_factor(&existing.transform);
        let mut kept_identity = false;
        for (inside, piece) in runs {
            let keep_identity = !inside && !kept_identity;
            let id = if keep_identity {
                kept_identity = true;
                existing.id.clone()
            } else {
                String::new()
            };
            let mut fragment = build_stroke_element(existing, id, piece, &paint, true);
            if !keep_identity {
                clear_fragment_identity(&mut fragment);
                pending_ids.push(next_elements.len());
            }
            if inside {
                selections.push(FragmentSelection::Pending(next_elements.len()));
            }
            next_elements.push(fragment);
        }
    }

    for index in pending_ids {
        next_elements[index].id = new_id("element");
    }
    let selected_ids = selections
        .into_iter()
        .map(|selection| match selection {
            FragmentSelection::Existing(id) => id,
            FragmentSelection::Pending(index) => next_elements[index].id.clone(),
        })
        .collect();
    *elements = next_elements;
    Ok(selected_ids)
}

pub(super) fn circle_ring(center: [f64; 2], radius: f64, cap_steps: usize) -> Vec<[f64; 2]> {
    (0..cap_steps * 2)
        .map(|step| {
            let angle = step as f64 * std::f64::consts::TAU / (cap_steps * 2) as f64;
            [
                center[0] + angle.cos() * radius,
                center[1] + angle.sin() * radius,
            ]
        })
        .collect()
}

pub(super) fn capsule_ring(
    from: [f64; 2],
    to: [f64; 2],
    radius: f64,
    cap_steps: usize,
) -> Vec<[f64; 2]> {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    if dx.hypot(dy) <= 1e-9 {
        return circle_ring(from, radius, cap_steps);
    }
    let angle = dy.atan2(dx);
    let mut ring = Vec::with_capacity((cap_steps + 1) * 2);
    for step in 0..=cap_steps {
        let theta = angle - std::f64::consts::FRAC_PI_2
            + step as f64 * std::f64::consts::PI / cap_steps as f64;
        ring.push([to[0] + theta.cos() * radius, to[1] + theta.sin() * radius]);
    }
    for step in 0..=cap_steps {
        let theta = angle
            + std::f64::consts::FRAC_PI_2
            + step as f64 * std::f64::consts::PI / cap_steps as f64;
        ring.push([
            from[0] + theta.cos() * radius,
            from[1] + theta.sin() * radius,
        ]);
    }
    ring
}

pub(super) fn eraser_cutter_rings_with_shape(
    points: &[[f64; 2]],
    radius: f64,
    tolerance: f64,
    square: bool,
) -> Result<Vec<Vec<[f64; 2]>>, GeometryError> {
    if !radius.is_finite() || radius <= 0.0 || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(invalid_merge_geometry(
            "eraser radius and tolerance must be positive and finite",
        ));
    }
    if points.len() > MAX_MERGE_ERASER_POINTS {
        return Err(GeometryError::ComplexityLimit);
    }
    if points
        .iter()
        .flatten()
        .any(|value| !value.is_finite() || value.abs() > 1e9)
    {
        return Err(invalid_merge_geometry(
            "eraser points must be finite and within coordinate limits",
        ));
    }
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let mut distinct = Vec::with_capacity(points.len());
    for &point in points {
        if distinct.last().is_none_or(|previous| *previous != point) {
            distinct.push(point);
        }
    }
    let rings = if square {
        if distinct.len() == 1 {
            vec![square_sweep_polygon(distinct[0], distinct[0], radius)]
        } else {
            distinct
                .windows(2)
                .map(|pair| square_sweep_polygon(pair[0], pair[1], radius))
                .collect::<Vec<_>>()
        }
    } else {
        let sweep_count = distinct.len().saturating_sub(1).max(1);
        let max_cap_steps = if distinct.len() == 1 {
            MAX_PATH_COMMANDS / 2
        } else {
            (MAX_PATH_COMMANDS / (sweep_count * 2)).saturating_sub(1)
        };
        if max_cap_steps < 3 {
            return Err(GeometryError::ComplexityLimit);
        }
        // Bound the sagitta of each round cap by the caller's flattening tolerance when the gesture
        // budget allows it. Dense tablet input degrades cap tessellation, never geometry count.
        let cosine = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
        let max_step_angle = 2.0 * cosine.acos();
        let desired_cap_steps = if max_step_angle.is_finite() && max_step_angle > 1e-6 {
            (std::f64::consts::PI / max_step_angle).ceil() as usize
        } else {
            32
        };
        let cap_steps = desired_cap_steps.clamp(6, 32).min(max_cap_steps);
        if distinct.len() == 1 {
            vec![circle_ring(distinct[0], radius, cap_steps)]
        } else {
            distinct
                .windows(2)
                .map(|pair| capsule_ring(pair[0], pair[1], radius, cap_steps))
                .collect::<Vec<_>>()
        }
    };
    let edge_count = rings.iter().try_fold(0usize, |total, ring| {
        total
            .checked_add(ring.len())
            .ok_or(GeometryError::ComplexityLimit)
    })?;
    if edge_count > MAX_PATH_COMMANDS {
        return Err(GeometryError::ComplexityLimit);
    }
    if rings
        .iter()
        .flatten()
        .flatten()
        .any(|value| !value.is_finite() || value.abs() > 1e9)
    {
        return Err(invalid_merge_geometry(
            "eraser sweep exceeds coordinate limits",
        ));
    }
    Ok(rings)
}

/// Cut a swept round eraser through raw Merge fills and strokes. Fill geometry is subtracted by
/// the capsule union; stroke centrelines use the bounded authoring eraser and are emitted as
/// separate surviving runs. Object Drawing and protected grouped elements are untouched.
///
/// Returns whether geometry changed. The operation is atomic on invalid/over-budget input. The
/// first surviving fragment retains the source ID and identity metadata; additional fragments
/// receive fresh IDs with name/scripts/accessibility cleared while keeping paint and appearance.
pub fn erase_merge_geometry_with_sweep(
    elements: &mut Vec<Element>,
    eraser_points: &[[f64; 2]],
    radius: f64,
    tolerance: f64,
) -> Result<bool, GeometryError> {
    erase_merge_geometry_with_metric(
        elements,
        eraser_points,
        [1., 0., 0., 1., 0., 0.],
        radius,
        tolerance,
    )
}

/// Cut raw Merge geometry with a round eraser measured after `local_to_metric`, retaining the
/// result in authored coordinates. A circular screen brush becomes its inverse affine
/// ellipse/capsule under edit-in-place instead of an over-large local circle.
pub fn erase_merge_geometry_with_metric(
    elements: &mut Vec<Element>,
    eraser_points: &[[f64; 2]],
    local_to_metric: [f64; 6],
    radius: f64,
    tolerance: f64,
) -> Result<bool, GeometryError> {
    erase_merge_geometry_with_shape(
        elements,
        eraser_points,
        local_to_metric,
        radius,
        tolerance,
        false,
    )
}

/// Cut raw Merge geometry with either a round (`square == false`) or square (`square == true`)
/// eraser measured after `local_to_metric`, retaining the result in authored coordinates.
pub fn erase_merge_geometry_with_shape(
    elements: &mut Vec<Element>,
    eraser_points: &[[f64; 2]],
    local_to_metric: [f64; 6],
    radius: f64,
    tolerance: f64,
    square: bool,
) -> Result<bool, GeometryError> {
    if local_to_metric.iter().any(|value| !value.is_finite()) {
        return Err(invalid_merge_geometry(
            "eraser metric must be finite and invertible",
        ));
    }
    let metric_to_local = invert_affine_matrix(local_to_metric)
        .ok_or_else(|| invalid_merge_geometry("eraser metric must be finite and invertible"))?;
    let metric_points: Vec<_> = eraser_points
        .iter()
        .map(|point| transform_point(local_to_metric, point[0], point[1]))
        .collect();
    let mut cutter_rings =
        eraser_cutter_rings_with_shape(&metric_points, radius, tolerance, square)?;
    for point in cutter_rings.iter_mut().flatten() {
        *point = transform_point(metric_to_local, point[0], point[1]);
    }
    if cutter_rings
        .iter()
        .flatten()
        .flatten()
        .any(|value| !value.is_finite() || value.abs() > 1e9)
    {
        return Err(invalid_merge_geometry(
            "inverse-mapped eraser sweep exceeds coordinate limits",
        ));
    }
    if cutter_rings.is_empty() {
        return Ok(false);
    }
    let cutter_bounds = rings_bounds(&cutter_rings).expect("nonempty eraser has bounds");
    let mut changed = false;
    let mut pending_ids = Vec::new();
    let mut next_elements = Vec::with_capacity(elements.len() + 2);

    for existing in elements.iter() {
        if !is_merge_fill(existing) && !is_merge_stroke(existing) {
            next_elements.push(existing.clone());
            continue;
        }
        let Some(existing_bounds) = element_fast_world_bounds(existing) else {
            next_elements.push(existing.clone());
            continue;
        };
        if !bounds_overlap(existing_bounds, cutter_bounds, SNAP_EPS) {
            next_elements.push(existing.clone());
            continue;
        }
        ensure_fragmentable_merge(existing)?;
        let Some((commands, fill_rule)) = element_world_commands(existing) else {
            next_elements.push(existing.clone());
            continue;
        };
        validate_path_commands(&commands)?;

        if is_merge_fill(existing) {
            let existing_rings = checked_rings(&commands, false)?;
            if existing_rings.is_empty() {
                next_elements.push(existing.clone());
                continue;
            }
            let (outside_pieces, interacted) = boolean_rings_with_interaction(
                &existing_rings,
                fill_rule,
                &cutter_rings,
                FillRule::NonZero,
                BooleanOp::Difference,
            );
            if !interacted {
                next_elements.push(existing.clone());
                continue;
            }
            validate_generated_pieces(&outside_pieces)?;
            changed = true;
            let paint =
                extract_fill_paint(&existing.kind).expect("a Merge fill always has fill paint");
            for (index, piece) in outside_pieces.into_iter().enumerate() {
                let keep_identity = index == 0;
                let mut fragment = build_fill_element(
                    existing,
                    if keep_identity {
                        existing.id.clone()
                    } else {
                        String::new()
                    },
                    piece,
                    FillRule::NonZero,
                    &paint,
                    true,
                );
                if !keep_identity {
                    clear_fragment_identity(&mut fragment);
                    pending_ids.push(next_elements.len());
                }
                next_elements.push(fragment);
            }
            continue;
        }

        let result = erase_path_stroke_with_shape(
            &commands,
            local_to_metric,
            &metric_points,
            radius,
            tolerance,
            square,
        )?;
        if !result.changed {
            next_elements.push(existing.clone());
            continue;
        }
        changed = true;
        let pieces = split_commands_into_contours(&result.commands);
        validate_generated_pieces(&pieces)?;
        let mut paint =
            extract_stroke_paint(&existing.kind).expect("a Merge stroke always has stroke paint");
        paint.width *= transform_scale_factor(&existing.transform);
        for (index, piece) in pieces.into_iter().enumerate() {
            let keep_identity = index == 0;
            let mut fragment = build_stroke_element(
                existing,
                if keep_identity {
                    existing.id.clone()
                } else {
                    String::new()
                },
                piece,
                &paint,
                true,
            );
            if !keep_identity {
                clear_fragment_identity(&mut fragment);
                pending_ids.push(next_elements.len());
            }
            next_elements.push(fragment);
        }
    }

    if changed {
        for index in pending_ids {
            next_elements[index].id = new_id("element");
        }
        *elements = next_elements;
    }
    Ok(changed)
}

/// Compatibility entry point for the T-128 editor call site. Invalid/over-budget input remains a
/// no-op as before; successful calls now also select exact Merge stroke fragments.
pub fn lasso_split_merge_fills(
    elements: &mut Vec<Element>,
    lasso_polygon: &[PathCommand],
) -> Vec<Id> {
    split_merge_geometry_by_polygon(elements, lasso_polygon).unwrap_or_default()
}
