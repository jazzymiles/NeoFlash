//! Timeline (frame) sounds: the `Player` turns keyframe sounds into host commands when
//! frames are entered. Hosts (Web Audio, the native editor) only schedule decoded audio;
//! which sound starts, stops or seeks, and at which offset, is decided here.
//!
//! Flash semantics: Event plays on every entry and overlaps; Start plays unless an instance
//! of the same sound is still playing (a host fact); Stop stops every instance of the sound;
//! Stream is locked to its span on the layer — it starts at the playhead's offset into the
//! span, restarts after a jump, and stops when the span ends or that timeline stops.
use crate::evaluate::Node;
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One instruction for the audio host, in the order the timeline produced them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SoundCommand {
    /// Event or Start sync. For Start the host skips the command while an instance of
    /// `resource` is still playing.
    Play {
        resource: Id,
        sync: SoundSync,
        /// Total number of plays; ignored when `loop_forever`.
        loops: u32,
        loop_forever: bool,
        /// `(position 0..=1 of the played length, left, right)` volume points.
        envelope: Vec<[f64; 3]>,
        /// Optional playback trim start in seconds within `resource`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        in_point: Option<f64>,
        /// Optional playback trim end in seconds within `resource`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        out_point: Option<f64>,
    },
    /// Stop every playing instance of `resource` (Stop sync).
    Stop { resource: Id },
    /// Start a timeline-locked stream on `channel` at `offset` seconds into the sound.
    StreamStart {
        channel: String,
        resource: Id,
        offset: f64,
        loops: u32,
        loop_forever: bool,
        envelope: Vec<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        in_point: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        out_point: Option<f64>,
    },
    /// Stop the stream on `channel`.
    StreamStop { channel: String },
    /// Stop every sound (Restart, `stopAllSounds()`, `SoundMixer.stopAll()`).
    StopAll,
}

#[derive(Default)]
pub(crate) struct SoundState {
    commands: Vec<SoundCommand>,
    /// Active stream channel -> last timeline frame it was synchronised with.
    streams: HashMap<String, u32>,
    started: bool,
}

/// Frame sounds are ignored on guide and folder layers, which are never published.
fn sound_layers(symbol: &Symbol) -> impl Iterator<Item = &Layer> {
    symbol
        .timeline
        .layers
        .iter()
        .filter(|layer| !matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder))
}

fn envelope(sound: &FrameSound) -> Vec<[f64; 3]> {
    sound.envelope_points()
}

/// Left and right gain of a `(position, left, right)` envelope at `position` (0..=1 of the
/// played length), linear between points and held before the first and after the last.
pub fn envelope_gains(points: &[[f64; 3]], position: f64) -> (f64, f64) {
    let Some(first) = points.first() else {
        return (1.0, 1.0);
    };
    let position = position.clamp(0.0, 1.0);
    let mut previous = first;
    for point in points {
        if point[0] >= position {
            if point[0] <= previous[0] {
                return (point[1], point[2]);
            }
            let t = (position - previous[0]) / (point[0] - previous[0]);
            return (
                previous[1] + (point[1] - previous[1]) * t,
                previous[2] + (point[2] - previous[2]) * t,
            );
        }
        previous = point;
    }
    (previous[1], previous[2])
}

