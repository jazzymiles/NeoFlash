use super::*;

pub(super) fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub(super) fn bez_coord(u: f64, p1: f64, p2: f64) -> f64 {
    let inv = 1.0 - u;
    3.0 * inv * inv * u * p1 + 3.0 * inv * u * u * p2 + u * u * u
}

pub(super) fn solve_bezier_x(target_x: f64, x1: f64, x2: f64) -> f64 {
    if target_x <= 0.0 {
        return 0.0;
    }
    if target_x >= 1.0 {
        return 1.0;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..64 {
        let mid = 0.5 * (low + high);
        if bez_coord(mid, x1, x2) < target_x {
            low = mid;
        } else {
            high = mid;
        }
    }
    0.5 * (low + high)
}

pub(super) fn subdivide_cubic_1d(p0: f64, p1: f64, p2: f64, p3: f64, s0: f64, s1: f64) -> [f64; 4] {
    let a01 = lerp_f64(p0, p1, s0);
    let a12 = lerp_f64(p1, p2, s0);
    let a23 = lerp_f64(p2, p3, s0);
    let a012 = lerp_f64(a01, a12, s0);
    let a123 = lerp_f64(a12, a23, s0);
    let a0123 = lerp_f64(a012, a123, s0);
    let (r0, r1, r2, r3) = (a0123, a123, a23, p3);
    let denom = 1.0 - s0;
    let u = if denom > 1e-12 {
        ((s1 - s0) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let b01 = lerp_f64(r0, r1, u);
    let b12 = lerp_f64(r1, r2, u);
    let b23 = lerp_f64(r2, r3, u);
    let b012 = lerp_f64(b01, b12, u);
    let b123 = lerp_f64(b12, b23, u);
    let b0123 = lerp_f64(b012, b123, u);
    [r0, b01, b012, b0123]
}

pub(super) fn subdivide_cubic_bezier(
    [x1, y1, x2, y2]: [f64; 4],
    t0: f64,
    t1: f64,
) -> Option<Easing> {
    if t1 - t0 <= 1e-9 {
        return None;
    }
    let s0 = solve_bezier_x(t0, x1, x2);
    let s1 = solve_bezier_x(t1, x1, x2);
    if s1 <= s0 {
        return None;
    }
    let xs = subdivide_cubic_1d(0.0, x1, x2, 1.0, s0, s1);
    let ys = subdivide_cubic_1d(0.0, y1, y2, 1.0, s0, s1);
    let dx = xs[3] - xs[0];
    let dy = ys[3] - ys[0];
    if dx <= 1e-9 || dy.abs() <= 1e-6 {
        return None;
    }
    let nx1 = ((xs[1] - xs[0]) / dx).clamp(0.0, 1.0);
    let nx2 = ((xs[2] - xs[0]) / dx).clamp(0.0, 1.0);
    let ny1 = (ys[1] - ys[0]) / dy;
    let ny2 = (ys[2] - ys[0]) / dy;
    if ![nx1, ny1, nx2, ny2].iter().all(|v| v.is_finite()) || ny1.abs() > 100.0 || ny2.abs() > 100.0
    {
        return None;
    }
    Some(Easing::CubicBezier {
        x1: nx1,
        y1: ny1,
        x2: nx2,
        y2: ny2,
    })
}

use crate::motion_tools::ExactMotionCurve;

pub(super) fn fit_cubic_bezier_samples(samples: &[(f64, f64)]) -> Option<Easing> {
    if samples.is_empty() {
        return Some(Easing::Linear);
    }
    let eval_fit = |x1: f64, x2: f64| -> (f64, f64, [f64; 4]) {
        let mut a00 = 0.0;
        let mut a01 = 0.0;
        let mut a11 = 0.0;
        let mut b0 = 0.0;
        let mut b1 = 0.0;
        let mut rows = Vec::with_capacity(samples.len());
        for &(x, target) in samples {
            let u = solve_bezier_x(x, x1, x2);
            let inv = 1.0 - u;
            let ca = 3.0 * inv * inv * u;
            let cb = 3.0 * inv * u * u;
            let rhs = target - u * u * u;
            rows.push((ca, cb, rhs));
            a00 += ca * ca;
            a01 += ca * cb;
            a11 += cb * cb;
            b0 += ca * rhs;
            b1 += cb * rhs;
        }
        let det = a00 * a11 - a01 * a01;
        let (raw_y1, raw_y2) = if det.abs() <= 1e-18 {
            let y1 = if a00 > 1e-18 { b0 / a00 } else { 1.0 / 3.0 };
            (y1, 2.0 / 3.0)
        } else {
            ((b0 * a11 - b1 * a01) / det, (a00 * b1 - a01 * b0) / det)
        };
        let y1 = raw_y1.clamp(-100.0, 100.0);
        let y2 = raw_y2.clamp(-100.0, 100.0);
        let mut sum_sq = 0.0;
        let mut max_err = 0.0_f64;
        for &(ca, cb, rhs) in &rows {
            let err = (ca * y1 + cb * y2 - rhs).abs();
            sum_sq += err * err;
            max_err = max_err.max(err);
        }
        (sum_sq, max_err, [x1, y1, x2, y2])
    };

    let mut best = eval_fit(1.0 / 3.0, 2.0 / 3.0);
    for g1 in [0.05, 0.15, 0.3, 0.5, 0.7, 0.85, 0.98] {
        for g2 in [0.05, 0.2, 0.4, 0.6, 0.8, 0.95, 0.999] {
            let cand = eval_fit(g1, g2);
            if cand.0 < best.0 {
                best = cand;
            }
        }
    }
    let (mut x1, _, mut x2, _) = (best.2[0], best.2[1], best.2[2], best.2[3]);
    let mut step = 0.08;
    for _ in 0..36 {
        let mut improved = false;
        for (dx1, dx2) in [
            (step, 0.0),
            (-step, 0.0),
            (0.0, step),
            (0.0, -step),
            (step, step),
            (-step, -step),
            (step, -step),
            (-step, step),
        ] {
            let nx1 = (x1 + dx1).clamp(0.0, 1.0);
            let nx2 = (x2 + dx2).clamp(0.0, 1.0);
            let cand = eval_fit(nx1, nx2);
            if cand.0 < best.0 {
                best = cand;
                x1 = nx1;
                x2 = nx2;
                improved = true;
            }
        }
        if !improved {
            step *= 0.5;
            if step < 1e-7 {
                break;
            }
        }
    }
    let [fx1, fy1, fx2, fy2] = best.2;
    if ![fx1, fy1, fx2, fy2].iter().all(|v| v.is_finite()) || best.1 > 5e-3 {
        return None;
    }
    Some(Easing::CubicBezier {
        x1: fx1,
        y1: fy1,
        x2: fx2,
        y2: fy2,
    })
}

pub(super) fn subdivide_left_single_motion_ease(
    ease: Easing,
    left_frame_count: u32,
    eval_span_len: f64,
) -> Option<Easing> {
    if matches!(ease, Easing::Bounce | Easing::Elastic) || left_frame_count < 2 {
        return None;
    }
    if ease == Easing::Linear || left_frame_count == 2 {
        return Some(Easing::Linear);
    }
    let n = left_frame_count as f64;
    let d_last = (left_frame_count - 1) as f64;
    let e_last = easing(d_last / eval_span_len.max(1.0), ease);
    if e_last.abs() <= 1e-6 {
        return None;
    }
    let scale = d_last / n;
    let mut samples = Vec::with_capacity((left_frame_count - 1) as usize);
    for k in 1..left_frame_count {
        let x = k as f64 / n;
        let frac = easing(k as f64 / eval_span_len.max(1.0), ease) / e_last;
        samples.push((x, scale * frac));
    }
    fit_cubic_bezier_samples(&samples)
}

pub(super) fn subdivide_single_ease(ease: Easing, t0: f64, t1: f64) -> Option<Easing> {
    if t1 - t0 <= 1e-9 {
        return None;
    }
    if (t0 - 0.0).abs() <= 1e-9 && (t1 - 1.0).abs() <= 1e-9 {
        return Some(ease);
    }
    match ease {
        Easing::Linear => Some(Easing::Linear),
        Easing::EaseIn => subdivide_cubic_bezier([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0], t0, t1),
        Easing::EaseOut => subdivide_cubic_bezier([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0], t0, t1),
        Easing::Flash { acceleration } => {
            let a = (acceleration / 100.0).clamp(-1.0, 1.0);
            subdivide_cubic_bezier(
                [1.0 / 3.0, (1.0 + a) / 3.0, 2.0 / 3.0, (2.0 + a) / 3.0],
                t0,
                t1,
            )
        }
        Easing::CubicBezier { x1, y1, x2, y2 } => subdivide_cubic_bezier([x1, y1, x2, y2], t0, t1),
        Easing::EaseInOut => {
            if t1 <= 0.5 {
                subdivide_cubic_bezier([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0], 2.0 * t0, 2.0 * t1)
            } else if t0 >= 0.5 {
                subdivide_cubic_bezier(
                    [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0],
                    2.0 * t0 - 1.0,
                    2.0 * t1 - 1.0,
                )
            } else {
                subdivide_cubic_bezier([0.42, 0.0, 0.58, 1.0], t0, t1)
            }
        }
        Easing::Bounce | Easing::Elastic => None,
    }
}

pub(super) type SubdividedMotionEases = (
    Easing,
    Vec<(TweenProperty, Easing)>,
    Vec<MotionTrackOverride>,
);

pub(super) fn subdivide_motion_eases(
    first_key: &Keyframe,
    t0: f64,
    t1: f64,
) -> Option<SubdividedMotionEases> {
    let base = subdivide_single_ease(first_key.tween?, t0, t1)?;
    let mut prop_eases = Vec::with_capacity(first_key.property_eases.len());
    for &(prop, ease) in &first_key.property_eases {
        prop_eases.push((prop, subdivide_single_ease(ease, t0, t1)?));
    }
    let mut tracks = first_key.motion_tracks.clone();
    for track in &mut tracks {
        if let Some(ease) = track.easing {
            track.easing = Some(subdivide_single_ease(ease, t0, t1)?);
        }
        if let Some(property_eases) = &mut track.property_eases {
            for (_, ease) in property_eases {
                *ease = subdivide_single_ease(*ease, t0, t1)?;
            }
        }
    }
    Some((base, prop_eases, tracks))
}

pub(super) fn subdivide_left_motion_eases(
    first_key: &Keyframe,
    left_frame_count: u32,
    eval_span_len: f64,
) -> Option<SubdividedMotionEases> {
    let base =
        subdivide_left_single_motion_ease(first_key.tween?, left_frame_count, eval_span_len)?;
    let mut prop_eases = Vec::with_capacity(first_key.property_eases.len());
    for &(prop, ease) in &first_key.property_eases {
        prop_eases.push((
            prop,
            subdivide_left_single_motion_ease(ease, left_frame_count, eval_span_len)?,
        ));
    }
    let mut tracks = first_key.motion_tracks.clone();
    for track in &mut tracks {
        if let Some(ease) = track.easing {
            track.easing = Some(subdivide_left_single_motion_ease(
                ease,
                left_frame_count,
                eval_span_len,
            )?);
        }
        if let Some(property_eases) = &mut track.property_eases {
            for (_, ease) in property_eases {
                *ease = subdivide_left_single_motion_ease(*ease, left_frame_count, eval_span_len)?;
            }
        }
    }
    Some((base, prop_eases, tracks))
}

pub(super) fn has_object_motion_path(key: &Keyframe) -> bool {
    key.motion_path.is_some()
        || key
            .motion_tracks
            .iter()
            .any(|track| matches!(track.motion_path, Some(MotionPathOverride::Path { .. })))
}

pub(super) fn has_noninvertible_motion_ease(key: &Keyframe) -> bool {
    matches!(key.tween, Some(Easing::Bounce | Easing::Elastic))
        || key
            .property_eases
            .iter()
            .any(|(_, ease)| matches!(ease, Easing::Bounce | Easing::Elastic))
        || key.motion_tracks.iter().any(|track| {
            matches!(track.easing, Some(Easing::Bounce | Easing::Elastic))
                || track.property_eases.as_ref().is_some_and(|property_eases| {
                    property_eases
                        .iter()
                        .any(|(_, ease)| matches!(ease, Easing::Bounce | Easing::Elastic))
                })
        })
}

pub(super) fn has_roving_motion_keys(layer: &Layer, start_idx: usize, end_frame: u32) -> bool {
    layer.keyframes[start_idx + 1..]
        .iter()
        .take_while(|key| key.frame < end_frame && key.keyed_properties.is_some())
        .any(|key| {
            key.roving
                || key.motion_keys.as_ref().is_some_and(|object_keys| {
                    object_keys.iter().any(|object_key| object_key.roving)
                })
        })
}

pub(super) fn effective_motion_ease_on_start(
    start: &Keyframe,
    element_id: &str,
    property: TweenProperty,
) -> Easing {
    if let Some(track) = start.motion_track(element_id) {
        if let Some(property_eases) = &track.property_eases
            && let Some((_, ease)) = property_eases.iter().find(|(p, _)| *p == property)
        {
            return *ease;
        }
        if let Some(ease) = track.easing {
            return ease;
        }
    }
    start
        .property_eases
        .iter()
        .find(|(p, _)| *p == property)
        .map(|(_, ease)| *ease)
        .or(start.tween)
        .unwrap_or(Easing::Linear)
}

pub(super) fn ease_stays_within_unit_range(ease: Easing) -> bool {
    match ease {
        Easing::CubicBezier { y1, y2, .. } => {
            (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2)
        }
        Easing::Bounce | Easing::Elastic => false,
        _ => true,
    }
}

pub(super) fn curved_tracks_compatible_with_subdivision(
    first_key: &Keyframe,
    all_linear: bool,
) -> bool {
    if all_linear {
        return true;
    }
    if first_key.motion_path.is_some() {
        let Some(ref_el) = first_key.elements.first() else {
            return false;
        };
        let xe = effective_motion_ease_on_start(first_key, &ref_el.id, TweenProperty::X);
        let ye = effective_motion_ease_on_start(first_key, &ref_el.id, TweenProperty::Y);
        if xe != ye || !ease_stays_within_unit_range(xe) {
            return false;
        }
        for el in first_key.elements.iter().skip(1) {
            if matches!(
                first_key.motion_path_for(&el.id),
                MotionPathSource::Legacy(_)
            ) {
                let ex = effective_motion_ease_on_start(first_key, &el.id, TweenProperty::X);
                let ey = effective_motion_ease_on_start(first_key, &el.id, TweenProperty::Y);
                if ex != xe || ey != ye {
                    return false;
                }
            }
        }
    }
    for track in &first_key.motion_tracks {
        if matches!(track.motion_path, Some(MotionPathOverride::Path { .. })) {
            let xe = effective_motion_ease_on_start(first_key, &track.element_id, TweenProperty::X);
            let ye = effective_motion_ease_on_start(first_key, &track.element_id, TweenProperty::Y);
            if xe != ye || !ease_stays_within_unit_range(xe) {
                return false;
            }
        }
    }
    true
}

pub(super) fn subdivide_single_motion_curve(
    commands: &[PathCommand],
    reference_id: &str,
    original: &Layer,
    first_key: &Keyframe,
    eval_end: u32,
    sub_waypoint_frames: &[u32],
) -> Option<Option<Vec<PathCommand>>> {
    let first = first_key.frame;
    let orig_waypoints = motion_path_waypoints(original, first, reference_id);
    if orig_waypoints.len() < 2 || sub_waypoint_frames.len() < 2 {
        return Some(None);
    }
    motion_path_polyline(commands)?;
    let curve = ExactMotionCurve::from_commands(commands)?;
    let orig_lengths: Vec<f64> = if commands.len() == orig_waypoints.len() {
        curve.cumulative_segment_lengths().to_vec()
    } else {
        let mut lens = Vec::with_capacity(orig_waypoints.len());
        lens.push(0.0);
        lens.extend(
            orig_waypoints[1..orig_waypoints.len() - 1]
                .iter()
                .map(|(_, p)| curve.nearest(*p)),
        );
        lens.push(curve.length());
        lens
    };
    let s_at_time = |time: f64| -> f64 {
        if time <= orig_waypoints[0].0 as f64 {
            return orig_lengths[0];
        }
        let idx = orig_waypoints
            .partition_point(|(f, _)| (*f as f64) <= time)
            .saturating_sub(1);
        if idx + 1 >= orig_waypoints.len() {
            return *orig_lengths.last().unwrap_or(&0.0);
        }
        let dt = ((orig_waypoints[idx + 1].0 - orig_waypoints[idx].0).max(1)) as f64;
        let frac = ((time - orig_waypoints[idx].0 as f64) / dt).clamp(0.0, 1.0);
        lerp_f64(orig_lengths[idx], orig_lengths[idx + 1], frac)
    };
    let x_ease = effective_motion_ease_on_start(first_key, reference_id, TweenProperty::X);
    let eval_len = (eval_end - first).max(1) as f64;
    let arc_targets: Vec<f64> = sub_waypoint_frames
        .iter()
        .map(|&f| {
            let prog = ((f.saturating_sub(first)) as f64 / eval_len).clamp(0.0, 1.0);
            let eff_t = first as f64 + easing(prog, x_ease) * eval_len;
            s_at_time(eff_t)
        })
        .collect();
    let s_first = *arc_targets.first()?;
    let s_last = *arc_targets.last()?;
    let out = if commands.len() == orig_waypoints.len() {
        let mut combined = Vec::with_capacity(sub_waypoint_frames.len());
        let mut single_per_interval = true;
        for pair in arc_targets.windows(2) {
            let sub = curve.subdivide_arc_range(pair[0], pair[1])?;
            if sub.len() != 2 {
                single_per_interval = false;
                break;
            }
            if combined.is_empty() {
                combined.push(sub[0]);
            }
            combined.push(sub[1]);
        }
        if single_per_interval && combined.len() >= 2 {
            combined
        } else {
            curve.subdivide_arc_range(s_first, s_last)?
        }
    } else {
        curve.subdivide_arc_range(s_first, s_last)?
    };
    motion_path_polyline(&out)?;
    Some(Some(out))
}

pub(super) fn collect_subspan_pos_waypoints(
    sub_start_frame: u32,
    sub_prop_keys: &[Keyframe],
    element_id: &str,
) -> Vec<u32> {
    let mut frames = vec![sub_start_frame];
    for key in sub_prop_keys {
        let kp = key.motion_key_properties_for(element_id);
        if (kp.contains(&TweenProperty::X) || kp.contains(&TweenProperty::Y))
            && key.elements.iter().any(|e| e.id == element_id)
        {
            frames.push(key.frame);
        }
    }
    frames
}

pub(super) fn subdivide_keyframe_motion_paths(
    sub_start: &mut Keyframe,
    sub_props: &[Keyframe],
    original: &Layer,
    first_key: &Keyframe,
    eval_end: u32,
) -> Option<()> {
    if let Some(legacy_cmds) = &first_key.motion_path {
        let ref_el = first_key.elements.first()?;
        let wps = collect_subspan_pos_waypoints(sub_start.frame, sub_props, &ref_el.id);
        sub_start.motion_path = subdivide_single_motion_curve(
            legacy_cmds,
            &ref_el.id,
            original,
            first_key,
            eval_end,
            &wps,
        )?;
    }
    let inherits_legacy = sub_start.motion_path.is_some();
    for track in &mut sub_start.motion_tracks {
        let orig_override = first_key
            .motion_track(&track.element_id)
            .and_then(|t| t.motion_path.as_ref());
        if let Some(MotionPathOverride::Path { commands }) = orig_override {
            let wps = collect_subspan_pos_waypoints(sub_start.frame, sub_props, &track.element_id);
            track.motion_path = match subdivide_single_motion_curve(
                commands,
                &track.element_id,
                original,
                first_key,
                eval_end,
                &wps,
            )? {
                Some(sub_cmds) => Some(MotionPathOverride::Path { commands: sub_cmds }),
                None => inherits_legacy.then_some(MotionPathOverride::Disabled),
            };
        }
    }
    Some(())
}

/// Subdivide a classic tween `[first, last]` onto `[sub_start, sub_end]`, returning
/// the tweened start key(s) in `sub_start..sub_end` (excluding the endpoint key at `sub_end`).
pub(super) fn subdivide_classic_subrange(
    sampling: TimelineSampleContext<'_>,
    original: &Layer,
    first_key: &Keyframe,
    first: u32,
    last: u32,
    sub_start: u32,
    sub_end: u32,
) -> Option<Vec<Keyframe>> {
    if first_key.tween_kind != TweenKind::Classic
        || original.properties.motion_guide.is_some()
        || sub_start >= sub_end
        || sub_start < first
        || sub_end > last
    {
        return None;
    }
    let ease = first_key.tween?;
    if matches!(ease, Easing::Bounce | Easing::Elastic) {
        return None;
    }
    if sub_start == first && sub_end == last {
        return Some(vec![first_key.clone()]);
    }
    if first_key.classic_rotate_turns > 0
        || first_key.classic_rotate_dir != ClassicRotateDirection::Auto
    {
        return None;
    }
    let make_start_key = |frame: u32, tween: Easing| -> Keyframe {
        if frame == first {
            let mut k = first_key.clone();
            k.tween = Some(tween);
            k
        } else {
            let mut k = sampled_key(sampling.timeline, original, frame, sampling.fps);
            k.tween = Some(tween);
            k.tween_kind = TweenKind::Classic;
            k.orient_to_path = first_key.orient_to_path;
            k.tween_scale = first_key.tween_scale;
            k.sync_symbols = first_key.sync_symbols;
            k.snap_to_guide = first_key.snap_to_guide;
            k
        }
    };
    if sub_end - sub_start == 1 {
        return Some(vec![make_start_key(sub_start, Easing::Linear)]);
    }
    let len = (last - first) as f64;
    let (t0, t1) = (
        (sub_start - first) as f64 / len,
        (sub_end - first) as f64 / len,
    );
    match ease {
        Easing::Linear => Some(vec![make_start_key(sub_start, Easing::Linear)]),
        Easing::Bounce | Easing::Elastic => None,
        Easing::EaseIn => {
            let sub = subdivide_cubic_bezier([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0], t0, t1)?;
            Some(vec![make_start_key(sub_start, sub)])
        }
        Easing::EaseOut => {
            let sub = subdivide_cubic_bezier([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0], t0, t1)?;
            Some(vec![make_start_key(sub_start, sub)])
        }
        Easing::Flash { acceleration } => {
            let a = (acceleration / 100.0).clamp(-1.0, 1.0);
            let sub = subdivide_cubic_bezier(
                [1.0 / 3.0, (1.0 + a) / 3.0, 2.0 / 3.0, (2.0 + a) / 3.0],
                t0,
                t1,
            )?;
            Some(vec![make_start_key(sub_start, sub)])
        }
        Easing::CubicBezier { x1, y1, x2, y2 } => {
            let sub = subdivide_cubic_bezier([x1, y1, x2, y2], t0, t1)?;
            Some(vec![make_start_key(sub_start, sub)])
        }
        Easing::EaseInOut => {
            let mut cuts = vec![sub_start];
            if sub_start * 2 < first + last && sub_end * 2 > first + last {
                if (first + last).is_multiple_of(2) {
                    let mid = (first + last) / 2;
                    if mid > sub_start && mid < sub_end {
                        cuts.push(mid);
                    }
                } else {
                    let m0 = (first + last) / 2;
                    let m1 = m0 + 1;
                    if m0 > sub_start && m0 < sub_end {
                        cuts.push(m0);
                    }
                    if m1 > sub_start && m1 < sub_end {
                        cuts.push(m1);
                    }
                }
            }
            cuts.push(sub_end);
            let mut out = Vec::with_capacity(cuts.len() - 1);
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if b - a == 1 {
                    out.push(make_start_key(a, Easing::Linear));
                } else if b * 2 <= first + last {
                    let u0 = 2.0 * (a - first) as f64 / len;
                    let u1 = 2.0 * (b - first) as f64 / len;
                    let sub =
                        subdivide_cubic_bezier([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0], u0, u1)?;
                    out.push(make_start_key(a, sub));
                } else {
                    let u0 = 2.0 * (a - first) as f64 / len - 1.0;
                    let u1 = 2.0 * (b - first) as f64 / len - 1.0;
                    let sub =
                        subdivide_cubic_bezier([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0], u0, u1)?;
                    out.push(make_start_key(a, sub));
                }
            }
            Some(out)
        }
    }
}

pub(super) fn charge_bake_budget(
    original: &Layer,
    first: u32,
    last: u32,
    budget: &mut usize,
) -> Result<(), String> {
    if last <= first {
        return Ok(());
    }
    let records = (last - first) as usize
        * original
            .key_at(first)
            .map_or(1, |key| key.elements.len().max(1));
    *budget = budget
        .checked_add(records)
        .ok_or("Frame edit is too large.")?;
    if *budget > MAX_BAKED_RECORDS {
        return Err("This edit would bake more than 100000 frame/object records. Split the tween into shorter sections first.".into());
    }
    Ok(())
}

/// Isolate `[cut_start, cut_end)` from surrounding tween spans while preserving editable
/// subdivided curves outside `[cut_start, cut_end)` (and baking `[cut_start, cut_end)`
/// only when `bake_interior` is requested or a non-subdividable span must fall back).
pub(super) fn isolate_range_impl(
    timeline: &Timeline,
    layer: &mut Layer,
    cut_start: u32,
    cut_end: u32,
    bake_interior: bool,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    let sampling = TimelineSampleContext { timeline, fps };
    layer.keyframes.sort_by_key(|key| key.frame);
    let original = layer.clone();
    let effective_end = exposure(&original, timeline.frame_count);

    enum SpanKind {
        Motion {
            start_idx: usize,
            end_frame: u32,
            eval_end: u32,
            last_prop: u32,
        },
        ClassicOrShape {
            start_idx: usize,
            last_frame: u32,
        },
    }
    let mut spans = Vec::new();
    let mut i = 0;
    while i < original.keyframes.len() {
        let key = &original.keyframes[i];
        if key.keyed_properties.is_none() && key.tween.is_some() {
            if key.tween_kind == TweenKind::Motion {
                let mut j = i + 1;
                while j < original.keyframes.len()
                    && original.keyframes[j].keyed_properties.is_some()
                {
                    j += 1;
                }
                let next_ordinary = original.keyframes.get(j).map(|k| k.frame);
                let last_prop = if j > i + 1 {
                    original.keyframes[j - 1].frame
                } else {
                    key.frame
                };
                let eval_end = next_ordinary
                    .or(original.properties.end_frame)
                    .unwrap_or(last_prop)
                    .max(last_prop)
                    .max(key.frame + 1);
                let end_frame = next_ordinary.unwrap_or(effective_end).max(key.frame + 1);
                spans.push(SpanKind::Motion {
                    start_idx: i,
                    end_frame,
                    eval_end,
                    last_prop,
                });
                i = j;
                continue;
            } else if let Some(next) = original.keyframes.get(i + 1) {
                spans.push(SpanKind::ClassicOrShape {
                    start_idx: i,
                    last_frame: next.frame,
                });
            }
        }
        i += 1;
    }

    for span in spans {
        match span {
            SpanKind::ClassicOrShape {
                start_idx,
                last_frame: last,
            } => {
                let first_key = &original.keyframes[start_idx];
                let first = first_key.frame;
                let touches_span = if bake_interior
                    || cut_start == cut_end
                    || first_key.tween_kind == TweenKind::Shape
                {
                    first < cut_end && last > cut_start
                } else {
                    first < cut_end && last >= cut_start
                };
                if !touches_span {
                    continue;
                }
                let build_subdivided = || -> Option<Vec<Keyframe>> {
                    let mut replacement = Vec::new();
                    if first < cut_start {
                        let left_end = cut_start - 1;
                        if left_end == first {
                            let mut k = first_key.clone();
                            k.tween = None;
                            replacement.push(k);
                        } else {
                            replacement.extend(subdivide_classic_subrange(
                                sampling, &original, first_key, first, last, first, left_end,
                            )?);
                            let mut end_k = sampled_key(timeline, &original, left_end, fps);
                            end_k.tween = None;
                            replacement.push(end_k);
                        }
                    }
                    if bake_interior {
                        let b0 = first.max(cut_start);
                        let b1 = last.min(cut_end);
                        for f in b0..b1 {
                            replacement.push(sampled_key(timeline, &original, f, fps));
                        }
                    }
                    if cut_end < last {
                        replacement.extend(subdivide_classic_subrange(
                            sampling, &original, first_key, first, last, cut_end, last,
                        )?);
                    }
                    Some(replacement)
                };
                if let Some(replacement) = build_subdivided() {
                    if bake_interior {
                        charge_bake_budget(
                            &original,
                            first.max(cut_start),
                            last.min(cut_end),
                            budget,
                        )?;
                    }
                    layer
                        .keyframes
                        .retain(|key| key.frame < first || key.frame >= last);
                    layer.keyframes.extend(replacement);
                } else {
                    charge_bake_budget(&original, first, last, budget)?;
                    layer
                        .keyframes
                        .retain(|key| key.frame < first || key.frame >= last);
                    layer.keyframes.extend(
                        (first..last).map(|frame| sampled_key(timeline, &original, frame, fps)),
                    );
                }
            }
            SpanKind::Motion {
                start_idx,
                end_frame,
                eval_end,
                last_prop,
            } => {
                let first_key = &original.keyframes[start_idx];
                let first = first_key.frame;
                if !(first < cut_end && end_frame > cut_start) {
                    continue;
                }
                let all_linear = first_key.tween == Some(Easing::Linear)
                    && first_key
                        .property_eases
                        .iter()
                        .all(|(_, e)| *e == Easing::Linear)
                    && first_key.motion_tracks.iter().all(|track| {
                        track.easing.is_none_or(|ease| ease == Easing::Linear)
                            && track.property_eases.as_ref().is_none_or(|property_eases| {
                                property_eases
                                    .iter()
                                    .all(|(_, ease)| *ease == Easing::Linear)
                            })
                    });
                let has_interior_props = original.keyframes[start_idx + 1..]
                    .iter()
                    .take_while(|key| key.frame < end_frame && key.keyed_properties.is_some())
                    .any(|key| key.frame < last_prop);
                let eval_span_len = (eval_end - first).max(1) as f64;
                let build_motion = || -> Option<Vec<Keyframe>> {
                    if original.properties.motion_guide.is_some()
                        || first_key.property_expressions.iter().any(|e| e.enabled)
                        || has_roving_motion_keys(&original, start_idx, end_frame)
                        || !curved_tracks_compatible_with_subdivision(first_key, all_linear)
                    {
                        return None;
                    }
                    if !all_linear && (has_interior_props || last_prop != eval_end) {
                        return None;
                    }
                    let mut replacement = Vec::new();
                    if first < cut_start {
                        let left_last = cut_start - 1;
                        if left_last == first {
                            let mut k = first_key.clone();
                            k.tween = None;
                            k.tween_kind = TweenKind::Classic;
                            k.property_eases.clear();
                            k.motion_tracks.clear();
                            k.motion_keys = None;
                            k.motion_path = None;
                            k.sync_symbols = false;
                            k.snap_to_guide = false;
                            k.orient_to_path = false;
                            replacement.push(k);
                        } else {
                            let mut left_start = first_key.clone();
                            if !all_linear {
                                let (sub_tween, sub_props, sub_tracks) =
                                    subdivide_left_motion_eases(
                                        first_key,
                                        cut_start - first,
                                        eval_span_len,
                                    )?;
                                left_start.tween = Some(sub_tween);
                                left_start.property_eases = sub_props;
                                left_start.motion_tracks = sub_tracks;
                            }
                            let mut left_props = Vec::new();
                            for k in &original.keyframes[start_idx + 1..] {
                                if k.frame >= end_frame || k.keyed_properties.is_none() {
                                    break;
                                }
                                if k.frame > first && k.frame < left_last {
                                    left_props.push(k.clone());
                                }
                            }
                            let mut end_prop = original
                                .keyframes
                                .iter()
                                .find(|k| k.frame == left_last)
                                .cloned()
                                .unwrap_or_else(|| {
                                    sampled_key(timeline, &original, left_last, fps)
                                });
                            end_prop.elements =
                                sample_layer_in_at_fps(timeline, &original, left_last, fps);
                            for el in &mut end_prop.elements {
                                if left_start.motion_orient_to_path_for(&el.id)
                                    && let Some(base_rot) =
                                        crate::evaluate::sample_motion_base_rotation(
                                            &original, left_last, &el.id,
                                        )
                                {
                                    el.transform.rotation = base_rot;
                                }
                            }
                            end_prop.tween = left_start.tween;
                            end_prop.tween_kind = TweenKind::Motion;
                            end_prop.keyed_properties = Some(TweenProperty::ALL.to_vec());
                            if !left_start.motion_tracks.is_empty()
                                || original.keyframes[start_idx + 1..]
                                    .iter()
                                    .take_while(|key| {
                                        key.frame < end_frame && key.keyed_properties.is_some()
                                    })
                                    .any(|key| key.motion_keys.is_some())
                            {
                                end_prop.motion_keys = Some(
                                    end_prop
                                        .elements
                                        .iter()
                                        .map(|element| MotionPropertyKey {
                                            element_id: element.id.clone(),
                                            properties: TweenProperty::ALL.to_vec(),
                                            roving: false,
                                        })
                                        .collect(),
                                );
                            } else {
                                end_prop.motion_keys = None;
                            }
                            end_prop.motion_tracks.clear();
                            end_prop.motion_path = None;
                            end_prop.property_eases.clear();
                            end_prop.sync_symbols = false;
                            end_prop.snap_to_guide = false;
                            left_props.push(end_prop);
                            subdivide_keyframe_motion_paths(
                                &mut left_start,
                                &left_props,
                                &original,
                                first_key,
                                eval_end,
                            )?;
                            replacement.push(left_start);
                            replacement.extend(left_props);
                        }
                    }
                    if bake_interior {
                        let b0 = first.max(cut_start);
                        let b1 = end_frame.min(cut_end);
                        for f in b0..b1 {
                            replacement.push(sampled_key(timeline, &original, f, fps));
                        }
                    }
                    if cut_end < end_frame {
                        if cut_end + 1 >= end_frame {
                            let mut k = sampled_key(timeline, &original, cut_end, fps);
                            if let Some(old) =
                                original.keyframes.iter().find(|old| old.frame == cut_end)
                            {
                                k.sound = old.sound.clone();
                            }
                            replacement.push(k);
                        } else {
                            let mut right_start = sampled_key(timeline, &original, cut_end, fps);
                            if let Some(old) =
                                original.keyframes.iter().find(|old| old.frame == cut_end)
                            {
                                right_start.sound = old.sound.clone();
                            }
                            let mut right_props: Vec<Keyframe> = original.keyframes
                                [start_idx + 1..]
                                .iter()
                                .take_while(|k| k.frame < end_frame && k.keyed_properties.is_some())
                                .filter(|k| k.frame > cut_end)
                                .cloned()
                                .collect();
                            if right_props.is_empty() {
                                replacement.push(right_start);
                            } else {
                                let (right_tween, right_prop_eases, right_tracks) = if !all_linear {
                                    let (sub_t, sub_p, sub_tracks) = subdivide_motion_eases(
                                        first_key,
                                        (cut_end - first) as f64 / eval_span_len,
                                        1.0,
                                    )?;
                                    (Some(sub_t), sub_p, sub_tracks)
                                } else {
                                    (
                                        first_key.tween,
                                        first_key.property_eases.clone(),
                                        first_key.motion_tracks.clone(),
                                    )
                                };
                                right_start.tween = right_tween;
                                right_start.tween_kind = TweenKind::Motion;
                                right_start.keyed_properties = None;
                                right_start.orient_to_path = first_key.orient_to_path;
                                right_start.property_eases = right_prop_eases;
                                right_start.motion_tracks = right_tracks;
                                right_start.motion_keys = None;
                                right_start.sync_symbols = first_key.sync_symbols;
                                right_start.snap_to_guide = first_key.snap_to_guide;
                                for el in &right_start.elements {
                                    if right_start.motion_orient_to_path_for(&el.id)
                                        && let Some(base_cut_rot) =
                                            crate::evaluate::sample_motion_base_rotation(
                                                &original, cut_end, &el.id,
                                            )
                                    {
                                        let delta_rot = el.transform.rotation - base_cut_rot;
                                        for rk in &mut right_props {
                                            if rk
                                                .motion_key_properties_for(&el.id)
                                                .contains(&TweenProperty::Rotation)
                                                && let Some(rel) =
                                                    rk.elements.iter_mut().find(|e| e.id == el.id)
                                            {
                                                rel.transform.rotation += delta_rot;
                                            }
                                        }
                                    }
                                }
                                subdivide_keyframe_motion_paths(
                                    &mut right_start,
                                    &right_props,
                                    &original,
                                    first_key,
                                    eval_end,
                                )?;
                                replacement.push(right_start);
                                replacement.extend(right_props);
                            }
                        }
                    }
                    Some(replacement)
                };
                if let Some(replacement) = build_motion() {
                    if bake_interior {
                        charge_bake_budget(
                            &original,
                            first.max(cut_start),
                            end_frame.min(cut_end),
                            budget,
                        )?;
                    }
                    layer
                        .keyframes
                        .retain(|key| key.frame < first || key.frame >= end_frame);
                    layer.keyframes.extend(replacement);
                } else {
                    charge_bake_budget(&original, first, end_frame, budget)?;
                    layer
                        .keyframes
                        .retain(|key| key.frame < first || key.frame >= end_frame);
                    layer.keyframes.extend(
                        (first..end_frame)
                            .map(|frame| sampled_key(timeline, &original, frame, fps)),
                    );
                }
            }
        }
    }
    layer.keyframes.sort_by_key(|key| key.frame);
    Ok(())
}

pub(super) fn isolate_range(
    timeline: &Timeline,
    layer: &mut Layer,
    cut_start: u32,
    cut_end: u32,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    isolate_range_impl(timeline, layer, cut_start, cut_end, false, budget, fps)
}

/// Used by `copy_frames`, which strips `key.tween = None` on copied keyframes and
/// therefore needs integer-frame poses inside `[start, end)`.
pub(super) fn bake_touched(
    timeline: &Timeline,
    layer: &mut Layer,
    start: u32,
    end: u32,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    isolate_range_impl(timeline, layer, start, end, true, budget, fps)
}
pub(super) fn split(timeline: &Timeline, layer: &mut Layer, frame: u32, fps: f64) {
    if frame >= exposure(layer, timeline.frame_count)
        || layer
            .keyframes
            .iter()
            .any(|key| key.frame == frame && key.keyed_properties.is_none())
    {
        return;
    }
    let key = sampled_key(timeline, layer, frame, fps);
    layer.keyframes.retain(|k| k.frame != frame);
    layer.keyframes.push(key);
    layer.keyframes.sort_by_key(|key| key.frame);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn split_span_at(
    timeline: &Timeline,
    layer: &mut Layer,
    start_pos: usize,
    span_start: u32,
    span_end: u32,
    at: u32,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    let first_key = layer.keyframes[start_pos].clone();
    if first_key.tween.is_none() {
        split(timeline, layer, at, fps);
        return Ok(());
    }
    match first_key.tween_kind {
        TweenKind::Motion => {
            isolate_range(timeline, layer, at, at, budget, fps)?;
            split(timeline, layer, at, fps);
            Ok(())
        }
        TweenKind::Classic => {
            let has_end_key = layer
                .keyframes
                .iter()
                .any(|k| k.frame == span_end && k.keyed_properties.is_none());
            if !has_end_key {
                layer.keyframes[start_pos].tween = None;
                split(timeline, layer, at, fps);
                return Ok(());
            }
            let sampling = TimelineSampleContext { timeline, fps };
            let original = layer.clone();
            if let (Some(left_keys), Some(right_keys)) = (
                subdivide_classic_subrange(
                    sampling, &original, &first_key, span_start, span_end, span_start, at,
                ),
                subdivide_classic_subrange(
                    sampling, &original, &first_key, span_start, span_end, at, span_end,
                ),
            ) {
                layer
                    .keyframes
                    .retain(|k| k.frame < span_start || k.frame >= span_end);
                layer.keyframes.extend(left_keys);
                layer.keyframes.extend(right_keys);
                layer.keyframes.sort_by_key(|k| k.frame);
            } else {
                charge_bake_budget(&original, span_start, span_end, budget)?;
                layer
                    .keyframes
                    .retain(|k| k.frame < span_start || k.frame >= span_end);
                layer.keyframes.extend(
                    (span_start..span_end)
                        .map(|frame| sampled_key(timeline, &original, frame, fps)),
                );
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            Ok(())
        }
        TweenKind::Shape => {
            let has_end_key = layer
                .keyframes
                .iter()
                .any(|k| k.frame == span_end && k.keyed_properties.is_none());
            if !has_end_key {
                layer.keyframes[start_pos].tween = None;
                split(timeline, layer, at, fps);
                return Ok(());
            }
            let mut classic_proxy = first_key.clone();
            classic_proxy.tween_kind = TweenKind::Classic;
            let mut original_un_guided = layer.clone();
            original_un_guided.properties.motion_guide = None;
            let sampling = TimelineSampleContext { timeline, fps };
            if let (Some(mut left_keys), Some(mut right_keys)) = (
                subdivide_classic_subrange(
                    sampling,
                    &original_un_guided,
                    &classic_proxy,
                    span_start,
                    span_end,
                    span_start,
                    at,
                ),
                subdivide_classic_subrange(
                    sampling,
                    &original_un_guided,
                    &classic_proxy,
                    span_start,
                    span_end,
                    at,
                    span_end,
                ),
            ) {
                for k in left_keys.iter_mut().chain(right_keys.iter_mut()) {
                    k.tween_kind = TweenKind::Shape;
                    k.orient_to_path = false;
                    k.shape_hints.clear();
                }
                layer
                    .keyframes
                    .retain(|k| k.frame < span_start || k.frame >= span_end);
                layer.keyframes.extend(left_keys);
                layer.keyframes.extend(right_keys);
                layer.keyframes.sort_by_key(|k| k.frame);
            } else {
                let original = layer.clone();
                charge_bake_budget(&original, span_start, span_end, budget)?;
                layer
                    .keyframes
                    .retain(|k| k.frame < span_start || k.frame >= span_end);
                layer.keyframes.extend(
                    (span_start..span_end)
                        .map(|frame| sampled_key(timeline, &original, frame, fps)),
                );
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            Ok(())
        }
    }
}
