use super::motion_keys::*;
use super::*;
use crate::motion_tools::{reverse_easing, reverse_path_commands};

#[derive(Clone)]
pub(super) struct ClassicSpanSource {
    pub(super) tween: Option<Easing>,
    pub(super) tween_kind: TweenKind,
    pub(super) shape_hints: Vec<ShapeHint>,
    pub(super) orient_to_path: bool,
    pub(super) property_eases: Vec<(TweenProperty, Easing)>,
    pub(super) sync_symbols: bool,
    pub(super) snap_to_guide: bool,
    pub(super) classic_rotate_turns: u32,
    pub(super) classic_rotate_dir: ClassicRotateDirection,
    pub(super) tween_scale: bool,
    pub(super) start_elements: Vec<Element>,
    pub(super) start_frame: u32,
}

pub(super) fn reverse_rotate_direction(dir: ClassicRotateDirection) -> ClassicRotateDirection {
    match dir {
        ClassicRotateDirection::Cw => ClassicRotateDirection::Ccw,
        ClassicRotateDirection::Ccw => ClassicRotateDirection::Cw,
        other => other,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fit_reversed_single_motion_ease(
    ease: Easing,
    m_start: u32,
    last_frame: u32,
    orig_last_prop: u32,
    orig_eval_end: u32,
    rev_start_frame: u32,
    rev_last_frame: u32,
    rev_eval_end: u32,
) -> Option<Easing> {
    if matches!(ease, Easing::Bounce | Easing::Elastic) {
        return None;
    }
    let d = last_frame.saturating_sub(m_start);
    if ease == Easing::Linear || d <= 1 {
        return Some(Easing::Linear);
    }
    let orig_len = (orig_eval_end - m_start).max(1) as f64;
    let prop_span = (orig_last_prop - m_start).max(1) as f64;
    let alpha_at = |k: u32| -> f64 {
        let f_orig = last_frame - k;
        let p_orig = ((f_orig - m_start) as f64 / orig_len).clamp(0.0, 1.0);
        let t_orig = m_start as f64 + easing(p_orig, ease) * orig_len;
        ((t_orig - m_start as f64) / prop_span).clamp(0.0, 1.0)
    };
    let alpha_0 = alpha_at(0);
    if alpha_0.abs() <= 1e-6 {
        return None;
    }
    let rev_len = (rev_eval_end - rev_start_frame).max(1) as f64;
    let d_rev = (rev_last_frame - rev_start_frame).max(1) as f64;
    let scale = d_rev / rev_len;
    let mut samples = Vec::with_capacity(d as usize);
    for k in 1..=d {
        let x = k as f64 / rev_len;
        let frac = (alpha_0 - alpha_at(k)) / alpha_0;
        samples.push((x, scale * frac));
    }
    fit_cubic_bezier_samples(&samples)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reverse_single_motion_span(
    timeline: &Timeline,
    original: &Layer,
    m_start: u32,
    m_excl_end: u32,
    first: u32,
    final_frame: u32,
    last: u32,
    layer_exposure: u32,
    fps: f64,
) -> Option<Vec<Keyframe>> {
    if m_excl_end <= m_start + 1 || original.properties.motion_guide.is_some() {
        return None;
    }
    let start_idx = original.keyframes.iter().position(|k| {
        k.frame == m_start && k.keyed_properties.is_none() && k.tween_kind == TweenKind::Motion
    })?;
    let orig_start = &original.keyframes[start_idx];
    orig_start.tween?;
    if has_noninvertible_motion_ease(orig_start)
        || orig_start.property_expressions.iter().any(|e| e.enabled)
    {
        return None;
    }
    let orig_props: Vec<&Keyframe> = original.keyframes[start_idx + 1..]
        .iter()
        .take_while(|k| k.frame < m_excl_end && k.keyed_properties.is_some())
        .collect();
    let last_frame = m_excl_end - 1;
    let rev_f = |f: u32| first + final_frame - f;
    let rev_start_frame = rev_f(last_frame);
    let rev_last_frame = rev_f(m_start);

    let all_linear = orig_start.tween == Some(Easing::Linear)
        && orig_start
            .property_eases
            .iter()
            .all(|(_, e)| *e == Easing::Linear)
        && orig_start.motion_tracks.iter().all(|track| {
            track.easing.is_none_or(|ease| ease == Easing::Linear)
                && track
                    .property_eases
                    .as_ref()
                    .is_none_or(|pe| pe.iter().all(|(_, ease)| *ease == Easing::Linear))
        });

    let next_ordinary = original.keyframes[start_idx + 1..]
        .iter()
        .find(|k| k.keyed_properties.is_none())
        .map(|k| k.frame);
    let orig_last_prop = orig_props.last().map_or(m_start, |k| k.frame);
    let orig_eval_end = next_ordinary
        .or(original.properties.end_frame)
        .unwrap_or(orig_last_prop)
        .max(orig_last_prop)
        .max(m_start + 1);
    let rev_has_following_end =
        (m_start > first) || (last < layer_exposure) || original.properties.end_frame.is_some();
    let rev_eval_end = if rev_has_following_end {
        rev_last_frame + 1
    } else {
        rev_last_frame
    }
    .max(rev_start_frame + 1);

    let (rev_tween, rev_prop_eases, mut rev_tracks) = if all_linear || orig_props.is_empty() {
        let base = orig_start.tween.map(reverse_easing)?;
        let props = orig_start
            .property_eases
            .iter()
            .map(|&(p, e)| (p, reverse_easing(e)))
            .collect();
        let mut tracks = orig_start.motion_tracks.clone();
        for track in &mut tracks {
            track.easing = track.easing.map(reverse_easing);
            if let Some(pe) = &mut track.property_eases {
                for (_, ease) in pe {
                    *ease = reverse_easing(*ease);
                }
            }
        }
        (base, props, tracks)
    } else if orig_eval_end == last_frame && rev_eval_end == rev_last_frame {
        let base = orig_start.tween.map(reverse_easing)?;
        let props = orig_start
            .property_eases
            .iter()
            .map(|&(p, e)| (p, reverse_easing(e)))
            .collect();
        let mut tracks = orig_start.motion_tracks.clone();
        for track in &mut tracks {
            track.easing = track.easing.map(reverse_easing);
            if let Some(pe) = &mut track.property_eases {
                for (_, ease) in pe {
                    *ease = reverse_easing(*ease);
                }
            }
        }
        (base, props, tracks)
    } else {
        if orig_props.iter().any(|k| k.frame != orig_last_prop)
            || has_roving_motion_keys(original, start_idx, m_excl_end)
            || !curved_tracks_compatible_with_subdivision(orig_start, false)
        {
            return None;
        }
        let fit_one = |e: Easing| {
            fit_reversed_single_motion_ease(
                e,
                m_start,
                last_frame,
                orig_last_prop,
                orig_eval_end,
                rev_start_frame,
                rev_last_frame,
                rev_eval_end,
            )
        };
        let base = fit_one(orig_start.tween?)?;
        let mut props = Vec::with_capacity(orig_start.property_eases.len());
        for &(p, e) in &orig_start.property_eases {
            props.push((p, fit_one(e)?));
        }
        let mut tracks = orig_start.motion_tracks.clone();
        for track in &mut tracks {
            if let Some(e) = track.easing {
                track.easing = Some(fit_one(e)?);
            }
            if let Some(pe) = &mut track.property_eases {
                for (_, ease) in pe {
                    *ease = fit_one(*ease)?;
                }
            }
        }
        (base, props, tracks)
    };

    let mut rev_start = sampled_key(timeline, original, last_frame, fps);
    if let Some(old_last) = orig_props.iter().find(|k| k.frame == last_frame) {
        rev_start.sound = old_last.sound.clone();
    }
    rev_start.frame = rev_start_frame;
    rev_start.keyed_properties = None;
    rev_start.motion_keys = None;
    rev_start.roving = false;
    rev_start.tween = Some(rev_tween);
    rev_start.tween_kind = TweenKind::Motion;
    rev_start.orient_to_path = orig_start.orient_to_path;
    rev_start.sync_symbols = orig_start.sync_symbols;
    rev_start.snap_to_guide = orig_start.snap_to_guide;
    rev_start.property_eases = rev_prop_eases;

    let mut orient_deltas: HashMap<Id, f64> = HashMap::new();
    for el in &rev_start.elements {
        if orig_start.motion_orient_to_path_for(&el.id)
            && let Some(base_end_rot) =
                crate::evaluate::sample_motion_base_rotation(original, last_frame, &el.id)
        {
            orient_deltas.insert(el.id.clone(), el.transform.rotation - base_end_rot);
        }
    }

    let mut rev_prop_keys: Vec<Keyframe> = Vec::new();
    for &k in orig_props.iter().rev() {
        if k.frame >= last_frame || k.frame <= m_start {
            continue;
        }
        let mut rk = k.clone();
        rk.frame = rev_f(k.frame);
        rk.tween = Some(rev_tween);
        rk.tween_kind = TweenKind::Motion;
        for el_idx in 0..rk.elements.len() {
            let el_id = rk.elements[el_idx].id.clone();
            if rk
                .motion_key_properties_for(&el_id)
                .contains(&TweenProperty::Rotation)
                && let Some(&dh) = orient_deltas.get(&el_id)
            {
                rk.elements[el_idx].transform.rotation += dh;
            }
        }
        rev_prop_keys.push(rk);
    }

    if !orig_props.is_empty() {
        let mut rev_end = orig_start.clone();
        rev_end.frame = rev_last_frame;
        rev_end.tween = Some(rev_tween);
        rev_end.tween_kind = TweenKind::Motion;
        rev_end.motion_tracks.clear();
        rev_end.motion_path = None;
        rev_end.property_eases.clear();
        rev_end.sync_symbols = false;
        rev_end.snap_to_guide = false;
        rev_end.orient_to_path = false;
        rev_end.roving = false;
        for el in &mut rev_end.elements {
            if let Some(&dh) = orient_deltas.get(&el.id) {
                el.transform.rotation += dh;
            }
        }
        let uses_object_keys = !orig_start.motion_tracks.is_empty()
            || orig_props.iter().any(|k| k.motion_keys.is_some());
        if uses_object_keys {
            let mut obj_keys = Vec::new();
            for el in &orig_start.elements {
                let mut union_props: Vec<TweenProperty> = Vec::new();
                for p in TweenProperty::ALL {
                    if orig_props
                        .iter()
                        .any(|k| k.motion_key_properties_for(&el.id).contains(&p))
                    {
                        union_props.push(p);
                    }
                }
                if !union_props.is_empty() {
                    obj_keys.push(MotionPropertyKey {
                        element_id: el.id.clone(),
                        properties: union_props,
                        roving: false,
                    });
                }
            }
            rev_end.motion_keys = Some(obj_keys);
            rev_end.sync_motion_key_union();
        } else {
            let mut union_props: Vec<TweenProperty> = Vec::new();
            for p in TweenProperty::ALL {
                if orig_props.iter().any(|k| {
                    k.keyed_properties
                        .as_ref()
                        .is_some_and(|kp| kp.contains(&p))
                }) {
                    union_props.push(p);
                }
            }
            if union_props.is_empty() {
                union_props = TweenProperty::ALL.to_vec();
            }
            rev_end.motion_keys = None;
            rev_end.keyed_properties = Some(union_props);
        }
        rev_prop_keys.push(rev_end);
    }

    let reverse_curve_with_held_tail =
        |cmds: &[PathCommand], ref_id: &str| -> Option<Vec<PathCommand>> {
            let orig_wps = motion_path_waypoints(original, m_start, ref_id);
            let mut rev_cmds = reverse_path_commands(cmds)?;
            let rev_wps = collect_subspan_pos_waypoints(rev_start.frame, &rev_prop_keys, ref_id);
            if cmds.len() == orig_wps.len()
                && rev_wps.len() == rev_cmds.len() + 1
                && let Some(PathCommand::MoveTo { x, y }) = rev_cmds.first().copied()
            {
                rev_cmds.insert(1, PathCommand::LineTo { x, y });
            }
            motion_path_polyline(&rev_cmds)?;
            Some(rev_cmds)
        };

    if let Some(legacy_cmds) = &orig_start.motion_path {
        let ref_el = orig_start.elements.first()?;
        rev_start.motion_path = Some(reverse_curve_with_held_tail(legacy_cmds, &ref_el.id)?);
    } else {
        rev_start.motion_path = None;
    }
    for track in &mut rev_tracks {
        if let Some(MotionPathOverride::Path { commands }) = &track.motion_path {
            let rev_cmds = reverse_curve_with_held_tail(commands, &track.element_id)?;
            track.motion_path = Some(MotionPathOverride::Path { commands: rev_cmds });
        }
    }
    rev_start.motion_tracks = rev_tracks;

    let mut out = Vec::with_capacity(1 + rev_prop_keys.len());
    out.push(rev_start);
    out.extend(rev_prop_keys);
    Some(out)
}

pub(super) fn reverse_classic_subinterval(
    timeline: &Timeline,
    guide_layer: Option<&Layer>,
    inside: &mut [Keyframe],
    first: u32,
    final_frame: u32,
) {
    let count = inside.len();
    if count == 0 {
        return;
    }
    let span_sources: Vec<ClassicSpanSource> = inside
        .iter()
        .map(|key| ClassicSpanSource {
            tween: key.tween,
            tween_kind: if key.tween_kind == TweenKind::Shape {
                TweenKind::Shape
            } else {
                TweenKind::Classic
            },
            shape_hints: key.shape_hints.clone(),
            orient_to_path: key.orient_to_path,
            property_eases: key.property_eases.clone(),
            sync_symbols: key.sync_symbols,
            snap_to_guide: key.snap_to_guide,
            classic_rotate_turns: key.classic_rotate_turns,
            classic_rotate_dir: key.classic_rotate_dir,
            tween_scale: key.tween_scale,
            start_elements: key.elements.clone(),
            start_frame: key.frame,
        })
        .collect();
    let _ = timeline;
    for (i, key) in inside.iter_mut().enumerate() {
        let src = &span_sources[if i == 0 { count - 1 } else { i - 1 }];
        key.tween = if i == 0 {
            src.tween
        } else {
            src.tween.map(reverse_easing)
        };
        if key.tween.is_some() {
            key.tween_kind = src.tween_kind;
            key.orient_to_path = src.orient_to_path;
            key.property_eases = if i == 0 {
                src.property_eases.clone()
            } else {
                src.property_eases
                    .iter()
                    .map(|&(p, e)| (p, reverse_easing(e)))
                    .collect()
            };
            key.sync_symbols = src.sync_symbols;
            key.snap_to_guide = src.snap_to_guide;
            key.classic_rotate_turns = src.classic_rotate_turns;
            key.classic_rotate_dir = if i == 0 {
                src.classic_rotate_dir
            } else {
                reverse_rotate_direction(src.classic_rotate_dir)
            };
            key.tween_scale = src.tween_scale;
            key.shape_hints = if key.tween_kind == TweenKind::Shape {
                if i == 0 {
                    src.shape_hints.clone()
                } else {
                    src.shape_hints
                        .iter()
                        .map(|h| ShapeHint {
                            label: h.label,
                            start_uv: h.end_uv,
                            end_uv: h.start_uv,
                        })
                        .collect()
                }
            } else {
                Vec::new()
            };
            if i > 0
                && key.tween_kind == TweenKind::Classic
                && let Some(guide) = guide_layer
                && let Some(path) = crate::guide_polyline(guide, src.start_frame)
            {
                let was_snapped = src.snap_to_guide;
                if was_snapped {
                    key.snap_to_guide = false;
                }
                for el in &mut key.elements {
                    if let Some(orig_a) = src.start_elements.iter().find(|e| e.id == el.id) {
                        let s0 = if was_snapped {
                            0.0
                        } else {
                            path.nearest([orig_a.transform.x, orig_a.transform.y])
                        };
                        let s1 = if was_snapped {
                            path.length()
                        } else {
                            path.nearest([el.transform.x, el.transform.y])
                        };
                        if was_snapped {
                            let [x1, y1] = path.point_at(s1);
                            el.transform.x = x1;
                            el.transform.y = y1;
                        }
                        if key.orient_to_path {
                            el.transform.rotation = orig_a.transform.rotation + path.tangent_at(s1)
                                - path.tangent_at(s0);
                        }
                    }
                }
            }
        } else {
            key.tween_kind = TweenKind::Classic;
            key.orient_to_path = false;
            key.property_eases.clear();
            key.sync_symbols = false;
            key.snap_to_guide = false;
            key.classic_rotate_turns = 0;
            key.classic_rotate_dir = ClassicRotateDirection::Auto;
            key.tween_scale = true;
            key.shape_hints.clear();
        }
        key.keyed_properties = None;
        key.motion_keys = None;
        key.motion_tracks.clear();
        key.motion_path = None;
        key.frame = first + final_frame - key.frame;
    }
    if let Some(guide) = guide_layer {
        for idx in 1..inside.len() {
            let src = &span_sources[idx - 1];
            if src.tween.is_some()
                && src.tween_kind == TweenKind::Classic
                && src.snap_to_guide
                && let Some(path) = crate::guide_polyline(guide, src.start_frame)
            {
                let [x0, y0] = path.point_at(0.0);
                for el in &mut inside[idx - 1].elements {
                    if src.start_elements.iter().any(|e| e.id == el.id) {
                        el.transform.x = x0;
                        el.transform.y = y0;
                    }
                }
            }
        }
    }
}
