//! Edit envelope modal dialog (NF2 `Dialog / Edit envelope`, DL-09; reference
//! `dialogs-sound-stroke.png`), opened by `Properties ▸ Sound ▸ Envelope…` and
//! `Library menu ▸ Edit envelope…`.
//!
//! Edits live on a draft until confirmed: `Cancel` / `Esc` discards without touching the project,
//! `OK` / `Enter` commits `FrameSound.effect` and `FrameSound.envelope` in a single Undo step
//! (`"Edit sound envelope"`).
use super::dialog_helpers::{FormDialog, dialog_width};
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{FrameSound, Id, SoundEffect};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

const MIN_TRIM_GAP_SECS: f64 = 0.01;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EditEnvelopeDraft {
    pub(crate) symbol_id: Id,
    pub(crate) layer_id: Id,
    pub(crate) key_frame: u32,
    pub(crate) resource: Id,
    pub(crate) effect: SoundEffect,
    pub(crate) left: Vec<(f32, f32)>,
    pub(crate) right: Vec<(f32, f32)>,
    pub(crate) selected_left: Option<usize>,
    pub(crate) selected_right: Option<usize>,
    pub(crate) in_point: Option<f64>,
    pub(crate) out_point: Option<f64>,
    had_explicit_out: bool,
    /// `0` = Seconds, `1` = Frames.
    pub(crate) unit: usize,
    /// Horizontal time ruler zoom (`1.0..=8.0`).
    pub(crate) zoom: f64,
}

impl EditEnvelopeDraft {
    pub(crate) fn from_sound(
        symbol_id: Id,
        layer_id: Id,
        key_frame: u32,
        sound: &FrameSound,
    ) -> Self {
        let points = sound.envelope_points();
        let (left, right) = split_stereo_envelope(&points);
        Self {
            symbol_id,
            layer_id,
            key_frame,
            resource: sound.resource.clone(),
            effect: sound.effect,
            left,
            right,
            selected_left: None,
            selected_right: None,
            in_point: sound.in_point,
            out_point: sound.out_point,
            had_explicit_out: sound.out_point.is_some(),
            unit: 0,
            zoom: 1.0,
        }
    }