/// Audio resources attached to any keyframe, for hosts that decode ahead of playback.
pub fn timeline_sound_resources(project: &Project) -> Vec<Id> {
    let mut ids: Vec<Id> = project
        .symbols
        .iter()
        .flat_map(sound_layers)
        .flat_map(|layer| &layer.keyframes)
        .filter_map(|key| key.sound.as_ref())
        .filter(|sound| sound.sync != SoundSync::Stop)
        .map(|sound| sound.resource.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Exclusive end frame of the span that starts at `key` on `layer`.
pub fn sound_span_end(layer: &Layer, key: &Keyframe, frame_count: u32) -> u32 {
    let next = layer
        .keyframes
        .iter()
        .filter(|k| k.frame > key.frame)
        .map(|k| k.frame)
        .min()
        .unwrap_or(frame_count);
    next.min(layer.properties.end_frame.unwrap_or(frame_count))
        .min(frame_count)
}

impl SoundState {
    fn push(&mut self, command: SoundCommand) {
        self.started = true;
        self.commands.push(command);
    }

    /// Event/Start/Stop sounds of the keyframes that begin exactly at `frame`.
    pub(crate) fn frame_entered(&mut self, symbol: &Symbol, frame: u32) {
        for layer in sound_layers(symbol) {
            let Some(key) = layer.key_at(frame).filter(|key| key.frame == frame) else {
                continue;
            };
            let Some(sound) = &key.sound else { continue };
            match sound.sync {
                SoundSync::Event | SoundSync::Start => self.push(SoundCommand::Play {
                    resource: sound.resource.clone(),
                    sync: sound.sync,
                    loops: sound.loops.max(1),
                    loop_forever: sound.loop_forever,
                    envelope: envelope(sound),
                    in_point: sound.in_point,
                    out_point: sound.out_point,
                }),
                SoundSync::Stop => self.push(SoundCommand::Stop {
                    resource: sound.resource.clone(),
                }),
                SoundSync::Stream => {}
            }
        }
    }

    /// Reconcile stream channels with the placed timelines. `running(path)` says whether the
    /// clock driving that timeline advances.
    pub(crate) fn sync_streams<'a>(
        &mut self,
        project: &Project,
        nodes: impl Iterator<Item = &'a Node>,
        running: impl Fn(&[Id]) -> bool,
    ) {
        let fps = f64::from(project.fps.max(1));
        let mut alive = HashSet::new();
        for node in nodes {
            let Some(symbol) = node.symbol_id.as_ref().and_then(|id| project.symbol(id)) else {
                continue;
            };
            if !running(&node.path) {
                continue;
            }
            for layer in sound_layers(symbol) {
                let Some(key) = layer.key_at(node.frame) else {
                    continue;
                };
                let Some(sound) = key.sound.as_ref().filter(|s| s.sync == SoundSync::Stream) else {
                    continue;
                };
                if node.frame >= sound_span_end(layer, key, symbol.timeline.frame_count) {
                    continue;
                }
                let channel =
                    serde_json::to_string(&(&node.path, &layer.id, key.frame)).unwrap_or_default();
                let continuous = self
                    .streams
                    .get(&channel)
                    .is_some_and(|&last| node.frame == last || node.frame == last + 1);
                if !continuous {
                    if self.streams.contains_key(&channel) {
                        self.push(SoundCommand::StreamStop {
                            channel: channel.clone(),
                        });
                    }
                    self.push(SoundCommand::StreamStart {
                        channel: channel.clone(),
                        resource: sound.resource.clone(),
                        offset: f64::from(node.frame - key.frame) / fps,
                        loops: sound.loops.max(1),
                        loop_forever: sound.loop_forever,
                        envelope: envelope(sound),
                        in_point: sound.in_point,
                        out_point: sound.out_point,
                    });
                }
                self.streams.insert(channel.clone(), node.frame);
                alive.insert(channel);
            }
        }
        let mut ended: Vec<_> = self
            .streams
            .keys()
            .filter(|channel| !alive.contains(*channel))
            .cloned()
            .collect();
        ended.sort();
        for channel in ended {
            self.streams.remove(&channel);
            self.push(SoundCommand::StreamStop { channel });
        }
    }

    /// Stop the streams only (a seek restarts them at the new offset).
    pub(crate) fn stop_streams(&mut self) {
        let mut channels: Vec<_> = self.streams.drain().map(|(channel, _)| channel).collect();
        channels.sort();
        for channel in channels {
            self.push(SoundCommand::StreamStop { channel });
        }
    }

    /// Stop everything. Streams resume at their offset on the next synchronised frame.
    pub(crate) fn stop_all(&mut self) {
        self.streams.clear();
        self.push(SoundCommand::StopAll);
    }

    /// A parent player already emitted the one host-global StopAll command. Discard
    /// linked pending starts and reset their local stream bookkeeping so streams can
    /// synchronise again on the next advancing frame without following that StopAll.
    pub(crate) fn clear_for_global_stop(&mut self) {
        self.commands.clear();
        self.streams.clear();
        self.started = false;
    }

    pub(crate) fn has_started(&self) -> bool {
        self.started
    }

    /// Preserve commands drained from linked players that are about to be retired.
    pub(crate) fn extend(&mut self, commands: Vec<SoundCommand>) {
        if !commands.is_empty() {
            self.started = true;
            self.commands.extend(commands);
        }
    }

    /// Restart: forget pending commands; tell the host to silence what an earlier session started.
    pub(crate) fn reset(&mut self) {
        let started = self.started;
        *self = Self::default();
        if started {
            self.push(SoundCommand::StopAll);
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<SoundCommand> {
        std::mem::take(&mut self.commands)
    }
}

impl SoundCommand {
    /// Namespace a stream channel of a runtime-created (linked) clip's own timeline.
    pub(crate) fn scoped(self, scope: &str) -> Self {
        match self {
            Self::StreamStart {
                channel,
                resource,
                offset,
                loops,
                loop_forever,
                envelope,
                in_point,
                out_point,
            } => Self::StreamStart {
                channel: format!("{scope}:{channel}"),
                resource,
                offset,
                loops,
                loop_forever,
                envelope,
                in_point,
                out_point,
            },
            Self::StreamStop { channel } => Self::StreamStop {
                channel: format!("{scope}:{channel}"),
            },
            other => other,
        }
    }
}

/// Upper bound on requested waveform peak columns per resource.
pub const MAX_WAVEFORM_BUCKETS: usize = 16_384;

/// Amplitude envelope peaks of an audio resource sampled across evenly spaced time buckets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveformPeaks {
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: usize,
    pub duration: f64,
    /// `[min, max]` sample bounds (`-1.0..=1.0`) per bucket for channel 0 (mono / left).
    pub left: Vec<[f32; 2]>,
    /// `[min, max]` sample bounds (`-1.0..=1.0`) per bucket for channel 1 (right), or empty for mono.
    pub right: Vec<[f32; 2]>,
}

impl WaveformPeaks {
    pub fn buckets(&self) -> usize {
        self.left.len()
    }

    /// Peak absolute amplitude (`0.0..=1.0`) in `bucket` for `channel` (`0` = left, `1` = right or mono fallback).
    pub fn peak_amplitude(&self, channel: usize, bucket: usize) -> f32 {
        let lane = if channel > 0 && !self.right.is_empty() {
            &self.right
        } else {
            &self.left
        };
        lane.get(bucket)
            .map_or(0.0, |&[lo, hi]| lo.abs().max(hi.abs()).clamp(0.0, 1.0))
    }

    /// Bucket index range corresponding to optional `in_point` and `out_point` trim timestamps in seconds.
    pub fn slice_range(
        &self,
        in_point: Option<f64>,
        out_point: Option<f64>,
    ) -> std::ops::Range<usize> {
        let total = self.left.len();
        if total == 0 || self.duration <= 0.0 {
            return 0..0;
        }
        let start = in_point
            .map(|t| ((t.max(0.0) / self.duration) * total as f64).floor() as usize)
            .unwrap_or(0)
            .min(total);
        let end = out_point
            .map(|t| ((t.max(0.0) / self.duration) * total as f64).ceil() as usize)
            .unwrap_or(total)
            .clamp(start, total);
        start..end
    }
}

fn clamp_sample(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn sample_frames_into_peaks(
    frames: usize,
    channels: u16,
    sample_rate: u32,
    buckets: usize,
    mut read_frame: impl FnMut(usize) -> (f32, f32),
) -> Option<WaveformPeaks> {
    if frames == 0 || channels == 0 || sample_rate == 0 || buckets == 0 {
        return None;
    }
    let count = buckets.min(MAX_WAVEFORM_BUCKETS).min(frames);
    let stereo = channels > 1;
    let mut left = Vec::with_capacity(count);
    let mut right = if stereo {
        Vec::with_capacity(count)
    } else {
        Vec::new()
    };
    for b in 0..count {
        let start = ((b as u64) * (frames as u64) / (count as u64)) as usize;
        let end = ((((b + 1) as u64) * (frames as u64) / (count as u64)) as usize)
            .max(start + 1)
            .min(frames);
        let (mut min_l, mut max_l) = (1.0_f32, -1.0_f32);
        let (mut min_r, mut max_r) = (1.0_f32, -1.0_f32);
        for frame in start..end {
            let (l, r) = read_frame(frame);
            let l = clamp_sample(l);
            min_l = min_l.min(l);
            max_l = max_l.max(l);
            if stereo {
                let r = clamp_sample(r);
                min_r = min_r.min(r);
                max_r = max_r.max(r);
            }
        }
        left.push([min_l, max_l]);
        if stereo {
            right.push([min_r, max_r]);
        }
    }
    Some(WaveformPeaks {
        channels,
        sample_rate,
        frames,
        duration: frames as f64 / f64::from(sample_rate),
        left,
        right,
    })
}

/// Sample amplitude envelope peaks from interleaved `f32` PCM samples.
pub fn sample_waveform_peaks(
    samples: &[f32],
    channels: u16,
    sample_rate: u32,
    buckets: usize,
) -> Option<WaveformPeaks> {
    let ch = usize::from(channels);
    if ch == 0 {
        return None;
    }
    let frames = samples.len() / ch;
    sample_frames_into_peaks(frames, channels, sample_rate, buckets, |frame| {
        let idx = frame * ch;
        let l = samples[idx];
        let r = if ch > 1 { samples[idx + 1] } else { l };
        (l, r)
    })
}

/// Pure RIFF/WAVE amplitude peak decoder (8/16/24/32-bit PCM and 32/64-bit IEEE float).
pub fn wav_waveform_peaks(bytes: &[u8], buckets: usize) -> Result<WaveformPeaks, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("invalid RIFF/WAVE header".into());
    }
    if buckets == 0 {
        return Err("waveform bucket count must be at least 1".into());
    }
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data_slice: Option<&[u8]> = None;

    while pos.saturating_add(8) <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(len).min(bytes.len());
        let body = &bytes[body_start..body_end];
        if id == b"fmt " {
            if body.len() < 16 {
                return Err("truncated WAV fmt chunk".into());
            }
            let mut format_tag = u16::from_le_bytes([body[0], body[1]]);
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            if format_tag == 0xFFFE && body.len() >= 26 {
                format_tag = u16::from_le_bytes([body[24], body[25]]);
            }
            fmt = Some((format_tag, channels, sample_rate, bits));
        } else if id == b"data" {
            data_slice = Some(body);
            if fmt.is_some() {
                break;
            }
        }
        let padded = len.saturating_add(len & 1);
        let Some(next) = body_start.checked_add(padded) else {
            break;
        };
        pos = next;
    }

    let (format_tag, channels, sample_rate, bits) =
        fmt.ok_or_else(|| "WAV fmt chunk is missing".to_owned())?;
    let data = data_slice.ok_or_else(|| "WAV data chunk is missing".to_owned())?;
    if channels == 0 || sample_rate == 0 {
        return Err("WAV channels and sample rate must be non-zero".into());
    }
    let bytes_per_sample = match (format_tag, bits) {
        (1, 8) => 1usize,
        (1, 16) => 2,
        (1, 24) => 3,
        (1, 32) | (3, 32) => 4,
        (3, 64) => 8,
        _ => {
            return Err(format!(
                "unsupported WAV sample encoding (format {format_tag}, {bits}-bit)"
            ));
        }
    };
    let ch = usize::from(channels);
    let stride = ch
        .checked_mul(bytes_per_sample)
        .filter(|&s| s > 0)
        .ok_or_else(|| "invalid WAV frame stride".to_owned())?;
    let frames = data.len() / stride;
    if frames == 0 {
        return Err("WAV contains no audio frames".into());
    }

    let decode_one = |raw: &[u8]| -> f32 {
        match (format_tag, bits) {
            (1, 8) => (f32::from(raw[0]) - 128.0) / 128.0,
            (1, 16) => f32::from(i16::from_le_bytes([raw[0], raw[1]])) / 32_768.0,
            (1, 24) => {
                let signed =
                    i32::from(raw[0]) | (i32::from(raw[1]) << 8) | (i32::from(raw[2] as i8) << 16);
                signed as f32 / 8_388_608.0
            }
            (1, 32) => {
                (i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64 / 2_147_483_648.0)
                    as f32
            }
            (3, 32) => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            (3, 64) => f64::from_le_bytes([
                raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
            ]) as f32,
            _ => 0.0,
        }
    };

    sample_frames_into_peaks(frames, channels, sample_rate, buckets, |frame| {
        let base = frame * stride;
        let l = decode_one(&data[base..base + bytes_per_sample]);
        let r = if ch > 1 {
            decode_one(&data[base + bytes_per_sample..base + 2 * bytes_per_sample])
        } else {
            l
        };
        (l, r)
    })
    .ok_or_else(|| "WAV contains no decodable samples".into())
}

