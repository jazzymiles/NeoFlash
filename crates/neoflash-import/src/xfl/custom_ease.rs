//! XFL `<tweens><CustomEase>` multi-segment 2D De Casteljau subdivision and
//! per-property (`position`, `rotation`, `scale`, `color`, `filters`) conversion.
use super::{Parser, child, elements};
use neoflash_core::{
    ClassicRotateDirection, ColorTransform, Easing, Element, ElementKind, InstancePlayback,
    Keyframe, Layer, LayerProperties, SymbolKind, TweenKind, TweenProperty, easing, sample_layer,
};
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
pub(super) struct XflCustomEaseCurve {
    pub target: String,
    pub points: Vec<[f64; 2]>,
    pub location: String,
}

#[derive(Clone, Debug)]
pub(super) struct PendingXflCustomEase {
    pub curves: Vec<XflCustomEaseCurve>,
    pub single_ease: bool,
    pub base_ease: Easing,
    pub location: String,
}

fn eval_cubic_scalar(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
}

fn solve_monotonic_cubic_x(x0: f64, c1x: f64, c2x: f64, x3: f64, target_x: f64) -> f64 {
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

fn subdivide_cubic_2d_interval(seg: [[f64; 2]; 4], s0: f64, s1: f64) -> [[f64; 2]; 4] {
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

pub(super) fn eval_multi_segment_points(pts: &[[f64; 2]], u: f64) -> f64 {
    if pts.len() < 4 || !(pts.len() - 1).is_multiple_of(3) {
        return u;
    }
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let seg_count = (pts.len() - 1) / 3;
    for m in 0..seg_count {
        let (p0, p1, p2, p3) = (pts[3 * m], pts[3 * m + 1], pts[3 * m + 2], pts[3 * m + 3]);
        if u <= p3[0] + 1e-9 || m + 1 == seg_count {
            let s = solve_monotonic_cubic_x(
                p0[0],
                p1[0].clamp(p0[0], p3[0]),
                p2[0].clamp(p0[0], p3[0]),
                p3[0],
                u,
            );
            return eval_cubic_scalar(p0[1], p1[1], p2[1], p3[1], s);
        }
    }
    1.0
}

fn single_segment_bezier(pts: &[[f64; 2]]) -> Option<Easing> {
    if pts.len() != 4 {
        return None;
    }
    Some(Easing::CubicBezier {
        x1: pts[1][0].clamp(0.0, 1.0),
        y1: pts[1][1],
        x2: pts[2][0].clamp(0.0, 1.0),
        y2: pts[2][1],
    })
}

impl Parser<'_> {
    pub(super) fn parse_frame_custom_eases(
        &mut self,
        frame_node: Node<'_, '_>,
        has_custom_ease: bool,
        single_ease: bool,
        fallback_ease: Easing,
        file: &str,
    ) -> (Option<Easing>, Option<PendingXflCustomEase>) {
        let loc = self.location(frame_node, file);
        let Some(tweens) = child(frame_node, "tweens") else {
            if has_custom_ease {
                self.report.error(
                    "custom_ease_not_supported",
                    &loc,
                    "hasCustomEase is true, but no <tweens> CustomEase curve was supplied.",
                );
            }
            return (None, None);
        };

        let ease_nodes: Vec<_> = elements(tweens)
            .filter(|n| n.has_tag_name("CustomEase"))
            .collect();
        if !has_custom_ease && ease_nodes.is_empty() {
            return (None, None);
        }
        if ease_nodes.is_empty() {
            self.report.warning(
                "custom_ease_not_supported",
                self.location(tweens, file),
                "hasCustomEase is true, but no CustomEase curve was supplied.",
            );
            return (None, None);
        }

        let mut parsed_curves = Vec::with_capacity(ease_nodes.len());
        let mut had_error = false;
        for ease_node in ease_nodes {
            self.audit(ease_node, file, &["target"], &["Point"]);
            let ease_loc = self.location(ease_node, file);
            let target = ease_node.attribute("target").unwrap_or("all").to_owned();
            let mut pts = Vec::new();
            for pt in elements(ease_node).filter(|n| n.has_tag_name("Point")) {
                self.audit(pt, file, &["x", "y"], &[]);
                let x = self.number(pt, "x", 0., file);
                let y = self.number(pt, "y", 0., file);
                pts.push([x, y]);
            }
            if pts.len() < 4 || !(pts.len() - 1).is_multiple_of(3) || pts.len() > 193 {
                self.report.error(
                    "custom_ease_not_supported",
                    &ease_loc,
                    "CustomEase curve must have 3*N + 1 Bézier control points.",
                );
                had_error = true;
                continue;
            }
            let last_pt = *pts.last().unwrap();
            if pts[0][0].abs() > 1e-6
                || pts[0][1].abs() > 1e-6
                || (last_pt[0] - 1.0).abs() > 1e-6
                || (last_pt[1] - 1.0).abs() > 1e-6
            {
                self.report.error(
                    "custom_ease_not_supported",
                    &ease_loc,
                    "CustomEase endpoints must span (0, 0) to (1, 1).",
                );
                had_error = true;
                continue;
            }
            let seg_count = (pts.len() - 1) / 3;
            let mut seg_valid = true;
            for m in 0..seg_count {
                let (p0, p1, p2, p3) = (pts[3 * m], pts[3 * m + 1], pts[3 * m + 2], pts[3 * m + 3]);
                let handles_in_bounds = if seg_count == 1 {
                    (0.0..=1.0).contains(&p1[0]) && (0.0..=1.0).contains(&p2[0])
                } else {
                    p3[0] > p0[0] + 1e-7
                        && p1[0] >= p0[0] - 1e-7
                        && p1[0] <= p3[0] + 1e-7
                        && p2[0] >= p0[0] - 1e-7
                        && p2[0] <= p3[0] + 1e-7
                };
                if !handles_in_bounds
                    || p0[1].abs() > 100.0
                    || p1[1].abs() > 100.0
                    || p2[1].abs() > 100.0
                    || p3[1].abs() > 100.0
                {
                    self.report.error(
                        "custom_ease_not_supported",
                        &ease_loc,
                        "CustomEase Bézier controls must have monotonic x in [0, 1] and |y| <= 100.",
                    );
                    seg_valid = false;
                    had_error = true;
                    break;
                }
            }
            if seg_valid {
                parsed_curves.push(XflCustomEaseCurve {
                    target,
                    points: pts,
                    location: ease_loc,
                });
            }
        }

        if had_error || parsed_curves.is_empty() {
            return (None, None);
        }

        // Check if this is a uniform global curve (`target="all"` or all supplied curves identical).
        let is_uniform_global = if single_ease {
            parsed_curves.iter().any(|c| c.target == "all")
                || (parsed_curves.len() > 1
                    && parsed_curves
                        .iter()
                        .all(|c| c.points == parsed_curves[0].points))
        } else {
            parsed_curves.len() == 1 && parsed_curves[0].target == "all"
                || (parsed_curves.len() >= 5
                    && parsed_curves
                        .iter()
                        .all(|c| c.points == parsed_curves[0].points))
        };

        if is_uniform_global {
            let chosen = parsed_curves
                .iter()
                .find(|c| c.target == "all")
                .unwrap_or(&parsed_curves[0]);
            if chosen.points.len() == 4 {
                return (single_segment_bezier(&chosen.points), None);
            }
            return (
                Some(fallback_ease),
                Some(PendingXflCustomEase {
                    curves: vec![XflCustomEaseCurve {
                        target: "all".into(),
                        points: chosen.points.clone(),
                        location: chosen.location.clone(),
                    }],
                    single_ease: true,
                    base_ease: fallback_ease,
                    location: chosen.location.clone(),
                }),
            );
        }

        // Per-property custom ease curves (`position`, `rotation`, `scale`, `color`, `filters`, `all`).
        let tween_loc = self.location(tweens, file);
        (
            Some(fallback_ease),
            Some(PendingXflCustomEase {
                curves: parsed_curves,
                single_ease: false,
                base_ease: fallback_ease,
                location: tween_loc,
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_pending_xfl_custom_eases(
        &mut self,
        layer: &mut Layer,
        has_motion_guide: bool,
        published_end: u32,
        pending: Vec<(u32, PendingXflCustomEase)>,
    ) {
        if pending.is_empty() {
            return;
        }
        // Process from latest keyframe to earliest so index insertions never shift earlier indices.
        let mut sorted = pending;
        sorted.sort_by_key(|(frame, _)| *frame);
        for (start_frame, spec) in sorted.into_iter().rev() {
            let Some(key_idx) = layer.keyframes.iter().position(|k| k.frame == start_frame) else {
                continue;
            };
            if key_idx + 1 >= layer.keyframes.len() {
                continue;
            }
            let kf_kind = layer.keyframes[key_idx].tween_kind;
            if layer.keyframes[key_idx].tween.is_none() || kf_kind != TweenKind::Classic {
                continue;
            }
            if layer.keyframes[key_idx].elements.len() != 1
                || layer.keyframes[key_idx + 1].elements.len() != 1
                || layer.keyframes[key_idx].elements[0].id
                    != layer.keyframes[key_idx + 1].elements[0].id
            {
                continue;
            }

            if spec.single_ease && spec.curves.len() == 1 && spec.curves[0].target == "all" {
                let pts = &spec.curves[0].points;
                match try_subdivide_global_xfl_custom_ease(layer, key_idx, pts, has_motion_guide) {
                    Ok(()) => {}
                    Err(_) => {
                        let mut by_target = BTreeMap::new();
                        by_target.insert("all", pts.as_slice());
                        if bake_custom_eased_poses(
                            layer,
                            key_idx,
                            &by_target,
                            spec.base_ease,
                            &mut self.motion_pose_budget,
                        ) {
                            self.report.warning(
                                "xfl_tween_easing",
                                &spec.location,
                                "Multi-segment XFL CustomEase has non-integer frame knots or unnormalizable sub-spans; sampled exact per-frame poses.",
                            );
                        } else {
                            self.report.warning(
                                "xfl_tween_easing",
                                &spec.location,
                                "Multi-segment XFL CustomEase exceeded the pose budget; approximated with standard tween easing.",
                            );
                        }
                    }
                }
                continue;
            }

            // Per-property `<CustomEase>` handling.
            let mut by_target: BTreeMap<&str, &[[f64; 2]]> = BTreeMap::new();
            let mut had_unsupported_target = false;
            for c in &spec.curves {
                match c.target.as_str() {
                    "all" | "position" | "rotation" | "scale" | "color" => {
                        by_target.insert(c.target.as_str(), c.points.as_slice());
                    }
                    "filters" => {
                        by_target.insert("filters", c.points.as_slice());
                        had_unsupported_target = true;
                    }
                    _ => {
                        had_unsupported_target = true;
                    }
                }
            }

            let can_use_native_property_eases = !had_unsupported_target
                && !has_motion_guide
                && by_target.values().all(|pts| pts.len() == 4)
                && key_idx + 2 == layer.keyframes.len()
                && layer.keyframes[key_idx + 1].tween.is_none()
                && layer.keyframes[key_idx + 1].frame.saturating_add(1) >= published_end
                && can_represent_as_motion_property_eases(
                    &layer.keyframes[key_idx],
                    &layer.keyframes[key_idx + 1],
                    &self.symbols,
                );

            if can_use_native_property_eases {
                let base = by_target
                    .get("all")
                    .and_then(|pts| single_segment_bezier(pts))
                    .unwrap_or(spec.base_ease);
                let mut prop_eases: Vec<(TweenProperty, Easing)> = Vec::new();
                if let Some(pts) = by_target.get("position")
                    && let Some(ez) = single_segment_bezier(pts)
                {
                    prop_eases.push((TweenProperty::X, ez));
                    prop_eases.push((TweenProperty::Y, ez));
                }
                if let Some(pts) = by_target.get("scale")
                    && let Some(ez) = single_segment_bezier(pts)
                {
                    prop_eases.push((TweenProperty::ScaleX, ez));
                    prop_eases.push((TweenProperty::ScaleY, ez));
                }
                if let Some(pts) = by_target.get("rotation")
                    && let Some(ez) = single_segment_bezier(pts)
                {
                    prop_eases.push((TweenProperty::Rotation, ez));
                    prop_eases.push((TweenProperty::Skew, ez));
                }
                if let Some(pts) = by_target.get("color")
                    && let Some(ez) = single_segment_bezier(pts)
                {
                    prop_eases.push((TweenProperty::Alpha, ez));
                    prop_eases.push((TweenProperty::Color, ez));
                }

                let start_kf = &mut layer.keyframes[key_idx];
                start_kf.tween = Some(base);
                start_kf.tween_kind = TweenKind::Motion;
                start_kf.property_eases = prop_eases;
                start_kf.classic_rotate_dir = ClassicRotateDirection::Auto;
                start_kf.classic_rotate_turns = 0;

                let end_kf = &mut layer.keyframes[key_idx + 1];
                end_kf.keyed_properties = Some(TweenProperty::ALL.to_vec());
            } else if bake_custom_eased_poses(
                layer,
                key_idx,
                &by_target,
                spec.base_ease,
                &mut self.motion_pose_budget,
            ) {
                self.report.warning(
                    "xfl_tween_easing",
                    &spec.location,
                    if had_unsupported_target {
                        "Per-property XFL CustomEase includes filter or non-track targets; sampled exact per-frame poses."
                    } else {
                        "Per-property XFL CustomEase on multi-segment or chained classic tween was sampled into exact per-frame poses."
                    },
                );
            } else {
                self.report.warning(
                    "xfl_tween_easing",
                    &spec.location,
                    "Per-property XFL CustomEase exceeded the pose budget; approximated with primary tween easing.",
                );
            }
        }
    }
}

fn can_represent_as_motion_property_eases(
    start_kf: &Keyframe,
    end_kf: &Keyframe,
    symbols: &HashMap<String, (String, SymbolKind)>,
) -> bool {
    if !start_kf.tween_scale
        || start_kf.orient_to_path
        || start_kf.classic_rotate_turns != 0
        || start_kf.classic_rotate_dir != ClassicRotateDirection::Auto
    {
        return false;
    }
    let a = &start_kf.elements[0];
    let b = &end_kf.elements[0];
    if (b.transform.rotation - a.transform.rotation).abs() > 180.0 || a.filters != b.filters {
        return false;
    }
    match (&a.kind, &b.kind) {
        (
            ElementKind::Instance {
                symbol_id: sa,
                playback: pa,
            },
            ElementKind::Instance {
                symbol_id: sb,
                playback: pb,
            },
        ) => {
            if sa != sb || pa != pb {
                return false;
            }
            let is_graphic = symbols
                .values()
                .any(|(sid, kind)| sid == sa && *kind == SymbolKind::Graphic);
            !is_graphic
                || matches!(
                    pa,
                    InstancePlayback::SingleFrame(_) | InstancePlayback::Independent
                )
        }
        (
            ElementKind::Bitmap {
                resource_id: ra, ..
            },
            ElementKind::Bitmap {
                resource_id: rb, ..
            },
        ) => ra == rb,
        _ => false,
    }
}

fn shortest_rotation_delta(a_deg: f64, b_deg: f64) -> f64 {
    let mut d = (b_deg - a_deg) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

#[allow(clippy::too_many_arguments)]
fn interpolate_element_with_eases(
    a_el: &Element,
    b_el: &Element,
    tween_scale: bool,
    v_pos: f64,
    v_scale: f64,
    v_rot: f64,
    v_col: f64,
    v_flt: f64,
) -> Element {
    let lerp_f = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let mut el = a_el.clone();
    el.transform.x = lerp_f(a_el.transform.x, b_el.transform.x, v_pos);
    el.transform.y = lerp_f(a_el.transform.y, b_el.transform.y, v_pos);
    if tween_scale {
        el.transform.scale_x = lerp_f(a_el.transform.scale_x, b_el.transform.scale_x, v_scale);
        el.transform.scale_y = lerp_f(a_el.transform.scale_y, b_el.transform.scale_y, v_scale);
    }
    let rot_delta = shortest_rotation_delta(a_el.transform.rotation, b_el.transform.rotation);
    el.transform.rotation = a_el.transform.rotation + rot_delta * v_rot;
    el.transform.skew = lerp_f(a_el.transform.skew, b_el.transform.skew, v_rot);
    el.transform.alpha = lerp_f(a_el.transform.alpha, b_el.transform.alpha, v_col).clamp(0.0, 1.0);
    el.transform.color_transform = ColorTransform {
        multipliers: std::array::from_fn(|c| {
            lerp_f(
                a_el.transform.color_transform.multipliers[c],
                b_el.transform.color_transform.multipliers[c],
                v_col,
            )
            .clamp(-10_000.0, 10_000.0)
        }),
        offsets: std::array::from_fn(|c| {
            lerp_f(
                a_el.transform.color_transform.offsets[c],
                b_el.transform.color_transform.offsets[c],
                v_col,
            )
            .clamp(-255.0, 255.0)
        }),
    };
    el.filters = if a_el.filters == b_el.filters {
        a_el.filters.clone()
    } else {
        let probe_frame = (v_flt.clamp(0.0, 1.0) * 1_000_000.0).round() as u32;
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
    // Cloned elements preserve `a_el.id` and `a_el.kind` (`InstancePlayback`), so
    // `neoflash_core::placement_start` tracks continuous placement age (`frame - start_frame`)
    // without inflating `firstFrame`.
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
            if tween_scale {
                *width = lerp_f(*aw, *bw, v_scale).max(0.0);
                *height = lerp_f(*ah, *bh, v_scale).max(0.0);
                *stroke_width = lerp_f(*asw, *bsw, v_scale).max(0.0);
            }
            *fill = std::array::from_fn(|c| {
                lerp_f(f64::from(af[c]), f64::from(bf[c]), v_col)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            *stroke = std::array::from_fn(|c| {
                lerp_f(f64::from(ast[c]), f64::from(bst[c]), v_col)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
        }
        _ => {}
    }
    el
}

fn try_subdivide_global_xfl_custom_ease(
    layer: &mut Layer,
    key_idx: usize,
    pts: &[[f64; 2]],
    has_motion_guide: bool,
) -> Result<(), &'static str> {
    if has_motion_guide {
        return Err("guided multi-segment custom ease");
    }
    let start_frame = layer.keyframes[key_idx].frame;
    let end_frame = layer.keyframes[key_idx + 1].frame;
    let dur = end_frame.saturating_sub(start_frame);
    if dur <= 1 {
        layer.keyframes[key_idx].tween = Some(Easing::Linear);
        return Ok(());
    }
    let seg_count = (pts.len() - 1) / 3;
    let mut splits = vec![0u32, dur];
    for m in 1..seg_count {
        let xk = pts[3 * m][0];
        let raw_f = xk * f64::from(dur);
        if (raw_f - raw_f.round()).abs() > 1e-5 {
            return Err("non-integer knot");
        }
        let k = (raw_f.round() as i64).clamp(0, i64::from(dur)) as u32;
        splits.push(k);
    }
    splits.sort_unstable();
    splits.dedup();

    let seg_for_interval = |u_a: f64, u_b: f64| -> Option<usize> {
        let mid = 0.5 * (u_a + u_b);
        (0..seg_count).find(|&m| pts[3 * m][0] - 1e-7 <= mid && mid <= pts[3 * m + 3][0] + 1e-7)
    };

    let mut split_v = Vec::with_capacity(splits.len());
    for &k in &splits {
        split_v.push(eval_multi_segment_points(
            pts,
            f64::from(k) / f64::from(dur),
        ));
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
        let m = seg_for_interval(ua, ub).ok_or("missing segment")?;
        let seg = [pts[3 * m], pts[3 * m + 1], pts[3 * m + 2], pts[3 * m + 3]];
        if ua < seg[0][0] - 1e-6 || ub > seg[3][0] + 1e-6 {
            return Err("sub-span crosses knot");
        }
        let sa = solve_monotonic_cubic_x(seg[0][0], seg[1][0], seg[2][0], seg[3][0], ua);
        let sb = solve_monotonic_cubic_x(seg[0][0], seg[1][0], seg[2][0], seg[3][0], ub);
        let sub = subdivide_cubic_2d_interval(seg, sa, sb);
        let dx = sub[3][0] - sub[0][0];
        let dy = sub[3][1] - sub[0][1];
        if dx <= 1e-9 {
            return Err("zero-width sub-span");
        }
        if dy.abs() <= 1e-7
            && (sub[1][1] - sub[0][1]).abs() <= 1e-6
            && (sub[2][1] - sub[0][1]).abs() <= 1e-6
        {
            sub_eases.push(Easing::Linear);
            continue;
        }
        if dy.abs() <= 1e-6 {
            return Err("inflected zero-delta sub-span");
        }
        let x1 = ((sub[1][0] - sub[0][0]) / dx).clamp(0.0, 1.0);
        let x2 = ((sub[2][0] - sub[0][0]) / dx).clamp(0.0, 1.0);
        let y1 = (sub[1][1] - sub[0][1]) / dy;
        let y2 = (sub[2][1] - sub[0][1]) / dy;
        if !y1.is_finite() || !y2.is_finite() || y1.abs() > 100.0 || y2.abs() > 100.0 {
            return Err("overshoot exceeds +/-100");
        }
        sub_eases.push(Easing::CubicBezier { x1, y1, x2, y2 });
    }

    let start_kf = layer.keyframes[key_idx].clone();
    let end_kf = layer.keyframes[key_idx + 1].clone();
    let a_el = &start_kf.elements[0];
    let b_el = &end_kf.elements[0];
    let rot_delta = shortest_rotation_delta(a_el.transform.rotation, b_el.transform.rotation);
    let target_end_rot = a_el.transform.rotation + rot_delta;
    if (target_end_rot - b_el.transform.rotation).abs() > 1e-6 && end_kf.tween.is_some() {
        return Err("branch-cut rotation continues into subsequent tween");
    }

    let mut inserted = Vec::with_capacity(splits.len().saturating_sub(2));
    for idx in 1..splits.len() - 1 {
        let k_off = splits[idx];
        let v = split_v[idx];
        let el = interpolate_element_with_eases(a_el, b_el, start_kf.tween_scale, v, v, v, v, v);
        inserted.push(Keyframe {
            frame: start_frame + k_off,
            elements: vec![el],
            tween: Some(sub_eases[idx]),
            tween_kind: TweenKind::Classic,
            tween_scale: start_kf.tween_scale,
            orient_to_path: start_kf.orient_to_path,
            sync_symbols: start_kf.sync_symbols,
            snap_to_guide: start_kf.snap_to_guide,
            ..Default::default()
        });
    }

    layer.keyframes[key_idx].tween = Some(sub_eases[0]);
    layer.keyframes[key_idx].classic_rotate_dir = ClassicRotateDirection::Auto;
    layer.keyframes[key_idx].classic_rotate_turns = 0;
    layer.keyframes[key_idx + 1].elements[0].transform.rotation = target_end_rot;
    layer.keyframes.splice(key_idx + 1..key_idx + 1, inserted);
    Ok(())
}

fn bake_custom_eased_poses(
    layer: &mut Layer,
    key_idx: usize,
    by_target: &BTreeMap<&str, &[[f64; 2]]>,
    base_ease: Easing,
    budget: &mut usize,
) -> bool {
    let start_frame = layer.keyframes[key_idx].frame;
    let end_frame = layer.keyframes[key_idx + 1].frame;
    let dur = end_frame.saturating_sub(start_frame);
    if dur <= 1 {
        layer.keyframes[key_idx].tween = Some(Easing::Linear);
        return true;
    }
    let count = (dur - 1) as usize;
    let a_el = layer.keyframes[key_idx].elements[0].clone();
    let b_el = layer.keyframes[key_idx + 1].elements[0].clone();
    let size = serde_json::to_vec(&a_el).map_or(256, |v| v.len());
    let cost = count.saturating_mul(size.div_ceil(2048).max(1));
    if count > 4095 || cost > *budget {
        return false;
    }

    let tween_scale = layer.keyframes[key_idx].tween_scale;

    let eval_prop = |name: &str, u: f64, fallback: f64| -> f64 {
        by_target
            .get(name)
            .map_or(fallback, |pts| eval_multi_segment_points(pts, u))
    };

    let mut poses = Vec::with_capacity(count);
    for step in 1..dur {
        let u = f64::from(step) / f64::from(dur);
        let v_all = by_target.get("all").map_or_else(
            || easing(u, base_ease),
            |pts| eval_multi_segment_points(pts, u),
        );
        let v_pos = eval_prop("position", u, v_all);
        let v_scale = eval_prop("scale", u, v_all);
        let v_rot = eval_prop("rotation", u, v_all);
        let v_col = eval_prop("color", u, v_all);
        let v_flt = eval_prop("filters", u, v_all);
        let el = interpolate_element_with_eases(
            &a_el,
            &b_el,
            tween_scale,
            v_pos,
            v_scale,
            v_rot,
            v_col,
            v_flt,
        );
        poses.push(Keyframe {
            frame: start_frame + step,
            elements: vec![el],
            ..Default::default()
        });
    }

    *budget -= cost;
    layer.keyframes[key_idx].tween = None;
    layer.keyframes[key_idx].orient_to_path = false;
    layer.keyframes[key_idx].classic_rotate_dir = ClassicRotateDirection::Auto;
    layer.keyframes[key_idx].classic_rotate_turns = 0;
    layer.keyframes.splice(key_idx + 1..key_idx + 1, poses);
    true
}
