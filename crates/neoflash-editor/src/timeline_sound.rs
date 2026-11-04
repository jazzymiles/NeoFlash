//! Timeline (frame) sounds in the editor: attaching library audio to keyframes (Timeline
//! controls ▸ Sound, Properties ▸ Sound) and the native preview of editor playback.
//!
//! What starts, stops or seeks is decided by the shared `Player` (`SoundCommand`, the same path
//! as HTML5). This module only decodes each resource once (rodio / Symphonia: MP3, WAV) and mixes
//! voices to the default Core Audio output. Scrubbing is silent; Test Movie plays in the browser.
use crate::app::Editor;
use neoflash_core::*;
use rodio::Source as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Longest decoded preview of one resource: ten minutes of 48 kHz stereo.
const MAX_DECODED_SAMPLES: usize = 48_000 * 2 * 600;
/// Commands kept for diagnostics and tests.
const HISTORY: usize = 256;
/// Channel of Properties ▸ Sound ▸ Play.
const AUDITION: &str = "properties-play";

/// Decoded PCM of one resource, interleaved.
pub(crate) struct DecodedSound {
    samples: Arc<[f32]>,
    channels: u16,
    rate: u32,
}

impl DecodedSound {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
    pub(crate) fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }
    pub(crate) fn channels(&self) -> u16 {
        self.channels
    }
}

pub(crate) fn decode_unit(resource: &ResourceUnit) -> Result<DecodedSound, String> {
    decode(resource)
}

fn decode(resource: &ResourceUnit) -> Result<DecodedSound, String> {
    let bytes = resource
        .decoded_bytes()
        .map_err(|error| error.to_string())?;
    let decoder = rodio::Decoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("{}: {error}", resource.path))?;
    let (channels, rate) = (decoder.channels(), decoder.sample_rate());
    let samples: Vec<f32> = decoder.take(MAX_DECODED_SAMPLES).collect();
    if samples.is_empty() || channels == 0 || rate == 0 {
        return Err(format!("{}: no decodable audio", resource.path));
    }
    Ok(DecodedSound {
        samples: samples.into(),
        channels,
        rate,
    })
}

/// One playing sound: stereo output with the command's trim window, loop count and volume envelope.
pub(crate) struct VoiceSource {
    sound: Arc<DecodedSound>,
    in_frame: usize,
    seg_frames: usize,
    frame: usize,
    end: Option<usize>,
    envelope: Vec<[f64; 3]>,
    envelope_frames: f64,
    right: Option<f32>,
}

impl VoiceSource {
    pub(crate) fn new(
        sound: Arc<DecodedSound>,
        offset: f64,
        loops: u32,
        loop_forever: bool,
        (in_point, out_point): (Option<f64>, Option<f64>),
        envelope: Vec<[f64; 3]>,
    ) -> Option<Self> {
        let total_frames = sound.frames();
        if total_frames == 0 {
            return None;
        }
        let rate = f64::from(sound.rate);
        let in_frame = in_point
            .map(|t| (t.max(0.0) * rate).round() as usize)
            .unwrap_or(0)
            .min(total_frames);
        let out_frame = out_point
            .map(|t| (t.max(0.0) * rate).round() as usize)
            .unwrap_or(total_frames)
            .min(total_frames);
        let seg_frames = out_frame.saturating_sub(in_frame);
        let end = (!loop_forever).then(|| seg_frames.saturating_mul(loops.max(1) as usize));
        let frame = (offset.max(0.) * rate) as usize;
        if seg_frames == 0 || end.is_some_and(|end| frame >= end) {
            return None;
        }
        let flat = envelope.iter().all(|p| p[1] == 1.0 && p[2] == 1.0);
        Some(Self {
            envelope_frames: end.unwrap_or(seg_frames) as f64,
            envelope: if flat { vec![] } else { envelope },
            sound,
            in_frame,
            seg_frames,
            frame,
            end,
            right: None,
        })
    }
}

