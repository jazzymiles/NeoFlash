use super::*;

pub(super) fn has_normalized_merge_part_kind(el: &Element) -> bool {
    if !el.merge {
        return false;
    }
    let (ElementKind::Path {
        commands,
        stroke_width,
        ..
    }
    | ElementKind::PaintedPath {
        commands,
        stroke_width,
        ..
    }) = &el.kind
    else {
        return false;
    };
    if commands.is_empty() {
        return false;
    }
    let has_fill = extract_fill_paint(&el.kind).is_some();
    let has_stroke = extract_stroke_paint(&el.kind).is_some();
    if has_fill && !has_stroke && *stroke_width == 0.0 {
        return true;
    }
    if !has_fill && has_stroke {
        let move_tos = commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count();
        return move_tos <= 1;
    }
    false
}

pub(super) fn is_already_normalized_merge_part(el: &Element) -> bool {
    is_identity_transform(&el.transform) && has_normalized_merge_part_kind(el)
}

pub(super) fn ensure_existing_merge_normalized(elements: &mut Vec<Element>) {
    if elements
        .iter()
        .all(|el| !el.merge || is_already_normalized_merge_part(el))
    {
        return;
    }
    let mut normalized = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if !el.merge || is_already_normalized_merge_part(&el) {
            normalized.push(el);
            continue;
        }
        if let Some((fill_opt, stroke_els)) = decompose_element_to_merge_parts(&el) {
            if let Some(f) = fill_opt {
                normalized.push(f);
            }
            normalized.extend(stroke_els);
        } else {
            normalized.push(el);
        }
    }
    *elements = normalized;
}

/// Apply a newly drawn shape (`new_element`) in Merge Drawing mode to the keyframe's `elements`,
/// performing planar fill union/difference, stroke slicing of fills, and stroke intersection
/// splitting against existing `merge == true` elements on the keyframe.
///
/// Returns the IDs of the newly created Merge element(s).
pub fn apply_merge_shape_to_keyframe(elements: &mut Vec<Element>, new_element: Element) -> Vec<Id> {
    apply_merge_shape_to_keyframe_inner(elements, new_element, false)
}

