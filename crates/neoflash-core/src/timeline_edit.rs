//! Atomic authoring edits over rectangular frame ranges. Frame scripts belong to
//! the symbol, so their inclusion is always explicit and never repeated per row.
use crate::*;
use std::collections::{HashMap, HashSet};

const MAX_FRAME: u32 = 1_000_000;
const MAX_BAKED_RECORDS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRange {
    pub layers: Vec<Id>,
    pub start: u32,
    /// Inclusive last selected frame.
    pub end: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameClipboard {
    pub frame_count: u32,
    pub layers: Vec<FrameLayerClip>,
    pub scripts: Vec<FrameScript>,
    pub includes_scripts: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameLayerClip {
    pub source_layer: Id,
    /// Relative frame coordinates, starting with the sampled first frame.
    pub keyframes: Vec<Keyframe>,
    /// Exclusive end within the copied rectangle; zero means empty exposure.
    pub end_frame: u32,
}
#[derive(Clone, Debug)]
pub enum TimelineEdit {
    /// Clear contents without shifting later frames. Used by Cut after copying.
    Clear {
        range: FrameRange,
        include_scripts: bool,
    },
    /// Remove time from selected rows, shifting their later frames left.
    Delete {
        range: FrameRange,
        include_scripts: bool,
    },
    /// Insert held time before `at`, shifting later frames on selected rows right.
    Insert {
        layers: Vec<Id>,
        at: u32,
        count: u32,
        include_scripts: bool,
    },
    Paste {
        clip: FrameClipboard,
        layers: Vec<Id>,
        at: u32,
        include_scripts: bool,
    },
    Move {
        range: FrameRange,
        layers: Vec<Id>,
        at: u32,
        copy: bool,
        include_scripts: bool,
    },
    /// Explicitly trim or extend exposure; trimming discards keys at/after end.
    SetEnd { layers: Vec<Id>, end_frame: u32 },
    /// Mirror the selected frames in time (Modify ▸ Timeline ▸ Reverse frames).
    /// Content before and after the range is unchanged.
    Reverse { range: FrameRange },
    /// Make every selected frame a keyframe holding its visible pose, or a
    /// blank keyframe (Convert to keyframes / Convert to blank keyframes).
    ConvertToKeyframes { range: FrameRange, blank: bool },
    /// Synchronise symbols: every graphic instance on a selected keyframe continues the nested
    /// timeline of the same instance (or the same symbol) on the keyframe before it, so the
    /// nested animation stays in step across keyframe splits and tweens.
    SyncSymbols { range: FrameRange },
}
fn layer_indices(timeline: &Timeline, ids: &[Id]) -> Result<Vec<usize>, String> {
    if ids.is_empty() {
        return Err("Select at least one artwork layer.".into());
    }
    let mut seen = HashSet::new();
    ids.iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err("A frame range cannot repeat a layer.".into());
            }
            let index = timeline
                .layers
                .iter()
                .position(|layer| &layer.id == id)
                .ok_or_else(|| "A selected layer no longer exists.".to_owned())?;
            let layer = &timeline.layers[index];
            if layer.properties.role == LayerRole::Folder {
                return Err("Select artwork rows, not a folder row.".into());
            }
            if !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                return Err(format!(
                    "Layer '{}' is hidden or locked. Show and unlock it before editing frames.",
                    layer.name
                ));
            }
            Ok(index)
        })
        .collect()
}
fn range_end(timeline: &Timeline, range: &FrameRange) -> Result<u32, String> {
    if range.start > range.end || range.end >= timeline.frame_count {
        return Err("The frame range is outside the timeline.".into());
    }
    Ok(range.end + 1)
}
fn exposure(layer: &Layer, count: u32) -> u32 {
    layer.properties.end_frame.unwrap_or(count)
}
fn blank(frame: u32) -> Keyframe {
    Keyframe {
        frame,
        elements: vec![],
        tween: None,
        label: String::new(),
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
struct TimelineSampleContext<'a> {
    timeline: &'a Timeline,
    fps: f64,
}

fn sampled_key(timeline: &Timeline, layer: &Layer, frame: u32, fps: f64) -> Keyframe {
    let mut key = blank(frame);
    key.elements = sample_layer_in_at_fps(timeline, layer, frame, fps);
    if let Some(original) = layer.keyframes.iter().find(|key| key.frame == frame) {
        key.label = original.label.clone();
    }
    key
}
fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn bez_coord(u: f64, p1: f64, p2: f64) -> f64 {
    let inv = 1.0 - u;
    3.0 * inv * inv * u * p1 + 3.0 * inv * u * u * p2 + u * u * u
}

fn solve_bezier_x(target_x: f64, x1: f64, x2: f64) -> f64 {
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

fn subdivide_cubic_1d(p0: f64, p1: f64, p2: f64, p3: f64, s0: f64, s1: f64) -> [f64; 4] {
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

fn subdivide_cubic_bezier([x1, y1, x2, y2]: [f64; 4], t0: f64, t1: f64) -> Option<Easing> {
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

fn reverse_easing(ease: Easing) -> Easing {
    match ease {
        Easing::Linear => Easing::Linear,
        Easing::EaseIn => Easing::EaseOut,
        Easing::EaseOut => Easing::EaseIn,
        Easing::EaseInOut => Easing::EaseInOut,
        Easing::Flash { acceleration } => Easing::Flash {
            acceleration: -acceleration,
        },
        Easing::CubicBezier { x1, y1, x2, y2 } => Easing::CubicBezier {
            x1: 1.0 - x2,
            y1: 1.0 - y2,
            x2: 1.0 - x1,
            y2: 1.0 - y1,
        },
        Easing::Bounce | Easing::Elastic => ease,
    }
}

fn subdivide_single_ease(ease: Easing, t0: f64, t1: f64) -> Option<Easing> {
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

type SubdividedMotionEases = (
    Easing,
    Vec<(TweenProperty, Easing)>,
    Vec<MotionTrackOverride>,
);

fn subdivide_motion_eases(first_key: &Keyframe, t0: f64, t1: f64) -> Option<SubdividedMotionEases> {
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

fn has_object_motion_path(key: &Keyframe) -> bool {
    key.motion_path.is_some()
        || key
            .motion_tracks
            .iter()
            .any(|track| matches!(track.motion_path, Some(MotionPathOverride::Path { .. })))
}

fn has_noninvertible_motion_ease(key: &Keyframe) -> bool {
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

fn has_object_motion_timing(layer: &Layer, start_idx: usize, end_frame: u32) -> bool {
    let start = &layer.keyframes[start_idx];
    start.motion_tracks.iter().any(|track| {
        track.easing.is_some_and(|ease| ease != Easing::Linear)
            || track.property_eases.as_ref().is_some_and(|property_eases| {
                property_eases
                    .iter()
                    .any(|(_, ease)| *ease != Easing::Linear)
            })
    }) || layer.keyframes[start_idx + 1..]
        .iter()
        .take_while(|key| key.frame < end_frame && key.keyed_properties.is_some())
        .any(|key| {
            key.motion_keys
                .as_ref()
                .is_some_and(|object_keys| object_keys.iter().any(|object_key| object_key.roving))
        })
}

/// Subdivide a classic tween `[first, last]` onto `[sub_start, sub_end]`, returning
/// the tweened start key(s) in `sub_start..sub_end` (excluding the endpoint key at `sub_end`).
fn subdivide_classic_subrange(
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

fn charge_bake_budget(
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
fn isolate_range_impl(
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
    let effective_end = original.properties.end_frame.unwrap_or(MAX_FRAME);

    enum SpanKind {
        Motion { start_idx: usize, end_frame: u32 },
        ClassicOrShape { start_idx: usize, last_frame: u32 },
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
                let end_frame = original
                    .keyframes
                    .get(j)
                    .map_or(effective_end, |k| k.frame)
                    .max(key.frame + 1);
                spans.push(SpanKind::Motion {
                    start_idx: i,
                    end_frame,
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
                let span_len = (end_frame - first).max(1) as f64;
                let can_subdivide_nonlinear = !all_linear
                    && !bake_interior
                    && original.properties.motion_guide.is_none()
                    && (first >= cut_start
                        || cut_start - 1 == first
                        || subdivide_motion_eases(
                            first_key,
                            0.0,
                            (cut_start - first) as f64 / span_len,
                        )
                        .is_some())
                    && (cut_end >= end_frame
                        || cut_end + 1 == end_frame
                        || subdivide_motion_eases(
                            first_key,
                            (cut_end - first) as f64 / span_len,
                            1.0,
                        )
                        .is_some());
                if (all_linear || can_subdivide_nonlinear)
                    && original.properties.motion_guide.is_none()
                    && !has_object_motion_path(first_key)
                    && !has_object_motion_timing(&original, start_idx, end_frame)
                {
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
                            if !all_linear
                                && let Some((sub_tween, sub_props, sub_tracks)) =
                                    subdivide_motion_eases(
                                        first_key,
                                        0.0,
                                        (cut_start - first) as f64 / span_len,
                                    )
                            {
                                left_start.tween = Some(sub_tween);
                                left_start.property_eases = sub_props;
                                left_start.motion_tracks = sub_tracks;
                            }
                            replacement.push(left_start.clone());
                            for k in &original.keyframes[start_idx + 1..] {
                                if k.frame >= end_frame || k.keyed_properties.is_none() {
                                    break;
                                }
                                if k.frame > first && k.frame < left_last {
                                    replacement.push(k.clone());
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
                            end_prop.tween = left_start.tween;
                            end_prop.tween_kind = TweenKind::Motion;
                            end_prop.keyed_properties = Some(TweenProperty::ALL.to_vec());
                            if original.keyframes[start_idx + 1..]
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
                            replacement.push(end_prop);
                        }
                    }
                    if bake_interior {
                        let b0 = first.max(cut_start);
                        let b1 = end_frame.min(cut_end);
                        charge_bake_budget(&original, b0, b1, budget)?;
                        for f in b0..b1 {
                            replacement.push(sampled_key(timeline, &original, f, fps));
                        }
                    }
                    if cut_end < end_frame {
                        if cut_end + 1 == end_frame {
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
                            let (right_tween, right_props, right_tracks) = if !all_linear
                                && let Some((sub_t, sub_p, sub_tracks)) = subdivide_motion_eases(
                                    first_key,
                                    (cut_end - first) as f64 / span_len,
                                    1.0,
                                ) {
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
                            right_start.property_eases = right_props;
                            right_start.motion_tracks = right_tracks;
                            right_start.motion_keys = None;
                            right_start.sync_symbols = first_key.sync_symbols;
                            right_start.snap_to_guide = first_key.snap_to_guide;
                            replacement.push(right_start);
                            for k in &original.keyframes[start_idx + 1..] {
                                if k.frame >= end_frame || k.keyed_properties.is_none() {
                                    break;
                                }
                                if k.frame > cut_end && k.frame < end_frame {
                                    replacement.push(k.clone());
                                }
                            }
                        }
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

fn isolate_range(
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
fn bake_touched(
    timeline: &Timeline,
    layer: &mut Layer,
    start: u32,
    end: u32,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    isolate_range_impl(timeline, layer, start, end, true, budget, fps)
}
fn split(timeline: &Timeline, layer: &mut Layer, frame: u32, fps: f64) {
    if frame >= layer.properties.end_frame.unwrap_or(MAX_FRAME)
        || layer.keyframes.iter().any(|key| key.frame == frame)
    {
        return;
    }
    let key = sampled_key(timeline, layer, frame, fps);
    layer.keyframes.push(key);
    layer.keyframes.sort_by_key(|key| key.frame);
}
pub fn copy_frames(
    project: &Project,
    symbol: &str,
    range: &FrameRange,
    include_scripts: bool,
) -> Result<FrameClipboard, String> {
    let fps = f64::from(project.fps);
    let timeline = &project
        .symbol(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let end = range_end(timeline, range)?;
    let indices = layer_indices(timeline, &range.layers)?;
    let mut budget = 0;
    let mut layers = Vec::with_capacity(indices.len());
    for index in indices {
        let mut layer = timeline.layers[index].clone();
        let effective_end = exposure(&layer, timeline.frame_count);
        layer.properties.end_frame = Some(effective_end);
        bake_touched(timeline, &mut layer, range.start, end, &mut budget, fps)?;
        split(timeline, &mut layer, range.start, fps);
        let keyframes = layer
            .keyframes
            .into_iter()
            .filter(|key| key.frame >= range.start && key.frame < end)
            .map(|mut key| {
                key.frame -= range.start;
                key.tween = None;
                key
            })
            .collect();
        layers.push(FrameLayerClip {
            source_layer: layer.id,
            keyframes,
            end_frame: effective_end.min(end).saturating_sub(range.start),
        });
    }
    let scripts = if include_scripts {
        timeline
            .scripts
            .iter()
            .filter(|script| script.frame >= range.start && script.frame < end)
            .cloned()
            .map(|mut script| {
                script.frame -= range.start;
                script
            })
            .collect()
    } else {
        vec![]
    };
    Ok(FrameClipboard {
        frame_count: end - range.start,
        layers,
        scripts,
        includes_scripts: include_scripts,
    })
}
fn clear_rows(
    timeline: &mut Timeline,
    range: &FrameRange,
    budget: &mut usize,
    fps: f64,
) -> Result<(), String> {
    let end = range_end(timeline, range)?;
    for index in layer_indices(timeline, &range.layers)? {
        let mut layer = timeline.layers[index].clone();
        isolate_range(timeline, &mut layer, range.start, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        layer
            .keyframes
            .retain(|key| key.frame < range.start || key.frame >= end);
        if range.start < layer.properties.end_frame.unwrap_or(0) {
            layer.keyframes.push(blank(range.start));
        }
        layer.keyframes.sort_by_key(|key| key.frame);
        timeline.layers[index] = layer;
    }
    Ok(())
}
fn remove_scripts(timeline: &mut Timeline, range: &FrameRange) {
    timeline
        .scripts
        .retain(|script| script.frame < range.start || script.frame > range.end);
}
fn paste_rows(
    project: &mut Project,
    symbol: &str,
    clip: &FrameClipboard,
    layers: &[Id],
    at: u32,
    include_scripts: bool,
    budget: &mut usize,
) -> Result<(), String> {
    let fps = f64::from(project.fps);
    let end = at
        .checked_add(clip.frame_count)
        .filter(|end| *end <= MAX_FRAME)
        .ok_or("Pasted frames exceed the timeline limit.")?;
    if clip.frame_count == 0 || clip.layers.len() != layers.len() {
        return Err(
            "Paste requires the same number of destination artwork layers as copied rows.".into(),
        );
    }
    if clip
        .scripts
        .iter()
        .any(|script| script.frame >= clip.frame_count)
    {
        return Err("Frame clipboard contains a script outside its copied range.".into());
    }
    if include_scripts && !clip.includes_scripts {
        return Err("This clipboard was copied without frame scripts. Copy again with Include frame scripts enabled.".into());
    }
    let timeline = &project
        .symbol(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let indices = layer_indices(timeline, layers)?;
    if include_scripts
        && clip.scripts.iter().any(|incoming| {
            timeline
                .scripts
                .iter()
                .any(|existing| existing.frame == at + incoming.frame)
        })
    {
        return Err("A destination frame already has global code. Move it first or paste without frame scripts; no code was replaced.".into());
    }
    let mut owners: HashMap<Id, Id> = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|layer| {
            layer.keyframes.iter().flat_map(move |key| {
                key.elements
                    .iter()
                    .map(move |e| (e.id.clone(), layer.id.clone()))
            })
        })
        .collect();
    let mut all_ids: HashSet<Id> = owners.keys().cloned().collect();
    for source in &clip.layers {
        all_ids.extend(
            source
                .keyframes
                .iter()
                .flat_map(|key| key.elements.iter().map(|e| e.id.clone())),
        );
    }
    let timeline = &mut project.symbol_mut(symbol).unwrap().timeline;
    timeline.frame_count = timeline.frame_count.max(end);
    for (source, index) in clip.layers.iter().zip(indices) {
        if source.end_frame > clip.frame_count
            || source
                .keyframes
                .iter()
                .any(|key| key.frame >= source.end_frame)
            || source
                .keyframes
                .windows(2)
                .any(|keys| keys[0].frame >= keys[1].frame)
        {
            return Err(
                "Frame clipboard has invalid exposure bounds or duplicate/unordered keys.".into(),
            );
        }
        let mut layer = timeline.layers[index].clone();
        let old_end = layer.properties.end_frame.unwrap_or(0);
        isolate_range(timeline, &mut layer, at, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        layer
            .keyframes
            .retain(|key| key.frame < at || key.frame >= end);
        // A paste beyond an existing end must leave the intervening gap empty.
        if at > old_end {
            layer.keyframes.push(blank(old_end));
        }
        let mut remap = HashMap::new();
        for element in source.keyframes.iter().flat_map(|key| &key.elements) {
            if owners
                .get(&element.id)
                .is_some_and(|owner| owner != &layer.id)
            {
                remap.entry(element.id.clone()).or_insert_with(|| {
                    let mut id = new_id("object");
                    while all_ids.contains(&id) {
                        id = new_id("object");
                    }
                    all_ids.insert(id.clone());
                    owners.insert(id.clone(), layer.id.clone());
                    id
                });
            } else {
                owners.insert(element.id.clone(), layer.id.clone());
            }
        }
        for key in &source.keyframes {
            let mut key = key.clone();
            key.frame += at;
            for track in &mut key.motion_tracks {
                if let Some(id) = remap.get(&track.element_id) {
                    track.element_id = id.clone();
                }
            }
            if let Some(object_keys) = &mut key.motion_keys {
                for object_key in object_keys {
                    if let Some(id) = remap.get(&object_key.element_id) {
                        object_key.element_id = id.clone();
                    }
                }
            }
            for element in &mut key.elements {
                if let Some(id) = remap.get(&element.id) {
                    element.id = id.clone();
                }
            }
            layer.keyframes.push(key);
        }
        let pasted_end = at + source.end_frame;
        let new_end = old_end.max(pasted_end);
        // Empty copied rows must clear their whole rectangle, not keep the old
        // preceding exposure alive. Keep later destination material untouched.
        if at < new_end && source.keyframes.first().is_none_or(|key| key.frame != 0) {
            layer.keyframes.push(blank(at));
        }
        if pasted_end < end && pasted_end < new_end {
            layer.keyframes.push(blank(pasted_end));
        }
        layer.properties.end_frame = Some(new_end);
        layer.keyframes.retain(|key| key.frame < new_end);
        layer.keyframes.sort_by_key(|key| key.frame);
        layer.keyframes.dedup_by_key(|key| key.frame);
        timeline.layers[index] = layer;
    }
    if include_scripts {
        timeline
            .scripts
            .extend(clip.scripts.iter().cloned().map(|mut script| {
                script.frame += at;
                script
            }));
        timeline.scripts.sort_by_key(|script| script.frame);
    }
    Ok(())
}
/// Applies a complete operation to a private candidate. Errors never leave a
/// partial document, changed IDs, deleted code, or an incomplete multi-row edit.
/// Playback first frame continuing `previous` (placed at `previous_start`) at `frame`, for an
/// instance placed at `start`; `None` when the playback mode has no first frame to adjust.
fn continued_playback(
    previous: InstancePlayback,
    previous_start: u32,
    current: InstancePlayback,
    start: u32,
    frame: u32,
    count: u32,
) -> Option<InstancePlayback> {
    let first = match previous {
        InstancePlayback::Loop | InstancePlayback::PlayOnce => 0,
        InstancePlayback::LoopFrom(first) | InstancePlayback::PlayOnceFrom(first) => first,
        InstancePlayback::Independent | InstancePlayback::SingleFrame(_) => return None,
    };
    // Unwrapped nested frame shown by the previous placement at `frame`, minus the age the
    // current placement already has there.
    let phase = u64::from(first) + u64::from(frame.saturating_sub(previous_start));
    let age = u64::from(frame.saturating_sub(start));
    let count = u64::from(count.max(1));
    let continued = match current {
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            (phase + count - age % count) % count
        }
        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
            phase.saturating_sub(age).min(count - 1)
        }
        InstancePlayback::Independent | InstancePlayback::SingleFrame(_) => return None,
    } as u32;
    Some(match current {
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) if continued == 0 => {
            InstancePlayback::Loop
        }
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            InstancePlayback::LoopFrom(continued)
        }
        _ if continued == 0 => InstancePlayback::PlayOnce,
        _ => InstancePlayback::PlayOnceFrom(continued),
    })
}

/// Synchronize graphic symbol playback on `layer` across `start..=end` without requiring a change.
pub fn sync_layer_symbols(project: &Project, layer: &mut Layer, start: u32, end: u32) -> usize {
    let graphics: HashMap<Id, u32> = project
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Graphic)
        .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
        .collect();
    sync_symbols(layer, start, end, &graphics, None)
}

/// Synchronize one graphic symbol placement without changing sibling instances on the layer.
pub fn sync_layer_symbol(
    project: &Project,
    layer: &mut Layer,
    start: u32,
    end: u32,
    element_id: &str,
) -> usize {
    let graphics: HashMap<Id, u32> = project
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Graphic)
        .map(|symbol| (symbol.id.clone(), symbol.timeline.frame_count.max(1)))
        .collect();
    sync_symbols(layer, start, end, &graphics, Some(element_id))
}

/// Synchronise symbols on one layer: keyframes in `start..=end` (in frame order) continue the
/// graphic instances of the keyframe before each of them. Returns the number of changed instances.
fn sync_symbols(
    layer: &mut Layer,
    start: u32,
    end: u32,
    graphics: &HashMap<Id, u32>,
    target_element: Option<&str>,
) -> usize {
    layer.keyframes.sort_by_key(|key| key.frame);
    let mut changed = 0;
    for index in 1..layer.keyframes.len() {
        let frame = layer.keyframes[index].frame;
        if frame < start || frame > end {
            continue;
        }
        let previous = layer.keyframes[index - 1].clone();
        for element_index in 0..layer.keyframes[index].elements.len() {
            let element = &layer.keyframes[index].elements[element_index];
            if target_element.is_some_and(|target| element.id != target) {
                continue;
            }
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &element.kind
            else {
                continue;
            };
            let Some(&count) = graphics.get(symbol_id) else {
                continue;
            };
            let instance = |e: &&Element| matches!(&e.kind, ElementKind::Instance { symbol_id: s, .. } if s == symbol_id);
            let exact_before = previous
                .elements
                .iter()
                .filter(instance)
                .find(|candidate| candidate.id == element.id);
            let Some(before) = exact_before.or_else(|| {
                if target_element.is_some() {
                    let ordinal = layer.keyframes[index].elements[..=element_index]
                        .iter()
                        .filter(|candidate| {
                            matches!(&candidate.kind, ElementKind::Instance { symbol_id: candidate_symbol, .. } if candidate_symbol == symbol_id)
                        })
                        .count()
                        .saturating_sub(1);
                    previous.elements.iter().filter(instance).nth(ordinal)
                } else {
                    previous.elements.iter().find(instance)
                }
            }) else {
                continue;
            };
            let ElementKind::Instance {
                playback: before_playback,
                ..
            } = &before.kind
            else {
                continue;
            };
            let previous_start =
                crate::evaluate::placement_start(layer, previous.frame, &before.id);
            let start = crate::evaluate::placement_start(layer, frame, &element.id);
            let Some(next) = continued_playback(
                *before_playback,
                previous_start,
                *playback,
                start,
                frame,
                count,
            ) else {
                continue;
            };
            if next != *playback {
                if let ElementKind::Instance { playback, .. } =
                    &mut layer.keyframes[index].elements[element_index].kind
                {
                    *playback = next;
                }
                changed += 1;
            }
        }
    }
    changed
}

/// Converts frames in `start..=end` on `layer` to keyframes at every `step` frames
/// (`start, start + step, start + 2*step, ...` up to `end`), sampling the evaluated poses
/// via `sample_layer_in_at_fps`, removing intermediate keyframes inside each `step` interval,
/// and either stripping tween/motion metadata (`strip_tween = true`, ToonMonkey `Tween2Keys`)
/// or preserving tween easing (`strip_tween = false`, Convert to Keyframes Advanced).
#[allow(dead_code)]
pub fn convert_layer_to_keyframes_stepped(
    timeline: &Timeline,
    layer: &mut Layer,
    start: u32,
    end: u32,
    step: u32,
    strip_tween: bool,
    fps: f64,
) -> usize {
    let step = step.max(1);
    if start > end || end >= MAX_FRAME {
        return 0;
    }
    layer.keyframes.sort_by_key(|key| key.frame);
    let original = layer.clone();
    let effective_end = original
        .properties
        .end_frame
        .unwrap_or(timeline.frame_count)
        .max(end + 1);
    if layer.properties.end_frame.is_some() {
        layer.properties.end_frame = Some(effective_end);
    }

    let mut stepped_frames: Vec<u32> = Vec::new();
    let mut f = start;
    while f <= end {
        stepped_frames.push(f);
        match f.checked_add(step) {
            Some(next) if next > f => f = next,
            _ => break,
        }
    }
    if stepped_frames.last().copied() != Some(end)
        && original
            .keyframes
            .iter()
            .any(|k| k.frame == end && k.keyed_properties.is_none())
    {
        stepped_frames.push(end);
    }

    let active_ease_at = |frame: u32| -> Option<(Easing, TweenKind)> {
        if let Some((span_start, _, span_end)) = motion_span(&original, frame) {
            let end_limit = span_end.unwrap_or(effective_end);
            if frame < end_limit {
                return span_start.tween.map(|ease| (ease, TweenKind::Classic));
            }
        }
        let idx = original.keyframes.partition_point(|k| k.frame <= frame);
        let key = idx.checked_sub(1).and_then(|i| original.keyframes.get(i))?;
        let next_frame = original
            .keyframes
            .get(idx)
            .map_or(effective_end, |next| next.frame);
        if frame < next_frame {
            key.tween.map(|ease| {
                let kind = if key.tween_kind == TweenKind::Shape {
                    TweenKind::Shape
                } else {
                    TweenKind::Classic
                };
                (ease, kind)
            })
        } else {
            None
        }
    };

    let mut replacement = Vec::with_capacity(stepped_frames.len());
    for (idx, &frame) in stepped_frames.iter().enumerate() {
        let mut key = sampled_key(timeline, &original, frame, fps);
        if let Some(old) = original.keyframes.iter().find(|k| k.frame == frame) {
            key.label = old.label.clone();
            key.sound = old.sound.clone();
        }
        if strip_tween {
            key.tween = None;
            key.tween_kind = TweenKind::Classic;
            key.keyed_properties = None;
            key.motion_keys = None;
            key.motion_tracks.clear();
            key.motion_path = None;
            key.property_eases.clear();
            key.shape_hints.clear();
        } else {
            let is_last_in_range = idx + 1 == stepped_frames.len() && frame == end;
            if is_last_in_range {
                if let Some(old_end) = original
                    .keyframes
                    .iter()
                    .find(|k| k.frame == frame && k.keyed_properties.is_none())
                {
                    key.tween = old_end.tween;
                    key.tween_kind =
                        if key.tween.is_some() && old_end.tween_kind == TweenKind::Shape {
                            TweenKind::Shape
                        } else {
                            TweenKind::Classic
                        };
                } else if let Some((ease, kind)) = active_ease_at(frame) {
                    key.tween = Some(ease);
                    key.tween_kind = kind;
                }
            } else if let Some((ease, kind)) = active_ease_at(frame) {
                key.tween = Some(ease);
                key.tween_kind = kind;
            }
            key.keyed_properties = None;
            key.motion_keys = None;
            key.motion_tracks.clear();
            key.motion_path = None;
            key.property_eases.clear();
        }
        replacement.push(key);
    }

    // Preserve pose immediately after `end` if the original layer was mid-tween across `end + 1`
    // and we stripped tweens inside `start..=end`.
    if strip_tween
        && end + 1 < effective_end
        && !original.keyframes.iter().any(|k| k.frame == end + 1)
        && active_ease_at(end).is_some()
    {
        let mut after_key = sampled_key(timeline, &original, end + 1, fps);
        if let Some((ease, kind)) = active_ease_at(end + 1) {
            after_key.tween = Some(ease);
            after_key.tween_kind = kind;
        }
        replacement.push(after_key);
    }

    layer
        .keyframes
        .retain(|key| key.frame < start || key.frame > end);
    layer.keyframes.extend(replacement);
    layer.keyframes.sort_by_key(|key| key.frame);
    layer.keyframes.dedup_by_key(|key| key.frame);
    layer.normalize_motion_keys();
    stepped_frames.len()
}

pub fn edit_frames(project: &mut Project, symbol: &str, edit: TimelineEdit) -> Result<(), String> {
    let mut candidate = project.clone();
    let fps = f64::from(candidate.fps);
    // Graphic symbols and their frame counts (Synchronise symbols only).
    let graphics: HashMap<Id, u32> = match &edit {
        TimelineEdit::SyncSymbols { .. } => candidate
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Graphic)
            .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
            .collect(),
        _ => HashMap::new(),
    };
    let timeline = &mut candidate
        .symbol_mut(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let old_count = timeline.frame_count;
    for layer in &mut timeline.layers {
        if layer.properties.end_frame.is_none() {
            layer.properties.end_frame = Some(old_count);
        }
    }
    let mut budget = 0;
    match edit {
        TimelineEdit::Clear {
            range,
            include_scripts,
        } => {
            clear_rows(timeline, &range, &mut budget, fps)?;
            if include_scripts {
                remove_scripts(timeline, &range);
            }
        }
        TimelineEdit::Delete {
            range,
            include_scripts,
        } => {
            let end = range_end(timeline, &range)?;
            let count = end - range.start;
            for index in layer_indices(timeline, &range.layers)? {
                let mut layer = timeline.layers[index].clone();
                let old_end = layer.properties.end_frame.unwrap_or(0);
                if layer.keyframes.iter().any(has_object_motion_path) {
                    isolate_range(timeline, &mut layer, range.start, end, &mut budget, fps)?;
                    split(timeline, &mut layer, end, fps);
                } else if end < old_end
                    && !layer
                        .keyframes
                        .iter()
                        .any(|k| k.frame == end && k.keyed_properties.is_none())
                    && let Some(pos) = layer.keyframes.iter().rposition(|k| {
                        k.frame >= range.start && k.frame < end && k.keyed_properties.is_none()
                    })
                {
                    if layer.keyframes[pos].tween.is_some() {
                        let sampled = sample_layer_in_at_fps(timeline, &layer, end, fps);
                        layer.keyframes[pos].elements = sampled;
                    }
                    layer.keyframes[pos].frame = end;
                }
                layer
                    .keyframes
                    .retain(|key| key.frame < range.start || key.frame >= end);
                for key in &mut layer.keyframes {
                    if key.frame >= end {
                        key.frame -= count;
                    }
                }
                let new_end = old_end.saturating_sub(old_end.min(end).saturating_sub(range.start));
                if layer.keyframes.is_empty() && new_end > 0 {
                    layer.keyframes.push(blank(0));
                }
                layer.properties.end_frame = Some(new_end);
                timeline.layers[index] = layer;
            }
            if include_scripts {
                remove_scripts(timeline, &range);
                for script in &mut timeline.scripts {
                    if script.frame >= end {
                        script.frame -= count;
                    }
                }
            }
        }
        TimelineEdit::Insert {
            layers,
            at,
            count,
            include_scripts,
        } => {
            if count == 0
                || at > MAX_FRAME
                || old_count
                    .max(at)
                    .checked_add(count)
                    .is_none_or(|end| end > MAX_FRAME)
            {
                return Err("Inserted time is outside the supported timeline range.".into());
            }
            for index in layer_indices(timeline, &layers)? {
                let mut layer = timeline.layers[index].clone();
                let old_end = layer.properties.end_frame.unwrap_or(0);
                if at <= old_end {
                    let has_key_at_or_before = layer.keyframes.iter().any(|k| k.frame <= at);
                    for key in &mut layer.keyframes {
                        if if has_key_at_or_before {
                            key.frame > at
                        } else {
                            key.frame >= at
                        } {
                            key.frame += count;
                        }
                    }
                    if layer.keyframes.is_empty() {
                        layer.keyframes.push(blank(0));
                    }
                    layer.properties.end_frame = Some(old_end + count);
                } else {
                    if layer.keyframes.is_empty() {
                        layer.keyframes.push(blank(0));
                    }
                    layer.properties.end_frame = Some(at + count);
                }
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
            }
            if include_scripts {
                for script in &mut timeline.scripts {
                    if script.frame > at {
                        script.frame += count;
                    }
                }
            }
        }
        TimelineEdit::Paste {
            clip,
            layers,
            at,
            include_scripts,
        } => paste_rows(
            &mut candidate,
            symbol,
            &clip,
            &layers,
            at,
            include_scripts,
            &mut budget,
        )?,
        TimelineEdit::Move {
            range,
            layers,
            at,
            copy,
            include_scripts,
        } => {
            if !copy && at == range.start && layers == range.layers {
                return Ok(());
            }
            let clip = copy_frames(&candidate, symbol, &range, include_scripts)?;
            if !copy {
                let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
                clear_rows(timeline, &range, &mut budget, fps)?;
                if include_scripts {
                    remove_scripts(timeline, &range);
                }
            }
            paste_rows(
                &mut candidate,
                symbol,
                &clip,
                &layers,
                at,
                include_scripts,
                &mut budget,
            )?;
        }
        TimelineEdit::Reverse { range } => {
            let end = range_end(timeline, &range)?;
            for index in layer_indices(timeline, &range.layers)? {
                let mut layer = timeline.layers[index].clone();
                let last = end.min(layer.properties.end_frame.unwrap_or(0));
                if last <= range.start + 1 {
                    continue;
                }
                // Tweens crossing either boundary are baked; tweens inside are kept.
                bake_touched(
                    timeline,
                    &mut layer,
                    range.start,
                    range.start,
                    &mut budget,
                    fps,
                )?;
                bake_touched(timeline, &mut layer, last, last, &mut budget, fps)?;
                // Motion curves and one-way Bounce/Elastic eases inside the range need sampled
                // poses before reversing; otherwise reversing keys would replace a spatial curve by
                // chords or play Bounce/Elastic ease-out forward instead of time-reversed.
                let mut bake_ranges: Vec<(u32, u32)> = Vec::new();
                for (i, key) in layer.keyframes.iter().enumerate() {
                    let has_explicit_object_data = key.tween_kind == TweenKind::Motion
                        && (!key.motion_tracks.is_empty()
                            || layer.keyframes[i + 1..]
                                .iter()
                                .take_while(|candidate| candidate.keyed_properties.is_some())
                                .any(|candidate| candidate.motion_keys.is_some()));
                    if has_object_motion_path(key) || has_explicit_object_data {
                        if let Some((_, _, span_end)) = motion_span(&layer, key.frame) {
                            let span_end = span_end.unwrap_or(last);
                            if key.frame < last && span_end > range.start {
                                bake_ranges.push((key.frame, span_end));
                            }
                        }
                    } else if has_noninvertible_motion_ease(key) {
                        let span_end = if key.tween_kind == TweenKind::Motion {
                            motion_span(&layer, key.frame)
                                .and_then(|(_, _, end)| end)
                                .unwrap_or(last)
                        } else {
                            layer.keyframes.get(i + 1).map_or(last, |next| next.frame)
                        };
                        if key.frame < last && span_end > range.start {
                            bake_ranges.push((key.frame, span_end));
                        }
                    }
                }
                for (start, span_end) in bake_ranges {
                    bake_touched(timeline, &mut layer, start, span_end, &mut budget, fps)?;
                }
                let original = layer.clone();
                for frame in [range.start, last - 1, last] {
                    split(timeline, &mut layer, frame, fps);
                }
                let (first, final_frame) = (range.start, last - 1);
                let mut inside: Vec<Keyframe> = layer
                    .keyframes
                    .iter()
                    .filter(|key| (first..=final_frame).contains(&key.frame))
                    .cloned()
                    .collect();
                layer
                    .keyframes
                    .retain(|key| !(first..=final_frame).contains(&key.frame));
                let span_sources: Vec<(Option<Easing>, TweenKind, Vec<ShapeHint>)> = inside
                    .iter()
                    .map(|key| {
                        let effective_kind = if key.keyed_properties.is_some()
                            || key.tween_kind == TweenKind::Motion
                        {
                            TweenKind::Classic
                        } else {
                            key.tween_kind
                        };
                        (key.tween, effective_kind, key.shape_hints.clone())
                    })
                    .collect();
                let count = inside.len();
                for (i, key) in inside.iter_mut().enumerate() {
                    // A segment a→b becomes b'→a' and keeps a's tween with time-reversed easing.
                    // Motion tween tracks do not survive reversal; their poses become classic keys.
                    if key.keyed_properties.take().is_some() || key.tween_kind == TweenKind::Motion
                    {
                        key.elements = sample_layer_in_at_fps(timeline, &original, key.frame, fps);
                        key.tween_kind = TweenKind::Classic;
                        key.motion_keys = None;
                        key.motion_tracks.clear();
                        key.motion_path = None;
                    }
                    let (source_tween, source_kind, source_hints) =
                        &span_sources[if i == 0 { count - 1 } else { i - 1 }];
                    key.tween = if i == 0 {
                        *source_tween
                    } else {
                        source_tween.map(reverse_easing)
                    };
                    key.tween_kind = if key.tween.is_some() {
                        *source_kind
                    } else {
                        TweenKind::Classic
                    };
                    key.shape_hints = if key.tween.is_some() && key.tween_kind == TweenKind::Shape {
                        if i == 0 {
                            source_hints.clone()
                        } else {
                            source_hints
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
                    for (_, ease) in &mut key.property_eases {
                        *ease = reverse_easing(*ease);
                    }
                    key.frame = first + final_frame - key.frame;
                }
                layer.keyframes.extend(inside);
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
            }
        }
        TimelineEdit::ConvertToKeyframes {
            range,
            blank: to_blank,
        } => {
            if range.start > range.end || range.end >= MAX_FRAME {
                return Err("The frame range is outside the timeline.".into());
            }
            let end = range.end + 1;
            for index in layer_indices(timeline, &range.layers)? {
                let mut layer = timeline.layers[index].clone();
                if layer.keyframes.is_empty() && end > 0 {
                    layer.keyframes.push(blank(0));
                }
                let old_end = layer.properties.end_frame.unwrap_or(0);
                let new_end = old_end.max(end);
                layer.properties.end_frame = Some(new_end);
                timeline.frame_count = timeline.frame_count.max(new_end);
                let last = end.min(new_end);
                if last <= range.start {
                    continue;
                }
                let original = layer.clone();
                let before_isolate = budget;
                isolate_range(timeline, &mut layer, range.start, last, &mut budget, fps)?;
                let isolate_charged = budget - before_isolate;
                let mut range_budget = 0;
                charge_bake_budget(&original, range.start, last, &mut range_budget)?;
                budget = before_isolate
                    .checked_add(isolate_charged.max(range_budget))
                    .ok_or("Frame edit is too large.")?;
                if budget > MAX_BAKED_RECORDS {
                    return Err("This edit would create more than 100000 frame/object records. Select a shorter range.".into());
                }
                split(timeline, &mut layer, last, fps);
                layer
                    .keyframes
                    .retain(|key| key.frame < range.start || key.frame >= last);
                layer.keyframes.extend((range.start..last).map(|frame| {
                    if to_blank {
                        let mut key = blank(frame);
                        if let Some(old) = original.keyframes.iter().find(|k| k.frame == frame) {
                            key.label = old.label.clone();
                        }
                        key
                    } else {
                        sampled_key(timeline, &original, frame, fps)
                    }
                }));
                layer.keyframes.sort_by_key(|key| key.frame);
                timeline.layers[index] = layer;
            }
        }
        TimelineEdit::SyncSymbols { range } => {
            range_end(timeline, &range)?;
            let mut changed = 0;
            for index in layer_indices(timeline, &range.layers)? {
                changed += sync_symbols(
                    &mut timeline.layers[index],
                    range.start,
                    range.end,
                    &graphics,
                    None,
                );
            }
            if changed == 0 {
                return Err("The graphic symbols in the selected frames are already in step. Synchronise symbols adjusts graphic instances (Loop or Play once) on keyframes that follow another keyframe.".into());
            }
        }
        TimelineEdit::SetEnd { layers, end_frame } => {
            if end_frame > MAX_FRAME {
                return Err("Layer exposure exceeds 1000000 frames.".into());
            }
            for index in layer_indices(timeline, &layers)? {
                let mut layer = timeline.layers[index].clone();
                let old_end = layer.properties.end_frame.unwrap_or(0);
                if end_frame < old_end {
                    isolate_range(timeline, &mut layer, end_frame, old_end, &mut budget, fps)?;
                }
                layer.keyframes.retain(|key| key.frame < end_frame);
                if layer.keyframes.is_empty() && end_frame > 0 {
                    layer.keyframes.push(blank(0));
                }
                layer.properties.end_frame = Some(end_frame);
                timeline.layers[index] = layer;
            }
        }
    }
    let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
    timeline.frame_count = timeline
        .layers
        .iter()
        .filter(|layer| layer.properties.role != LayerRole::Folder)
        .filter_map(|layer| layer.properties.end_frame)
        .chain(timeline.scripts.iter().map(|script| script.frame + 1))
        .max()
        .unwrap_or(1)
        .max(1);
    for layer in &mut timeline.layers {
        if layer.properties.role == LayerRole::Folder {
            layer.properties.end_frame = Some(timeline.frame_count);
            layer
                .keyframes
                .retain(|key| key.frame < timeline.frame_count);
        }
    }
    for layer in &mut timeline.layers {
        layer.normalize_motion_keys();
    }
    candidate.format_version = candidate.format_version.max(5);
    candidate.upgrade_format_for_features();
    candidate.prune_authoring_groups();
    candidate.validate().map_err(|error| error.to_string())?;
    *project = candidate;
    Ok(())
}

impl Timeline {
    /// Returns the indices of visible timeline rows in top-to-bottom display order
    /// (front-to-back sibling order, with folder/mask/guide headers above their children).
    pub fn visible_layer_indices(&self) -> Vec<usize> {
        let indices: std::collections::HashMap<_, _> = self
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| (layer.id.as_str(), index))
            .collect();
        let mut children = vec![Vec::new(); self.layers.len()];
        let mut roots = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            let parent = layer
                .properties
                .mask
                .as_ref()
                .or(layer.properties.motion_guide.as_ref())
                .or(layer.properties.parent.as_ref())
                .and_then(|id| indices.get(id.as_str()).copied());
            if let Some(parent) = parent {
                children[parent].push(index);
            } else {
                roots.push(index);
            }
        }
        let mut visible = Vec::with_capacity(self.layers.len());
        let mut stack = roots;
        let mut visited = std::collections::HashSet::new();
        while let Some(index) = stack.pop() {
            if !visited.insert(index) {
                continue;
            }
            visible.push(index);
            let layer = &self.layers[index];
            if layer.properties.role != LayerRole::Folder || !layer.properties.collapsed {
                stack.extend(children[index].iter().copied());
            }
        }
        visible
    }

    /// Returns true when the timeline has a non-empty `FrameScript` at `frame`.
    pub fn has_frame_script(&self, frame: u32) -> bool {
        self.scripts
            .iter()
            .any(|script| script.frame == frame && !script.script.source.trim().is_empty())
    }

    /// Returns the layer index (`self.layers[index]`) whose keyframe at `frame` displays
    /// and owns the `a` marker for the timeline's `FrameScript` at `frame`, or `None` when
    /// there is no non-empty `FrameScript` at `frame` or no visible non-folder layer has an
    /// ordinary keyframe at `frame` (in which case the `a` marker is shown on the ruler).
    pub fn action_layer_for_frame(&self, frame: u32) -> Option<usize> {
        if frame >= self.frame_count || !self.has_frame_script(frame) {
            return None;
        }
        self.visible_layer_indices().into_iter().find(|&index| {
            let layer = &self.layers[index];
            if layer.properties.role == LayerRole::Folder {
                return false;
            }
            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(self.frame_count)
                .min(self.frame_count);
            frame < layer_end
                && layer
                    .keyframes
                    .iter()
                    .any(|key| key.frame == frame && key.keyed_properties.is_none())
        })
    }

    /// Returns true if the keyframe on `layer_index` at `frame` is the action-bearing
    /// keyframe that carries the timeline's `FrameScript` at `frame`.
    pub fn key_carries_frame_script(&self, layer_index: usize, frame: u32) -> bool {
        self.action_layer_for_frame(frame) == Some(layer_index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMoveError {
    MissingSymbol,
    MissingLayer,
    InvalidLayerOrBounds,
    MissingSourceKey,
    DestinationKeyConflict,
    DestinationScriptConflict,
}

impl Project {
    /// Moves or copies a single keyframe on `layer` from `from` to `to` within `symbol_id`.
    /// When the keyframe at `(layer, from)` is the action-bearing keyframe for the timeline's
    /// `FrameScript` at `from`, that `FrameScript` moves or copies atomically with the keyframe.
    pub fn move_keyframe(
        &mut self,
        symbol_id: &str,
        layer: usize,
        from: u32,
        to: u32,
        copy: bool,
        layer_editable: bool,
    ) -> Result<bool, KeyMoveError> {
        let symbol = self.symbol(symbol_id).ok_or(KeyMoveError::MissingSymbol)?;
        let is_button = symbol.kind == SymbolKind::Button;
        let timeline = &symbol.timeline;
        let l = timeline
            .layers
            .get(layer)
            .ok_or(KeyMoveError::MissingLayer)?;
        let max_frames = if is_button {
            timeline.frame_count
        } else {
            MAX_FRAME
        };
        if !layer_editable
            || l.properties.role == LayerRole::Folder
            || to >= max_frames
            || from == to
        {
            return Err(KeyMoveError::InvalidLayerOrBounds);
        }
        if l.keyframes.iter().any(|k| k.frame == to) {
            return Err(KeyMoveError::DestinationKeyConflict);
        }
        let mut key = l
            .keyframes
            .iter()
            .find(|k| k.frame == from)
            .cloned()
            .ok_or(KeyMoveError::MissingSourceKey)?;
        let carries_script = timeline.key_carries_frame_script(layer, from);
        let script = carries_script
            .then(|| {
                timeline
                    .scripts
                    .iter()
                    .find(|script| script.frame == from && !script.script.source.trim().is_empty())
                    .cloned()
            })
            .flatten();
        if script.is_some() && timeline.has_frame_script(to) {
            return Err(KeyMoveError::DestinationScriptConflict);
        }

        let old_count = timeline.frame_count;
        let old_end = l.properties.end_frame.unwrap_or(old_count);
        let was_last = l.keyframes.last().is_some_and(|k| k.frame == from);
        let still_last = l
            .keyframes
            .iter()
            .filter(|k| k.frame != from)
            .all(|k| k.frame < to);
        let layer_id = l.id.clone();
        let was_trailing_single =
            !copy && from > 0 && was_last && still_last && old_end == from + 1;
        let new_layer_end = if was_trailing_single {
            to + 1
        } else {
            old_end.max(to + 1)
        };

        let symbol_mut = self
            .symbol_mut(symbol_id)
            .ok_or(KeyMoveError::MissingSymbol)?;
        let l = &mut symbol_mut.timeline.layers[layer];
        if !copy {
            l.keyframes.retain(|k| k.frame != from);
        }
        key.frame = to;
        if copy {
            key.label.clear();
        }
        l.keyframes.push(key);
        l.keyframes.sort_by_key(|k| k.frame);
        l.normalize_motion_keys();

        let moved_script = script.is_some();
        if let Some(mut script) = script {
            let scripts = &mut symbol_mut.timeline.scripts;
            if !copy {
                scripts.retain(|item| item.frame != from);
            }
            scripts.retain(|item| item.frame != to);
            script.frame = to;
            scripts.push(script);
            scripts.sort_by_key(|item| item.frame);
        }

        if new_layer_end != old_end {
            let timeline = &mut symbol_mut.timeline;
            for other in &mut timeline.layers {
                if other.id == layer_id {
                    other.properties.end_frame = Some(new_layer_end);
                } else if other.properties.role != LayerRole::Folder
                    && other.properties.end_frame.is_none()
                {
                    other.properties.end_frame = Some(old_count);
                }
            }
            let next_count = timeline
                .layers
                .iter()
                .filter(|other| other.properties.role != LayerRole::Folder)
                .map(|other| other.properties.end_frame.unwrap_or(old_count))
                .chain(timeline.scripts.iter().map(|sc| sc.frame + 1))
                .max()
                .unwrap_or(1)
                .max(1);
            timeline.frame_count = next_count;
            for other in &mut timeline.layers {
                if other.properties.role == LayerRole::Folder
                    && other.properties.end_frame.is_some()
                {
                    other.properties.end_frame = Some(next_count);
                }
            }
            self.format_version = self.format_version.max(5);
        } else if let Some(end) = &mut symbol_mut.timeline.layers[layer].properties.end_frame {
            *end = (*end).max(to + 1);
        }
        self.upgrade_format_for_features();
        Ok(moved_script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_frames_inverts_easing_curves_to_mirror_poses_in_time() {
        for ease in [
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::Flash { acceleration: 60.0 },
            Easing::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0,
            },
            Easing::Bounce,
            Easing::Elastic,
        ] {
            let mut p = Project::blank();
            p.format_version = 6;
            let root = p.root.clone();
            let lid = p.symbols[0].timeline.layers[0].id.clone();
            let rect = |x: f64| Element {
                id: "r".into(),
                transform: Transform {
                    x,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 10.0,
                    height: 10.0,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Default::default()
            };
            p.symbols[0].timeline.frame_count = 11;
            p.symbols[0].timeline.layers[0].keyframes = vec![
                Keyframe {
                    frame: 0,
                    elements: vec![rect(0.0)],
                    tween: Some(ease),
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 10,
                    elements: vec![rect(100.0)],
                    ..Keyframe::default()
                },
            ];
            p.upgrade_format_for_features();
            p.validate().unwrap();
            let original_xs: Vec<f64> = (0..=10)
                .map(|f| {
                    sample_layer(&p.symbols[0].timeline.layers[0], f)[0]
                        .transform
                        .x
                })
                .collect();

            edit_frames(
                &mut p,
                &root,
                TimelineEdit::Reverse {
                    range: FrameRange {
                        layers: vec![lid],
                        start: 0,
                        end: 10,
                    },
                },
            )
            .unwrap();

            for f in 0..=10 {
                let reversed_x = sample_layer(&p.symbols[0].timeline.layers[0], f)[0]
                    .transform
                    .x;
                let expected_x = original_xs[(10 - f) as usize];
                assert!(
                    (reversed_x - expected_x).abs() < 1e-4,
                    "ease {ease:?} frame {f}: reversed_x={reversed_x} != expected_x={expected_x}"
                );
            }
        }
    }

    #[test]
    fn move_keyframe_moves_and_copies_attached_frame_script_without_stealing_from_other_layers() {
        let demo = Project::demo();
        let root = demo.root.clone();
        // In Project::demo() ("Ideas in motion"), Scene 1 has layers:
        // [0: Typography, 1: Accents, 2: Orbit instances].
        // Top visible row is layer 2 ("Orbit instances"), which carries the frame 1 (0) script.
        let timeline = &demo.symbol(&root).unwrap().timeline;
        assert_eq!(timeline.action_layer_for_frame(0), Some(2));
        assert!(timeline.key_carries_frame_script(2, 0));
        assert!(!timeline.key_carries_frame_script(1, 0));
        assert!(!timeline.key_carries_frame_script(0, 0));

        // Moving a non-bearing layer's keyframe at frame 0 (e.g. Typography or Accents) must not steal the script.
        let mut non_bearing = demo.clone();
        let moved_script = non_bearing
            .move_keyframe(&root, 0, 0, 13, false, true)
            .unwrap();
        assert!(!moved_script);
        assert_eq!(
            non_bearing.symbol(&root).unwrap().timeline.scripts[0].frame,
            0
        );
        assert_eq!(
            non_bearing
                .symbol(&root)
                .unwrap()
                .timeline
                .action_layer_for_frame(0),
            Some(2)
        );
        non_bearing.validate().unwrap();

        // Moving the action-bearing keyframe on Orbit instances (layer 2) from frame 0 to 13
        // moves the keyframe AND its attached FrameScript atomically.
        let mut moved = demo.clone();
        moved.symbol_mut(&root).unwrap().timeline.layers[2].keyframes[0].tween =
            Some(Easing::Linear);
        moved.symbol_mut(&root).unwrap().timeline.layers[2].keyframes[0].tween_kind =
            TweenKind::Motion;
        let moved_script = moved.move_keyframe(&root, 2, 0, 13, false, true).unwrap();
        assert!(moved_script);
        let tl = &moved.symbol(&root).unwrap().timeline;
        assert_eq!(
            tl.layers[2]
                .keyframes
                .iter()
                .map(|k| k.frame)
                .collect::<Vec<_>>(),
            vec![13]
        );
        assert_eq!(
            tl.scripts.iter().map(|s| s.frame).collect::<Vec<_>>(),
            vec![13]
        );
        assert_eq!(tl.action_layer_for_frame(0), None);
        assert_eq!(tl.action_layer_for_frame(13), Some(2));
        moved.validate().unwrap();

        // Option-drag / copy duplicates both the keyframe and the FrameScript.
        let mut copied = demo.clone();
        let copied_script = copied.move_keyframe(&root, 2, 0, 13, true, true).unwrap();
        assert!(copied_script);
        let tl = &copied.symbol(&root).unwrap().timeline;
        assert_eq!(
            tl.layers[2]
                .keyframes
                .iter()
                .map(|k| k.frame)
                .collect::<Vec<_>>(),
            vec![0, 13]
        );
        assert_eq!(
            tl.scripts.iter().map(|s| s.frame).collect::<Vec<_>>(),
            vec![0, 13]
        );
        assert_eq!(tl.action_layer_for_frame(0), Some(2));
        assert_eq!(tl.action_layer_for_frame(13), Some(2));
        copied.validate().unwrap();

        // Destination key collision or script collision refuses the whole gesture atomically.
        let before_collision = copied.clone();
        assert_eq!(
            copied.move_keyframe(&root, 2, 0, 13, false, true),
            Err(KeyMoveError::DestinationKeyConflict)
        );
        assert_eq!(copied, before_collision);

        let mut script_conflict = demo.clone();
        script_conflict
            .symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                frame: 13,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "this.stop();".into(),
                },
            });
        let before_script_conflict = script_conflict.clone();
        assert_eq!(
            script_conflict.move_keyframe(&root, 2, 0, 13, false, true),
            Err(KeyMoveError::DestinationScriptConflict)
        );
        assert_eq!(script_conflict, before_script_conflict);
    }

    #[test]
    fn convert_layer_to_keyframes_stepped_samples_on_twos_and_threes_with_or_without_stripping_tween()
     {
        let mut p = Project::blank();
        p.format_version = 6;
        p.symbols[0].timeline.frame_count = 13;
        let rect = |x: f64| Element {
            id: "r".into(),
            transform: Transform {
                x,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [0, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Default::default()
        };
        p.symbols[0].timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![rect(0.0)],
                tween: Some(Easing::EaseInOut),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 12,
                elements: vec![rect(120.0)],
                ..Keyframe::default()
            },
        ];

        // Tween2Keys (on 2s, strip_tween = true)
        let mut p_twos = p.clone();
        let tl_snapshot = p_twos.symbols[0].timeline.clone();
        let count = convert_layer_to_keyframes_stepped(
            &tl_snapshot,
            &mut p_twos.symbols[0].timeline.layers[0],
            0,
            12,
            2,
            true,
            24.0,
        );
        assert_eq!(count, 7);
        let kfs = &p_twos.symbols[0].timeline.layers[0].keyframes;
        assert_eq!(
            kfs.iter().map(|k| k.frame).collect::<Vec<_>>(),
            vec![0, 2, 4, 6, 8, 10, 12]
        );
        assert!(
            kfs.iter()
                .all(|k| k.tween.is_none() && k.keyed_properties.is_none())
        );
        p_twos.validate().unwrap();

        // Convert to Keyframes Advanced (on 3s, strip_tween = false, preserves easing)
        let mut p_threes = p;
        let tl_snapshot = p_threes.symbols[0].timeline.clone();
        let count = convert_layer_to_keyframes_stepped(
            &tl_snapshot,
            &mut p_threes.symbols[0].timeline.layers[0],
            0,
            12,
            3,
            false,
            24.0,
        );
        assert_eq!(count, 5);
        let kfs = &p_threes.symbols[0].timeline.layers[0].keyframes;
        assert_eq!(
            kfs.iter().map(|k| k.frame).collect::<Vec<_>>(),
            vec![0, 3, 6, 9, 12]
        );
        assert_eq!(kfs[0].tween, Some(Easing::EaseInOut));
        assert_eq!(kfs[1].tween, Some(Easing::EaseInOut));
        assert_eq!(kfs[4].tween, None);
        p_threes.validate().unwrap();
    }
}