    pub(crate) fn apply_preset(&mut self, effect: SoundEffect) {
        self.effect = effect;
        if effect != SoundEffect::Custom {
            let points: Vec<[f64; 3]> = effect
                .envelope()
                .iter()
                .map(|&(p, l, r)| [p, l, r])
                .collect();
            let (left, right) = split_stereo_envelope(&points);
            self.left = left;
            self.right = right;
            self.selected_left = None;
            self.selected_right = None;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn preview_sound(&self, base: Option<&FrameSound>) -> FrameSound {
        let mut fallback = 1.0_f64;
        if let Some(inp) = self.in_point.filter(|v| v.is_finite()) {
            fallback = fallback.max(inp + MIN_TRIM_GAP_SECS);
        }
        if let Some(outp) = self.out_point.filter(|v| v.is_finite()) {
            fallback = fallback.max(outp);
        }
        self.preview_sound_with_duration(base, fallback)
    }

    pub(crate) fn preview_sound_with_duration(
        &self,
        base: Option<&FrameSound>,
        duration_secs: f64,
    ) -> FrameSound {
        let mut sound = base
            .cloned()
            .unwrap_or_else(|| FrameSound::new(self.resource.clone()));
        let (norm_in, norm_out, _, _) = normalize_trim_bounds(
            self.in_point,
            self.out_point,
            duration_secs,
            self.had_explicit_out,
        );
        sound.resource = self.resource.clone();
        sound.effect = self.effect;
        sound.envelope = (self.effect == SoundEffect::Custom)
            .then(|| merge_stereo_envelope(&self.left, &self.right));
        sound.in_point = norm_in;
        sound.out_point = norm_out;
        sound
    }
}

pub(crate) fn effective_trim_gap(duration_secs: f64) -> f64 {
    MIN_TRIM_GAP_SECS
        .min(duration_secs.max(1e-3) * 0.25)
        .max(1e-5)
}

pub(crate) fn normalize_trim_bounds(
    in_point: Option<f64>,
    out_point: Option<f64>,
    duration_secs: f64,
    had_explicit_out: bool,
) -> (Option<f64>, Option<f64>, f64, f64) {
    let cap = duration_secs.clamp(1e-3, FrameSound::MAX_SOUND_TRIM_SECONDS);
    let min_gap = effective_trim_gap(cap);
    let max_in = (cap - min_gap).max(0.0);
    let eff_in = in_point
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, max_in);
    let min_out = (eff_in + min_gap).min(cap);
    let eff_out = out_point
        .filter(|v| v.is_finite())
        .unwrap_or(cap)
        .clamp(min_out, cap);
    let mut norm_in = if eff_in <= 1e-5 { None } else { Some(eff_in) };
    let mut norm_out = if (cap - eff_out) <= 1e-5 && !had_explicit_out {
        None
    } else {
        Some(eff_out)
    };
    if let (Some(i), Some(o)) = (norm_in, norm_out)
        && o <= i
    {
        if i + min_gap <= FrameSound::MAX_SOUND_TRIM_SECONDS {
            norm_out = Some(i + min_gap);
        } else {
            let clamped_i = (o - min_gap).max(0.0);
            norm_in = (clamped_i > 1e-5).then_some(clamped_i);
        }
    }
    (norm_in, norm_out, eff_in, eff_out)
}

pub(crate) fn project_envelope_to_view(
    canonical: &[(f32, f32)],
    eff_in: f64,
    eff_out: f64,
    visible_secs: f64,
) -> (Vec<(f32, f32)>, Vec<usize>) {
    let seg_secs = (eff_out - eff_in).max(1e-6);
    let vis = visible_secs.max(1e-6);
    let mut view_points = Vec::with_capacity(canonical.len());
    let mut origins = Vec::with_capacity(canonical.len());
    for (idx, &(t_seg, gain)) in canonical.iter().enumerate() {
        let sec = eff_in + f64::from(t_seg.clamp(0.0, 1.0)) * seg_secs;
        let t_view = sec / vis;
        if (-1e-5..=1.0 + 1e-5).contains(&t_view) {
            view_points.push((t_view.clamp(0.0, 1.0) as f32, gain.clamp(0.0, 1.0)));
            origins.push(idx);
        }
    }
    (view_points, origins)
}

pub(crate) fn apply_view_envelope_edits(
    canonical: &[(f32, f32)],
    view_before: &[(f32, f32)],
    origins: &[usize],
    view_after: &[(f32, f32)],
    eff_in: f64,
    eff_out: f64,
    visible_secs: f64,
) -> (Vec<(f32, f32)>, Option<usize>) {
    let seg_secs = (eff_out - eff_in).max(1e-6);
    let vis = visible_secs.max(1e-6);
    let mut next = canonical.to_vec();
    if view_after.len() == view_before.len() {
        if let Some(k) = changed_point_index(view_before, view_after)
            && let Some(&orig_idx) = origins.get(k)
            && orig_idx < next.len()
        {
            let (t_view, gain) = view_after[k];
            let raw_t_seg = ((f64::from(t_view) * vis - eff_in) / seg_secs) as f32;
            let lo = if orig_idx == 0 {
                0.0
            } else {
                next[orig_idx - 1].0
            };
            let hi = if orig_idx + 1 == next.len() {
                1.0
            } else {
                next[orig_idx + 1].0
            };
            next[orig_idx] = (raw_t_seg.clamp(lo, hi), gain.clamp(0.0, 1.0));
            return (next, Some(orig_idx));
        }
        return (next, None);
    }
    if view_after.len() > view_before.len() {
        let ins_k = (0..view_after.len())
            .find(|&k| view_before.get(k) != Some(&view_after[k]))
            .unwrap_or(view_after.len() - 1);
        let (t_view, gain) = view_after[ins_k];
        let t_seg = (((f64::from(t_view) * vis - eff_in) / seg_secs).clamp(0.0, 1.0)) as f32;
        let orig_ins = next.partition_point(|q| q.0 < t_seg);
        next.insert(orig_ins, (t_seg, gain.clamp(0.0, 1.0)));
        return (next, Some(orig_ins));
    }
    (next, None)
}

#[allow(clippy::too_many_arguments)]
fn paint_envelope_continuation_segments(
    painter: &egui::Painter,
    lane_rect: egui::Rect,
    canonical: &[(f32, f32)],
    view_points: &[(f32, f32)],
    origins: &[usize],
    eff_in: f64,
    eff_out: f64,
    visible_secs: f64,
    color: egui::Color32,
) {
    if canonical.is_empty() || visible_secs <= 0.0 {
        return;
    }
    let vis_start = eff_in.clamp(0.0, visible_secs);
    let vis_end = eff_out.clamp(0.0, visible_secs);
    if vis_end <= vis_start {
        return;
    }
    let seg_secs = (eff_out - eff_in).max(1e-6);
    let inner = lane_rect.shrink(c::envelope::ANCHOR_POINT / 2.0);
    let at = |t_view: f32, v: f32| -> egui::Pos2 {
        egui::pos2(
            inner.left() + t_view.clamp(0.0, 1.0) * inner.width(),
            inner.bottom() - v.clamp(0.0, 1.0) * inner.height(),
        )
    };
    let stroke = egui::Stroke::new(1.5_f32, color);
    if view_points.is_empty() {
        let t0 = ((vis_start - eff_in) / seg_secs).clamp(0.0, 1.0);
        let t1 = ((vis_end - eff_in) / seg_secs).clamp(0.0, 1.0);
        let v0 = sample_channel(canonical, t0) as f32;
        let v1 = sample_channel(canonical, t1) as f32;
        painter.line_segment(
            [
                at((vis_start / visible_secs) as f32, v0),
                at((vis_end / visible_secs) as f32, v1),
            ],
            stroke,
        );
        return;
    }
    let (tv0, v0) = view_points[0];
    let orig0 = origins[0];
    let tv_start = (vis_start / visible_secs) as f32;
    if orig0 > 0 {
        let t_edge = ((vis_start - eff_in) / seg_secs).clamp(0.0, 1.0);
        let v_edge = sample_channel(canonical, t_edge) as f32;
        painter.line_segment([at(tv_start, v_edge), at(tv0, v0)], stroke);
    } else if tv0 > tv_start + 1e-4 {
        painter.line_segment([at(tv_start, v0), at(tv0, v0)], stroke);
    }
    let (tvn, vn) = *view_points.last().unwrap();
    let orign = *origins.last().unwrap();
    let tv_end = (vis_end / visible_secs) as f32;
    if orign + 1 < canonical.len() {
        let t_edge = ((vis_end - eff_in) / seg_secs).clamp(0.0, 1.0);
        let v_edge = sample_channel(canonical, t_edge) as f32;
        painter.line_segment([at(tvn, vn), at(tv_end, v_edge)], stroke);
    } else if tvn + 1e-4 < tv_end {
        painter.line_segment([at(tvn, vn), at(tv_end, vn)], stroke);
    }
}

fn channel_waveform_columns(
    lane: &[[f32; 2]],
    audio_duration: f64,
    visible_secs: f64,
    columns: usize,
) -> Vec<(f32, f32)> {
    if columns == 0 || lane.is_empty() || audio_duration <= 0.0 || visible_secs <= 0.0 {
        return Vec::new();
    }
    let bucket_secs = audio_duration / lane.len() as f64;
    let col_secs = visible_secs / columns as f64;
    (0..columns)
        .map(|col| {
            let t0 = col as f64 * col_secs;
            if t0 >= audio_duration {
                return (0.0, 0.0);
            }
            let t1 = ((col + 1) as f64 * col_secs).min(audio_duration);
            let a = ((t0 / bucket_secs).floor() as usize).min(lane.len() - 1);
            let b = ((t1 / bucket_secs).ceil() as usize).clamp(a + 1, lane.len());
            lane[a..b]
                .iter()
                .fold((0.0_f32, 0.0_f32), |(lo, hi), &[x, y]| {
                    (lo.min(x), hi.max(y))
                })
        })
        .collect()
}

/// Splice underlay shapes (`paint_sound_peaks` + trim dim/guides) strictly after both `paint_span`
/// row backgrounds + centre `hline` of `EnvelopeLane` and strictly behind its border, channel
/// badge, envelope polyline, and anchor handles, optionally stripping synthetic `SpanType::Sound`
/// placeholder bars when real decoded audio peaks are available.
fn splice_envelope_lane_underlay(
    ctx: &egui::Context,
    base_painter: &egui::Painter,
    lane_span: (usize, usize, usize),
    strip_synthetic_peaks: Option<egui::Color32>,
    paint: impl FnOnce(&egui::Painter),
) {
    let (before_lane_idx, after_lane_idx, rendered_points) = lane_span;
    let layer_id = base_painter.layer_id();
    let opacity = base_painter.opacity();
    let scratch_layer = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("nf-edit-envelope-wave-scratch"),
    );
    let scratch = base_painter.clone().with_layer_id(scratch_layer);
    paint(&scratch);
    ctx.graphics_mut(|graphics| {
        let added: Vec<_> = {
            let list = graphics.entry(scratch_layer);
            let items = list.all_entries().cloned().collect();
            *list = Default::default();
            items
        };
        if added.is_empty() && strip_synthetic_peaks.is_none() {
            return;
        }
        let target = graphics.entry(layer_id);
        let mut all: Vec<_> = target.all_entries().cloned().collect();
        let start = before_lane_idx.min(all.len());
        let end = after_lane_idx.clamp(start, all.len());
        // `EnvelopeLane` paints its unfilled `rect_stroke` border immediately after both
        // `paint_span` backgrounds and the centre `hline`, followed by channel `text`, curve, and
        // `AnchorPoint` handles.
        let fg_fallback = (3 + 2 * rendered_points).min(end - start);
        let bg_end = all[start..end]
            .iter()
            .position(|entry| {
                matches!(
                    &entry.shape,
                    egui::Shape::Rect(r) if r.fill == egui::Color32::TRANSPARENT
                )
            })
            .map_or(end - fg_fallback, |rel| start + rel);
        let bg_kept: Vec<_> = all[start..bg_end]
            .iter()
            .filter(|entry| {
                strip_synthetic_peaks.is_none_or(|peak_color| {
                    let painted_peak = peak_color.gamma_multiply(opacity);
                    !matches!(
                        entry.shape,
                        egui::Shape::LineSegment { stroke, .. }
                            if stroke.color == painted_peak || stroke.color == peak_color
                    )
                })
            })
            .cloned()
            .collect();
        all.splice(start..bg_end, bg_kept.into_iter().chain(added));
        *target = Default::default();
        for entry in all {
            target.add(entry.clip_rect, entry.shape);
        }
    });
}

