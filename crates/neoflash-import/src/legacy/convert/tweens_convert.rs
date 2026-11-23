use super::*;

/// Stable content digest for provenance; not a security hash.
pub(super) fn fnv1a64(bytes: &[u8]) -> u64 {
    super::super::fnv1a64_bytes(bytes)
}
pub(super) fn group_symbol(id: &str, name: String, elements: Vec<Element>) -> Symbol {
    group_symbol_with_effects(id, name, elements, Vec::new(), Vec::new(), Vec::new())
}
pub(super) fn group_symbol_with_effects(
    id: &str,
    name: String,
    elements: Vec<Element>,
    bitmap_fills: Vec<AuthoredBitmapFill>,
    appearances: Vec<ElementAppearance>,
    vector_effects: Vec<ElementVectorEffects>,
) -> Symbol {
    Symbol {
        id: id.into(),
        name,
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![Layer {
                id: format!("{id}-layer"),
                name: "Members".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
                properties: Default::default(),
            }],
            scripts: vec![],
        },
        class_name: None,
        linkage_id: None,
    }
}
pub(super) fn invert_affine_2d(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() <= 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[4] + c * m[5]);
    let ty = -(b * m[4] + d * m[5]);
    let inv = [a, b, c, d, tx, ty];
    inv.iter()
        .all(|v| v.is_finite() && v.abs() <= 1e9)
        .then_some(inv)
}

pub(super) fn hoist_generated_group_transform(generated: &mut [Symbol], el: &mut Element) {
    let ElementKind::Instance { symbol_id, .. } = &el.kind else {
        return;
    };
    if !symbol_id.ends_with("-group") {
        return;
    }
    let Some(sym) = generated.iter_mut().find(|s| s.id == *symbol_id) else {
        return;
    };
    if sym.timeline.layers.len() != 1 || sym.timeline.layers[0].keyframes.len() != 1 {
        return;
    }
    let kf = &mut sym.timeline.layers[0].keyframes[0];
    let Some(first) = kf.elements.first() else {
        return;
    };
    let geom = Transform {
        x: first.transform.x,
        y: first.transform.y,
        scale_x: first.transform.scale_x,
        scale_y: first.transform.scale_y,
        rotation: first.transform.rotation,
        skew: first.transform.skew,
        ..Transform::default()
    };
    if geom == Transform::default() {
        return;
    }
    let g_mat = neoflash_core::transform_matrix(geom);
    let Some(inv_g) = invert_affine_2d(g_mat) else {
        return;
    };
    let outer_mat =
        neoflash_core::affine_multiply(neoflash_core::transform_matrix(el.transform), g_mat);
    let Some(new_outer) = Transform::from_matrix(outer_mat) else {
        return;
    };
    let mut updated_members = Vec::with_capacity(kf.elements.len());
    for member in &kf.elements {
        let rel_mat = neoflash_core::affine_multiply(
            inv_g,
            neoflash_core::transform_matrix(member.transform),
        );
        let Some(rel_tf) = Transform::from_matrix(rel_mat) else {
            return;
        };
        updated_members.push(rel_tf);
    }
    for (member, rel_tf) in kf.elements.iter_mut().zip(updated_members) {
        member.transform.x = rel_tf.x;
        member.transform.y = rel_tf.y;
        member.transform.scale_x = rel_tf.scale_x;
        member.transform.scale_y = rel_tf.scale_y;
        member.transform.rotation = rel_tf.rotation;
        member.transform.skew = rel_tf.skew;
    }
    el.transform.x = new_outer.x;
    el.transform.y = new_outer.y;
    el.transform.scale_x = new_outer.scale_x;
    el.transform.scale_y = new_outer.scale_y;
    el.transform.rotation = new_outer.rotation;
    el.transform.skew = new_outer.skew;
}

pub(super) fn group_keyframes_structurally_equal(a: &Keyframe, b: &Keyframe) -> bool {
    if a.elements.len() != b.elements.len()
        || a.bitmap_fills.len() != b.bitmap_fills.len()
        || a.appearances.len() != b.appearances.len()
        || a.vector_effects.len() != b.vector_effects.len()
    {
        return false;
    }
    for (ea, eb) in a.elements.iter().zip(&b.elements) {
        if ea.name != eb.name
            || ea.transform != eb.transform
            || ea.kind != eb.kind
            || ea.visible != eb.visible
            || ea.blend_mode != eb.blend_mode
            || ea.filters != eb.filters
        {
            return false;
        }
    }
    for (bfa, bfb) in a.bitmap_fills.iter().zip(&b.bitmap_fills) {
        if bfa.resource_id != bfb.resource_id
            || bfa.matrix != bfb.matrix
            || bfa.repeat != bfb.repeat
            || bfa.smoothing != bfb.smoothing
        {
            return false;
        }
    }
    for (apa, apb) in a.appearances.iter().zip(&b.appearances) {
        if apa.entries != apb.entries {
            return false;
        }
    }
    for (va, vb) in a.vector_effects.iter().zip(&b.vector_effects) {
        if va.procedural_effects != vb.procedural_effects
            || va.freeform_gradient != vb.freeform_gradient
            || va.gradient_mesh != vb.gradient_mesh
            || va.envelope_warp != vb.envelope_warp
            || va.vector_blend != vb.vector_blend
        {
            return false;
        }
    }
    true
}