impl Iterator for VoiceSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.end.is_some_and(|end| self.frame >= end) {
            return None;
        }
        let channels = usize::from(self.sound.channels);
        let index = (self.in_frame + (self.frame % self.seg_frames)) * channels;
        let left = self.sound.samples[index];
        let right = if channels > 1 {
            self.sound.samples[index + 1]
        } else {
            left
        };
        let (gain_left, gain_right) = if self.envelope.is_empty() {
            (1.0, 1.0)
        } else {
            envelope_gains(&self.envelope, self.frame as f64 / self.envelope_frames)
        };
        self.frame += 1;
        self.right = Some(right * gain_right as f32);
        Some(left * gain_left as f32)
    }
}

impl rodio::Source for VoiceSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.sound.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Voice {
    sink: rodio::Sink,
    resource: Id,
    /// Stream channel (or the Properties audition); `None` for Event/Start sounds.
    channel: Option<String>,
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.sink.stop();
    }
}

type DecodedResult = Result<Arc<DecodedSound>, String>;
type DecodedCache = HashMap<Id, (Arc<str>, DecodedResult)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WaveformResolution {
    Buckets(usize),
    /// The exact timeline zoom (screen points per second), before the duration is known.
    Zoom(u64),
}

type WaveformKey = (Id, WaveformResolution);
type PeakResult = Result<Arc<WaveformPeaks>, String>;

struct CachedWaveform {
    /// Keep the allocation alive: a naked address can be reused after Replace/Undo.
    source: Arc<str>,
    result: Option<PeakResult>,
}

type WaveformCache = HashMap<WaveformKey, CachedWaveform>;

struct WaveformJob {
    unit: ResourceUnit,
    resolution: WaveformResolution,
    decoded: Option<DecodedResult>,
    ctx: eframe::egui::Context,
}

struct WaveformReady {
    key: WaveformKey,
    source: Arc<str>,
    decoded: DecodedResult,
    peaks: PeakResult,
}

/// One lazy worker serializes decoding across zooms. Dropping the editor closes its channel;
/// it never joins a decoding thread on the UI thread, and late results cannot mutate a document.
struct WaveformWorker {
    requests: mpsc::Sender<WaveformJob>,
    results: mpsc::Receiver<WaveformReady>,
}