pub(crate) type StereoEnvelopeLanes = (Vec<(f32, f32)>, Vec<(f32, f32)>);

pub(crate) fn split_stereo_envelope(points: &[[f64; 3]]) -> StereoEnvelopeLanes {
    if points.is_empty() {
        return (vec![(0.0, 1.0)], vec![(0.0, 1.0)]);
    }
    let left = points
        .iter()
        .map(|p| (p[0].clamp(0.0, 1.0) as f32, p[1].clamp(0.0, 1.0) as f32))
        .collect();
    let right = points
        .iter()
        .map(|p| (p[0].clamp(0.0, 1.0) as f32, p[2].clamp(0.0, 1.0) as f32))
        .collect();
    (left, right)
}

fn sample_channel(points: &[(f32, f32)], t: f64) -> f64 {
    if points.is_empty() {
        return 1.0;
    }
    let t = t.clamp(0.0, 1.0);
    let first = points[0];
    if t <= f64::from(first.0) {
        return f64::from(first.1).clamp(0.0, 1.0);
    }
    for pair in points.windows(2) {
        let (t0, v0) = (f64::from(pair[0].0), f64::from(pair[0].1));
        let (t1, v1) = (f64::from(pair[1].0), f64::from(pair[1].1));
        if t <= t1 {
            let span = t1 - t0;
            let u = if span > 1e-9 { (t - t0) / span } else { 1.0 };
            return (v0 + (v1 - v0) * u).clamp(0.0, 1.0);
        }
    }
    f64::from(points.last().unwrap().1).clamp(0.0, 1.0)
}