pub(super) fn deduplicate_tweened_group_endpoint(
    generated: &mut Vec<Symbol>,
    provenance: &mut [serde_json::Value],
    prev_el: &Element,
    curr_el: &mut Element,
) {
    let (
        ElementKind::Instance {
            symbol_id: a_id, ..
        },
        ElementKind::Instance {
            symbol_id: b_id,
            playback,
        },
    ) = (&prev_el.kind, &curr_el.kind)
    else {
        return;
    };
    if a_id == b_id || !a_id.ends_with("-group") || !b_id.ends_with("-group") {
        return;
    }
    let Some(a_prefix) = a_id.strip_suffix("-group").map(str::to_owned) else {
        return;
    };
    let Some(b_prefix) = b_id.strip_suffix("-group").map(str::to_owned) else {
        return;
    };
    let Some(sym_a) = generated.iter().find(|s| s.id == *a_id) else {
        return;
    };
    let Some(sym_b) = generated.iter().find(|s| s.id == *b_id) else {
        return;
    };
    if sym_a.timeline.layers.len() == 1
        && sym_b.timeline.layers.len() == 1
        && sym_a.timeline.layers[0].keyframes.len() == 1
        && sym_b.timeline.layers[0].keyframes.len() == 1
        && group_keyframes_structurally_equal(
            &sym_a.timeline.layers[0].keyframes[0],
            &sym_b.timeline.layers[0].keyframes[0],
        )
    {
        let old_b = b_id.clone();
        let reused_a = a_id.clone();
        let pb = *playback;
        let b_member_prefix = format!("{b_prefix}-member-");
        let a_member_prefix = format!("{a_prefix}-member-");
        let b_paint_id = format!("{b_prefix}-paint");
        let a_paint_id = format!("{a_prefix}-paint");
        generated.retain(|s| {
            s.id != old_b && !(s.id.starts_with(&b_member_prefix) && s.id.ends_with("-group"))
        });
        for entry in provenance.iter_mut() {
            let Some(current) = entry.get("target").and_then(|v| v.as_str()) else {
                continue;
            };
            if current == b_paint_id {
                entry["target"] = serde_json::Value::String(a_paint_id.clone());
            } else if let Some(rest) = current.strip_prefix(&b_member_prefix) {
                entry["target"] = serde_json::Value::String(format!("{a_member_prefix}{rest}"));
            }
        }
        curr_el.kind = ElementKind::Instance {
            symbol_id: reused_a,
            playback: pb,
        };
    }
}

pub(super) fn placement_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (
                ElementKind::Bitmap { resource_id: a, .. },
                ElementKind::Bitmap { resource_id: b, .. },
            ) => a == b,
            (
                ElementKind::TextBox {
                    text: ta,
                    width: wa,
                    height: ha,
                    format: fa,
                    word_wrap: wwa,
                    multiline: ma,
                },
                ElementKind::TextBox {
                    text: tb,
                    width: wb,
                    height: hb,
                    format: fb,
                    word_wrap: wwb,
                    multiline: mb,
                },
            ) => {
                ta == tb
                    && (wa - wb).abs() < 1e-6
                    && (ha - hb).abs() < 1e-6
                    && fa == fb
                    && wwa == wwb
                    && ma == mb
            }
            _ => false,
        }
}
pub(super) fn color_transform(c: &TimelineColorTransform) -> Result<ColorTransform, String> {
    let mut result = ColorTransform::default();
    match c.effect_type.unwrap_or(0) {
        0 => {}
        1 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            for k in 0..3 {
                result.multipliers[k] = 1. - p.abs();
                result.offsets[k] = p.max(0.) * 255.;
            }
        }
        2 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            let color = shape::color(c.effect_rgba.unwrap_or(0));
            for (k, ch) in color.iter().enumerate().take(3) {
                result.multipliers[k] = 1. - p;
                result.offsets[k] = p * f64::from(*ch);
            }
        }
        4 => result.multipliers[3] = c.multipliers[3] as f64 / 256.,
        3 => {
            result.multipliers = c.multipliers.map(|v| v as f64 / 256.);
            result.offsets = c.offsets.map(f64::from);
        }
        _ => return Err("Unknown color effect".into()),
    }
    Ok(result)
}
/// Tween easing and interpolation kind from the frame key mode
/// (low byte: 0 none, 1 classic motion, 2 shape, 3 motion object, 4 IK pose).
pub(super) fn ease(f: &TimelineFrame) -> Result<(Option<Easing>, TweenKind), String> {
    let (tween, kind, defer) = ease_with_split_flag(f)?;
    if defer {
        return Err("Multi-segment custom easing is not supported".into());
    }
    Ok((tween, kind))
}

pub(super) fn uniform_custom_ease_curve(f: &TimelineFrame) -> Result<&[[f64; 6]], String> {
    if f.single_ease_curve == Some(1) {
        let c = f
            .custom_ease
            .get(5)
            .ok_or("Missing global custom-ease curve")?;
        return Ok(c.as_slice());
    }
    if f.custom_ease.len() == 6
        && !f.custom_ease[5].is_empty()
        && f.custom_ease[0..5].iter().all(|ch| ch == &f.custom_ease[5])
    {
        return Ok(f.custom_ease[5].as_slice());
    }
    Err("Separate per-property easing curves are not supported".into())
}