impl WaveformWorker {
    fn new() -> Result<Self, String> {
        let (requests, jobs) = mpsc::channel::<WaveformJob>();
        let (results_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("neoflash-waveform".into())
            .spawn(move || {
                let mut decoded = DecodedCache::new();
                while let Ok(job) = jobs.recv() {
                    let source = job.unit.data_base64.clone();
                    let sound = match decoded.get(&job.unit.id) {
                        Some((cached, result)) if Arc::ptr_eq(cached, &source) => result.clone(),
                        _ => {
                            let result = job
                                .decoded
                                .unwrap_or_else(|| decode(&job.unit).map(Arc::new));
                            decoded.insert(job.unit.id.clone(), (source.clone(), result.clone()));
                            result
                        }
                    };
                    let peaks = sound.as_ref().map_err(Clone::clone).and_then(|sound| {
                        let buckets = match job.resolution {
                            WaveformResolution::Buckets(buckets) => buckets,
                            WaveformResolution::Zoom(zoom) => {
                                (sound.duration() * f64::from_bits(zoom)).ceil() as usize
                            }
                        }
                        .clamp(1, MAX_WAVEFORM_BUCKETS);
                        sample_waveform_peaks(&sound.samples, sound.channels, sound.rate, buckets)
                            .map(Arc::new)
                            .ok_or_else(|| format!("{}: no waveform samples", job.unit.path))
                    });
                    if results_tx
                        .send(WaveformReady {
                            key: (job.unit.id, job.resolution),
                            source,
                            decoded: sound,
                            peaks,
                        })
                        .is_err()
                    {
                        break;
                    }
                    job.ctx.request_repaint();
                }
            })
            .map_err(|error| format!("Cannot start audio waveform worker: {error}"))?;
        Ok(Self { requests, results })
    }
}

/// Re-express cached source peaks at the visible span columns. Only peak lookup/remapping happens
/// here; PCM decoding and peak reduction are always on the worker. Time starts at the span key,
/// not the scrolled viewport, and repeats respect the same trim/loop window as VoiceSource.
pub(crate) fn visible_waveform_columns(
    peaks: &WaveformPeaks,
    sound: &FrameSound,
    seconds_per_column: f64,
    offset: f64,
    columns: usize,
) -> Vec<Vec<(f32, f32)>> {
    let start = sound.in_point.unwrap_or(0.).clamp(0., peaks.duration);
    let end = sound
        .out_point
        .unwrap_or(peaks.duration)
        .clamp(start, peaks.duration);
    let length = end - start;
    let total = if sound.loop_forever {
        f64::INFINITY
    } else {
        length * f64::from(sound.loops.max(1))
    };
    let source_bucket = peaks.duration / peaks.buckets().max(1) as f64;
    let sample_range = |lane: &[[f32; 2]], from: f64, to: f64| {
        let a = ((from / source_bucket).floor() as usize).min(lane.len());
        let b = ((to / source_bucket).ceil() as usize).min(lane.len());
        lane[a..b]
            .iter()
            .fold((0.0_f32, 0.0_f32), |(lo, hi), &[a, b]| {
                (lo.min(a), hi.max(b))
            })
    };
    let lane_columns = |lane: &[[f32; 2]]| {
        (0..columns)
            .map(|column| {
                let time = offset + column as f64 * seconds_per_column;
                if sound.sync == SoundSync::Stop
                    || length <= 0.
                    || time >= total
                    || seconds_per_column <= 0.
                {
                    return (0., 0.);
                }
                let width = seconds_per_column.min(total - time);
                // A column covering a whole loop sees every source peak, without iterating 10,000 loops.
                if width >= length {
                    return sample_range(lane, start, end);
                }
                let phase = time.rem_euclid(length);
                let first_end = (phase + width).min(length);
                let (mut lo, mut hi) = sample_range(lane, start + phase, start + first_end);
                if phase + width > length {
                    let (a, b) = sample_range(lane, start, start + phase + width - length);
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
                (lo, hi)
            })
            .collect()
    };
    let mut lanes = vec![lane_columns(&peaks.left)];
    if !peaks.right.is_empty() {
        lanes.push(lane_columns(&peaks.right));
    }
    lanes
}

/// Editor audio: the output device (opened on first use), decoded resources and voices.
pub(crate) struct TimelineAudio {
    /// Control ▸ Mute sounds.
    pub(crate) muted: bool,
    native: bool,
    output: Option<rodio::OutputStream>,
    failure: Option<String>,
    decoded: DecodedCache,
    waveforms: WaveformCache,
    waveform_worker: Option<WaveformWorker>,
    voices: Vec<Voice>,
    preview: Option<u64>,
    auditioning_headless: bool,
    /// Local import paths of audio resources for Sound properties ▸ Update (session-only).
    pub(crate) source_paths: HashMap<Id, std::path::PathBuf>,
    /// Recent commands of editor playback (diagnostics and tests).
    pub(crate) history: Vec<SoundCommand>,
}

impl Default for TimelineAudio {
    fn default() -> Self {
        Self {
            muted: false,
            // Tests and screenshot captures never open an audio device.
            native: !cfg!(test) && std::env::var_os("NEOFLASH_SCREENSHOT").is_none(),
            output: None,
            failure: None,
            decoded: HashMap::new(),
            waveforms: HashMap::new(),
            waveform_worker: None,
            voices: vec![],
            preview: None,
            auditioning_headless: false,
            source_paths: HashMap::new(),
            history: vec![],
        }
    }
}

impl TimelineAudio {
    /// Decoded audio of `resource`, cached until its bytes change.
    pub(crate) fn decoded(
        &mut self,
        project: &Project,
        resource: &str,
    ) -> Result<Arc<DecodedSound>, String> {
        let unit = project
            .resources
            .iter()
            .find(|unit| unit.id == resource)
            .ok_or_else(|| format!("sound resource {resource} is missing"))?;
        self.collect_waveforms(project);
        if let Some((cached, result)) = self.decoded.get(resource)
            && Arc::ptr_eq(cached, &unit.data_base64)
        {
            return result.clone();
        }
        let result = decode(unit).map(Arc::new);
        self.decoded.insert(
            resource.to_owned(),
            (unit.data_base64.clone(), result.clone()),
        );
        result
    }

    fn collect_waveforms(&mut self, project: &Project) {
        let Some(worker) = &self.waveform_worker else {
            return;
        };
        while let Ok(ready) = worker.results.try_recv() {
            if !project
                .resources
                .iter()
                .any(|unit| unit.id == ready.key.0 && Arc::ptr_eq(&unit.data_base64, &ready.source))
            {
                continue;
            }
            self.decoded
                .insert(ready.key.0.clone(), (ready.source.clone(), ready.decoded));
            if let Some(cached) = self.waveforms.get_mut(&ready.key)
                && Arc::ptr_eq(&cached.source, &ready.source)
            {
                cached.result = Some(ready.peaks);
            }
        }
    }

    fn request_waveform(
        &mut self,
        project: &Project,
        resource: &str,
        resolution: WaveformResolution,
        ctx: &eframe::egui::Context,
    ) -> Result<Option<Arc<WaveformPeaks>>, String> {
        self.collect_waveforms(project);
        let unit = project
            .resources
            .iter()
            .find(|unit| unit.id == resource)
            .ok_or_else(|| format!("sound resource {resource} is missing"))?;
        let key = (resource.to_owned(), resolution);
        if let Some(cached) = self.waveforms.get(&key)
            && Arc::ptr_eq(&cached.source, &unit.data_base64)
        {
            return cached.result.clone().transpose();
        }
        // Replace invalidates every zoom. Bound zoom variants retained during continuous zooming.
        self.waveforms.retain(|(id, _), cached| {
            id != resource || Arc::ptr_eq(&cached.source, &unit.data_base64)
        });
        if self
            .waveforms
            .keys()
            .filter(|(id, _)| id == resource)
            .count()
            >= 8
        {
            self.waveforms
                .retain(|(id, _), cached| id != resource || cached.result.is_none());
        }
        if self.waveform_worker.is_none() {
            self.waveform_worker = Some(WaveformWorker::new()?);
        }
        let decoded = self
            .decoded
            .get(resource)
            .filter(|(source, _)| Arc::ptr_eq(source, &unit.data_base64))
            .map(|(_, sound)| sound.clone());
        self.waveform_worker
            .as_ref()
            .unwrap()
            .requests
            .send(WaveformJob {
                unit: unit.clone(),
                resolution,
                decoded,
                ctx: ctx.clone(),
            })
            .map_err(|_| "Audio waveform worker stopped".to_owned())?;
        self.waveforms.insert(
            key,
            CachedWaveform {
                source: unit.data_base64.clone(),
                result: None,
            },
        );
        Ok(None)
    }

    /// Nonblocking peaks for an overview (Properties) or a test. None means still queued/decoding;
    /// the worker requests a repaint once ready, and errors are cached like successful results.
    pub(crate) fn waveform_peaks(
        &mut self,
        project: &Project,
        resource: &str,
        buckets: usize,
        ctx: &eframe::egui::Context,
    ) -> Result<Option<Arc<WaveformPeaks>>, String> {
        self.request_waveform(
            project,
            resource,
            WaveformResolution::Buckets(buckets.clamp(1, MAX_WAVEFORM_BUCKETS)),
            ctx,
        )
    }

    /// Cached decoded metadata for passive UI inspection. Waveform requests populate this cache;
    /// unlike explicit Play/Test, inspecting or scrubbing a sound never decodes on the UI thread.
    pub(crate) fn decoded_if_ready(
        &mut self,
        project: &Project,
        resource: &str,
    ) -> Option<DecodedResult> {
        self.collect_waveforms(project);
        let unit = project.resources.iter().find(|unit| unit.id == resource)?;
        self.decoded
            .get(resource)
            .filter(|(source, _)| Arc::ptr_eq(source, &unit.data_base64))
            .map(|(_, result)| result.clone())
    }

    /// Draw the visible columns only. Sound's decorative gallery waveform is deliberately omitted:
    /// Static is the same NF2 span background, and the approved decoded-peaks painter supplies ink.
    pub(crate) fn paint_waveform(
        &mut self,
        project: &Project,
        sound: &FrameSound,
        painter: &eframe::egui::Painter,
        rect: eframe::egui::Rect,
        points_per_second: f64,
    ) {
        let visible = rect.intersect(painter.clip_rect());
        if visible.width() <= 0. || points_per_second <= 0. {
            return;
        }
        let ctx = painter.ctx();
        let peaks = if sound.sync == SoundSync::Stop {
            None
        } else {
            self.request_waveform(
                project,
                &sound.resource,
                WaveformResolution::Zoom(points_per_second.to_bits()),
                ctx,
            )
            .ok()
            .flatten()
        };
        let lanes = peaks.map_or_else(Vec::new, |peaks| {
            visible_waveform_columns(
                &peaks,
                sound,
                1. / points_per_second,
                f64::from(visible.left() - rect.left()) / points_per_second,
                visible.width().ceil() as usize,
            )
        });
        let channels: Vec<_> = lanes.iter().map(Vec::as_slice).collect();
        let visible = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(visible.left(), rect.top()),
            eframe::egui::pos2(visible.right(), rect.bottom()),
        );
        neoflash_ui::components::timeline::paint_sound_peaks(painter, ctx, visible, &channels);
    }

    fn mixer(&mut self) -> Option<&rodio::mixer::Mixer> {
        if !self.native || self.failure.is_some() {
            return None;
        }
        if self.output.is_none() {
            match rodio::OutputStreamBuilder::open_default_stream() {
                Ok(mut stream) => {
                    stream.log_on_drop(false);
                    self.output = Some(stream);
                }
                Err(error) => {
                    self.failure = Some(format!("No audio output: {error}"));
                    return None;
                }
            }
        }
        self.output.as_ref().map(|stream| stream.mixer())
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        project: &Project,
        resource: Id,
        channel: Option<String>,
        offset: f64,
        (loops, loop_forever): (u32, bool),
        trim: (Option<f64>, Option<f64>),
        envelope: Vec<[f64; 3]>,
    ) {
        if self.muted || !self.native {
            return;
        }
        let sound = match self.decoded(project, &resource) {
            Ok(sound) => sound,
            Err(error) => {
                self.failure.get_or_insert(error);
                return;
            }
        };
        let Some(source) = VoiceSource::new(sound, offset, loops, loop_forever, trim, envelope)
        else {
            return;
        };
        let Some(mixer) = self.mixer() else { return };
        let sink = rodio::Sink::connect_new(mixer);
        sink.append(source);
        self.voices.push(Voice {
            sink,
            resource,
            channel,
        });
    }

    /// Apply the playback's sound commands in order.
    pub(crate) fn apply(&mut self, project: &Project, commands: Vec<SoundCommand>) {
        self.voices.retain(|voice| !voice.sink.empty());
        for command in commands {
            self.history.push(command.clone());
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
            match command {
                SoundCommand::StopAll => self
                    .voices
                    .retain(|v| v.channel.as_deref() == Some(AUDITION)),
                SoundCommand::Stop { resource } => self
                    .voices
                    .retain(|v| v.resource != resource || v.channel.is_some()),
                SoundCommand::StreamStop { channel } => {
                    self.voices.retain(|v| v.channel.as_ref() != Some(&channel))
                }
                SoundCommand::Play {
                    resource,
                    sync,
                    loops,
                    loop_forever,
                    envelope,
                    in_point,
                    out_point,
                } => {
                    if sync == SoundSync::Start
                        && self.voices.iter().any(|v| v.resource == resource)
                    {
                        continue;
                    }
                    self.start(
                        project,
                        resource,
                        None,
                        0.,
                        (loops, loop_forever),
                        (in_point, out_point),
                        envelope,
                    );
                }
                SoundCommand::StreamStart {
                    channel,
                    resource,
                    offset,
                    loops,
                    loop_forever,
                    envelope,
                    in_point,
                    out_point,
                } => {
                    self.voices.retain(|v| v.channel.as_ref() != Some(&channel));
                    let channel = Some(channel);
                    self.start(
                        project,
                        resource,
                        channel,
                        offset,
                        (loops, loop_forever),
                        (in_point, out_point),
                        envelope,
                    );
                }
            }
        }
    }

    /// A new (or no) playback session silences the previous session's sounds.
    fn follow_preview(&mut self, epoch: Option<u64>) {
        if self.preview != epoch {
            self.voices
                .retain(|voice| voice.channel.as_deref() == Some(AUDITION));
            self.preview = epoch;
        }
    }

    /// Properties ▸ Sound ▸ Play / Edit envelope ▸ Play: hear the sound once with its envelope.
    pub(crate) fn audition(&mut self, project: &Project, sound: &FrameSound) {
        self.voices
            .retain(|v| v.channel.as_deref() != Some(AUDITION));
        let envelope = sound.envelope_points();
        self.start(
            project,
            sound.resource.clone(),
            Some(AUDITION.into()),
            0.,
            (1, false),
            (sound.in_point, sound.out_point),
            envelope,
        );
    }

    /// Sound properties ▸ Test: audition a draft resource unit directly without mutating the document.
    pub(crate) fn audition_unit(&mut self, unit: &ResourceUnit) -> Result<(), String> {
        self.stop_audition();
        let sound = Arc::new(decode(unit)?);
        if self.muted {
            return Ok(());
        }
        if !self.native {
            self.auditioning_headless = true;
            return Ok(());
        }
        let Some(source) = VoiceSource::new(sound, 0., 1, false, (None, None), vec![]) else {
            return Ok(());
        };
        let Some(mixer) = self.mixer() else {
            return Ok(());
        };
        let sink = rodio::Sink::connect_new(mixer);
        sink.append(source);
        self.voices.push(Voice {
            sink,
            resource: unit.id.clone(),
            channel: Some(AUDITION.into()),
        });
        Ok(())
    }

    pub(crate) fn stop_audition(&mut self) {
        self.auditioning_headless = false;
        self.voices
            .retain(|v| v.channel.as_deref() != Some(AUDITION));
    }

    pub(crate) fn is_auditioning(&mut self) -> bool {
        self.voices.retain(|voice| !voice.sink.empty());
        self.auditioning_headless
            || self
                .voices
                .iter()
                .any(|v| v.channel.as_deref() == Some(AUDITION))
    }
    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted {
            self.auditioning_headless = false;
            self.voices.clear();
        }
    }
}