/// Bounded single-frame audio slice emitted during interactive timeline playhead scrubbing (`T-108`).
/// Pure evaluation (`evaluate.rs`) and onion skin never emit scrub slices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrubSoundSlice {
    pub resource: Id,
    /// Playback start offset in seconds within the audio resource (including `in_point`).
    pub start_seconds: f64,
    /// Duration of the scrubbed frame slice in seconds (`1.0 / fps`), clamped to `out_point`.
    pub duration_seconds: f64,
    /// Stereo gain `[left, right]` at the scrubbed frame offset.
    pub gains: [f32; 2],
}

/// Compute the bounded (`1.0 / fps` s) audio slices for active stream sounds (and keyframe-entry
/// event sounds) at `frame` when scrubbing the playhead interactively (`T-108`).
pub fn scrub_sound_slices(symbol: &Symbol, frame: u32, fps: u32) -> Vec<ScrubSoundSlice> {
    let fps_f = f64::from(fps.max(1));
    let frame_dur = 1.0 / fps_f;
    let mut slices = Vec::new();
    for layer in sound_layers(symbol) {
        let Some(key) = layer.key_at(frame) else {
            continue;
        };
        let Some(sound) = &key.sound else {
            continue;
        };
        let span_end = sound_span_end(layer, key, symbol.timeline.frame_count);
        if frame >= span_end {
            continue;
        }
        let is_stream = sound.sync == SoundSync::Stream;
        let is_entry_event =
            matches!(sound.sync, SoundSync::Event | SoundSync::Start) && key.frame == frame;
        if !is_stream && !is_entry_event {
            continue;
        }
        let rel_seconds = f64::from(frame.saturating_sub(key.frame)) / fps_f;
        let in_sec = sound.in_point.unwrap_or(0.0).max(0.0);
        let start_sec = in_sec + rel_seconds;
        if let Some(out_sec) = sound.out_point
            && start_sec >= out_sec
        {
            continue;
        }
        let dur = match sound.out_point {
            Some(out_sec) => (out_sec - start_sec).clamp(0.0, frame_dur),
            None => frame_dur,
        };
        if dur <= 1e-6 {
            continue;
        }
        let span_frames = span_end.saturating_sub(key.frame).max(1);
        let norm_pos =
            (f64::from(frame.saturating_sub(key.frame)) / f64::from(span_frames)).clamp(0.0, 1.0);
        let env = envelope(sound);
        let gains = sample_envelope_gains(&env, norm_pos);
        slices.push(ScrubSoundSlice {
            resource: sound.resource.clone(),
            start_seconds: start_sec,
            duration_seconds: dur,
            gains,
        });
    }
    slices
}

