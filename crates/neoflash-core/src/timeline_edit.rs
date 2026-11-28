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
pub(super) struct TimelineSampleContext<'a> {
    pub(super) timeline: &'a Timeline,
    pub(super) fps: f64,
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
        key.ik_joint_specs = span_key.ik_joint_specs.clone();
        key.ik_skins = span_key
            .ik_skins
            .iter()
            .filter(|s| present.contains(s.shape_element_id.as_str()))
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

mod clipboard_sync;
mod motion_keys;
mod range_ops;
mod reverse_curves;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use self::clipboard_sync::{
    convert_layer_to_keyframes_stepped, copy_frames, sync_layer_symbol, sync_layer_symbols,
};
#[allow(unused_imports)]
pub use self::range_ops::{KeyMoveError, edit_frames};
