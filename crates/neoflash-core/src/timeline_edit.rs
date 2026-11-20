//! Atomic authoring edits over rectangular frame ranges. Format-13 frame scripts follow their
//! explicitly owning row; compatible legacy layerless scripts retain their global semantics.
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
    /// Non-destructive montage / Razor split at `at`: splits an active `Stream` video or `Stream`
    /// sound span (advancing the right segment's source in-point), rejects `Event`/`Stop` media
    /// mid-span splits atomically, and inserts a pose-preserving Keyframe (with Graphic symbol
    /// frame synchronization) on standard authoring spans.
    SplitClip { layers: Vec<Id>, at: u32 },
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

/// Whether a frame script follows a rectangular edit of `layers`.
///
/// Format-13 scripts have a stable authoring owner and only follow that selected row. A legacy
/// layerless script has no safe row to infer during a structural edit, so it deliberately keeps
/// the pre-format-13 global Include frame scripts behaviour: any selected row includes, removes,
/// or ripples it. This avoids guessing an owner or silently changing old documents.
fn script_follows_layers(script: &FrameScript, layers: &[Id]) -> bool {
    script
        .layer
        .as_ref()
        .is_none_or(|owner| layers.iter().any(|layer| layer == owner))
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
    if let Some(span_key) = layer
        .keyframes
        .iter()
        .rfind(|k| k.frame <= frame && k.keyed_properties.is_none())
    {
        let present: std::collections::HashSet<&str> =
            key.elements.iter().map(|e| e.id.as_str()).collect();
        key.instance_overrides = span_key
            .instance_overrides
            .iter()
            .filter(|ov| present.contains(ov.instance_id.as_str()))
            .cloned()
            .collect();
        key.appearances = span_key
            .appearances
            .iter()
            .filter(|app| present.contains(app.element_id.as_str()))
            .cloned()
            .collect();
        key.bitmap_fills = span_key
            .bitmap_fills
            .iter()
            .filter(|bf| present.contains(bf.element_id.as_str()))
            .cloned()
            .collect();
        key.asset_warps = span_key
            .asset_warps
            .iter()
            .filter(|w| present.contains(w.element_id.as_str()))
            .cloned()
            .collect();
        key.element_parents = span_key
            .element_parents
            .iter()
            .filter(|p| {
                present.contains(p.child_id.as_str()) && present.contains(p.parent_id.as_str())
            })
            .cloned()
            .collect();
        key.controller_elements = span_key
            .controller_elements
            .iter()
            .filter(|id| present.contains(id.as_str()))
            .cloned()
            .collect();
        key.extended_accessibility = span_key
            .extended_accessibility
            .iter()
            .filter(|acc| present.contains(acc.element_id.as_str()))
            .cloned()
            .collect();
        key.vector_effects = span_key
            .vector_effects
            .iter()
            .filter(|fx| present.contains(fx.element_id.as_str()))
            .cloned()
            .collect();
    }
    if let Some(original) = layer.keyframes.iter().find(|key| key.frame == frame) {
        key.label = original.label.clone();
        key.label_type = original.label_type;
        key.video = original.video.clone();
    }
    key
}

/// Convert a timeline frame delta into rounded source media milliseconds at `fps`.
pub fn frame_delta_ms(delta_frames: u32, fps: f64) -> u32 {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        24.0
    };
    ((f64::from(delta_frames) * 1000.0) / safe_fps)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Split an authored `TimelineVideoSpan` starting at `span_start` into `(left, right)` at
/// `cut_frame` (`cut_frame > span_start`). Rejects `Event` and `Stop` sync modes atomically
/// because asynchronous video playback has no deterministic frame-locked split point.
pub fn split_video_span_at(
    span: &TimelineVideoSpan,
    span_start: u32,
    cut_frame: u32,
    fps: f64,
) -> Result<(TimelineVideoSpan, TimelineVideoSpan), String> {
    if cut_frame <= span_start {
        return Err("Video split point must be strictly after the clip start frame.".into());
    }
    if span.sync != VideoSyncMode::Stream {
        return Err(
            "Cannot split an Event or Stop video span mid-clip because its playback is not frame-locked. Switch Sync to Stream before splitting, or edit at keyframe boundaries."
                .into(),
        );
    }
    let delta_ms = frame_delta_ms(cut_frame - span_start, fps);
    let raw_right_in = span.start_offset_ms.saturating_add(delta_ms);
    let mut left = span.clone();
    let mut right = span.clone();
    if let Some(end_ms) = span.end_offset_ms {
        let win = end_ms.saturating_sub(span.start_offset_ms).max(1);
        if span.loop_video {
            right.start_offset_ms = span.start_offset_ms.saturating_add(delta_ms % win);
        } else if raw_right_in >= end_ms {
            right.start_offset_ms = end_ms.saturating_sub(1);
        } else {
            right.start_offset_ms = raw_right_in;
        }
        right.end_offset_ms = Some(end_ms.max(right.start_offset_ms.saturating_add(1)));
    } else {
        right.start_offset_ms = raw_right_in;
    }
    if !span.loop_video {
        let boundary_ms = right.start_offset_ms;
        left.end_offset_ms = Some(boundary_ms.max(left.start_offset_ms.saturating_add(1)));
        left.cue_points.retain(|cp| cp.time_ms < boundary_ms);
        right.cue_points.retain(|cp| cp.time_ms >= boundary_ms);
    }
    Ok((left, right))
}

/// Split a `SoundSync::Stream` `FrameSound` at `cut_frame` into `(left, right)`, tightening
/// `left.out_point` and advancing `right.in_point` for non-looping stream clips.
pub fn split_stream_sound_at(
    sound: &FrameSound,
    span_start: u32,
    cut_frame: u32,
    fps: f64,
) -> (FrameSound, FrameSound) {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        24.0
    };
    let delta_sec = f64::from(cut_frame.saturating_sub(span_start)) / safe_fps;
    let max_in = (FrameSound::MAX_SOUND_TRIM_SECONDS - 1e-3).max(0.0);
    let base_in = sound.in_point.unwrap_or(0.0).clamp(0.0, max_in);
    let raw_in = (base_in + delta_sec).clamp(0.0, max_in);
    let mut left = sound.clone();
    let mut right = sound.clone();
    let is_looping = sound.loop_forever || sound.loops > 1;
    let next_in = if let Some(outp) = sound.out_point {
        let clamped_out = outp.clamp(
            (base_in + 1e-3).min(FrameSound::MAX_SOUND_TRIM_SECONDS),
            FrameSound::MAX_SOUND_TRIM_SECONDS,
        );
        let win = (clamped_out - base_in).max(1e-3);
        if is_looping {
            (base_in + (delta_sec % win)).min((clamped_out - 1e-3).max(0.0))
        } else if raw_in >= clamped_out {
            (clamped_out - 1e-3).max(0.0)
        } else {
            raw_in
        }
    } else {
        raw_in
    }
    .min(max_in);
    if !is_looping {
        left.in_point = Some(base_in);
        let left_out = next_in.max((base_in + 1e-3).min(FrameSound::MAX_SOUND_TRIM_SECONDS));
        left.out_point = Some(left_out);
    }
    right.in_point = Some(next_in);
    if let Some(outp) = right.out_point
        && outp <= next_in
    {
        right.out_point = Some((next_in + 1e-3).min(FrameSound::MAX_SOUND_TRIM_SECONDS));
    }
    (left, right)
}

type VideoLayerCut = (Option<(u32, TimelineVideoSpan)>, Option<TimelineVideoSpan>);