/// Merge independent L and R `(t, gain)` handle lists into sorted `[position, left, right]` points
/// capped at [`FrameSound::MAX_ENVELOPE_POINTS`].
pub(crate) fn merge_stereo_envelope(left: &[(f32, f32)], right: &[(f32, f32)]) -> Vec<[f64; 3]> {
    let mut left_sorted: Vec<(f32, f32)> = left
        .iter()
        .map(|&(t, v)| (t.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
        .collect();
    let mut right_sorted: Vec<(f32, f32)> = right
        .iter()
        .map(|&(t, v)| (t.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
        .collect();
    left_sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    right_sorted.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut times: Vec<f64> = left_sorted
        .iter()
        .chain(right_sorted.iter())
        .map(|&(t, _)| f64::from(t).clamp(0.0, 1.0))
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let mut unique_times: Vec<f64> = Vec::with_capacity(times.len());
    for t in times {
        if unique_times
            .last()
            .is_none_or(|prev| (t - *prev).abs() > 1e-4)
        {
            unique_times.push(t);
        }
    }
    if unique_times.is_empty() {
        return vec![[0.0, 1.0, 1.0]];
    }
    let mut merged: Vec<[f64; 3]> = unique_times
        .into_iter()
        .map(|t| {
            [
                t,
                sample_channel(&left_sorted, t),
                sample_channel(&right_sorted, t),
            ]
        })
        .collect();
    while merged.len() > FrameSound::MAX_ENVELOPE_POINTS {
        // Drop the interior point closest in time to its neighbours so endpoints and shape stay intact.
        let mut best_idx = 1;
        let mut best_span = f64::INFINITY;
        for i in 1..merged.len() - 1 {
            let span = merged[i + 1][0] - merged[i - 1][0];
            if span < best_span {
                best_span = span;
                best_idx = i;
            }
        }
        merged.remove(best_idx);
    }
    merged
}

fn unique_time_count(left: &[(f32, f32)], right: &[(f32, f32)]) -> usize {
    let mut times: Vec<f64> = left
        .iter()
        .chain(right.iter())
        .map(|&(t, _)| f64::from(t).clamp(0.0, 1.0))
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let mut count = 0;
    let mut last: Option<f64> = None;
    for t in times {
        if last.is_none_or(|prev| (t - prev).abs() > 1e-4) {
            count += 1;
            last = Some(t);
        }
    }
    count
}

fn changed_point_index(before: &[(f32, f32)], after: &[(f32, f32)]) -> Option<usize> {
    after
        .iter()
        .enumerate()
        .find(|&(i, p)| before.get(i) != Some(p))
        .map(|(i, _)| i)
}

fn choose_second_step(visible_secs: f64) -> f64 {
    for step in [
        0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 4.0, 5.0, 10.0,
        15.0, 30.0, 60.0,
    ] {
        if visible_secs / step <= 9.0 {
            return step;
        }
    }
    (visible_secs / 8.0).ceil().max(1.0)
}

impl Editor {
    /// Open the `Edit envelope` modal dialog for the frame sound at the playhead.
    pub(crate) fn open_edit_envelope(&mut self) {
        self.finish_edits();
        let target = if let Some((key_frame, sound)) = self.frame_sound() {
            if !self.layer_editable() {
                self.message("Select an unlocked, visible layer to edit its sound envelope.");
                return;
            }
            let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
            Some((self.layer, layer_id, key_frame, sound))
        } else {
            let timeline = &self.symbol().timeline;
            timeline.layers.iter().enumerate().find_map(|(idx, layer)| {
                if neoflash_core::layer_effectively_locked(timeline, layer)
                    || !neoflash_core::layer_effectively_visible(timeline, layer)
                    || !self.is_layer_id_effectively_soloed(&layer.id)
                {
                    return None;
                }
                let key = layer.key_at(self.frame)?;
                let sound = key.sound.clone()?;
                Some((idx, layer.id.clone(), key.frame, sound))
            })
        };
        let Some((layer_idx, layer_id, key_frame, sound)) = target else {
            self.message("Select a keyframe with a sound first.");
            return;
        };
        self.layer = layer_idx;
        let symbol_id = self.symbol_id().to_owned();
        self.edit_envelope = Some(EditEnvelopeDraft::from_sound(
            symbol_id, layer_id, key_frame, &sound,
        ));
    }

    pub(crate) fn cancel_edit_envelope(&mut self) {
        if self.edit_envelope.take().is_some() {
            self.timeline_audio.stop_audition();
        }
    }

    fn edit_envelope_duration_secs(&mut self, draft: &EditEnvelopeDraft) -> f64 {
        self.timeline_audio
            .decoded(&self.project, &draft.resource)
            .map(|d| d.duration().max(1e-3))
            .unwrap_or_else(|_| {
                let frames = self.symbol().timeline.frame_count.max(1) as f64;
                let fps = self.project.fps.max(1) as f64;
                let mut fallback = (frames / fps).max(1.0);
                if let Some(inp) = draft.in_point.filter(|v| v.is_finite()) {
                    fallback = fallback.max(inp + MIN_TRIM_GAP_SECS);
                }
                if let Some(outp) = draft.out_point.filter(|v| v.is_finite()) {
                    fallback = fallback.max(outp);
                }
                fallback.clamp(1e-3, FrameSound::MAX_SOUND_TRIM_SECONDS)
            })
    }

    pub(crate) fn commit_edit_envelope(&mut self, draft: EditEnvelopeDraft) {
        self.timeline_audio.stop_audition();
        let duration_secs = self.edit_envelope_duration_secs(&draft);
        let (norm_in, norm_out, _, _) = normalize_trim_bounds(
            draft.in_point,
            draft.out_point,
            duration_secs,
            draft.had_explicit_out,
        );
        let new_envelope = (draft.effect == SoundEffect::Custom)
            .then(|| merge_stereo_envelope(&draft.left, &draft.right));
        let unchanged = self
            .project
            .symbol(&draft.symbol_id)
            .and_then(|s| s.timeline.layers.iter().find(|l| l.id == draft.layer_id))
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == draft.key_frame))
            .and_then(|k| k.sound.as_ref())
            .is_some_and(|s| {
                s.effect == draft.effect
                    && s.envelope == new_envelope
                    && s.in_point == norm_in
                    && s.out_point == norm_out
            });
        if unchanged {
            return;
        }
        self.transact_named("Edit sound envelope", |s| {
            if let Some(sound) = s
                .project
                .symbol_mut(&draft.symbol_id)
                .and_then(|sym| {
                    sym.timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == draft.layer_id)
                })
                .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == draft.key_frame))
                .and_then(|k| k.sound.as_mut())
            {
                sound.effect = draft.effect;
                sound.envelope = new_envelope;
                sound.in_point = norm_in;
                sound.out_point = norm_out;
            }
        });
    }

    pub(crate) fn edit_envelope_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.edit_envelope.clone() else {
            return;
        };
        let nf = neoflash_ui::palette(ctx);
        let duration_secs = self.edit_envelope_duration_secs(&draft);
        let fps = self.project.fps.max(1) as f64;

        let mut close_cancel = false;
        let mut close_ok = false;
        let mut play_preview = false;
        let mut stop_preview = false;
        let mut over_limit = false;

        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let width = dialog_width(ctx, metrics::dialog_l());
        let peak_buckets = ((width as f64 * draft.zoom).ceil() as usize).clamp(64, 8192);
        let decoded_peaks = self.timeline_audio.decoded_waveform_peaks(
            &self.project,
            &draft.resource,
            peak_buckets,
        );
        // Channel lanes are artwork three list rows high.
        let lane_height = 3.0 * metrics::row();
        let modal = egui::Modal::new(egui::Id::new("edit-envelope"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Edit envelope", width, None);
                close_cancel |= dialog.close;
                dialog.page(ui, "edit-envelope-page", |ui, _| {
                    ui.horizontal(|ui| {
                        let mut picked = None;
                        c::Select::new("edit-envelope-effect", draft.effect.label())
                            .prefix("Effect")
                            .width(metrics::field_l())
                            .show(ui, |ui| {
                                for option in SoundEffect::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(option.label())
                                                .checked(option == draft.effect),
                                        )
                                        .clicked()
                                    {
                                        picked = Some(option);
                                        ui.close();
                                    }
                                }
                            });
                        if let Some(effect) = picked {
                            draft.apply_preset(effect);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            c::Segmented::show(
                                ui,
                                &mut draft.unit,
                                &[(0, "Seconds"), (1, "Frames")],
                                false,
                            );
                        });
                    });

                    let visible_secs = (duration_secs / draft.zoom.max(1.0)).max(1e-4);
                    let min_gap = effective_trim_gap(duration_secs);
                    let (_, _, mut eff_in, mut eff_out) = normalize_trim_bounds(
                        draft.in_point,
                        draft.out_point,
                        duration_secs,
                        draft.had_explicit_out,
                    );

                    let layer_id = ui.layer_id();
                    let before_l = ui
                        .ctx()
                        .graphics_mut(|g| g.entry(layer_id).all_entries().count());
                    let (mut view_left, origins_left) =
                        project_envelope_to_view(&draft.left, eff_in, eff_out, visible_secs);
                    let view_left_before = view_left.clone();
                    let view_selected_left = draft
                        .selected_left
                        .and_then(|orig| origins_left.iter().position(|&o| o == orig));
                    let resp_l = ui.add(c::EnvelopeLane {
                        channel: "L",
                        points: &mut view_left,
                        selected: view_selected_left,
                        height: lane_height,
                    });
                    paint_envelope_continuation_segments(
                        ui.painter(),
                        resp_l.rect,
                        &draft.left,
                        &view_left,
                        &origins_left,
                        eff_in,
                        eff_out,
                        visible_secs,
                        nf.accent_default,
                    );
                    let rendered_points_l = view_left.len();
                    let after_l = ui
                        .ctx()
                        .graphics_mut(|g| g.entry(layer_id).all_entries().count());
                    if resp_l.changed() {
                        let (candidate_left, changed_orig) = apply_view_envelope_edits(
                            &draft.left,
                            &view_left_before,
                            &origins_left,
                            &view_left,
                            eff_in,
                            eff_out,
                            visible_secs,
                        );
                        if candidate_left.len() > draft.left.len()
                            && unique_time_count(&candidate_left, &draft.right)
                                > FrameSound::MAX_ENVELOPE_POINTS
                        {
                            over_limit = true;
                        } else {
                            draft.left = candidate_left;
                            draft.selected_left = changed_orig.or(draft.selected_left);
                            draft.selected_right = None;
                            draft.effect = SoundEffect::Custom;
                        }
                    }

                    let (ruler, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), metrics::toggle_row()),
                        egui::Sense::hover(),
                    );
                    if draft.unit == 1 {
                        let visible_frames = ((visible_secs * fps).round() as u32).max(1);
                        let major = (visible_frames / 8).max(1).div_ceil(5) * 5;
                        c::paint_frame_scale(
                            ui.painter(),
                            ui.ctx(),
                            ruler,
                            visible_frames,
                            major.max(1),
                        );
                    } else {
                        let step = choose_second_step(visible_secs);
                        let steps = (visible_secs / step).floor() as usize;
                        for i in 0..=steps {
                            let s = i as f64 * step;
                            let x = ruler.left()
                                + (s / visible_secs).clamp(0.0, 1.0) as f32 * ruler.width();
                            let align = if i == 0 {
                                egui::Align2::LEFT_CENTER
                            } else if (s - visible_secs).abs() < step * 0.05 {
                                egui::Align2::RIGHT_CENTER
                            } else {
                                egui::Align2::CENTER_CENTER
                            };
                            let label = if step >= 1.0 {
                                format!("{:.0}s", s)
                            } else if step >= 0.1 {
                                format!("{:.1}s", s)
                            } else if step >= 0.01 {
                                format!("{:.2}s", s)
                            } else {
                                format!("{:.3}s", s)
                            };
                            ui.painter().text(
                                egui::pos2(x, ruler.center().y),
                                align,
                                label,
                                neoflash_ui::typography::font(TextStyle::MonoSmall),
                                nf.text_tertiary,
                            );
                        }
                    }

                    let x_for_sec = |sec: f64| -> f32 {
                        ruler.left() + (sec / visible_secs).clamp(0.0, 1.0) as f32 * ruler.width()
                    };
                    let handle_half = space::s2();
                    let out_drag_id = egui::Id::new("edit-envelope-trim-out");
                    let out_handle_active =
                        eff_out <= visible_secs + 1e-9 || ui.ctx().is_being_dragged(out_drag_id);
                    let in_x_initial = x_for_sec(eff_in);
                    let out_x_initial = x_for_sec(eff_out);
                    let mid_x = if out_handle_active {
                        (in_x_initial + out_x_initial) * 0.5
                    } else {
                        ruler.right()
                    };
                    let in_hit = if eff_in <= visible_secs + 1e-9
                        || ui
                            .ctx()
                            .is_being_dragged(egui::Id::new("edit-envelope-trim-in"))
                    {
                        let left = (in_x_initial - handle_half).max(ruler.left());
                        let right = (in_x_initial + handle_half)
                            .min(mid_x)
                            .min(ruler.right())
                            .max(left);
                        egui::Rect::from_min_max(
                            egui::pos2(left, ruler.top()),
                            egui::pos2(right, ruler.bottom()),
                        )
                    } else {
                        egui::Rect::NOTHING
                    };
                    let out_hit = if out_handle_active {
                        let right = (out_x_initial + handle_half).min(ruler.right());
                        let left = (out_x_initial - handle_half)
                            .max(mid_x)
                            .max(ruler.left())
                            .min(right);
                        egui::Rect::from_min_max(
                            egui::pos2(left, ruler.top()),
                            egui::pos2(right, ruler.bottom()),
                        )
                    } else {
                        egui::Rect::NOTHING
                    };
                    let in_resp = ui.interact(
                        in_hit,
                        egui::Id::new("edit-envelope-trim-in"),
                        egui::Sense::click_and_drag(),
                    );
                    let out_resp = ui.interact(out_hit, out_drag_id, egui::Sense::click_and_drag());
                    if in_resp.hovered()
                        || in_resp.dragged()
                        || out_resp.hovered()
                        || out_resp.dragged()
                    {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if in_resp.dragged() {
                        let next_in = if let Some(pos) = in_resp.interact_pointer_pos() {
                            (f64::from(pos.x - ruler.left()) / f64::from(ruler.width().max(1.0)))
                                * visible_secs
                        } else {
                            eff_in
                                + (f64::from(in_resp.drag_delta().x)
                                    / f64::from(ruler.width().max(1.0)))
                                    * visible_secs
                        }
                        .clamp(0.0, (eff_out - min_gap).max(0.0));
                        let (norm_in, norm_out, new_in, new_out) = normalize_trim_bounds(
                            Some(next_in),
                            Some(eff_out),
                            duration_secs,
                            draft.had_explicit_out,
                        );
                        eff_in = new_in;
                        eff_out = new_out;
                        draft.in_point = norm_in;
                        draft.out_point = norm_out;
                    }
                    if out_resp.dragged() {
                        let min_out = (eff_in + min_gap).min(duration_secs);
                        let next_out = if let Some(pos) = out_resp.interact_pointer_pos() {
                            (f64::from(pos.x - ruler.left()) / f64::from(ruler.width().max(1.0)))
                                * visible_secs
                        } else {
                            eff_out
                                + (f64::from(out_resp.drag_delta().x)
                                    / f64::from(ruler.width().max(1.0)))
                                    * visible_secs
                        }
                        .clamp(min_out, duration_secs);
                        let (norm_in, norm_out, new_in, new_out) = normalize_trim_bounds(
                            Some(eff_in),
                            Some(next_out),
                            duration_secs,
                            draft.had_explicit_out,
                        );
                        eff_in = new_in;
                        eff_out = new_out;
                        draft.in_point = norm_in;
                        draft.out_point = norm_out;
                    }
                    let in_x = x_for_sec(eff_in);
                    let out_x = x_for_sec(eff_out);
                    let tick_arm = space::s1();
                    let in_color =
                        if in_resp.hovered() || in_resp.dragged() || draft.in_point.is_some() {
                            nf.accent_default
                        } else {
                            nf.border_strong
                        };
                    let out_color =
                        if out_resp.hovered() || out_resp.dragged() || draft.out_point.is_some() {
                            nf.accent_default
                        } else {
                            nf.border_strong
                        };
                    if eff_in <= visible_secs {
                        let stroke = egui::Stroke::new(1.5_f32, in_color);
                        ui.painter().vline(in_x, ruler.y_range(), stroke);
                        ui.painter().hline(
                            in_x..=(in_x + tick_arm).min(ruler.right()),
                            ruler.top(),
                            stroke,
                        );
                        ui.painter().hline(
                            in_x..=(in_x + tick_arm).min(ruler.right()),
                            ruler.bottom(),
                            stroke,
                        );
                    }
                    if eff_out <= visible_secs {
                        let stroke = egui::Stroke::new(1.5_f32, out_color);
                        ui.painter().vline(out_x, ruler.y_range(), stroke);
                        ui.painter().hline(
                            (out_x - tick_arm).max(ruler.left())..=out_x,
                            ruler.top(),
                            stroke,
                        );
                        ui.painter().hline(
                            (out_x - tick_arm).max(ruler.left())..=out_x,
                            ruler.bottom(),
                            stroke,
                        );
                    }

                    let before_r = ui
                        .ctx()
                        .graphics_mut(|g| g.entry(layer_id).all_entries().count());
                    let (mut view_right, origins_right) =
                        project_envelope_to_view(&draft.right, eff_in, eff_out, visible_secs);
                    let view_right_before = view_right.clone();
                    let view_selected_right = draft
                        .selected_right
                        .and_then(|orig| origins_right.iter().position(|&o| o == orig));
                    let resp_r = ui.add(c::EnvelopeLane {
                        channel: "R",
                        points: &mut view_right,
                        selected: view_selected_right,
                        height: lane_height,
                    });
                    paint_envelope_continuation_segments(
                        ui.painter(),
                        resp_r.rect,
                        &draft.right,
                        &view_right,
                        &origins_right,
                        eff_in,
                        eff_out,
                        visible_secs,
                        nf.accent_default,
                    );
                    let rendered_points_r = view_right.len();
                    let after_r = ui
                        .ctx()
                        .graphics_mut(|g| g.entry(layer_id).all_entries().count());
                    if resp_r.changed() {
                        let (candidate_right, changed_orig) = apply_view_envelope_edits(
                            &draft.right,
                            &view_right_before,
                            &origins_right,
                            &view_right,
                            eff_in,
                            eff_out,
                            visible_secs,
                        );
                        if candidate_right.len() > draft.right.len()
                            && unique_time_count(&draft.left, &candidate_right)
                                > FrameSound::MAX_ENVELOPE_POINTS
                        {
                            over_limit = true;
                        } else {
                            draft.right = candidate_right;
                            draft.selected_right = changed_orig.or(draft.selected_right);
                            draft.selected_left = None;
                            draft.effect = SoundEffect::Custom;
                        }
                    }

                    let cols = resp_l.rect.width().ceil().max(1.0) as usize;
                    let (left_cols, right_cols) = if let Some(ref peaks) = decoded_peaks {
                        let l_cols = channel_waveform_columns(
                            &peaks.left,
                            peaks.duration,
                            visible_secs,
                            cols,
                        );
                        let r_source = if peaks.right.is_empty() {
                            &peaks.left
                        } else {
                            &peaks.right
                        };
                        let r_cols =
                            channel_waveform_columns(r_source, peaks.duration, visible_secs, cols);
                        (l_cols, r_cols)
                    } else {
                        (Vec::new(), Vec::new())
                    };
                    let paint_trim_shade = |painter: &egui::Painter, rect: egui::Rect| {
                        if eff_in > 0.0 {
                            let rx = in_x.clamp(rect.left(), rect.right());
                            if rx > rect.left() {
                                painter.rect_filled(
                                    egui::Rect::from_min_max(
                                        rect.min,
                                        egui::pos2(rx, rect.bottom()),
                                    ),
                                    0.0,
                                    nf.overlay,
                                );
                                painter.vline(
                                    rx,
                                    rect.y_range(),
                                    egui::Stroke::new(1.0_f32, nf.border_strong),
                                );
                            }
                        }
                        if eff_out < visible_secs {
                            let lx = out_x.clamp(rect.left(), rect.right());
                            if lx < rect.right() {
                                painter.rect_filled(
                                    egui::Rect::from_min_max(egui::pos2(lx, rect.top()), rect.max),
                                    0.0,
                                    nf.overlay,
                                );
                                painter.vline(
                                    lx,
                                    rect.y_range(),
                                    egui::Stroke::new(1.0_f32, nf.border_strong),
                                );
                            }
                        }
                    };
                    let strip_synthetic = decoded_peaks.is_some().then_some(nf.success_default);
                    // Splice `R` first, then `L` so `L`'s `[before_l..after_l]` indices never shift.
                    splice_envelope_lane_underlay(
                        ui.ctx(),
                        ui.painter(),
                        (before_r, after_r, rendered_points_r),
                        strip_synthetic,
                        |painter| {
                            if !right_cols.is_empty() {
                                c::timeline::paint_sound_peaks(
                                    painter,
                                    ui.ctx(),
                                    resp_r.rect,
                                    &[right_cols.as_slice()],
                                );
                            }
                            paint_trim_shade(painter, resp_r.rect);
                        },
                    );
                    splice_envelope_lane_underlay(
                        ui.ctx(),
                        ui.painter(),
                        (before_l, after_l, rendered_points_l),
                        strip_synthetic,
                        |painter| {
                            if !left_cols.is_empty() {
                                c::timeline::paint_sound_peaks(
                                    painter,
                                    ui.ctx(),
                                    resp_l.rect,
                                    &[left_cols.as_slice()],
                                );
                            }
                            paint_trim_shade(painter, resp_l.rect);
                        },
                    );
                });
                dialog.footer(
                    ui,
                    |ui| {
                        ui.spacing_mut().item_spacing.x = space::s1();
                        play_preview |= ui.add(c::IconButton::new(i16::PLAY, "Play")).clicked();
                        stop_preview |= ui.add(c::IconButton::new(i16::STOP, "Stop")).clicked();
                        if ui
                            .add_enabled(
                                draft.zoom < 8.0,
                                c::IconButton::new(i16::ZOOMIN, "Zoom in"),
                            )
                            .clicked()
                        {
                            draft.zoom = (draft.zoom * 2.0).min(8.0);
                        }
                        if ui
                            .add_enabled(
                                draft.zoom > 1.0,
                                c::IconButton::new(i16::ZOOMOUT, "Zoom out"),
                            )
                            .clicked()
                        {
                            draft.zoom = (draft.zoom * 0.5).max(1.0);
                        }
                        ui.add_space(space::s2());
                        ui.add(
                            egui::Label::new(
                                rich(
                                    "Drag handles to shape volume · click a lane to add a point",
                                    TextStyle::Small,
                                )
                                .color(nf.text_secondary),
                            )
                            .truncate(),
                        );
                    },
                    |ui| {
                        close_ok |= ui.add(c::Button::primary("OK")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        if over_limit {
            self.message("Envelope can have at most 32 points.");
        }
        if play_preview {
            let base = self.frame_sound().map(|(_, s)| s);
            let preview = draft.preview_sound_with_duration(base.as_ref(), duration_secs);
            self.timeline_audio.audition(&self.project, &preview);
        }
        if stop_preview {
            self.timeline_audio.stop_audition();
        }

        if !close_cancel && !close_ok {
            let typing = kb.was_typing || ctx.wants_keyboard_input();
            let popup_open = kb.was_popup_open || super::dialog_helpers::is_any_popup_open(ctx);
            if !typing
                && !popup_open
                && ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                })
            {
                if let Some(idx) = draft.selected_left
                    && draft.left.len() > 1
                    && idx < draft.left.len()
                {
                    draft.left.remove(idx);
                    draft.selected_left = (idx > 0).then(|| idx - 1);
                    draft.effect = SoundEffect::Custom;
                } else if let Some(idx) = draft.selected_right
                    && draft.right.len() > 1
                    && idx < draft.right.len()
                {
                    draft.right.remove(idx);
                    draft.selected_right = (idx > 0).then(|| idx - 1);
                    draft.effect = SoundEffect::Custom;
                }
            }
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => close_ok = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        if close_cancel {
            self.cancel_edit_envelope();
        } else if close_ok {
            self.edit_envelope = None;
            self.commit_edit_envelope(draft);
        } else {
            self.edit_envelope = Some(draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Keyframe, Project, ResourceUnit};

    fn tiny_wav_bytes(sample_rate: u32, num_frames: usize) -> Vec<u8> {
        let channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let data_bytes = (num_frames * usize::from(channels) * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data_bytes as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        wav.extend_from_slice(&(channels * (bits_per_sample / 8)).to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_frames {
            let sample = ((i as f32 * 0.25).sin() * 16_000.0) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    #[test]
    fn t414_edit_envelope_short_sfx_trim_validation_and_zoom_lane_mapping() {
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.format_version = 6;
        // 20 ms WAV SFX (882 samples @ 44.1 kHz < 100 ms).
        let res_id = "sfx-20ms".to_owned();
        project.resources.push(
            ResourceUnit::from_bytes(
                &res_id,
                "audio/click.wav",
                "audio/wav",
                &tiny_wav_bytes(44_100, 882),
            )
            .unwrap(),
        );
        project.symbols[0].timeline.layers[0].keyframes = vec![Keyframe {
            frame: 0,
            sound: Some(FrameSound::new(res_id.clone())),
            ..Keyframe::default()
        }];
        project.validate().unwrap();

        let mut editor = Editor::from_project(&ctx, project);
        editor.open_edit_envelope();
        let mut draft = editor.edit_envelope.clone().expect("dialog opened");

        // 1. Duration is preserved at ~20 ms (not inflated to 100 ms), with min_gap <= 5 ms.
        let dur = editor.edit_envelope_duration_secs(&draft);
        assert!(
            (dur - 0.02).abs() < 0.002,
            "20 ms WAV duration must not inflate to 0.1s: {dur}"
        );
        let gap = effective_trim_gap(dur);
        assert!((1e-5..=0.005 + 1e-6).contains(&gap));

        // 2. Inverted / colliding (in_point >= out_point) drafts normalize cleanly on commit
        //    so `project.validate()` never fails!
        draft.in_point = Some(0.015);
        draft.out_point = Some(0.010);
        draft.had_explicit_out = true;
        editor.edit_envelope = None;
        editor.commit_edit_envelope(draft.clone());
        editor
            .project
            .validate()
            .expect("normalized trim bounds must pass project.validate()");
        let (_, committed) = editor.frame_sound().unwrap();
        assert!(committed.out_point.unwrap() > committed.in_point.unwrap());

        // VoiceSource plays the short <20 ms trimmed window without rounding seg_frames to 0.
        let decoded = editor
            .timeline_audio
            .decoded(&editor.project, &res_id)
            .unwrap();
        let mut voice = crate::timeline_sound::VoiceSource::new(
            decoded,
            0.0,
            1,
            false,
            (committed.in_point, committed.out_point),
            committed.envelope_points(),
        )
        .expect("short trimmed SFX must produce playable VoiceSource");
        assert!(voice.next().is_some());

        // 3. Zoomed/trimmed EnvelopeLane coordinate projection & back-mapping:
        //    On a 2.0s sound trimmed to [0.4s..1.6s] (seg_secs = 1.2s) at 2x zoom (visible = 1.0s):
        let canonical = vec![(0.0_f32, 0.2_f32), (0.5_f32, 0.8_f32), (1.0_f32, 1.0_f32)];
        let (view_pts, origins) = project_envelope_to_view(&canonical, 0.4, 1.6, 1.0);
        // Handle 0 (0.4s -> t_view = 0.4) and Handle 1 (1.0s -> t_view = 1.0) are visible;
        // Handle 2 (1.6s -> t_view = 1.6) is offscreen and NOT clamped onto 1.0s!
        assert_eq!(origins, vec![0, 1]);
        assert!((view_pts[0].0 - 0.4).abs() < 1e-5);
        assert!((view_pts[1].0 - 1.0).abs() < 1e-5);

        // Dragging Handle 0 from t_view = 0.4 (0.4s) to t_view = 0.7 (0.7s -> t_seg = 0.3/1.2 = 0.25):
        let mut dragged = view_pts.clone();
        dragged[0] = (0.7, 0.5);
        let (updated, sel) =
            apply_view_envelope_edits(&canonical, &view_pts, &origins, &dragged, 0.4, 1.6, 1.0);
        assert_eq!(sel, Some(0));
        assert!((updated[0].0 - 0.25).abs() < 1e-5);
        assert!((updated[0].1 - 0.5).abs() < 1e-5);
        assert_eq!(updated[2], (1.0, 1.0));

        // Render dialog at 4x zoom (eff_out > visible_secs) to ensure offscreen out_hit doesn't panic or overlap.
        draft.zoom = 4.0;
        draft.in_point = Some(0.002);
        draft.out_point = Some(0.018);
        editor.edit_envelope = Some(draft);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            editor.edit_envelope_dialog(ctx);
        });
    }
}