pub(super) fn ease_with_split_flag(
    f: &TimelineFrame,
) -> Result<(Option<Easing>, TweenKind, bool), String> {
    let classic = || Easing::Flash {
        acceleration: f.acceleration.unwrap_or(0) as i16 as f64,
    };
    match f.key_mode.unwrap_or(0) & 255 {
        0 if f.morph_shape.is_some() => Ok((Some(classic()), TweenKind::Shape, false)),
        0 => Ok((None, TweenKind::Classic, false)),
        1 => {
            if f.custom_ease_present.unwrap_or(0) != 0 {
                let c = uniform_custom_ease_curve(f)?;
                if c.len() == 2 && c[0][2..4] == [0., 0.] && c[1][2..4] == [1., 1.] {
                    let (x1, y1, x2, y2) = (c[0][4], c[0][5], c[1][0], c[1][1]);
                    if !(0.0..=1.0).contains(&x1)
                        || !(0.0..=1.0).contains(&x2)
                        || !y1.is_finite()
                        || !y2.is_finite()
                        || y1.abs() > 100.0
                        || y2.abs() > 100.0
                    {
                        return Err(
                            "Custom-ease handle coordinates are outside representable bounds"
                                .into(),
                        );
                    }
                    Ok((
                        Some(Easing::CubicBezier { x1, y1, x2, y2 }),
                        TweenKind::Classic,
                        false,
                    ))
                } else if c.len() >= 3 {
                    Ok((Some(classic()), TweenKind::Classic, true))
                } else {
                    Err("Multi-segment custom easing is not supported".into())
                }
            } else {
                Ok((Some(classic()), TweenKind::Classic, false))
            }
        }
        2 => Ok((Some(classic()), TweenKind::Shape, false)),
        // Motion-object and IK spans keep their stored keyframe poses.
        _ => Ok((None, TweenKind::Classic, false)),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_wrap_tweened_frame_shapes_into_group(
    generated: &mut Vec<Symbol>,
    previous: &mut Keyframe,
    elements: &mut Vec<Element>,
    bitmap_fills: &mut Vec<AuthoredBitmapFill>,
    appearances: &mut Vec<ElementAppearance>,
    vector_effects: &mut Vec<ElementVectorEffects>,
    stream_index: usize,
    layer_obj_id: u32,
    curr_frame_id: u32,
) {
    let prev_is_raw_vec =
        !previous.elements.is_empty() && previous.elements.iter().all(is_vector_path);
    let curr_is_raw_vec = !elements.is_empty() && elements.iter().all(is_vector_path);
    if !prev_is_raw_vec && !curr_is_raw_vec {
        return;
    }
    let is_gen_group = |els: &[Element], gens: &[Symbol]| -> bool {
        els.len() == 1
            && matches!(
                &els[0].kind,
                ElementKind::Instance { symbol_id, .. }
                    if symbol_id.ends_with("-group") && gens.iter().any(|s| s.id == *symbol_id)
            )
    };
    if !((prev_is_raw_vec || is_gen_group(&previous.elements, generated))
        && (curr_is_raw_vec || is_gen_group(elements, generated)))
    {
        return;
    }

    let mut candidate_gens = generated.clone();
    let mut cand_prev_els = previous.elements.clone();
    let mut cand_prev_bf = previous.bitmap_fills.clone();
    let mut cand_prev_app = previous.appearances.clone();
    let mut cand_prev_fx = previous.vector_effects.clone();
    if prev_is_raw_vec {
        let gid = format!(
            "fla-{stream_index}-{layer_obj_id}-fshape-{}-group",
            previous.frame
        );
        let wrapper_id = format!(
            "fla-{stream_index}-{layer_obj_id}-fshape-{}",
            previous.frame
        );
        candidate_gens.push(group_symbol_with_effects(
            &gid,
            format!("Frame shape {}", previous.frame + 1),
            std::mem::take(&mut cand_prev_els),
            std::mem::take(&mut cand_prev_bf),
            std::mem::take(&mut cand_prev_app),
            std::mem::take(&mut cand_prev_fx),
        ));
        cand_prev_els.push(Element {
            id: wrapper_id,
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: gid,
                playback: InstancePlayback::SingleFrame(0),
            },
            ..Element::default()
        });
    }

    let mut cand_curr_els = elements.clone();
    let mut cand_curr_bf = bitmap_fills.clone();
    let mut cand_curr_app = appearances.clone();
    let mut cand_curr_fx = vector_effects.clone();
    if curr_is_raw_vec {
        let gid = format!("fla-{stream_index}-{layer_obj_id}-fshape-f{curr_frame_id}-group");
        let wrapper_id = format!("fla-{stream_index}-{layer_obj_id}-fshape-f{curr_frame_id}");
        candidate_gens.push(group_symbol_with_effects(
            &gid,
            format!("Frame shape object {curr_frame_id}"),
            std::mem::take(&mut cand_curr_els),
            std::mem::take(&mut cand_curr_bf),
            std::mem::take(&mut cand_curr_app),
            std::mem::take(&mut cand_curr_fx),
        ));
        cand_curr_els.push(Element {
            id: wrapper_id,
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: gid,
                playback: InstancePlayback::SingleFrame(0),
            },
            ..Element::default()
        });
    }

    hoist_generated_group_transform(&mut candidate_gens, &mut cand_prev_els[0]);
    hoist_generated_group_transform(&mut candidate_gens, &mut cand_curr_els[0]);

    let (ElementKind::Instance { symbol_id: sa, .. }, ElementKind::Instance { symbol_id: sb, .. }) =
        (&cand_prev_els[0].kind, &cand_curr_els[0].kind)
    else {
        return;
    };
    let Some(sym_a) = candidate_gens.iter().find(|s| s.id == *sa) else {
        return;
    };
    let Some(sym_b) = candidate_gens.iter().find(|s| s.id == *sb) else {
        return;
    };
    if sym_a.timeline.layers.len() == 1
        && sym_b.timeline.layers.len() == 1
        && sym_a.timeline.layers[0].keyframes.len() == 1
        && sym_b.timeline.layers[0].keyframes.len() == 1
        && group_keyframes_structurally_equal(
            &sym_a.timeline.layers[0].keyframes[0],
            &sym_b.timeline.layers[0].keyframes[0],
        )
    {
        *generated = candidate_gens;
        previous.elements = cand_prev_els;
        previous.bitmap_fills = cand_prev_bf;
        previous.appearances = cand_prev_app;
        previous.vector_effects = cand_prev_fx;
        *elements = cand_curr_els;
        *bitmap_fills = cand_curr_bf;
        *appearances = cand_curr_app;
        *vector_effects = cand_curr_fx;
    }
}