fn sample_envelope_gains(points: &[[f64; 3]], pos: f64) -> [f32; 2] {
    if points.is_empty() {
        return [1.0, 1.0];
    }
    if pos <= points[0][0] {
        return [points[0][1] as f32, points[0][2] as f32];
    }
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if pos <= b[0] {
            let span = (b[0] - a[0]).max(1e-9);
            let t = ((pos - a[0]) / span).clamp(0.0, 1.0);
            let l = a[1] + (b[1] - a[1]) * t;
            let r = a[2] + (b[2] - a[2]) * t;
            return [l as f32, r as f32];
        }
    }
    let last = points[points.len() - 1];
    [last[1] as f32, last[2] as f32]
}

/// Deterministic audio-to-viseme keyframe generator (`T-109` Auto Lip-Sync).
///
/// Maps each timeline frame in `start_frame .. start_frame + span_frames` to one of 6 viseme
/// mouth poses (`[neutral, mbp, fv_consonant, ei_narrow, ou_round, ah_open]`), returning
/// `(timeline_frame, graphic_single_frame_index)` pairs wherever the viseme changes.
pub fn compute_lip_sync_keyframes(
    peaks: &WaveformPeaks,
    in_point: Option<f64>,
    out_point: Option<f64>,
    start_frame: u32,
    span_frames: u32,
    fps: u32,
    viseme_frames: &[u32; 6],
) -> Vec<(u32, u32)> {
    if span_frames == 0 || peaks.buckets() == 0 || peaks.duration <= 0.0 {
        return Vec::new();
    }
    let fps_f = f64::from(fps.max(1));
    let in_sec = in_point.unwrap_or(0.0).max(0.0);
    let out_sec = out_point.unwrap_or(peaks.duration).min(peaks.duration);
    let total_buckets = peaks.buckets();

    let mut result: Vec<(u32, u32)> = Vec::new();
    let mut last_viseme: Option<u32> = None;

    for offset in 0..span_frames {
        let frame = start_frame + offset;
        let t0 = in_sec + f64::from(offset) / fps_f;
        let t1 = (in_sec + f64::from(offset + 1) / fps_f).min(out_sec);
        let energy = if t0 >= out_sec || t1 <= t0 {
            0.0_f32
        } else {
            let b0 = ((t0 / peaks.duration) * total_buckets as f64).floor() as usize;
            let b1 = ((t1 / peaks.duration) * total_buckets as f64)
                .ceil()
                .max((b0 + 1) as f64) as usize;
            let b0 = b0.min(total_buckets.saturating_sub(1));
            let b1 = b1.clamp(b0 + 1, total_buckets);
            let mut sum = 0.0_f32;
            let mut max_amp = 0.0_f32;
            for b in b0..b1 {
                let amp_l = peaks.peak_amplitude(0, b);
                let amp_r = peaks.peak_amplitude(1, b);
                let amp = amp_l.max(amp_r);
                sum += amp;
                max_amp = max_amp.max(amp);
            }
            let mean = sum / ((b1 - b0) as f32);
            0.6 * max_amp + 0.4 * mean
        };

        let viseme_idx = if energy < 0.04 {
            0 // Neutral / closed mouth
        } else if energy < 0.16 {
            1 // M, B, P
        } else if energy < 0.32 {
            2 // F, V, D, T, S
        } else if energy < 0.52 {
            3 // E, I
        } else if energy < 0.72 {
            4 // O, U, W
        } else {
            5 // A, AH wide open
        };

        let target_pose = viseme_frames[viseme_idx];
        if last_viseme != Some(target_pose) {
            result.push((frame, target_pose));
            last_viseme = Some(target_pose);
        }
    }

    result
}