/// Inspect `original` for an active `Keyframe.video` span at `cut_frame`.
/// Returns `(Some((span_start_frame, left_video)), Some(right_video))` when `cut_frame` splits the
/// interior of a video span, `(None, Some(exact_video))` when `cut_frame` lands on the video
/// keyframe's start, or `(None, None)` when no video span covers `cut_frame`.
fn video_cut_on_layer(
    original: &Layer,
    effective_end: u32,
    cut_frame: u32,
    fps: f64,
) -> Result<VideoLayerCut, String> {
    if cut_frame >= effective_end {
        return Ok((None, None));
    }
    let Some(start_pos) = original
        .keyframes
        .iter()
        .rposition(|k| k.frame <= cut_frame && k.keyed_properties.is_none())
    else {
        return Ok((None, None));
    };
    let start_key = &original.keyframes[start_pos];
    let span_end = original.keyframes[start_pos + 1..]
        .iter()
        .find(|k| k.keyed_properties.is_none())
        .map_or(effective_end, |k| k.frame);
    if cut_frame >= span_end {
        return Ok((None, None));
    }
    let Some(video) = &start_key.video else {
        return Ok((None, None));
    };
    if cut_frame == start_key.frame {
        return Ok((None, Some(video.clone())));
    }
    let (left, right) = split_video_span_at(video, start_key.frame, cut_frame, fps)?;
    Ok((Some((start_key.frame, left)), Some(right)))
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

use crate::motion_tools::{ExactMotionCurve, reverse_easing, reverse_path_commands};

fn fit_cubic_bezier_samples(samples: &[(f64, f64)]) -> Option<Easing> {
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

fn subdivide_left_single_motion_ease(
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

fn subdivide_left_motion_eases(
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

fn has_roving_motion_keys(layer: &Layer, start_idx: usize, end_frame: u32) -> bool {
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

fn effective_motion_ease_on_start(
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

fn ease_stays_within_unit_range(ease: Easing) -> bool {
    match ease {
        Easing::CubicBezier { y1, y2, .. } => {
            (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2)
        }
        Easing::Bounce | Easing::Elastic => false,
        _ => true,
    }
}

fn curved_tracks_compatible_with_subdivision(first_key: &Keyframe, all_linear: bool) -> bool {
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

fn subdivide_single_motion_curve(
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

fn collect_subspan_pos_waypoints(
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

fn subdivide_keyframe_motion_paths(
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
fn split_span_at(
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
        let original = layer.clone();
        let (_, head_video) = video_cut_on_layer(&original, effective_end, range.start, fps)?;
        if end < effective_end {
            let _ = video_cut_on_layer(&original, effective_end, end, fps)?;
        }
        let clip_end = effective_end.min(end).saturating_sub(range.start);
        let preserve_curves = original.properties.motion_guide.is_none()
            && clip_end >= 2
            && original.keyframes.iter().any(has_object_motion_path)
            && !original
                .keyframes
                .iter()
                .any(|k| k.property_expressions.iter().any(|e| e.enabled));
        let keyframes = if preserve_curves {
            isolate_range(
                timeline,
                &mut layer,
                range.start,
                range.start,
                &mut budget,
                fps,
            )?;
            if end < effective_end {
                isolate_range(timeline, &mut layer, end, end, &mut budget, fps)?;
            }
            split(timeline, &mut layer, range.start, fps);
            let mut extracted: Vec<Keyframe> = layer
                .keyframes
                .into_iter()
                .filter(|key| key.frame >= range.start && key.frame < end)
                .map(|mut key| {
                    if key.frame == range.start {
                        if head_video.is_some() {
                            key.video = head_video.clone();
                        }
                    } else if key.keyed_properties.is_none()
                        && let Some(orig_key) = original
                            .keyframes
                            .iter()
                            .find(|k| k.frame == key.frame && k.keyed_properties.is_none())
                        && orig_key.video.is_some()
                    {
                        key.video = orig_key.video.clone();
                    }
                    key.frame -= range.start;
                    key
                })
                .collect();
            let mut temp = Layer {
                id: layer.id.clone(),
                name: layer.name.clone(),
                visible: true,
                locked: false,
                keyframes: std::mem::take(&mut extracted),
                properties: LayerProperties {
                    end_frame: Some(clip_end),
                    ..Default::default()
                },
            };
            temp.normalize_motion_keys();
            temp.keyframes
        } else {
            layer.properties.end_frame = Some(effective_end);
            bake_touched(timeline, &mut layer, range.start, end, &mut budget, fps)?;
            split(timeline, &mut layer, range.start, fps);
            layer
                .keyframes
                .into_iter()
                .filter(|key| key.frame >= range.start && key.frame < end)
                .map(|mut key| {
                    if key.frame == range.start {
                        if head_video.is_some() {
                            key.video = head_video.clone();
                        }
                    } else if let Some(orig_key) = original
                        .keyframes
                        .iter()
                        .find(|k| k.frame == key.frame && k.keyed_properties.is_none())
                        && orig_key.video.is_some()
                    {
                        key.video = orig_key.video.clone();
                    }
                    key.frame -= range.start;
                    key.tween = None;
                    key
                })
                .collect()
        };
        layers.push(FrameLayerClip {
            source_layer: layer.id,
            keyframes,
            end_frame: clip_end,
        });
    }
    let scripts = if include_scripts {
        timeline
            .scripts
            .iter()
            .filter(|script| {
                script.frame >= range.start
                    && script.frame < end
                    && script_follows_layers(script, &range.layers)
            })
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
    let fallback_count = timeline.frame_count;
    for index in layer_indices(timeline, &range.layers)? {
        let mut layer = timeline.layers[index].clone();
        let effective_end = exposure(&layer, fallback_count);
        let original = layer.clone();
        let (left_split, _) = video_cut_on_layer(&original, effective_end, range.start, fps)?;
        let (_, tail_video) = video_cut_on_layer(&original, effective_end, end, fps)?;
        isolate_range(timeline, &mut layer, range.start, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        if let Some((left_frame, left_video)) = left_split
            && let Some(left_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
        {
            left_key.video = Some(left_video);
        }
        if let Some(right_video) = tail_video
            && let Some(right_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == end && k.keyed_properties.is_none())
        {
            right_key.video = Some(right_video);
        }
        layer
            .keyframes
            .retain(|key| key.frame < range.start || key.frame >= end);
        if range.start < effective_end {
            layer.keyframes.push(blank(range.start));
        }
        layer.keyframes.sort_by_key(|key| key.frame);
        timeline.layers[index] = layer;
    }
    Ok(())
}
fn remove_scripts(timeline: &mut Timeline, range: &FrameRange) {
    timeline.scripts.retain(|script| {
        script.frame < range.start
            || script.frame > range.end
            || !script_follows_layers(script, &range.layers)
    });
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
    let script_layer_map: HashMap<Id, Id> = clip
        .layers
        .iter()
        .zip(&indices)
        .map(|(source, &index)| {
            (
                source.source_layer.clone(),
                timeline.layers[index].id.clone(),
            )
        })
        .collect();
    if include_scripts
        && clip.scripts.iter().any(|script| {
            script
                .layer
                .as_ref()
                .is_some_and(|owner| !script_layer_map.contains_key(owner))
        })
    {
        return Err(
            "Frame clipboard contains a script whose owning layer was not copied; no frames or code were pasted."
                .into(),
        );
    }
    if include_scripts
        && clip.scripts.iter().any(|incoming| {
            let incoming_layer = incoming
                .layer
                .as_ref()
                .and_then(|layer| script_layer_map.get(layer));
            timeline.scripts.iter().any(|existing| {
                existing.frame == at + incoming.frame
                    && match (existing.layer.as_ref(), incoming_layer) {
                        (Some(existing), Some(incoming)) => existing == incoming,
                        // A legacy layerless record has no safe independent destination.
                        _ => true,
                    }
            })
        })
    {
        return Err("A destination layer/frame already has code, or legacy layerless code makes the destination global. Move it first or paste without frame scripts; no code was replaced.".into());
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
    let prev_count = timeline.frame_count;
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
        let old_end = exposure(&layer, prev_count);
        let original = layer.clone();
        let (left_split, _) = video_cut_on_layer(&original, old_end, at, fps)?;
        let (_, tail_video) = video_cut_on_layer(&original, old_end, end, fps)?;
        isolate_range(timeline, &mut layer, at, end, budget, fps)?;
        split(timeline, &mut layer, end, fps);
        if let Some((left_frame, left_video)) = left_split
            && let Some(left_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
        {
            left_key.video = Some(left_video);
        }
        if let Some(right_video) = tail_video
            && let Some(right_key) = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == end && k.keyed_properties.is_none())
        {
            right_key.video = Some(right_video);
        }
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
            key.remap_all_ids(&remap, None, None);
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
                if let Some(source_owner) = script.layer.take() {
                    // Preflight above guarantees a destination owner. Preserve the explicit
                    // source ID even if that invariant is changed later; never degrade an owned
                    // script into the legacy layerless representation.
                    script.layer = Some(
                        script_layer_map
                            .get(&source_owner)
                            .cloned()
                            .unwrap_or(source_owner),
                    );
                }
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
    let symbols: HashMap<Id, (SymbolKind, u32)> = project
        .symbols
        .iter()
        .map(|s| (s.id.clone(), (s.kind, s.timeline.frame_count.max(1))))
        .collect();
    sync_symbols(layer, start, end, &symbols, None)
}

/// Synchronize one graphic symbol placement without changing sibling instances on the layer.
pub fn sync_layer_symbol(
    project: &Project,
    layer: &mut Layer,
    start: u32,
    end: u32,
    element_id: &str,
) -> usize {
    let symbols: HashMap<Id, (SymbolKind, u32)> = project
        .symbols
        .iter()
        .map(|symbol| {
            (
                symbol.id.clone(),
                (symbol.kind, symbol.timeline.frame_count.max(1)),
            )
        })
        .collect();
    sync_symbols(layer, start, end, &symbols, Some(element_id))
}

/// Synchronise symbols on one layer: keyframes in `start..=end` (in frame order) continue the
/// graphic instances of the keyframe before each of them. Returns the number of changed instances.
fn sync_symbols(
    layer: &mut Layer,
    start: u32,
    end: u32,
    symbols: &HashMap<Id, (SymbolKind, u32)>,
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
            let Some(&(sym_kind, count)) = symbols.get(symbol_id) else {
                continue;
            };
            if element.instance_behavior.unwrap_or(sym_kind) != SymbolKind::Graphic {
                continue;
            }
            let is_graphic_of_symbol = |candidate: &Element| {
                matches!(&candidate.kind, ElementKind::Instance { symbol_id: s, .. } if s == symbol_id)
                    && candidate.instance_behavior.unwrap_or(sym_kind) == SymbolKind::Graphic
            };
            let same_id_on_previous = previous
                .elements
                .iter()
                .find(|candidate| candidate.id == element.id);
            let Some(before) = (if let Some(exact) = same_id_on_previous {
                is_graphic_of_symbol(exact).then_some(exact)
            } else if target_element.is_some() {
                let ordinal = layer.keyframes[index].elements[..=element_index]
                    .iter()
                    .filter(|candidate| is_graphic_of_symbol(candidate))
                    .count()
                    .saturating_sub(1);
                previous
                    .elements
                    .iter()
                    .filter(|candidate| is_graphic_of_symbol(candidate))
                    .nth(ordinal)
            } else {
                previous
                    .elements
                    .iter()
                    .find(|candidate| is_graphic_of_symbol(candidate))
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

#[derive(Clone)]
struct ClassicSpanSource {
    tween: Option<Easing>,
    tween_kind: TweenKind,
    shape_hints: Vec<ShapeHint>,
    orient_to_path: bool,
    property_eases: Vec<(TweenProperty, Easing)>,
    sync_symbols: bool,
    snap_to_guide: bool,
    classic_rotate_turns: u32,
    classic_rotate_dir: ClassicRotateDirection,
    tween_scale: bool,
    start_elements: Vec<Element>,
    start_frame: u32,
}

fn reverse_rotate_direction(dir: ClassicRotateDirection) -> ClassicRotateDirection {
    match dir {
        ClassicRotateDirection::Cw => ClassicRotateDirection::Ccw,
        ClassicRotateDirection::Ccw => ClassicRotateDirection::Cw,
        other => other,
    }
}

#[allow(clippy::too_many_arguments)]
fn fit_reversed_single_motion_ease(
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
fn reverse_single_motion_span(
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

fn reverse_classic_subinterval(
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

pub fn edit_frames(project: &mut Project, symbol: &str, edit: TimelineEdit) -> Result<(), String> {
    let mut candidate = project.clone();
    let fps = f64::from(candidate.fps);
    let is_set_end = matches!(edit, TimelineEdit::SetEnd { .. });
    // Symbols and their frame counts (Synchronise symbols and Razor / SplitClip).
    let symbols_for_sync: HashMap<Id, (SymbolKind, u32)> = match &edit {
        TimelineEdit::SyncSymbols { .. } | TimelineEdit::SplitClip { .. } => candidate
            .symbols
            .iter()
            .map(|s| (s.id.clone(), (s.kind, s.timeline.frame_count.max(1))))
            .collect(),
        _ => HashMap::new(),
    };
    let timeline = &mut candidate
        .symbol_mut(symbol)
        .ok_or("The edited symbol no longer exists.")?
        .timeline;
    let old_count = timeline.frame_count;
    let originally_open_end: HashSet<Id> = timeline
        .layers
        .iter()
        .filter(|l| l.properties.role != LayerRole::Folder && l.properties.end_frame.is_none())
        .map(|l| l.id.clone())
        .collect();
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
                let old_end = exposure(&layer, old_count);
                let original = layer.clone();
                let (left_split, _) = video_cut_on_layer(&original, old_end, range.start, fps)?;
                let (end_mid_split, tail_video) = video_cut_on_layer(&original, old_end, end, fps)?;
                if layer.keyframes.iter().any(has_object_motion_path) || end_mid_split.is_some() {
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
                if let Some((left_frame, left_video)) = left_split
                    && let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
                {
                    left_key.video = Some(left_video);
                }
                if end_mid_split.is_some()
                    && let Some(right_video) = tail_video
                    && let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == end && k.keyed_properties.is_none())
                {
                    right_key.video = Some(right_video);
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
                    if script.frame >= end && script_follows_layers(script, &range.layers) {
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
                let old_end = exposure(&layer, old_count);
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
                    if script.frame > at && script_follows_layers(script, &layers) {
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
                let layer_exposure = exposure(&layer, old_count);
                let last = end.min(layer_exposure);
                if last <= range.start + 1 {
                    continue;
                }
                let (first, final_frame) = (range.start, last - 1);
                isolate_range(timeline, &mut layer, first, first, &mut budget, fps)?;
                isolate_range(timeline, &mut layer, last, last, &mut budget, fps)?;

                let mut bake_ranges: Vec<(u32, u32)> = Vec::new();
                for (i, key) in layer.keyframes.iter().enumerate() {
                    if key.keyed_properties.is_some() || key.tween.is_none() {
                        continue;
                    }
                    let must_bake = has_noninvertible_motion_ease(key)
                        || key.property_expressions.iter().any(|e| e.enabled)
                        || (key.tween_kind == TweenKind::Motion
                            && layer.properties.motion_guide.is_some());
                    if must_bake {
                        let span_end = if key.tween_kind == TweenKind::Motion {
                            layer.keyframes[i + 1..]
                                .iter()
                                .find(|k| k.keyed_properties.is_none())
                                .map_or(last, |next| next.frame)
                        } else {
                            layer.keyframes.get(i + 1).map_or(last, |next| next.frame)
                        };
                        if key.frame < last && span_end > first {
                            bake_ranges.push((key.frame, span_end));
                        }
                    }
                }
                for (b_start, b_end) in bake_ranges {
                    bake_touched(timeline, &mut layer, b_start, b_end, &mut budget, fps)?;
                }

                // Discover remaining Motion spans inside `[first .. last)`.
                let mut motion_windows: Vec<(u32, u32)> = Vec::new();
                {
                    let mut idx = 0;
                    while idx < layer.keyframes.len() {
                        let key = &layer.keyframes[idx];
                        if key.frame >= first
                            && key.frame < last
                            && key.keyed_properties.is_none()
                            && key.tween.is_some()
                            && key.tween_kind == TweenKind::Motion
                        {
                            let mut next_idx = idx + 1;
                            while next_idx < layer.keyframes.len()
                                && layer.keyframes[next_idx].keyed_properties.is_some()
                            {
                                next_idx += 1;
                            }
                            let excl_end = layer
                                .keyframes
                                .get(next_idx)
                                .map_or(last, |k| k.frame.min(last));
                            motion_windows.push((key.frame, excl_end));
                            idx = next_idx;
                            continue;
                        }
                        idx += 1;
                    }
                }

                for &(m_start, m_excl_end) in &motion_windows {
                    isolate_range(timeline, &mut layer, m_start, m_start, &mut budget, fps)?;
                    isolate_range(
                        timeline,
                        &mut layer,
                        m_excl_end,
                        m_excl_end,
                        &mut budget,
                        fps,
                    )?;
                }

                let mut reversed_motion_blocks: Vec<Keyframe> = Vec::new();
                let mut preserved_motion_windows: Vec<(u32, u32)> = Vec::new();
                for (m_start, m_excl_end) in motion_windows {
                    if m_excl_end <= m_start + 1 {
                        bake_touched(timeline, &mut layer, m_start, m_excl_end, &mut budget, fps)?;
                        continue;
                    }
                    if let Some(rev_keys) = reverse_single_motion_span(
                        timeline,
                        &layer,
                        m_start,
                        m_excl_end,
                        first,
                        final_frame,
                        last,
                        layer_exposure,
                        fps,
                    ) {
                        if m_start > first {
                            split(timeline, &mut layer, m_start - 1, fps);
                        }
                        if m_excl_end < last {
                            split(timeline, &mut layer, m_excl_end, fps);
                        }
                        preserved_motion_windows.push((m_start, m_excl_end));
                        reversed_motion_blocks.extend(rev_keys);
                    } else {
                        bake_touched(timeline, &mut layer, m_start, m_excl_end, &mut budget, fps)?;
                    }
                }

                let in_preserved_motion = |f: u32| {
                    preserved_motion_windows
                        .iter()
                        .any(|&(ms, me)| f >= ms && f < me)
                };
                for boundary in [first, final_frame, last] {
                    if !in_preserved_motion(boundary) {
                        split(timeline, &mut layer, boundary, fps);
                    }
                }
                layer.keyframes.retain(|k| !in_preserved_motion(k.frame));

                // Partition `[first ..= final_frame]` into maximal non-Motion sub-intervals and
                // reverse Classic / Shape / Hold spans within each sub-interval.
                let guide_layer = layer
                    .properties
                    .motion_guide
                    .as_ref()
                    .and_then(|gid| timeline.layers.iter().find(|l| &l.id == gid))
                    .cloned();
                let mut cursor = first;
                let mut non_motion_intervals: Vec<(u32, u32)> = Vec::new();
                for &(ms, me) in &preserved_motion_windows {
                    if cursor < ms {
                        non_motion_intervals.push((cursor, ms - 1));
                    }
                    cursor = me;
                }
                if cursor <= final_frame {
                    non_motion_intervals.push((cursor, final_frame));
                }

                let mut reversed_non_motion: Vec<Keyframe> = Vec::new();
                for (sub_first, sub_final) in non_motion_intervals {
                    for b in [sub_first, sub_final] {
                        split(timeline, &mut layer, b, fps);
                    }
                    let mut sub_inside: Vec<Keyframe> = layer
                        .keyframes
                        .iter()
                        .filter(|k| (sub_first..=sub_final).contains(&k.frame))
                        .cloned()
                        .collect();
                    layer
                        .keyframes
                        .retain(|k| !(sub_first..=sub_final).contains(&k.frame));
                    reverse_classic_subinterval(
                        timeline,
                        guide_layer.as_ref(),
                        &mut sub_inside,
                        first,
                        final_frame,
                    );
                    reversed_non_motion.extend(sub_inside);
                }

                layer.keyframes.extend(reversed_non_motion);
                layer.keyframes.extend(reversed_motion_blocks);
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
                let old_end = exposure(&layer, old_count);
                let new_end = old_end.max(end);
                let original = layer.clone();
                let (left_split, _) = video_cut_on_layer(&original, new_end, range.start, fps)?;
                let (_, tail_video) =
                    video_cut_on_layer(&original, new_end, end.min(new_end), fps)?;
                let last = end.min(new_end);
                if last <= range.start {
                    layer.properties.end_frame = Some(new_end);
                    timeline.frame_count = timeline.frame_count.max(new_end);
                    timeline.layers[index] = layer;
                    continue;
                }
                let mut per_frame_videos: HashMap<u32, TimelineVideoSpan> = HashMap::new();
                if !to_blank {
                    for frame in range.start..last {
                        if let (_, Some(v)) = video_cut_on_layer(&original, new_end, frame, fps)? {
                            per_frame_videos.insert(frame, v);
                        }
                    }
                }
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
                layer.properties.end_frame = Some(new_end);
                timeline.frame_count = timeline.frame_count.max(new_end);
                split(timeline, &mut layer, last, fps);
                if let Some((left_frame, left_video)) = left_split
                    && let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == left_frame && k.keyed_properties.is_none())
                {
                    left_key.video = Some(left_video);
                }
                if let Some(right_video) = tail_video
                    && let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == last && k.keyed_properties.is_none())
                {
                    right_key.video = Some(right_video);
                }
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
                        let mut key = sampled_key(timeline, &original, frame, fps);
                        if let Some(v) = per_frame_videos.remove(&frame) {
                            key.video = Some(v);
                        }
                        key
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
                    &symbols_for_sync,
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
                let old_end = exposure(&layer, old_count);
                if end_frame < old_end {
                    let _ = video_cut_on_layer(&layer, old_end, end_frame, fps)?;
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
        TimelineEdit::SplitClip { layers, at } => {
            if at == 0 || at >= MAX_FRAME {
                return Err("Place the playhead or Razor inside an active span to split.".into());
            }
            let indices = layer_indices(timeline, &layers)?;
            let mut split_count = 0usize;
            for index in indices {
                let mut layer = timeline.layers[index].clone();
                let layer_end = exposure(&layer, old_count);
                if at >= layer_end {
                    continue;
                }
                if layer
                    .keyframes
                    .iter()
                    .any(|k| k.frame == at && k.keyed_properties.is_none())
                {
                    continue;
                }
                let Some(start_pos) = layer
                    .keyframes
                    .iter()
                    .rposition(|k| k.frame < at && k.keyed_properties.is_none())
                else {
                    continue;
                };
                let span_start = layer.keyframes[start_pos].frame;
                let span_end = layer.keyframes[start_pos + 1..]
                    .iter()
                    .find(|k| k.keyed_properties.is_none())
                    .map_or(layer_end, |k| k.frame);
                if at >= span_end {
                    continue;
                }
                let video_split = if let Some(video) = &layer.keyframes[start_pos].video {
                    Some(split_video_span_at(video, span_start, at, fps)?)
                } else {
                    None
                };
                let sound_split = if let Some(sound) = &layer.keyframes[start_pos].sound {
                    if sound.sync == SoundSync::Stream {
                        Some(split_stream_sound_at(sound, span_start, at, fps))
                    } else if layer.keyframes[start_pos].elements.is_empty()
                        && video_split.is_none()
                    {
                        return Err(
                            "Cannot split an Event, Start, or Stop sound span mid-clip because asynchronous sound is not frame-locked. Switch Sync to Stream to split audio at a frame."
                                .into(),
                        );
                    } else {
                        None
                    }
                } else {
                    None
                };
                split_span_at(
                    timeline,
                    &mut layer,
                    start_pos,
                    span_start,
                    span_end,
                    at,
                    &mut budget,
                    fps,
                )?;
                if let Some((left_video, right_video)) = video_split {
                    if let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == span_start && k.keyed_properties.is_none())
                    {
                        left_key.video = Some(left_video);
                    }
                    if let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == at && k.keyed_properties.is_none())
                    {
                        right_key.video = Some(right_video);
                    }
                }
                if let Some((left_sound, right_sound)) = sound_split {
                    if let Some(left_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == span_start && k.keyed_properties.is_none())
                    {
                        left_key.sound = Some(left_sound);
                    }
                    if let Some(right_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == at && k.keyed_properties.is_none())
                    {
                        right_key.sound = Some(right_sound);
                    }
                }
                let _ = sync_symbols(&mut layer, at, at, &symbols_for_sync, None);
                timeline.layers[index] = layer;
                split_count += 1;
            }
            if split_count == 0 {
                return Err(
                    "Place the playhead or Razor inside an active span (between keyframes) to split."
                        .into(),
                );
            }
        }
    }
    let timeline = &mut candidate.symbol_mut(symbol).unwrap().timeline;
    for layer in &mut timeline.layers {
        if layer.properties.role != LayerRole::Folder && layer.properties.end_frame.is_none() {
            layer.properties.end_frame = Some(old_count);
        }
    }
    timeline.frame_count = timeline
        .layers
        .iter()
        .filter(|layer| layer.properties.role != LayerRole::Folder)
        .filter_map(|layer| layer.properties.end_frame)
        .chain(timeline.scripts.iter().map(|script| script.frame + 1))
        .max()
        .unwrap_or(1)
        .max(1);
    if !is_set_end {
        let new_count = timeline.frame_count;
        for layer in &mut timeline.layers {
            if layer.properties.role != LayerRole::Folder
                && originally_open_end.contains(&layer.id)
                && layer.properties.end_frame == Some(new_count)
            {
                layer.properties.end_frame = None;
            }
        }
    }
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
    candidate.prune_authoring_groups();
    candidate.prune_frame_script_layers();
    candidate.format_version = candidate.format_version.max(5);
    candidate.upgrade_format_for_features();
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

    /// Resolve the authoring row for one frame script. New documents store the stable layer ID;
    /// legacy scripts without it retain the old deterministic topmost-key fallback.
    pub fn action_layer_for_script(&self, script: &FrameScript) -> Option<usize> {
        if script.frame >= self.frame_count || script.script.source.trim().is_empty() {
            return None;
        }
        if let Some(layer_id) = &script.layer {
            return self.layers.iter().position(|layer| {
                &layer.id == layer_id
                    && layer.properties.role != LayerRole::Folder
                    && layer
                        .keyframes
                        .iter()
                        .any(|key| key.frame == script.frame && key.keyed_properties.is_none())
            });
        }
        self.inferred_action_layer_for_frame(script.frame)
    }

    fn inferred_action_layer_for_frame(&self, frame: u32) -> Option<usize> {
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

    /// The script visible in Actions for one exact layer/frame context. Explicitly owned scripts
    /// win. A layerless legacy script uses its inferred marker row when one exists; if no exact
    /// ordinary key can host the marker, it retains its historical global authoring visibility
    /// from any non-folder row while the Timeline keeps the marker on the ruler.
    pub fn frame_script_for_layer(&self, layer_index: usize, frame: u32) -> Option<&FrameScript> {
        self.frame_script_index_for_layer(layer_index, frame)
            .and_then(|index| self.scripts.get(index))
    }

    /// Index form of [`Timeline::frame_script_for_layer`] for layer-aware mutation paths.
    pub fn frame_script_index_for_layer(&self, layer_index: usize, frame: u32) -> Option<usize> {
        let layer_id = &self.layers.get(layer_index)?.id;
        self.scripts
            .iter()
            .position(|script| script.frame == frame && script.layer.as_ref() == Some(layer_id))
            .or_else(|| {
                self.scripts.iter().position(|script| {
                    script.frame == frame
                        && !script.script.source.trim().is_empty()
                        && script.layer.is_none()
                        && self.action_layer_for_script(script).map_or_else(
                            || {
                                self.layers
                                    .get(layer_index)
                                    .is_some_and(|layer| layer.properties.role != LayerRole::Folder)
                            },
                            |owner| owner == layer_index,
                        )
                })
            })
    }

    /// Returns the layer index (`self.layers[index]`) whose keyframe at `frame` displays
    /// and owns the `a` marker for the timeline's `FrameScript` at `frame`, or `None` when
    /// there is no non-empty `FrameScript` at `frame` or no visible non-folder layer has an
    /// ordinary keyframe at `frame` (in which case the `a` marker is shown on the ruler).
    pub fn action_layer_for_frame(&self, frame: u32) -> Option<usize> {
        if frame >= self.frame_count || !self.has_frame_script(frame) {
            return None;
        }
        self.scripts
            .iter()
            .filter(|script| script.frame == frame)
            .find_map(|script| self.action_layer_for_script(script))
    }

    /// Returns true when at least one non-empty script at `frame` cannot be shown on a layer row.
    /// The Timeline paints one ruler marker in that case, even if another same-frame script does
    /// have a valid row owner.
    pub fn frame_has_unplaced_script(&self, frame: u32) -> bool {
        self.scripts.iter().any(|script| {
            script.frame == frame
                && !script.script.source.trim().is_empty()
                && self.action_layer_for_script(script).is_none()
        })
    }

    /// Returns true if the keyframe on `layer_index` at `frame` is the action-bearing
    /// keyframe that carries the timeline's `FrameScript` at `frame`.
    pub fn key_carries_frame_script(&self, layer_index: usize, frame: u32) -> bool {
        self.scripts.iter().any(|script| {
            script.frame == frame && self.action_layer_for_script(script) == Some(layer_index)
        })
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
        let layer_id = l.id.clone();
        let carries_script = timeline.key_carries_frame_script(layer, from);
        let script_index = carries_script
            .then(|| {
                timeline.scripts.iter().position(|script| {
                    script.frame == from && timeline.action_layer_for_script(script) == Some(layer)
                })
            })
            .flatten();
        if script_index.is_some() && timeline.frame_script_for_layer(layer, to).is_some() {
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

        let moved_script = script_index.is_some();
        if let Some(script_index) = script_index {
            let scripts = &mut symbol_mut.timeline.scripts;
            let mut script = if copy {
                scripts[script_index].clone()
            } else {
                scripts.remove(script_index)
            };
            script.frame = to;
            script.layer = Some(layer_id.clone());
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

    fn format13_range_script_project() -> (Project, Id, Id, Id) {
        let mut project = Project::blank();
        project.format_version = 13;
        let root = project.root.clone();
        let first_layer: Id = "range-script-first".into();
        let second_layer: Id = "range-script-second".into();
        let timeline = &mut project.symbol_mut(&root).unwrap().timeline;
        timeline.frame_count = 10;
        timeline.layers[0].id = first_layer.clone();
        timeline.layers[0].name = "First".into();
        timeline.layers[0].properties.end_frame = Some(10);
        timeline.layers.push(Layer {
            id: second_layer.clone(),
            name: "Second".into(),
            visible: true,
            locked: false,
            keyframes: vec![blank(0)],
            properties: LayerProperties {
                end_frame: Some(10),
                ..LayerProperties::default()
            },
        });
        timeline.scripts = [
            ("first-2", first_layer.clone(), 2),
            ("second-2", second_layer.clone(), 2),
            ("first-6", first_layer.clone(), 6),
            ("second-6", second_layer.clone(), 6),
        ]
        .into_iter()
        .map(|(source, layer, frame)| FrameScript {
            layer: Some(layer),
            frame,
            script: Script {
                language: ScriptLanguage::JavaScript,
                source: source.into(),
            },
        })
        .collect();
        project.validate().unwrap();
        (project, root, first_layer, second_layer)
    }

    fn script_position(project: &Project, root: &Id, source: &str) -> (Option<Id>, u32) {
        let script = project
            .symbol(root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .find(|script| script.script.source == source)
            .unwrap_or_else(|| panic!("missing test frame script {source}"));
        (script.layer.clone(), script.frame)
    }

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
        let mut demo = Project::demo();
        let root = demo.root.clone();
        // In Project::demo() ("Ideas in motion"), Scene 1 has layers:
        // [0: Typography, 1: Accents, 2: Orbit instances].
        // Give two layers independent scripts at the same timeline frame. Runtime execution is
        // timeline-wide, while authoring ownership and direct-key drag remain layer-specific.
        demo.format_version = 13;
        let orbit_layer = demo.symbol(&root).unwrap().timeline.layers[2].id.clone();
        let accents_layer = demo.symbol(&root).unwrap().timeline.layers[1].id.clone();
        demo.symbol_mut(&root).unwrap().timeline.scripts[0].layer = Some(orbit_layer.clone());
        demo.symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: Some(accents_layer.clone()),
                frame: 0,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "trace(\"accent action\");".into(),
                },
            });
        demo.validate().unwrap();

        let timeline = &demo.symbol(&root).unwrap().timeline;
        assert_eq!(timeline.action_layer_for_frame(0), Some(2));
        assert!(timeline.key_carries_frame_script(2, 0));
        assert!(timeline.key_carries_frame_script(1, 0));
        assert!(!timeline.key_carries_frame_script(0, 0));

        // Moving a non-bearing layer's keyframe at frame 0 must not steal either script.
        let mut non_bearing = demo.clone();
        let moved_script = non_bearing
            .move_keyframe(&root, 0, 0, 13, false, true)
            .unwrap();
        assert!(!moved_script);
        assert_eq!(
            non_bearing
                .symbol(&root)
                .unwrap()
                .timeline
                .scripts
                .iter()
                .map(|script| script.frame)
                .collect::<Vec<_>>(),
            vec![0, 0]
        );
        non_bearing.validate().unwrap();

        // Moving the action-bearing keyframe on Orbit instances (layer 2) from frame 0 to 13
        // moves only its attached FrameScript; the Accents script stays at frame 0.
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
            vec![0, 13]
        );
        assert!(tl.scripts.iter().any(|script| {
            script.layer.as_ref() == Some(&accents_layer)
                && script.frame == 0
                && script.script.source.contains("accent action")
        }));
        assert!(
            tl.scripts.iter().any(|script| {
                script.layer.as_ref() == Some(&orbit_layer) && script.frame == 13
            })
        );
        assert_eq!(tl.action_layer_for_frame(0), Some(1));
        assert_eq!(tl.action_layer_for_frame(13), Some(2));
        moved.validate().unwrap();

        // Option-drag / copy duplicates the Orbit script and leaves the same-frame Accents script.
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
            vec![0, 0, 13]
        );
        assert_eq!(tl.action_layer_for_frame(0), Some(2));
        assert_eq!(tl.action_layer_for_frame(13), Some(2));
        assert_eq!(
            tl.scripts
                .iter()
                .filter(|script| script.layer.as_ref() == Some(&accents_layer))
                .count(),
            1
        );
        assert_eq!(
            tl.scripts
                .iter()
                .filter(|script| script.layer.as_ref() == Some(&orbit_layer))
                .count(),
            2
        );
        copied.validate().unwrap();

        // Destination key collision or script collision refuses the whole gesture atomically.
        let before_collision = copied.clone();
        assert_eq!(
            copied.move_keyframe(&root, 2, 0, 13, false, true),
            Err(KeyMoveError::DestinationKeyConflict)
        );
        assert_eq!(copied, before_collision);

        // A script on another layer at the destination is not a collision.
        let mut neighbor_destination = demo.clone();
        neighbor_destination
            .symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: Some(accents_layer.clone()),
                frame: 13,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "this.stop();".into(),
                },
            });
        assert!(
            neighbor_destination
                .move_keyframe(&root, 2, 0, 13, false, true)
                .unwrap()
        );
        let destination_scripts = neighbor_destination
            .symbol(&root)
            .unwrap()
            .timeline
            .scripts
            .iter()
            .filter(|script| script.frame == 13)
            .collect::<Vec<_>>();
        assert_eq!(destination_scripts.len(), 2);
        assert!(
            destination_scripts
                .iter()
                .any(|script| script.layer.as_ref() == Some(&accents_layer))
        );
        assert!(
            destination_scripts
                .iter()
                .any(|script| script.layer.as_ref() == Some(&orbit_layer))
        );
        neighbor_destination.validate().unwrap();

        // A script owned by the moved key's layer at the destination remains a collision.
        let mut script_conflict = demo.clone();
        script_conflict
            .symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(FrameScript {
                layer: Some(orbit_layer),
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
    fn rectangular_frame_edits_follow_only_the_selected_format13_script_owner() {
        let (project, root, first, second) = format13_range_script_project();
        let range = FrameRange {
            layers: vec![first.clone()],
            start: 2,
            end: 2,
        };

        let clip = copy_frames(&project, &root, &range, true).unwrap();
        assert_eq!(clip.scripts.len(), 1);
        assert_eq!(clip.scripts[0].layer.as_ref(), Some(&first));
        assert_eq!(clip.scripts[0].script.source, "first-2");

        let mut pasted = project.clone();
        edit_frames(
            &mut pasted,
            &root,
            TimelineEdit::Paste {
                clip: clip.clone(),
                layers: vec![second.clone()],
                at: 4,
                include_scripts: true,
            },
        )
        .unwrap();
        assert!(
            pasted
                .symbol(&root)
                .unwrap()
                .timeline
                .scripts
                .iter()
                .any(|script| {
                    script.script.source == "first-2"
                        && script.layer.as_ref() == Some(&second)
                        && script.frame == 4
                })
        );
        assert_eq!(
            script_position(&pasted, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        pasted.validate().unwrap();

        let mut moved = project.clone();
        edit_frames(
            &mut moved,
            &root,
            TimelineEdit::Move {
                range: range.clone(),
                layers: vec![first.clone()],
                at: 4,
                copy: false,
                include_scripts: true,
            },
        )
        .unwrap();
        assert_eq!(
            script_position(&moved, &root, "first-2"),
            (Some(first.clone()), 4)
        );
        assert_eq!(
            script_position(&moved, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        moved.validate().unwrap();

        let mut cleared = project.clone();
        edit_frames(
            &mut cleared,
            &root,
            TimelineEdit::Clear {
                range: range.clone(),
                include_scripts: true,
            },
        )
        .unwrap();
        assert!(
            cleared
                .symbol(&root)
                .unwrap()
                .timeline
                .scripts
                .iter()
                .all(|script| script.script.source != "first-2")
        );
        assert_eq!(
            script_position(&cleared, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        cleared.validate().unwrap();

        let mut deleted = project.clone();
        edit_frames(
            &mut deleted,
            &root,
            TimelineEdit::Delete {
                range: FrameRange {
                    layers: vec![first.clone()],
                    start: 2,
                    end: 3,
                },
                include_scripts: true,
            },
        )
        .unwrap();
        assert!(
            deleted
                .symbol(&root)
                .unwrap()
                .timeline
                .scripts
                .iter()
                .all(|script| script.script.source != "first-2")
        );
        assert_eq!(
            script_position(&deleted, &root, "first-6"),
            (Some(first.clone()), 4)
        );
        assert_eq!(
            script_position(&deleted, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        assert_eq!(
            script_position(&deleted, &root, "second-6"),
            (Some(second.clone()), 6)
        );
        deleted.validate().unwrap();

        let mut inserted = project.clone();
        edit_frames(
            &mut inserted,
            &root,
            TimelineEdit::Insert {
                layers: vec![first.clone()],
                at: 1,
                count: 2,
                include_scripts: true,
            },
        )
        .unwrap();
        assert_eq!(
            script_position(&inserted, &root, "first-2"),
            (Some(first.clone()), 4)
        );
        assert_eq!(
            script_position(&inserted, &root, "first-6"),
            (Some(first.clone()), 8)
        );
        assert_eq!(
            script_position(&inserted, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        assert_eq!(
            script_position(&inserted, &root, "second-6"),
            (Some(second), 6)
        );
        inserted.validate().unwrap();

        let mut invalid_clip = clip;
        invalid_clip.scripts[0].layer = Some("not-a-copied-layer".into());
        let mut atomic = project.clone();
        let before = atomic.clone();
        let error = edit_frames(
            &mut atomic,
            &root,
            TimelineEdit::Paste {
                clip: invalid_clip,
                layers: vec![first],
                at: 4,
                include_scripts: true,
            },
        )
        .unwrap_err();
        assert!(error.contains("owning layer was not copied"));
        assert_eq!(atomic, before);
    }

    #[test]
    fn legacy_layerless_scripts_keep_global_rectangular_edit_semantics() {
        let (mut project, root, first, second) = format13_range_script_project();
        project.symbol_mut(&root).unwrap().timeline.scripts.extend([
            FrameScript {
                layer: None,
                frame: 2,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "legacy-2".into(),
                },
            },
            FrameScript {
                layer: None,
                frame: 6,
                script: Script {
                    language: ScriptLanguage::JavaScript,
                    source: "legacy-6".into(),
                },
            },
        ]);
        project.validate().unwrap();

        // Layerless records intentionally retain the old global Include frame scripts contract:
        // selecting any row copies/removes them and ripples their time.
        let clip = copy_frames(
            &project,
            &root,
            &FrameRange {
                layers: vec![first.clone()],
                start: 2,
                end: 2,
            },
            true,
        )
        .unwrap();
        assert!(
            clip.scripts
                .iter()
                .any(|script| script.layer.is_none() && script.script.source == "legacy-2")
        );
        assert!(
            clip.scripts
                .iter()
                .all(|script| script.script.source != "second-2")
        );

        let mut cleared = project.clone();
        edit_frames(
            &mut cleared,
            &root,
            TimelineEdit::Clear {
                range: FrameRange {
                    layers: vec![first.clone()],
                    start: 2,
                    end: 2,
                },
                include_scripts: true,
            },
        )
        .unwrap();
        assert!(
            cleared
                .symbol(&root)
                .unwrap()
                .timeline
                .scripts
                .iter()
                .all(|script| script.script.source != "legacy-2")
        );
        assert_eq!(
            script_position(&cleared, &root, "second-2"),
            (Some(second.clone()), 2)
        );

        let mut deleted = project.clone();
        edit_frames(
            &mut deleted,
            &root,
            TimelineEdit::Delete {
                range: FrameRange {
                    layers: vec![first.clone()],
                    start: 2,
                    end: 3,
                },
                include_scripts: true,
            },
        )
        .unwrap();
        assert_eq!(script_position(&deleted, &root, "legacy-6"), (None, 4));
        assert_eq!(
            script_position(&deleted, &root, "second-6"),
            (Some(second.clone()), 6)
        );

        let mut inserted = project;
        edit_frames(
            &mut inserted,
            &root,
            TimelineEdit::Insert {
                layers: vec![first],
                at: 1,
                count: 2,
                include_scripts: true,
            },
        )
        .unwrap();
        assert_eq!(script_position(&inserted, &root, "legacy-2"), (None, 4));
        assert_eq!(script_position(&inserted, &root, "legacy-6"), (None, 8));
        assert_eq!(
            script_position(&inserted, &root, "second-2"),
            (Some(second.clone()), 2)
        );
        assert_eq!(
            script_position(&inserted, &root, "second-6"),
            (Some(second), 6)
        );
        inserted.validate().unwrap();
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

    #[test]
    fn video_and_sound_montage_splits_advance_in_points_preserve_metadata_and_reject_event_splits()
    {
        let mut p = Project::blank();
        p.fps = 20; // 50 ms per frame for exact integer verification
        p.format_version = 12;
        let root_id = p.root.clone();
        p.resources.push(
            ResourceUnit::from_bytes(
                "vid_1",
                "video/clip.mp4",
                "video/mp4",
                b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41",
            )
            .unwrap(),
        );
        p.resources.push(
            ResourceUnit::from_bytes(
                "aud_1",
                "audio/bed.wav",
                "audio/wav",
                b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00",
            )
            .unwrap(),
        );

        let video_layer_id = p.symbols[0].timeline.layers[0].id.clone();
        p.symbols[0].timeline.frame_count = 40;
        p.symbols[0].timeline.layers[0].properties.end_frame = Some(40);
        p.symbols[0].timeline.layers[0].keyframes[0] = Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "vid_el".into(),
                name: "clip.mp4".into(),
                kind: ElementKind::Video {
                    width: 320.0,
                    height: 180.0,
                },
                ..Element::default()
            }],
            video: Some(TimelineVideoSpan {
                resource: "vid_1".into(),
                sync: VideoSyncMode::Stream,
                start_offset_ms: 200,
                end_offset_ms: Some(3000),
                loop_video: false,
                muted: true,
                cue_points: vec![
                    VideoCuePoint {
                        name: "intro".into(),
                        time_ms: 350,
                        kind: VideoCuePointKind::Navigation,
                        parameters: vec![],
                    },
                    VideoCuePoint {
                        name: "beat".into(),
                        time_ms: 900,
                        kind: VideoCuePointKind::Event,
                        parameters: vec![("tag".into(), "drop".into())],
                    },
                ],
                deployment: VideoDeploymentMode::Embedded,
                external_path: None,
                stream_url: None,
            }),
            ..Keyframe::default()
        };

        let mut audio_sound = FrameSound::new("aud_1");
        audio_sound.sync = SoundSync::Stream;
        audio_sound.in_point = Some(0.2);
        audio_sound.out_point = Some(3.0);
        let audio_layer_id: Id = "aud_layer".into();
        p.symbols[0].timeline.layers.push(Layer {
            id: audio_layer_id.clone(),
            name: "Audio".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                sound: Some(audio_sound),
                ..Keyframe::default()
            }],
            properties: LayerProperties {
                end_frame: Some(40),
                linked_layer: Some(video_layer_id.clone()),
                ..LayerProperties::default()
            },
        });
        p.symbols[0].timeline.layers[0].properties.linked_layer = Some(audio_layer_id.clone());
        p.validate().unwrap();

        // 1. Atomic SplitClip at frame 10 (500 ms after start -> 200 + 500 = 700 ms).
        edit_frames(
            &mut p,
            &root_id,
            TimelineEdit::SplitClip {
                layers: vec![video_layer_id.clone(), audio_layer_id.clone()],
                at: 10,
            },
        )
        .unwrap();
        let vkfs = &p.symbols[0].timeline.layers[0].keyframes;
        assert_eq!(vkfs.len(), 2);
        let left_v = vkfs[0].video.as_ref().unwrap();
        let right_v = vkfs[1].video.as_ref().unwrap();
        assert_eq!(left_v.start_offset_ms, 200);
        assert_eq!(left_v.end_offset_ms, Some(700));
        assert!(left_v.muted);
        assert_eq!(left_v.cue_points.len(), 1);
        assert_eq!(left_v.cue_points[0].name, "intro");
        assert_eq!(right_v.start_offset_ms, 700);
        assert_eq!(right_v.end_offset_ms, Some(3000));
        assert!(right_v.muted);
        assert_eq!(right_v.cue_points.len(), 1);
        assert_eq!(right_v.cue_points[0].name, "beat");

        let akfs = &p.symbols[0].timeline.layers[1].keyframes;
        assert_eq!(akfs.len(), 2);
        let left_s = akfs[0].sound.as_ref().unwrap();
        let right_s = akfs[1].sound.as_ref().unwrap();
        assert!((left_s.out_point.unwrap() - 0.7).abs() < 1e-6);
        assert!((right_s.in_point.unwrap() - 0.7).abs() < 1e-6);

        // 2. Ripple Delete of frames 10..15 advances the second segment's in-point from 700 -> 950 ms.
        edit_frames(
            &mut p,
            &root_id,
            TimelineEdit::Delete {
                range: FrameRange {
                    layers: vec![video_layer_id.clone()],
                    start: 10,
                    end: 14,
                },
                include_scripts: false,
            },
        )
        .unwrap();
        let vkfs = &p.symbols[0].timeline.layers[0].keyframes;
        assert_eq!(vkfs[1].frame, 10);
        assert_eq!(vkfs[1].video.as_ref().unwrap().start_offset_ms, 950);

        // 3. Copy & Move preserve Keyframe.video metadata.
        let copied = copy_frames(
            &p,
            &root_id,
            &FrameRange {
                layers: vec![video_layer_id.clone()],
                start: 10,
                end: 19,
            },
            false,
        )
        .unwrap();
        assert_eq!(
            copied.layers[0].keyframes[0]
                .video
                .as_ref()
                .unwrap()
                .start_offset_ms,
            950
        );

        // 4. Event / Stop video mid-span split or partial delete rejects atomically without mutating p.
        p.symbols[0].timeline.layers[0].keyframes[1]
            .video
            .as_mut()
            .unwrap()
            .sync = VideoSyncMode::Event;
        let snapshot = p.clone();
        let err = edit_frames(
            &mut p,
            &root_id,
            TimelineEdit::SplitClip {
                layers: vec![video_layer_id.clone(), audio_layer_id.clone()],
                at: 18,
            },
        )
        .unwrap_err();
        assert!(err.contains("Event or Stop"));
        assert_eq!(
            p, snapshot,
            "failed Event split must leave both linked layers untouched"
        );
    }

    #[test]
    fn sync_symbols_and_split_clip_respect_per_instance_behavior_overrides() {
        let mut p = Project::blank();
        let root_id = p.root.clone();
        p.symbols[0].timeline.frame_count = 10;
        let layer_id = p.symbols[0].timeline.layers[0].id.clone();

        let mut mc_sym = p.symbols[0].clone();
        mc_sym.id = "mc_lib".into();
        mc_sym.name = "HeroMC".into();
        mc_sym.kind = SymbolKind::MovieClip;
        mc_sym.timeline.layers[0].id = "mc-layer".into();
        mc_sym.timeline.frame_count = 8;

        let mut gfx_sym = p.symbols[0].clone();
        gfx_sym.id = "gfx_lib".into();
        gfx_sym.name = "BadgeGfx".into();
        gfx_sym.kind = SymbolKind::Graphic;
        gfx_sym.timeline.layers[0].id = "gfx-layer".into();
        gfx_sym.timeline.frame_count = 8;
        p.symbols.push(mc_sym);
        p.symbols.push(gfx_sym);

        let inst_with_behavior =
            |id: &str, sym: &str, behavior: SymbolKind, playback: InstancePlayback| Element {
                id: id.into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: sym.into(),
                    playback,
                },
                instance_behavior: Some(behavior),
                ..Element::default()
            };

        p.symbols[0].timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![
                    inst_with_behavior(
                        "mc_as_gfx_0",
                        "mc_lib",
                        SymbolKind::Graphic,
                        InstancePlayback::LoopFrom(2),
                    ),
                    inst_with_behavior(
                        "gfx_as_mc_0",
                        "gfx_lib",
                        SymbolKind::MovieClip,
                        InstancePlayback::LoopFrom(2),
                    ),
                ],
                ..Default::default()
            },
            Keyframe {
                frame: 5,
                elements: vec![
                    inst_with_behavior(
                        "mc_as_gfx_5",
                        "mc_lib",
                        SymbolKind::Graphic,
                        InstancePlayback::Loop,
                    ),
                    inst_with_behavior(
                        "gfx_as_mc_5",
                        "gfx_lib",
                        SymbolKind::MovieClip,
                        InstancePlayback::Loop,
                    ),
                ],
                ..Default::default()
            },
        ];

        // 1. sync_layer_symbols syncs the Graphic-overridden MovieClip ((2 + 5) % 8 = 7)
        // while leaving the MovieClip-overridden Graphic untouched (even though its playback field is Loop).
        let mut layer_copy = p.symbols[0].timeline.layers[0].clone();
        let changed = sync_layer_symbols(&p, &mut layer_copy, 0, 9);
        assert_eq!(changed, 1);
        assert_eq!(
            layer_copy.keyframes[1].elements[0].kind,
            ElementKind::Instance {
                symbol_id: "mc_lib".into(),
                playback: InstancePlayback::LoopFrom(7),
            }
        );
        assert_eq!(
            layer_copy.keyframes[1].elements[1].kind,
            ElementKind::Instance {
                symbol_id: "gfx_lib".into(),
                playback: InstancePlayback::Loop,
            }
        );

        // 2. TimelineEdit::SplitClip and TimelineEdit::SyncSymbols also respect instance_behavior.
        edit_frames(
            &mut p,
            &root_id,
            TimelineEdit::SplitClip {
                layers: vec![layer_id.clone()],
                at: 3,
            },
        )
        .unwrap();
        let split_key = p.symbols[0].timeline.layers[0]
            .keyframes
            .iter()
            .find(|k| k.frame == 3)
            .unwrap();
        // Same element ID across split: placement_start == 0, so LoopFrom(2) stays LoopFrom(2).
        assert_eq!(
            split_key.elements[0].kind,
            ElementKind::Instance {
                symbol_id: "mc_lib".into(),
                playback: InstancePlayback::LoopFrom(2),
            }
        );
        assert_eq!(
            split_key.elements[1].kind,
            ElementKind::Instance {
                symbol_id: "gfx_lib".into(),
                playback: InstancePlayback::LoopFrom(2),
            }
        );

        edit_frames(
            &mut p,
            &root_id,
            TimelineEdit::SyncSymbols {
                range: FrameRange {
                    layers: vec![layer_id],
                    start: 5,
                    end: 5,
                },
            },
        )
        .unwrap();
        let k5 = p.symbols[0].timeline.layers[0]
            .keyframes
            .iter()
            .find(|k| k.frame == 5)
            .unwrap();
        assert_eq!(
            k5.elements[0].kind,
            ElementKind::Instance {
                symbol_id: "mc_lib".into(),
                playback: InstancePlayback::LoopFrom(7),
            }
        );
        assert_eq!(
            k5.elements[1].kind,
            ElementKind::Instance {
                symbol_id: "gfx_lib".into(),
                playback: InstancePlayback::Loop,
            }
        );
    }
}