pub(super) fn eval_cubic_scalar(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
}

pub(super) fn parse_contents_3d_perspective(
    c: &super::super::contents::BinaryContentsPrefix,
) -> (Option<f64>, Option<[f64; 2]>) {
    let Some(version) = c.revision else {
        return (None, None);
    };
    if version < 0x45 {
        return (None, None);
    };
    let Some(ext) = c.extensions.as_ref() else {
        return (None, None);
    };
    let tail_len = match version {
        0x45 => 8,
        0x46 => 16,
        _ => 20,
    };
    if ext.encoded.end < ext.encoded.start + tail_len {
        return (None, None);
    }
    let base = ext.encoded.end - tail_len;
    let Some(slice) = c.original_bytes.get(base..ext.encoded.end) else {
        return (None, None);
    };
    let fov = f64::from_le_bytes(slice[0..8].try_into().unwrap());
    let view_angle = (fov.is_finite() && fov > 0.0 && fov < 180.0).then_some(fov);
    let vanishing_point = if version >= 0x46 && slice.len() >= 16 {
        let vx = i32::from_le_bytes(slice[8..12].try_into().unwrap());
        let vy = i32::from_le_bytes(slice[12..16].try_into().unwrap());
        (vx != i32::MIN && vy != i32::MIN).then_some([f64::from(vx) / 20.0, f64::from(vy) / 20.0])
    } else {
        None
    };
    (view_angle, vanishing_point)
}

pub(super) fn sample_fla_ik_pose_matrices(
    original_bytes: &[u8],
    span: TimelineByteSpan,
    key: &Keyframe,
    duration: u32,
    cursor: u32,
    budget: &mut usize,
) -> Option<Vec<Keyframe>> {
    if duration <= 1
        || key.elements.is_empty()
        || key.tween.is_some()
        || duration > 1_000_000_u32.saturating_sub(cursor)
    {
        return None;
    }
    let raw = original_bytes.get(span.start..span.end)?;
    if !raw.len().is_multiple_of(24) {
        return None;
    }
    let total_mats = raw.len() / 24;
    let elem_count = key.elements.len();
    let expected = elem_count.checked_mul(duration as usize)?;
    if total_mats != expected {
        return None;
    }
    let count = (duration - 1) as usize;
    let elem_bytes: usize = key
        .elements
        .iter()
        .map(|el| serde_json::to_vec(el).map_or(256, |v| v.len()))
        .sum();
    let cost = count.saturating_mul(elem_bytes.div_ceil(2048).max(1));
    if count > 4095 || cost > *budget {
        return None;
    }

    let mut mats = Vec::with_capacity(total_mats);
    for chunk in raw.as_chunks::<24>().0 {
        let words: [i32; 6] = std::array::from_fn(|i| {
            i32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap())
        });
        mats.push(shape::matrix(words));
    }

    let mul_mat = |m1: [f64; 6], m2: [f64; 6]| -> [f64; 6] {
        [
            m1[0] * m2[0] + m1[2] * m2[1],
            m1[1] * m2[0] + m1[3] * m2[1],
            m1[0] * m2[2] + m1[2] * m2[3],
            m1[1] * m2[2] + m1[3] * m2[3],
            m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
            m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
        ]
    };

    let mut any_changed = false;
    let mut poses = Vec::with_capacity(count);
    for step in 1..duration {
        let mut frame_elems = Vec::with_capacity(elem_count);
        for (e_idx, base_el) in key.elements.iter().enumerate() {
            let mut el = base_el.clone();
            let m0 = mats[e_idx * (duration as usize)];
            let mf = mats[e_idx * (duration as usize) + (step as usize)];
            let delta = invert_affine_matrix(m0).map_or(mf, |inv0| mul_mat(mf, inv0));
            if (delta[0] - 1.0).abs() > 1e-7
                || delta[1].abs() > 1e-7
                || delta[2].abs() > 1e-7
                || (delta[3] - 1.0).abs() > 1e-7
                || delta[4].abs() > 1e-7
                || delta[5].abs() > 1e-7
            {
                any_changed = true;
            }
            let composed = mul_mat(delta, transform_matrix(base_el.transform));
            if let Some(mut tr) = Transform::from_matrix(composed) {
                tr.alpha = base_el.transform.alpha;
                tr.color_transform = base_el.transform.color_transform;
                el.transform = tr;
            }
            frame_elems.push(el);
        }
        poses.push(Keyframe {
            frame: cursor.saturating_add(step),
            elements: frame_elems,
            ..Default::default()
        });
    }

    if !any_changed {
        return None;
    }
    *budget -= cost;
    Some(poses)
}