impl Editor {
    /// Audio resources of the document as `(id, path)`, in library order.
    pub(crate) fn audio_resources(&self) -> Vec<(Id, String)> {
        self.project
            .resources
            .iter()
            .filter(|unit| unit.is_audio())
            .map(|unit| (unit.id.clone(), unit.path.clone()))
            .collect()
    }

    /// The sound of the keyframe that starts the span under the playhead on the current layer.
    pub(crate) fn frame_sound(&self) -> Option<(u32, FrameSound)> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        Some((key.frame, key.sound.clone()?))
    }

    /// Attach `resource` to the current keyframe (default sync Event; an existing sound keeps its
    /// sync, repeat and effect), or remove the sound with `None`. Creates a blank keyframe when the
    /// layer has none at or before the playhead.
    fn attach_frame_sound(&mut self, resource: Option<Id>) {
        let (layer, frame) = (self.layer, self.frame);
        let has_key = self.symbol().timeline.layers[layer].key_at(frame).is_some();
        if !has_key && resource.is_some() {
            self.ensure_key(true);
        }
        if let Some(key) = self.symbol_mut().timeline.layers[layer].key_at_mut(frame) {
            key.sound = resource.map(|id| match key.sound.take() {
                Some(sound) => FrameSound {
                    resource: id,
                    ..sound
                },
                None => FrameSound::new(id),
            });
        }
    }

    /// Timeline controls ▸ Sound ▸ <library sound>, Properties ▸ Sound select. One Undo step.
    pub(crate) fn set_frame_sound(&mut self, resource: Option<Id>) {
        self.finish_edits();
        if !self.layer_editable() {
            self.message("Select an unlocked, visible layer first.");
            return;
        }
        let attached = resource.is_some();
        self.transact(|editor| editor.attach_frame_sound(resource));
        if attached {
            self.show_frame_sound();
        }
    }

    /// Select the sound's keyframe so Properties shows its Sound section.
    fn show_frame_sound(&mut self) {
        if let Some((start, _)) = self.frame_sound() {
            let layer = self.symbol().timeline.layers[self.layer].id.clone();
            self.select_frame_range(FrameRange {
                layers: vec![layer],
                start,
                end: start,
            });
            self.show_panel(crate::workspace::PanelId::Properties);
        }
    }

    /// Properties ▸ Sound: change sync, repeat or effect of the current keyframe's sound.
    pub(crate) fn edit_frame_sound(&mut self, edit: impl FnOnce(&mut FrameSound)) {
        self.finish_edits();
        if !self.layer_editable() || self.frame_sound().is_none() {
            return;
        }
        let (layer, frame) = (self.layer, self.frame);
        self.transact(|editor| {
            if let Some(sound) = editor.symbol_mut().timeline.layers[layer]
                .key_at_mut(frame)
                .and_then(|key| key.sound.as_mut())
            {
                edit(sound);
                sound.loops = sound.loops.clamp(1, FrameSound::MAX_LOOPS);
            }
        });
    }

    /// Set or clear Time In / Time Out playback trim points (seconds) on the current keyframe's
    /// sound in a single Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_frame_sound_trim(&mut self, in_point: Option<f64>, out_point: Option<f64>) {
        self.finish_edits();
        if !self.layer_editable() || self.frame_sound().is_none() {
            return;
        }
        let clamp_time = |value: Option<f64>| {
            value
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS))
        };
        let in_point = clamp_time(in_point).filter(|&v| v > 0.0);
        let out_point = clamp_time(out_point);
        if let (Some(inp), Some(outp)) = (in_point, out_point)
            && outp <= inp
        {
            return;
        }
        if let Some(0.0) = out_point {
            return;
        }
        let (layer, frame) = (self.layer, self.frame);
        self.transact_named("Trim sound", |editor| {
            if let Some(sound) = editor.symbol_mut().timeline.layers[layer]
                .key_at_mut(frame)
                .and_then(|key| key.sound.as_mut())
            {
                sound.in_point = in_point;
                sound.out_point = out_point;
            }
        });
    }

    /// Timeline controls ▸ Sound ▸ Import sound…: add the file as a resource and attach it to
    /// the current keyframe in one Undo step.
    pub(crate) fn import_frame_sound(&mut self, path: &Path) -> Result<(), String> {
        self.finish_edits();
        if !self.layer_editable() {
            return Err("Select an unlocked, visible layer first.".into());
        }
        let used: usize = self
            .project
            .resources
            .iter()
            .map(ResourceUnit::byte_len)
            .sum();
        let unit =
            crate::resources::read_resource(path, MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used))?;
        if !unit.is_audio() {
            return Err(format!(
                "{} is not a sound file. Import MP3 or WAV audio.",
                unit.path
            ));
        }
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        candidate.resources.push(unit.clone());
        candidate.validate().map_err(|error| error.to_string())?;
        let id = unit.id.clone();
        self.transact(|editor| {
            editor.project.format_version = editor.project.format_version.max(3);
            editor.project.resources.push(unit);
            editor.attach_frame_sound(Some(id.clone()));
        });
        self.timeline_audio
            .source_paths
            .insert(id, path.to_path_buf());
        self.show_frame_sound();
        Ok(())
    }

    pub(crate) fn import_frame_sound_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Sound", &["mp3", "wav"])
            .pick_file()
        else {
            return;
        };
        match self.import_frame_sound(&path) {
            Ok(()) => self.message("Sound imported and attached to the keyframe."),
            Err(error) => self.message(format!("Sound import failed: {error}")),
        }
    }

    /// Feed editor playback's sound commands to the native audio preview (once per UI frame).
    pub(crate) fn pump_timeline_audio(&mut self) {
        let epoch = self.preview_epoch();
        let commands = self.drain_preview_sounds();
        self.timeline_audio.follow_preview(epoch);
        self.timeline_audio.apply(&self.project, commands);
        if let Some(failure) = self.timeline_audio.failure.take() {
            self.message(format!("Timeline sound preview: {failure}"));
            // Keep a missing device sticky so playback does not retry every frame.
            if failure.starts_with("No audio output") {
                self.timeline_audio.failure = Some(failure);
                self.timeline_audio.native = false;
            }
        }
    }

    /// Control ▸ Mute sounds (⌥⌘M).
    pub(crate) fn toggle_mute_sounds(&mut self) {
        let muted = !self.timeline_audio.muted;
        self.timeline_audio.set_muted(muted);
        self.message(if muted { "Sounds muted." } else { "Sounds on." });
    }
}