pub(super) fn apply_merge_shape_to_keyframe_inner(
    elements: &mut Vec<Element>,
    new_element: Element,
    already_dissolved: bool,
) -> Vec<Id> {
    ensure_existing_merge_normalized(elements);
    let Some((mut new_fill_opt, new_strokes)) = decompose_element_to_merge_parts(&new_element)
    else {
        let id = new_element.id.clone();
        elements.push(new_element);
        return vec![id];
    };

    let has_own_stroke = !new_strokes.is_empty();
    let mut created_ids = Vec::new();

    // 1. Fill interaction against existing Merge elements.
    if let Some(mut new_fill) = new_fill_opt.take() {
        let Some((mut new_cmds, mut new_rule)) = element_world_commands(&new_fill) else {
            return Vec::new();
        };
        let mut new_rings = flatten_closed_rings(&new_cmds);
        let new_paint = extract_fill_paint(&new_fill.kind);

        if !new_rings.is_empty()
            && let Some(new_fp) = &new_paint
        {
            // Dissolve internal overlaps of compound / Brush strokes up front so the stroke
            // stores and tests only its clean exterior boundary and holes.
            if new_rings.len() > 1 && !already_dissolved {
                let dissolved = dissolve_rings_to_islands(&new_rings, new_rule);
                if !dissolved.is_empty() {
                    new_cmds = dissolved.into_iter().flatten().collect();
                    new_rule = FillRule::NonZero;
                    new_rings = flatten_closed_rings(&new_cmds);
                }
            }
            let new_bbox_opt = rings_bounds(&new_rings);
            let mut cached_trim_fill: Option<PreparedMergeRings> = None;
            let mut pending_union_rings: Vec<Vec<[f64; 2]>> = Vec::new();

            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge {
                    next_elements.push(existing);
                    continue;
                }
                let Some(new_bbox) = new_bbox_opt else {
                    next_elements.push(existing);
                    continue;
                };
                if is_merge_fill(&existing) {
                    let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_fast_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let Some((ex_cmds, ex_rule)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let ex_rings = flatten_closed_rings(&ex_cmds);
                    let Some(ex_bbox) = rings_bounds(&ex_rings) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let prepared_fill = cached_trim_fill
                        .get_or_insert_with(|| PreparedMergeRings::new(&new_rings, new_rule));
                    if !rings_interact_with_prepared(
                        &ex_rings,
                        ex_rule,
                        ex_bbox,
                        &prepared_fill.segments,
                        &prepared_fill.segment_bvh,
                        &prepared_fill.fill,
                    ) {
                        next_elements.push(existing);
                        continue;
                    }
                    let same_paint = extract_fill_paint(&existing.kind).as_ref() == Some(new_fp);
                    if !has_own_stroke && same_paint {
                        if ex_rule == FillRule::NonZero {
                            pending_union_rings.extend(ex_rings);
                        } else {
                            for island in dissolve_rings_to_islands(&ex_rings, ex_rule) {
                                pending_union_rings.extend(flatten_closed_rings(&island));
                            }
                        }
                        continue;
                    }
                    // Different colour (or separated by new shape's outline stroke): subtract
                    // `new_fill` from `existing`.
                    let local_new_rings =
                        prepared_fill.rings_overlapping(&new_rings, ex_bbox, SNAP_EPS * 4.0);
                    let Some(local_new_bbox) = rings_bounds(&local_new_rings) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let (mut diff, interacted) = boolean_rings_with_interaction(
                        &ex_rings,
                        ex_rule,
                        &local_new_rings,
                        new_rule,
                        BooleanOp::Difference,
                    );
                    if !interacted {
                        next_elements.push(existing);
                        continue;
                    }
                    let extends_outside_new = ex_bbox.0[0] < local_new_bbox.0[0] - SNAP_EPS * 8.0
                        || ex_bbox.0[1] < local_new_bbox.0[1] - SNAP_EPS * 8.0
                        || ex_bbox.1[0] > local_new_bbox.1[0] + SNAP_EPS * 8.0
                        || ex_bbox.1[1] > local_new_bbox.1[1] + SNAP_EPS * 8.0;
                    if diff.is_empty() && extends_outside_new {
                        diff = boolean_rings_full(
                            &ex_rings,
                            ex_rule,
                            &local_new_rings,
                            new_rule,
                            BooleanOp::Difference,
                        )
                        .0;
                        if diff.is_empty() {
                            next_elements.push(existing);
                            continue;
                        }
                    }
                    if let Some(ex_fp) = extract_fill_paint(&existing.kind) {
                        for (idx, piece_cmds) in diff.into_iter().enumerate() {
                            let piece_id = if idx == 0 {
                                existing.id.clone()
                            } else {
                                new_id("element")
                            };
                            next_elements.push(build_fill_element(
                                &existing,
                                piece_id,
                                piece_cmds,
                                FillRule::NonZero,
                                &ex_fp,
                                true,
                            ));
                        }
                    }
                } else if is_merge_stroke(&existing) {
                    let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_fast_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let Some((ex_cmds, _)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let polylines = flatten_stroke_polylines(&ex_cmds);
                    let Some(sp) = extract_stroke_paint(&existing.kind) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let prepared_fill = cached_trim_fill
                        .get_or_insert_with(|| PreparedMergeRings::new(&new_rings, new_rule));
                    let mut piece_idx = 0usize;
                    for poly in polylines {
                        for trimmed in trim_polyline_inside_prepared_fill(
                            &poly,
                            &prepared_fill.segments,
                            &prepared_fill.segment_bvh,
                            &prepared_fill.fill,
                        ) {
                            let cmds = polyline_to_commands(&trimmed);
                            if cmds.len() >= 2 {
                                let piece_id = if piece_idx == 0 {
                                    existing.id.clone()
                                } else {
                                    new_id("element")
                                };
                                piece_idx += 1;
                                next_elements.push(build_stroke_element(
                                    &existing, piece_id, cmds, &sp, true,
                                ));
                            }
                        }
                    }
                } else {
                    next_elements.push(existing);
                }
            }
            if !pending_union_rings.is_empty() {
                let (unioned, _) = boolean_rings_with_interaction(
                    &pending_union_rings,
                    FillRule::NonZero,
                    &new_rings,
                    new_rule,
                    BooleanOp::Union,
                );
                if !unioned.is_empty() {
                    new_cmds = unioned.into_iter().flatten().collect();
                    new_rule = FillRule::NonZero;
                }
            }
            *elements = next_elements;
            new_fill = build_fill_element(
                &new_fill,
                new_fill.id.clone(),
                new_cmds,
                new_rule,
                new_fp,
                true,
            );
            created_ids.push(new_fill.id.clone());
            elements.push(new_fill);
        }
    }

    // 2. Stroke interaction against existing Merge fills and strokes.
    if !new_strokes.is_empty() {
        let own_fill_id = created_ids.first().cloned();
        let mut stroke_polylines_all = Vec::new();
        for ns in &new_strokes {
            if let Some((cmds, _)) = element_world_commands(ns) {
                stroke_polylines_all.extend(flatten_stroke_polylines(&cmds));
            }
        }
        let new_stroke_segs = polylines_to_segments(&stroke_polylines_all);
        let new_strokes_bbox = segments_bounds(&new_stroke_segs);

        // 2a. Slice any existing Merge fills crossed by the new stroke(s).
        if !stroke_polylines_all.is_empty()
            && let Some(strokes_bbox) = new_strokes_bbox
        {
            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge
                    || !is_merge_fill(&existing)
                    || own_fill_id.as_deref() == Some(existing.id.as_str())
                {
                    next_elements.push(existing);
                    continue;
                }
                let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                if !bounds_overlap(ex_fast_bbox, strokes_bbox, SNAP_EPS * 2.0) {
                    next_elements.push(existing);
                    continue;
                }
                let Some((ex_cmds, ex_rule)) = element_world_commands(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                let ex_rings = flatten_closed_rings(&ex_cmds);
                let sliced =
                    slice_fill_rings_with_polylines(&ex_rings, ex_rule, &stroke_polylines_all);
                if sliced.len() <= 1 {
                    next_elements.push(existing);
                } else if let Some(ex_fp) = extract_fill_paint(&existing.kind) {
                    for (idx, piece_cmds) in sliced.into_iter().enumerate() {
                        let piece_id = if idx == 0 {
                            existing.id.clone()
                        } else {
                            new_id("element")
                        };
                        next_elements.push(build_fill_element(
                            &existing,
                            piece_id,
                            piece_cmds,
                            FillRule::NonZero,
                            &ex_fp,
                            true,
                        ));
                    }
                }
            }
            *elements = next_elements;
        }

        // 2b. Split existing Merge strokes at intersections with the new stroke(s), and split
        // the new stroke(s) at intersections with existing Merge strokes & fill boundaries.
        let mut existing_stroke_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
        let mut existing_fill_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();

        let mut next_elements = Vec::with_capacity(elements.len() + new_strokes.len());
        for existing in elements.drain(..) {
            if !existing.merge {
                next_elements.push(existing);
                continue;
            }
            let overlaps_new_strokes = new_strokes_bbox
                .zip(element_fast_world_bounds(&existing))
                .is_some_and(|(sb, eb)| bounds_overlap(eb, sb, SNAP_EPS * 2.0));
            if !overlaps_new_strokes {
                next_elements.push(existing);
                continue;
            }
            if is_merge_fill(&existing) {
                if own_fill_id.as_deref() != Some(existing.id.as_str())
                    && let Some((ex_cmds, ex_rule)) = element_world_commands(&existing)
                {
                    let rings = flatten_closed_rings(&ex_cmds);
                    existing_fill_segs.extend(fill_boundary_segments(&rings, ex_rule));
                }
                next_elements.push(existing);
            } else if is_merge_stroke(&existing) {
                let Some((ex_cmds, _)) = element_world_commands(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                let polys = flatten_stroke_polylines(&ex_cmds);
                existing_stroke_segs.extend(polylines_to_segments(&polys));
                let Some(sp) = extract_stroke_paint(&existing.kind) else {
                    next_elements.push(existing);
                    continue;
                };
                let mut piece_idx = 0usize;
                for poly in polys {
                    for piece in split_polyline_with_segments(&poly, &new_stroke_segs, false) {
                        let cmds = polyline_to_commands(&piece);
                        if cmds.len() >= 2 {
                            let piece_id = if piece_idx == 0 {
                                existing.id.clone()
                            } else {
                                new_id("element")
                            };
                            piece_idx += 1;
                            next_elements
                                .push(build_stroke_element(&existing, piece_id, cmds, &sp, true));
                        }
                    }
                }
            } else {
                next_elements.push(existing);
            }
        }
        *elements = next_elements;

        for ns in new_strokes {
            let Some((ns_cmds, _)) = element_world_commands(&ns) else {
                continue;
            };
            let Some(sp) = extract_stroke_paint(&ns.kind) else {
                continue;
            };
            let polys = flatten_stroke_polylines(&ns_cmds);
            let mut piece_idx = 0usize;
            for poly in polys {
                let poly_bbox = ring_bounds(&poly);
                let mut cutters: Vec<([f64; 2], [f64; 2])> = existing_stroke_segs
                    .iter()
                    .copied()
                    .filter(|&(a, b)| {
                        poly_bbox.is_some_and(|pb| {
                            bounds_overlap(
                                pb,
                                (
                                    [a[0].min(b[0]), a[1].min(b[1])],
                                    [a[0].max(b[0]), a[1].max(b[1])],
                                ),
                                SNAP_EPS * 2.0,
                            )
                        })
                    })
                    .collect();
                cutters.extend(existing_fill_segs.iter().copied().filter(|&(a, b)| {
                    poly_bbox.is_some_and(|pb| {
                        bounds_overlap(
                            pb,
                            (
                                [a[0].min(b[0]), a[1].min(b[1])],
                                [a[0].max(b[0]), a[1].max(b[1])],
                            ),
                            SNAP_EPS * 2.0,
                        )
                    }) && !segment_lies_on_polyline((a, b), &poly)
                }));
                for piece in split_polyline_with_segments(&poly, &cutters, true) {
                    let cmds = polyline_to_commands(&piece);
                    if cmds.len() >= 2 {
                        let piece_id = if piece_idx == 0 {
                            ns.id.clone()
                        } else {
                            new_id("element")
                        };
                        piece_idx += 1;
                        created_ids.push(piece_id.clone());
                        elements.push(build_stroke_element(&ns, piece_id, cmds, &sp, true));
                    }
                }
            }
        }
    }

    created_ids
}

/// Re-apply Merge Drawing planar interactions after existing Merge-mode elements (`moved_ids`)
/// have been moved or transformed on the stage.
pub fn remerge_moved_elements_in_keyframe(
    elements: &mut Vec<Element>,
    moved_ids: &[Id],
) -> Vec<Id> {
    if moved_ids.is_empty() {
        return Vec::new();
    }
    let moved_set: std::collections::HashSet<&str> = moved_ids.iter().map(String::as_str).collect();
    let original_moved_ids: Vec<Id> = elements
        .iter()
        .filter(|element| element.merge && moved_set.contains(element.id.as_str()))
        .map(|element| element.id.clone())
        .collect();
    if original_moved_ids.is_empty() {
        return Vec::new();
    }

    // Merge mutates several existing shapes while it works. Keep the operation atomic: a geometry
    // limit or malformed path must leave the already-moved artwork intact instead of deleting the
    // selected elements (the former unbounded batch did exactly that above the flattening budget).
    let before = elements.clone();
    if let Some(ids) = remerge_moved_elements_inner(elements, &moved_set) {
        ids
    } else {
        *elements = before;
        original_moved_ids
    }
}

pub(super) fn batch_flattened_point_count(commands: &[PathCommand]) -> Option<usize> {
    let rings = flatten_closed_rings(commands);
    if rings.is_empty() || rings.iter().any(|ring| signed_ring_area(ring) <= 0.0) {
        return None;
    }
    // Concatenating NonZero paths is a union only when every candidate contributes non-negative
    // winding everywhere. Strictly positive rings prove that cheaply. Any hole, clockwise shell,
    // or degenerate contour takes the exact sequential path; containment alone is insufficient
    // because two overlapping negative rings can create negative winding that a later positive
    // candidate would cancel.
    rings
        .iter()
        .try_fold(0usize, |total, ring| total.checked_add(ring.len()))
}

/// Bake a Stage transform once so the existing normalized-fill batch can actually accept real
/// drag results. Merge fills are already planar parts; an affine transform preserves that fact.
pub(super) fn bake_batchable_moved_fill(element: Element) -> Element {
    if is_already_normalized_merge_part(&element)
        || !has_normalized_merge_part_kind(&element)
        || !is_merge_fill(&element)
    {
        return element;
    }
    if let Some((Some(fill), strokes)) = decompose_element_to_merge_parts(&element)
        && strokes.is_empty()
    {
        return fill;
    }
    element
}

/// Compatibility for a temporary compound cutter. The compound output is removed after it updates
/// other paint, so authored IDs and names may differ; presentation and dormant paint state must
/// still agree because they affect that one-pass interaction.
pub(super) fn merge_fill_batch_compatible(template: &Element, candidate: &Element) -> bool {
    // Filters, object scripts and accessibility operate at element boundaries; even identical
    // values on two disconnected shapes are not equivalent to one combined display object.
    let has_default_presentation = |element: &Element| {
        element.transform == Transform::default()
            && element.scripts.is_empty()
            && element.accessibility.is_none()
            && element.visible
            && !element.cache_as_bitmap
            && element.blend_mode.is_normal()
            && element.filters.is_empty()
    };
    if !has_default_presentation(template) || !has_default_presentation(candidate) {
        return false;
    }
    match (&template.kind, &candidate.kind) {
        (
            ElementKind::Path {
                fill_rule: template_rule,
                fill: template_fill,
                stroke: template_stroke,
                stroke_width: template_stroke_width,
                ..
            },
            ElementKind::Path {
                fill_rule: candidate_rule,
                fill: candidate_fill,
                stroke: candidate_stroke,
                stroke_width: candidate_stroke_width,
                ..
            },
        ) => {
            template_rule == candidate_rule
                && template_fill == candidate_fill
                && template_stroke == candidate_stroke
                && template_stroke_width == candidate_stroke_width
        }
        (
            ElementKind::PaintedPath {
                fill_rule: template_rule,
                fill: template_fill,
                stroke: template_stroke,
                fill_gradient: template_fill_gradient,
                stroke_gradient: template_stroke_gradient,
                stroke_width: template_stroke_width,
                ..
            },
            ElementKind::PaintedPath {
                fill_rule: candidate_rule,
                fill: candidate_fill,
                stroke: candidate_stroke,
                fill_gradient: candidate_fill_gradient,
                stroke_gradient: candidate_stroke_gradient,
                stroke_width: candidate_stroke_width,
                ..
            },
        ) => {
            template_rule == candidate_rule
                && template_fill == candidate_fill
                && template_stroke == candidate_stroke
                && template_fill_gradient == candidate_fill_gradient
                && template_stroke_gradient == candidate_stroke_gradient
                && template_stroke_width == candidate_stroke_width
        }
        _ => false,
    }
}

pub(super) fn batch_interacts_with_same_paint_fill(
    elements: &[Element],
    new_rings: &[Vec<[f64; 2]>],
    new_rule: FillRule,
    new_paint: &FillPaint,
) -> bool {
    let Some(new_bbox) = rings_bounds(new_rings) else {
        return false;
    };
    let is_bbox_candidate = |existing: &Element| {
        is_merge_fill(existing)
            && extract_fill_paint(&existing.kind).as_ref() == Some(new_paint)
            && element_fast_world_bounds(existing)
                .is_some_and(|bounds| bounds_overlap(bounds, new_bbox, SNAP_EPS * 2.0))
    };
    if !elements.iter().any(is_bbox_candidate) {
        return false;
    }
    let prepared = PreparedMergeRings::new(new_rings, new_rule);
    elements.iter().any(|existing| {
        if !is_bbox_candidate(existing) {
            return false;
        }
        let Some((ex_commands, ex_rule)) = element_world_commands(existing) else {
            return false;
        };
        let ex_rings = flatten_closed_rings(&ex_commands);
        let Some(ex_bbox) = rings_bounds(&ex_rings) else {
            return false;
        };
        bounds_overlap(ex_bbox, new_bbox, SNAP_EPS * 2.0)
            && rings_interact_with_prepared(
                &ex_rings,
                ex_rule,
                ex_bbox,
                &prepared.segments,
                &prepared.segment_bvh,
                &prepared.fill,
            )
    })
}

pub(super) fn apply_remerged_element(
    elements: &mut Vec<Element>,
    result_ids: &mut Vec<Id>,
    element: Element,
) -> bool {
    let already_dissolved = is_already_normalized_merge_part(&element)
        && matches!(
            element_world_commands(&element),
            Some((_, FillRule::NonZero))
        );
    let ids = apply_merge_shape_to_keyframe_inner(elements, element, already_dissolved);
    if ids.is_empty() {
        return false;
    }
    result_ids.extend(ids);
    true
}

pub(super) fn remerge_moved_elements_inner(
    elements: &mut Vec<Element>,
    moved_set: &std::collections::HashSet<&str>,
) -> Option<Vec<Id>> {
    let mut moved = Vec::new();
    let mut remaining = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if el.merge && moved_set.contains(el.id.as_str()) {
            moved.push(bake_batchable_moved_fill(el));
        } else {
            remaining.push(el);
        }
    }
    *elements = remaining;
    if moved.is_empty() {
        return Some(Vec::new());
    }
    // The first exact application would normalize these parts too. Doing it once up front makes
    // the same-paint interaction preflight match the subsequent compound pass without changing
    // the atomic outer transaction.
    ensure_existing_merge_normalized(elements);
    let batch_same_paint = moved.len() > 16;
    let mut result_ids = Vec::new();
    let mut idx = 0;
    while idx < moved.len() {
        if batch_same_paint
            && is_already_normalized_merge_part(&moved[idx])
            && is_merge_fill(&moved[idx])
            && let Some(fp) = extract_fill_paint(&moved[idx].kind)
            && let Some((mut combined_cmds, FillRule::NonZero)) =
                element_world_commands(&moved[idx])
            && combined_cmds.len() <= REMERGE_BATCH_MAX_COMMANDS
            && let Some(mut combined_points) = batch_flattened_point_count(&combined_cmds)
            && combined_points <= REMERGE_BATCH_MAX_POINTS
        {
            let mut end = idx + 1;
            while end < moved.len() {
                if !is_already_normalized_merge_part(&moved[end])
                    || !is_merge_fill(&moved[end])
                    || extract_fill_paint(&moved[end].kind).as_ref() != Some(&fp)
                    || !merge_fill_batch_compatible(&moved[idx], &moved[end])
                {
                    break;
                }
                let Some((cmds, FillRule::NonZero)) = element_world_commands(&moved[end]) else {
                    break;
                };
                let Some(points) = batch_flattened_point_count(&cmds) else {
                    break;
                };
                if combined_cmds.len().saturating_add(cmds.len()) > REMERGE_BATCH_MAX_COMMANDS
                    || combined_points.saturating_add(points) > REMERGE_BATCH_MAX_POINTS
                {
                    break;
                }
                combined_cmds.extend(cmds);
                combined_points += points;
                end += 1;
            }
            if end > idx + 1 {
                let batched = build_fill_element(
                    &moved[idx],
                    moved[idx].id.clone(),
                    combined_cmds,
                    FillRule::NonZero,
                    &fp,
                    true,
                );
                let (batch_commands, batch_rule) = element_world_commands(&batched)?;
                let batch_rings = flatten_closed_rings(&batch_commands);
                if batch_interacts_with_same_paint_fill(elements, &batch_rings, batch_rule, &fp) {
                    // A real same-paint union must choose identities through the exact existing
                    // order. Do not let the acceleration compound decide that policy.
                    for element in &mut moved[idx..end] {
                        if !apply_remerged_element(
                            elements,
                            &mut result_ids,
                            std::mem::take(element),
                        ) {
                            return None;
                        }
                    }
                    idx = end;
                    continue;
                }

                // Use the compound only as a single cutter against other paints and strokes. The
                // moved fills themselves remain separate authored elements with their original
                // IDs, names and order.
                let compound_id = batched.id.clone();
                let ids = apply_merge_shape_to_keyframe_inner(elements, batched, true);
                if ids.len() != 1 || ids[0] != compound_id {
                    return None;
                }
                let compound_position = elements
                    .iter()
                    .rposition(|element| element.id == compound_id)?;
                elements.remove(compound_position);
                for element in &mut moved[idx..end] {
                    let element = std::mem::take(element);
                    result_ids.push(element.id.clone());
                    elements.push(element);
                }
                idx = end;
                continue;
            }
        }
        let element = std::mem::take(&mut moved[idx]);
        if !apply_remerged_element(elements, &mut result_ids, element) {
            return None;
        }
        idx += 1;
    }
    Some(result_ids)
}

/// Find all Merge-mode stroke IDs on `elements` that lie along or touch the boundary of the
/// Merge-mode fill `fill_id` (used when double-clicking a Merge fill on stage).
pub fn merge_fill_adjacent_stroke_ids(elements: &[Element], fill_id: &str) -> Vec<Id> {
    let Some(fill_el) = elements
        .iter()
        .find(|e| e.id == fill_id && is_merge_fill(e))
    else {
        return Vec::new();
    };
    let Some((fill_cmds, fill_rule)) = element_world_commands(fill_el) else {
        return Vec::new();
    };
    let fill_rings = flatten_closed_rings(&fill_cmds);
    let fill_segs = fill_boundary_segments(&fill_rings, fill_rule);
    if fill_segs.is_empty() {
        return Vec::new();
    }
    let Some(fill_bbox) = segments_bounds(&fill_segs) else {
        return Vec::new();
    };
    let fill_bvh = SegmentBvh::from_segments(&fill_segs);

    let mut result = vec![fill_id.to_string()];
    for el in elements {
        if !is_merge_stroke(el) {
            continue;
        }
        let Some(el_bbox) = element_fast_world_bounds(el) else {
            continue;
        };
        if !bounds_overlap(el_bbox, fill_bbox, ADJACENT_TOL) {
            continue;
        }
        let Some((s_cmds, _)) = element_world_commands(el) else {
            continue;
        };
        let polys = flatten_stroke_polylines(&s_cmds);
        let touches = polys.iter().any(|poly| {
            poly.iter()
                .all(|&pt| fill_bvh.any_within_distance(&fill_segs, pt, ADJACENT_TOL))
        });
        if touches {
            result.push(el.id.clone());
        }
    }
    result
}

/// Find all Merge-mode stroke IDs on `elements` of the same stroke color/gradient connected to
/// `stroke_id` by shared endpoints (used when double-clicking a Merge stroke on stage).
pub fn merge_connected_stroke_ids(elements: &[Element], stroke_id: &str) -> Vec<Id> {
    let Some(target_paint) = elements
        .iter()
        .find(|el| el.id == stroke_id && is_merge_stroke(el))
        .and_then(|el| extract_stroke_paint(&el.kind))
        .map(|sp| (sp.color, sp.gradient))
    else {
        return Vec::new();
    };
    let mut strokes: Vec<(Id, Vec<[f64; 2]>)> = Vec::new();
    for el in elements {
        if !is_merge_stroke(el) {
            continue;
        }
        let Some(sp) = extract_stroke_paint(&el.kind) else {
            continue;
        };
        if (sp.color, sp.gradient) != target_paint {
            continue;
        }
        let Some((cmds, _)) = element_world_commands(el) else {
            continue;
        };
        let mut endpoints = Vec::new();
        for poly in flatten_stroke_polylines(&cmds) {
            if let (Some(&first), Some(&last)) = (poly.first(), poly.last()) {
                endpoints.push(first);
                endpoints.push(last);
            }
        }
        strokes.push((el.id.clone(), endpoints));
    }

    let Some(start_idx) = strokes.iter().position(|(id, _)| id == stroke_id) else {
        return Vec::new();
    };
    let mut visited = vec![false; strokes.len()];
    visited[start_idx] = true;
    let mut queue = std::collections::VecDeque::from([start_idx]);
    let mut result = Vec::new();

    while let Some(curr) = queue.pop_front() {
        result.push(strokes[curr].0.clone());
        for other in 0..strokes.len() {
            if visited[other] {
                continue;
            }
            let connected = strokes[curr].1.iter().any(|&a| {
                strokes[other]
                    .1
                    .iter()
                    .any(|&b| dist_sq(a, b) <= ADJACENT_TOL * ADJACENT_TOL)
            });
            if connected {
                visited[other] = true;
                queue.push_back(other);
            }
        }
    }
    result
}