pub(super) fn solve_monotonic_cubic_x(x0: f64, c1x: f64, c2x: f64, x3: f64, target_x: f64) -> f64 {
    if target_x <= x0 {
        return 0.0;
    }
    if target_x >= x3 {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if eval_cubic_scalar(x0, c1x, c2x, x3, mid) < target_x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

pub(super) fn subdivide_cubic_2d_interval(seg: [[f64; 2]; 4], s0: f64, s1: f64) -> [[f64; 2]; 4] {
    let lerp2 =
        |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let split_left = |c: [[f64; 2]; 4], t: f64| -> [[f64; 2]; 4] {
        let p01 = lerp2(c[0], c[1], t);
        let p12 = lerp2(c[1], c[2], t);
        let p23 = lerp2(c[2], c[3], t);
        let p012 = lerp2(p01, p12, t);
        let p123 = lerp2(p12, p23, t);
        let p0123 = lerp2(p012, p123, t);
        [c[0], p01, p012, p0123]
    };
    if s1 <= 1e-12 {
        return [seg[0]; 4];
    }
    let left = split_left(seg, s1.clamp(0.0, 1.0));
    let rev = [left[3], left[2], left[1], left[0]];
    let right_rev = split_left(rev, (1.0 - (s0 / s1).clamp(0.0, 1.0)).clamp(0.0, 1.0));
    [right_rev[3], right_rev[2], right_rev[1], right_rev[0]]
}

pub(super) fn try_split_multi_segment_custom_ease(
    layer: &mut Layer,
    key_idx: usize,
    f: &TimelineFrame,
) -> Result<(), String> {
    if layer.properties.motion_guide.is_some() {
        return Err("Guided multi-segment custom easing is not supported".into());
    }
    if key_idx + 1 >= layer.keyframes.len() {
        return Ok(());
    }
    if layer.keyframes[key_idx].tween.is_none()
        || layer.keyframes[key_idx].tween_kind != TweenKind::Classic
        || layer.keyframes[key_idx].elements.len() != 1
        || layer.keyframes[key_idx + 1].elements.len() != 1
        || layer.keyframes[key_idx].elements[0].id != layer.keyframes[key_idx + 1].elements[0].id
    {
        return Err("Multi-segment custom easing is not supported".into());
    }
    let knots = uniform_custom_ease_curve(f)?;
    if knots.len() < 3 || knots.len() > 64 {
        return Err("Multi-segment custom easing is not supported".into());
    }
    if knots[0][2].abs() > 1e-7
        || knots[0][3].abs() > 1e-7
        || (knots.last().unwrap()[2] - 1.0).abs() > 1e-7
        || (knots.last().unwrap()[3] - 1.0).abs() > 1e-7
    {
        return Err("Custom-ease endpoints must span (0,0) to (1,1)".into());
    }
    for m in 0..knots.len() - 1 {
        let (x0, y0, c1x, c1y) = (knots[m][2], knots[m][3], knots[m][4], knots[m][5]);
        let (c2x, c2y, x3, y3) = (
            knots[m + 1][0],
            knots[m + 1][1],
            knots[m + 1][2],
            knots[m + 1][3],
        );
        if ![x0, y0, c1x, c1y, c2x, c2y, x3, y3]
            .iter()
            .all(|v| v.is_finite())
            || x3 <= x0 + 1e-7
            || c1x < x0 - 1e-7
            || c1x > x3 + 1e-7
            || c2x < x0 - 1e-7
            || c2x > x3 + 1e-7
        {
            return Err("Non-monotonic custom-ease curve is not supported".into());
        }
    }

    let start_frame = layer.keyframes[key_idx].frame;
    let end_frame = layer.keyframes[key_idx + 1].frame;
    let dur = end_frame.saturating_sub(start_frame);
    if dur <= 1 {
        layer.keyframes[key_idx].tween = Some(Easing::Linear);
        return Ok(());
    }

    let mut splits = vec![0u32, dur];
    for &knot in &knots[1..knots.len() - 1] {
        let xk = knot[2];
        let raw_f = xk * f64::from(dur);
        let k_lo = (raw_f.floor() as i64).clamp(0, i64::from(dur)) as u32;
        let k_hi = (raw_f.ceil() as i64).clamp(0, i64::from(dur)) as u32;
        splits.push(k_lo);
        splits.push(k_hi);
    }
    splits.sort_unstable();
    splits.dedup();

    let seg_for_interval = |u_a: f64, u_b: f64| -> Option<usize> {
        let mid = 0.5 * (u_a + u_b);
        (0..knots.len() - 1).find(|&m| knots[m][2] - 1e-7 <= mid && mid <= knots[m + 1][2] + 1e-7)
    };
    let eval_curve_at_u = |u: f64| -> Result<f64, String> {
        if u <= 0.0 {
            return Ok(0.0);
        }
        if u >= 1.0 {
            return Ok(1.0);
        }
        let m = (0..knots.len() - 1)
            .find(|&m| knots[m][2] - 1e-9 <= u && u <= knots[m + 1][2] + 1e-9)
            .ok_or("Custom-ease time lies outside knot segments")?;
        let s = solve_monotonic_cubic_x(
            knots[m][2],
            knots[m][4],
            knots[m + 1][0],
            knots[m + 1][2],
            u,
        );
        Ok(eval_cubic_scalar(
            knots[m][3],
            knots[m][5],
            knots[m + 1][1],
            knots[m + 1][3],
            s,
        ))
    };

    let mut split_v = Vec::with_capacity(splits.len());
    for &k in &splits {
        split_v.push(eval_curve_at_u(f64::from(k) / f64::from(dur))?);
    }

    let mut sub_eases = Vec::with_capacity(splits.len() - 1);
    for pair in splits.windows(2) {
        let (ka, kb) = (pair[0], pair[1]);
        if kb <= ka + 1 {
            sub_eases.push(Easing::Linear);
            continue;
        }
        let ua = f64::from(ka) / f64::from(dur);
        let ub = f64::from(kb) / f64::from(dur);
        let m = seg_for_interval(ua, ub).ok_or("Missing custom-ease segment for sub-span")?;
        if ua < knots[m][2] - 1e-7 || ub > knots[m + 1][2] + 1e-7 {
            return Err("Custom-ease sub-span crosses a knot boundary".into());
        }
        let seg = [
            [knots[m][2], knots[m][3]],
            [knots[m][4], knots[m][5]],
            [knots[m + 1][0], knots[m + 1][1]],
            [knots[m + 1][2], knots[m + 1][3]],
        ];
        let sa = solve_monotonic_cubic_x(seg[0][0], seg[1][0], seg[2][0], seg[3][0], ua);
        let sb = solve_monotonic_cubic_x(seg[0][0], seg[1][0], seg[2][0], seg[3][0], ub);
        let sub = subdivide_cubic_2d_interval(seg, sa, sb);
        let dx = sub[3][0] - sub[0][0];
        let dy = sub[3][1] - sub[0][1];
        if dx <= 1e-9 {
            return Err("Zero-width custom-ease sub-span".into());
        }
        if dy.abs() <= 1e-7
            && (sub[1][1] - sub[0][1]).abs() <= 1e-6
            && (sub[2][1] - sub[0][1]).abs() <= 1e-6
        {
            sub_eases.push(Easing::Linear);
            continue;
        }
        if dy.abs() <= 1e-6 {
            return Err("Zero-delta inflected custom-ease sub-span cannot be normalized".into());
        }
        let x1 = ((sub[1][0] - sub[0][0]) / dx).clamp(0.0, 1.0);
        let x2 = ((sub[2][0] - sub[0][0]) / dx).clamp(0.0, 1.0);
        let y1 = (sub[1][1] - sub[0][1]) / dy;
        let y2 = (sub[2][1] - sub[0][1]) / dy;
        if !y1.is_finite() || !y2.is_finite() || y1.abs() > 100.0 || y2.abs() > 100.0 {
            return Err("Custom-ease sub-segment exceeds +/-100 overshoot limit".into());
        }
        sub_eases.push(Easing::CubicBezier { x1, y1, x2, y2 });
    }

    let start_kf = layer.keyframes[key_idx].clone();
    let end_kf = layer.keyframes[key_idx + 1].clone();
    let a_el = &start_kf.elements[0];
    let b_el = &end_kf.elements[0];

    let turns_deg = (start_kf
        .classic_rotate_turns
        .min(Keyframe::MAX_ROTATE_TURNS) as f64)
        * 360.0;
    let total_rot_delta = match start_kf.classic_rotate_dir {
        ClassicRotateDirection::Auto => {
            let d = b_el.transform.rotation - a_el.transform.rotation;
            if turns_deg == 0.0 {
                d
            } else {
                d + (if d < 0.0 { -1.0 } else { 1.0 }) * turns_deg
            }
        }
        ClassicRotateDirection::None => 0.0,
        ClassicRotateDirection::Cw => {
            (b_el.transform.rotation - a_el.transform.rotation).rem_euclid(360.0) + turns_deg
        }
        ClassicRotateDirection::Ccw => {
            -((a_el.transform.rotation - b_el.transform.rotation).rem_euclid(360.0) + turns_deg)
        }
    };
    let target_end_rot = a_el.transform.rotation + total_rot_delta;
    if (target_end_rot - b_el.transform.rotation).abs() > 1e-6 && end_kf.tween.is_some() {
        return Err("Directed multi-turn rotation continues into a subsequent tween".into());
    }

    let lerp_f = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let mut inserted = Vec::with_capacity(splits.len().saturating_sub(2));
    for idx in 1..splits.len() - 1 {
        let k_off = splits[idx];
        let v = split_v[idx];
        let mut el = a_el.clone();
        el.transform.x = lerp_f(a_el.transform.x, b_el.transform.x, v);
        el.transform.y = lerp_f(a_el.transform.y, b_el.transform.y, v);
        if start_kf.tween_scale {
            el.transform.scale_x = lerp_f(a_el.transform.scale_x, b_el.transform.scale_x, v);
            el.transform.scale_y = lerp_f(a_el.transform.scale_y, b_el.transform.scale_y, v);
        }
        el.transform.rotation = a_el.transform.rotation + total_rot_delta * v;
        el.transform.skew = lerp_f(a_el.transform.skew, b_el.transform.skew, v);
        el.transform.alpha = lerp_f(a_el.transform.alpha, b_el.transform.alpha, v).clamp(0.0, 1.0);
        el.transform.color_transform = ColorTransform {
            multipliers: std::array::from_fn(|c| {
                lerp_f(
                    a_el.transform.color_transform.multipliers[c],
                    b_el.transform.color_transform.multipliers[c],
                    v,
                )
            }),
            offsets: std::array::from_fn(|c| {
                lerp_f(
                    a_el.transform.color_transform.offsets[c],
                    b_el.transform.color_transform.offsets[c],
                    v,
                )
            }),
        };
        el.filters = if a_el.filters == b_el.filters {
            a_el.filters.clone()
        } else {
            let probe_frame = (v.clamp(0.0, 1.0) * 1_000_000.0).round() as u32;
            let probe_layer = Layer {
                id: "probe".into(),
                name: String::new(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![a_el.clone()],
                        tween: Some(Easing::Linear),
                        tween_kind: TweenKind::Classic,
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 1_000_000,
                        elements: vec![b_el.clone()],
                        ..Default::default()
                    },
                ],
                properties: LayerProperties::default(),
            };
            sample_layer(&probe_layer, probe_frame)
                .into_iter()
                .next()
                .map_or_else(|| a_el.filters.clone(), |sampled| sampled.filters)
        };
        match (&a_el.kind, &b_el.kind, &mut el.kind) {
            (
                ElementKind::Rectangle {
                    width: aw,
                    height: ah,
                    fill: af,
                    stroke: ast,
                    stroke_width: asw,
                },
                ElementKind::Rectangle {
                    width: bw,
                    height: bh,
                    fill: bf,
                    stroke: bst,
                    stroke_width: bsw,
                },
                ElementKind::Rectangle {
                    width,
                    height,
                    fill,
                    stroke,
                    stroke_width,
                },
            )
            | (
                ElementKind::Ellipse {
                    width: aw,
                    height: ah,
                    fill: af,
                    stroke: ast,
                    stroke_width: asw,
                },
                ElementKind::Ellipse {
                    width: bw,
                    height: bh,
                    fill: bf,
                    stroke: bst,
                    stroke_width: bsw,
                },
                ElementKind::Ellipse {
                    width,
                    height,
                    fill,
                    stroke,
                    stroke_width,
                },
            ) => {
                if start_kf.tween_scale {
                    *width = lerp_f(*aw, *bw, v).max(0.0);
                    *height = lerp_f(*ah, *bh, v).max(0.0);
                }
                *fill = std::array::from_fn(|c| {
                    lerp_f(f64::from(af[c]), f64::from(bf[c]), v)
                        .round()
                        .clamp(0.0, 255.0) as u8
                });
                *stroke = std::array::from_fn(|c| {
                    lerp_f(f64::from(ast[c]), f64::from(bst[c]), v)
                        .round()
                        .clamp(0.0, 255.0) as u8
                });
                *stroke_width = lerp_f(*asw, *bsw, v).max(0.0);
            }
            (
                ElementKind::Video {
                    width: aw,
                    height: ah,
                },
                ElementKind::Video {
                    width: bw,
                    height: bh,
                },
                ElementKind::Video { width, height },
            ) if start_kf.tween_scale => {
                *width = lerp_f(*aw, *bw, v).max(1.0);
                *height = lerp_f(*ah, *bh, v).max(1.0);
            }
            _ => {}
        }

        inserted.push(Keyframe {
            frame: start_frame + k_off,
            elements: vec![el],
            tween: Some(sub_eases[idx]),
            tween_kind: TweenKind::Classic,
            tween_scale: start_kf.tween_scale,
            orient_to_path: start_kf.orient_to_path,
            sync_symbols: start_kf.sync_symbols,
            snap_to_guide: start_kf.snap_to_guide,
            classic_rotate_dir: ClassicRotateDirection::Auto,
            classic_rotate_turns: 0,
            ..Default::default()
        });
    }

    layer.keyframes[key_idx].tween = Some(sub_eases[0]);
    layer.keyframes[key_idx].classic_rotate_dir = ClassicRotateDirection::Auto;
    layer.keyframes[key_idx].classic_rotate_turns = 0;
    layer.keyframes[key_idx + 1].elements[0].transform.rotation = target_end_rot;
    if !inserted.is_empty() {
        layer.keyframes.splice(key_idx + 1..key_idx + 1, inserted);
    }
    Ok(())
}

pub(super) fn apply_affine_point(m: [f64; 6], pt: [f64; 2]) -> [f64; 2] {
    [
        m[0] * pt[0] + m[2] * pt[1] + m[4],
        m[1] * pt[0] + m[3] * pt[1] + m[5],
    ]
}

pub(super) fn point_dist_sq(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

pub(super) fn best_hint_stage_point(
    geom: Option<&ShapeHintGeometry>,
    raw_px: [f64; 2],
    matrix: Option<[f64; 6]>,
) -> [f64; 2] {
    let (Some(g), Some(m)) = (geom, matrix) else {
        return raw_px;
    };
    let transformed = apply_affine_point(m, raw_px);
    if (transformed[0] - raw_px[0]).abs() < 1e-6 && (transformed[1] - raw_px[1]).abs() < 1e-6 {
        return raw_px;
    }
    let d_raw = point_dist_sq(
        raw_px,
        g.nearest_with_snap(raw_px, DEFAULT_HINT_VERTEX_SNAP_RADIUS),
    );
    let d_xform = point_dist_sq(
        transformed,
        g.nearest_with_snap(transformed, DEFAULT_HINT_VERTEX_SNAP_RADIUS),
    );
    if d_xform + 1e-6 < d_raw {
        transformed
    } else {
        raw_px
    }
}

pub(super) fn point_to_hint_uv(geom: Option<&ShapeHintGeometry>, pt: [f64; 2]) -> [f64; 2] {
    if !pt[0].is_finite() || !pt[1].is_finite() {
        return [0.5, 0.5];
    }
    if let Some(g) = geom {
        let diag = (g.high[0] - g.low[0]).hypot(g.high[1] - g.low[1]);
        let snap_tol = (diag * 0.15).clamp(2.0, 24.0);
        let snapped = g.nearest_with_snap(pt, DEFAULT_HINT_VERTEX_SNAP_RADIUS);
        if point_dist_sq(pt, snapped) <= snap_tol * snap_tol {
            return g.snapped_uv(pt);
        }
        return std::array::from_fn(|axis| {
            let size = g.high[axis] - g.low[axis];
            if size > 1e-9 {
                ((pt[axis] - g.low[axis]) / size).clamp(0.0, 1.0)
            } else {
                0.5
            }
        });
    }
    [
        (pt[0] / 550.0).clamp(0.0, 1.0),
        (pt[1] / 400.0).clamp(0.0, 1.0),
    ]
}

pub(super) fn resolve_fla_morph_hints(
    s: &BinaryTimelineStream,
    morph_id: u32,
    start_elements: &[Element],
    end_elements: &[Element],
) -> (Vec<ShapeHint>, bool) {
    let Ok(morph_obj) = object(s, morph_id) else {
        return (Vec::new(), true);
    };
    let Some(TimelinePayload::Morph(morph)) = &morph_obj.payload else {
        return (Vec::new(), true);
    };
    if morph.hints.is_empty() {
        return (Vec::new(), false);
    }
    let start_geom = ShapeHintGeometry::from_elements(start_elements);
    let end_geom = ShapeHintGeometry::from_elements(end_elements);
    let mat_a = morph.matrix_words_start.map(shape::matrix);
    let mat_b = morph.matrix_words_end.map(shape::matrix);

    let mut seen_labels = std::collections::HashSet::new();
    let mut hints = Vec::new();
    let mut had_unconverted = morph.hints.len() > ShapeHint::MAX_HINTS;
    for raw_hint_id in &morph.hints {
        if hints.len() >= ShapeHint::MAX_HINTS {
            had_unconverted = true;
            break;
        }
        let Some(hint_id) = raw_hint_id else {
            had_unconverted = true;
            continue;
        };
        let Ok(hint_obj) = object(s, *hint_id) else {
            had_unconverted = true;
            continue;
        };
        let Some(TimelinePayload::MorphHint(hint)) = &hint_obj.payload else {
            had_unconverted = true;
            continue;
        };
        let (Some(pa), Some(pb)) = (hint.point_a, hint.point_b) else {
            had_unconverted = true;
            continue;
        };
        let label = hint
            .index
            .and_then(|idx| (idx < 26).then(|| (b'a' + idx as u8) as char))
            .filter(|c| !seen_labels.contains(c))
            .or_else(|| ('a'..='z').find(|c| !seen_labels.contains(c)));
        let Some(label) = label else {
            break;
        };
        seen_labels.insert(label);

        let raw_a = [pa[0] as f64 / 20.0, pa[1] as f64 / 20.0];
        let raw_b = [pb[0] as f64 / 20.0, pb[1] as f64 / 20.0];
        let pt_a = best_hint_stage_point(start_geom.as_ref(), raw_a, mat_a);
        let pt_b = best_hint_stage_point(end_geom.as_ref(), raw_b, mat_b);
        hints.push(ShapeHint {
            label,
            start_uv: point_to_hint_uv(start_geom.as_ref(), pt_a),
            end_uv: point_to_hint_uv(end_geom.as_ref(), pt_b),
        });
    }
    hints.sort_by_key(|h| h.label);
    (hints, had_unconverted)
}
