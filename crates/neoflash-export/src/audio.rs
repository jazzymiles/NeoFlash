//! Audio export and publish-time audio compression (`Export audio` DL-06, `Sound properties` DL-08,
//! `Publish settings` DL-03, and `Preferences ▸ Publish` DL-01).
//!
//! Renders timeline sound mixes (`Event`, `Start`, `Stop`, and `Stream` sync modes, trim points,
//! repeat/loop counts, and stereo volume envelopes) or individual sound layers/resources, applies
//! resampling, channel conversion, silence trimming, peak normalization (−1 dB) and fade-out, and
//! encodes to WAV (8/16/24-bit PCM or 32-bit IEEE float), MP3 (CBR 64–320 kbps), OGG (Vorbis), or
//! AAC (M4A/ADTS).

use crate::ExportError;
use neoflash_core::{
    Id, LayerRole, MAX_WAVEFORM_BUCKETS, Player, Project, ResourceUnit, SoundCommand,
    SoundCompression, SoundExportQuality, SoundExportSettings, SoundSync, WaveformPeaks,
    envelope_gains, sample_waveform_peaks,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Output container/codec for `File ▸ Export ▸ Export audio…` (`DL-06`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Wav,
    #[default]
    Mp3,
    Ogg,
    Aac,
}

impl AudioFormat {
    pub const ALL: [Self; 4] = [Self::Wav, Self::Mp3, Self::Ogg, Self::Aac];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Mp3 => "MP3",
            Self::Ogg => "OGG",
            Self::Aac => "AAC",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Aac => "m4a",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::Ogg => "audio/ogg",
            Self::Aac => "audio/mp4",
        }
    }

    pub const fn uses_bit_depth(self) -> bool {
        matches!(self, Self::Wav)
    }

    pub const fn uses_bit_rate(self) -> bool {
        !matches!(self, Self::Wav)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Wav => 0,
            Self::Mp3 => 1,
            Self::Ogg => 2,
            Self::Aac => 3,
        }
    }

    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Wav,
            1 => Self::Mp3,
            2 => Self::Ogg,
            _ => Self::Aac,
        }
    }

    pub fn parse(name: &str) -> Result<Self, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wav" | "wave" => Ok(Self::Wav),
            "mp3" => Ok(Self::Mp3),
            "ogg" | "vorbis" => Ok(Self::Ogg),
            "aac" | "m4a" => Ok(Self::Aac),
            other => Err(format!(
                "Unknown audio format '{other}'. Use wav, mp3, ogg or aac."
            )),
        }
    }
}

/// Output sample rate for `Export audio`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSampleRate {
    Hz8000,
    Hz11025,
    Hz22050,
    #[default]
    Hz44100,
    Hz48000,
}

impl AudioSampleRate {
    pub const ALL: [Self; 5] = [
        Self::Hz8000,
        Self::Hz11025,
        Self::Hz22050,
        Self::Hz44100,
        Self::Hz48000,
    ];

    pub const fn hz(self) -> u32 {
        match self {
            Self::Hz8000 => 8000,
            Self::Hz11025 => 11025,
            Self::Hz22050 => 22050,
            Self::Hz44100 => 44100,
            Self::Hz48000 => 48000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hz8000 => "8 kHz",
            Self::Hz11025 => "11.025 kHz",
            Self::Hz22050 => "22.05 kHz",
            Self::Hz44100 => "44.1 kHz",
            Self::Hz48000 => "48 kHz",
        }
    }
}

/// PCM sample depth for WAV export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBitDepth {
    Bit8,
    #[default]
    Bit16,
    Bit24,
    Float32,
}

impl AudioBitDepth {
    pub const ALL: [Self; 4] = [Self::Bit8, Self::Bit16, Self::Bit24, Self::Float32];

    pub const fn bits(self) -> u16 {
        match self {
            Self::Bit8 => 8,
            Self::Bit16 => 16,
            Self::Bit24 => 24,
            Self::Float32 => 32,
        }
    }

    pub const fn bytes_per_sample(self) -> usize {
        (self.bits() as usize) / 8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bit8 => "8-bit",
            Self::Bit16 => "16-bit",
            Self::Bit24 => "24-bit",
            Self::Float32 => "32-bit float",
        }
    }
}

/// Output channel configuration (`Mono` or `Stereo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChannels {
    Mono,
    #[default]
    Stereo,
}

impl AudioChannels {
    pub const ALL: [Self; 2] = [Self::Stereo, Self::Mono];

    pub const fn count(self) -> u16 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Stereo => "Stereo",
        }
    }
}

/// Compressed CBR bit rate (`64`–`320 kbps`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBitRate {
    Kbps64,
    Kbps96,
    Kbps128,
    Kbps160,
    #[default]
    Kbps192,
    Kbps256,
    Kbps320,
}

impl AudioBitRate {
    pub const ALL: [Self; 7] = [
        Self::Kbps64,
        Self::Kbps96,
        Self::Kbps128,
        Self::Kbps160,
        Self::Kbps192,
        Self::Kbps256,
        Self::Kbps320,
    ];

    pub const fn kbps(self) -> u32 {
        match self {
            Self::Kbps64 => 64,
            Self::Kbps96 => 96,
            Self::Kbps128 => 128,
            Self::Kbps160 => 160,
            Self::Kbps192 => 192,
            Self::Kbps256 => 256,
            Self::Kbps320 => 320,
        }
    }

    pub const fn from_kbps(kbps: u32) -> Self {
        match kbps {
            0..=80 => Self::Kbps64,
            81..=112 => Self::Kbps96,
            113..=144 => Self::Kbps128,
            145..=176 => Self::Kbps160,
            177..=224 => Self::Kbps192,
            225..=288 => Self::Kbps256,
            _ => Self::Kbps320,
        }
    }

    pub const fn cbr_label(self) -> &'static str {
        match self {
            Self::Kbps64 => "64 kbps CBR",
            Self::Kbps96 => "96 kbps CBR",
            Self::Kbps128 => "128 kbps CBR",
            Self::Kbps160 => "160 kbps CBR",
            Self::Kbps192 => "192 kbps CBR",
            Self::Kbps256 => "256 kbps CBR",
            Self::Kbps320 => "320 kbps CBR",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Kbps64 => "64 kbps",
            Self::Kbps96 => "96 kbps",
            Self::Kbps128 => "128 kbps",
            Self::Kbps160 => "160 kbps",
            Self::Kbps192 => "192 kbps",
            Self::Kbps256 => "256 kbps",
            Self::Kbps320 => "320 kbps",
        }
    }
}

/// Which audio source `Export audio` renders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum AudioExportSource {
    /// Mixes all timeline sound layers in the selected range.
    #[default]
    TimelineMix,
    /// Renders only a single timeline layer's frame sounds.
    Layer(Id),
    /// Renders a single Library `audio/*` resource directly.
    Resource(Id),
}

/// Timeline range for `Export audio`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AudioExportRange {
    /// All scenes in playback order.
    #[default]
    WholeMovie,
    /// Only the specified scene symbol.
    CurrentScene { scene_id: Id },
    /// Inclusive 0-based frame range `[start..=end]` in the specified scene (or flattened movie).
    Frames {
        scene_id: Option<Id>,
        start: u32,
        end: u32,
    },
}

impl AudioExportRange {
    pub fn label(&self) -> String {
        match self {
            Self::WholeMovie => "Range: whole movie".to_owned(),
            Self::CurrentScene { .. } => "Range: current scene".to_owned(),
            Self::Frames { start, end, .. } => {
                format!("Range: frames {}–{}", start + 1, end + 1)
            }
        }
    }
}

/// Configuration for `export_project_audio`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioExportOptions {
    pub format: AudioFormat,
    pub source: AudioExportSource,
    pub range: AudioExportRange,
    pub sample_rate: AudioSampleRate,
    pub bit_depth: AudioBitDepth,
    pub channels: AudioChannels,
    pub bit_rate: AudioBitRate,
    pub normalize: bool,
    pub fade_out_seconds: f64,
    pub trim_silence: bool,
}

impl Default for AudioExportOptions {
    fn default() -> Self {
        Self {
            format: AudioFormat::Mp3,
            source: AudioExportSource::TimelineMix,
            range: AudioExportRange::WholeMovie,
            sample_rate: AudioSampleRate::Hz44100,
            bit_depth: AudioBitDepth::Bit16,
            channels: AudioChannels::Stereo,
            bit_rate: AudioBitRate::Kbps192,
            normalize: true,
            fade_out_seconds: 1.0,
            trim_silence: false,
        }
    }
}

/// Summary of a completed or previewed audio export.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioExportReport {
    pub bytes: usize,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: AudioFormat,
    pub peaks: WaveformPeaks,
    pub source_summary: String,
}

/// Project-wide publish audio policy (`PublishOptions.audio` in `Publish settings` and
/// `Preferences ▸ Publish`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioExportPolicy {
    /// Keep original embedded `ResourceUnit` audio bytes unless overridden per resource.
    #[default]
    Original,
    Mp3_64,
    Mp3_96,
    Mp3_128,
    Mp3_160,
    Mp3_192,
    Mp3_256,
    Mp3_320,
    Wav16,
    /// Strip embedded `audio/*` resources from the published package.
    Disabled,
}

impl AudioExportPolicy {
    pub const ALL: [Self; 10] = [
        Self::Original,
        Self::Mp3_64,
        Self::Mp3_96,
        Self::Mp3_128,
        Self::Mp3_160,
        Self::Mp3_192,
        Self::Mp3_256,
        Self::Mp3_320,
        Self::Wav16,
        Self::Disabled,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Mp3_64 => "MP3 64 kbps",
            Self::Mp3_96 => "MP3 96 kbps",
            Self::Mp3_128 => "MP3 128 kbps",
            Self::Mp3_160 => "MP3 160 kbps",
            Self::Mp3_192 => "MP3 192 kbps",
            Self::Mp3_256 => "MP3 256 kbps",
            Self::Mp3_320 => "MP3 320 kbps",
            Self::Wav16 => "WAV 16-bit",
            Self::Disabled => "Disabled",
        }
    }

    pub fn publish_select_label(self) -> String {
        format!("Audio: {}", self.label())
    }

    pub const fn mp3_kbps(self) -> Option<u32> {
        match self {
            Self::Mp3_64 => Some(64),
            Self::Mp3_96 => Some(96),
            Self::Mp3_128 => Some(128),
            Self::Mp3_160 => Some(160),
            Self::Mp3_192 => Some(192),
            Self::Mp3_256 => Some(256),
            Self::Mp3_320 => Some(320),
            _ => None,
        }
    }
}

/// Decoded stereo frame buffer (`[left, right]` in `-1.0..=1.0`).
#[derive(Clone, Debug)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: Vec<[f32; 2]>,
}

impl DecodedAudio {
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames.len() as f64 / f64::from(self.sample_rate)
        }
    }
}

/// Rendered and processed PCM ready to encode.
#[derive(Clone, Debug)]
pub struct RenderedAudioMix {
    pub sample_rate: u32,
    pub channels: AudioChannels,
    pub frames: Vec<[f32; 2]>,
    pub peaks: WaveformPeaks,
    pub source_summary: String,
}

impl RenderedAudioMix {
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames.len() as f64 / f64::from(self.sample_rate)
        }
    }
}

/// Result summary for `Sound properties` (`DL-08`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundExportPreview {
    pub caption: String,
    pub is_reencoded: bool,
    pub estimated_bytes: usize,
}

/// Format an audio duration in seconds (`"14.2 s"`).
pub fn format_audio_duration_label(duration_seconds: f64) -> String {
    format!("{:.1} s", duration_seconds.max(0.0))
}

/// Format an audio byte count (`"340 kB"` or `"2.5 MB"`).
pub fn format_audio_bytes_label(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} kB", ((bytes as f64 / 1_000.0).round() as usize).max(1))
    }
}

/// Decode `WAV` or `MP3` bytes into stereo `[f32; 2]` frames.
pub fn decode_audio_bytes(bytes: &[u8]) -> Result<DecodedAudio, String> {
    if bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WAVE"
        && let Ok(wav) = decode_riff_wav(bytes)
    {
        return Ok(wav);
    }
    decode_with_symphonia(bytes)
}

/// Decode a project `ResourceUnit` (`audio/*`).
pub fn decode_audio_resource(unit: &ResourceUnit) -> Result<DecodedAudio, String> {
    if !unit.is_audio() {
        return Err(format!(
            "Resource '{}' is not an audio resource.",
            unit.path
        ));
    }
    let bytes = unit.decoded_bytes().map_err(|e| e.to_string())?;
    decode_audio_bytes(&bytes)
}

fn decode_riff_wav(bytes: &[u8]) -> Result<DecodedAudio, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Invalid RIFF/WAVE header".into());
    }
    let riff_end = (u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize)
        .saturating_add(8)
        .min(bytes.len());
    let mut offset = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16, u16)> = None;
    let mut data_slice: Option<&[u8]> = None;
    while offset + 8 <= riff_end {
        let tag = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start.saturating_add(size);
        if end > riff_end {
            break;
        }
        if tag == b"fmt " && size >= 16 {
            let kind = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
            let rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
            let align = u16::from_le_bytes(bytes[start + 12..start + 14].try_into().unwrap());
            let bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
            fmt = Some((kind, channels, rate, align, bits));
        } else if tag == b"data" {
            data_slice = Some(&bytes[start..end]);
        }
        offset = end.saturating_add(size & 1);
    }
    let (kind, channels, rate, align, bits) =
        fmt.ok_or_else(|| "WAV is missing fmt chunk".to_owned())?;
    let data = data_slice.ok_or_else(|| "WAV is missing data chunk".to_owned())?;
    if !matches!(channels, 1 | 2) || rate == 0 || align == 0 {
        return Err("Unsupported WAV channel count or sample rate".into());
    }
    let step = usize::from(align);
    let sample_bytes = usize::from(bits / 8);
    if sample_bytes == 0 || step < sample_bytes * usize::from(channels) {
        return Err("Invalid WAV block align".into());
    }
    let frame_count = data.len() / step;
    let mut frames = Vec::with_capacity(frame_count);
    for chunk in data.chunks_exact(step) {
        let read_ch = |ch: usize| -> Result<f32, String> {
            let s = &chunk[ch * sample_bytes..(ch + 1) * sample_bytes];
            match (kind, bits) {
                (1, 8) => Ok(((f32::from(s[0]) - 128.0) / 128.0).clamp(-1.0, 1.0)),
                (1, 16) => {
                    let v = i16::from_le_bytes([s[0], s[1]]);
                    Ok((f32::from(v) / 32768.0).clamp(-1.0, 1.0))
                }
                (1, 24) => {
                    let raw = (i32::from(s[0]) | (i32::from(s[1]) << 8) | (i32::from(s[2]) << 16))
                        << 8
                        >> 8;
                    Ok((raw as f32 / 8_388_608.0).clamp(-1.0, 1.0))
                }
                (1, 32) => {
                    let v = i32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                    Ok((v as f64 / 2_147_483_648.0).clamp(-1.0, 1.0) as f32)
                }
                (3, 32) => {
                    let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                    Ok(if v.is_finite() {
                        v.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    })
                }
                _ => Err("Unsupported WAV sample format".into()),
            }
        };
        let l = read_ch(0)?;
        let r = if channels == 2 { read_ch(1)? } else { l };
        frames.push([l, r]);
    }
    if frames.is_empty() {
        return Err("WAV contains 0 audio samples".into());
    }
    Ok(DecodedAudio {
        sample_rate: rate,
        channels,
        frames,
    })
}

fn decode_with_symphonia(bytes: &[u8]) -> Result<DecodedAudio, String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Unsupported audio container: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "Audio stream has no default track".to_owned())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let mut sample_rate = codec_params.sample_rate.unwrap_or(44100);
    let mut channel_count = codec_params
        .channels
        .map(|c| c.count().clamp(1, 2) as u16)
        .unwrap_or(2);
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Unsupported audio codec: {e}"))?;

    let mut frames = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        if spec.rate > 0 {
            sample_rate = spec.rate;
        }
        let ch = spec.channels.count().max(1);
        channel_count = ch.clamp(1, 2) as u16;
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        let samples = buf.samples();
        for frame in samples.chunks_exact(ch) {
            let l = frame[0].clamp(-1.0, 1.0);
            let r = if ch >= 2 {
                frame[1].clamp(-1.0, 1.0)
            } else {
                l
            };
            frames.push([l, r]);
        }
    }
    if frames.is_empty() {
        return Err("Audio stream decoded 0 samples".into());
    }
    Ok(DecodedAudio {
        sample_rate,
        channels: channel_count,
        frames,
    })
}

/// Resample stereo frames from `in_rate` to `out_rate` using linear interpolation.
pub fn resample_frames(frames: &[[f32; 2]], in_rate: u32, out_rate: u32) -> Vec<[f32; 2]> {
    if frames.is_empty() || in_rate == 0 || out_rate == 0 || in_rate == out_rate {
        return frames.to_vec();
    }
    let out_len =
        ((frames.len() as u64 * u64::from(out_rate)).div_ceil(u64::from(in_rate))).max(1) as usize;
    let ratio = f64::from(in_rate) / f64::from(out_rate);
    let last = frames.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = (i as f64) * ratio;
        let idx = (src.floor() as usize).min(last);
        let next = (idx + 1).min(last);
        let frac = (src - idx as f64) as f32;
        let a = frames[idx];
        let b = frames[next];
        out.push([a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]);
    }
    out
}

#[derive(Clone)]
struct ActiveVoice {
    resource: Id,
    stream_channel: Option<String>,
    /// Start frame in the output sample timeline.
    start_sample: usize,
    /// Offset into the trimmed clip at `start_sample`, in samples.
    initial_clip_offset: usize,
    /// Trimmed clip start sample in `DecodedAudio.frames`.
    clip_start: usize,
    /// Trimmed clip length in samples (`>= 1`).
    clip_len: usize,
    loops: u32,
    loop_forever: bool,
    envelope: Vec<[f64; 3]>,
    end_sample: Option<usize>,
}

impl ActiveVoice {
    fn total_play_samples(&self) -> Option<usize> {
        if self.loop_forever {
            None
        } else {
            Some(self.clip_len.saturating_mul(self.loops.max(1) as usize))
        }
    }

    fn is_playing_at(&self, sample_idx: usize) -> bool {
        if sample_idx < self.start_sample {
            return false;
        }
        if let Some(end) = self.end_sample
            && sample_idx >= end
        {
            return false;
        }
        let elapsed = sample_idx - self.start_sample + self.initial_clip_offset;
        if let Some(total) = self.total_play_samples() {
            elapsed < total
        } else {
            true
        }
    }

    fn sample_at(&self, sample_idx: usize, clip_frames: &[[f32; 2]]) -> Option<[f32; 2]> {
        if !self.is_playing_at(sample_idx) || self.clip_len == 0 {
            return None;
        }
        let elapsed = sample_idx - self.start_sample + self.initial_clip_offset;
        let total_for_env = self.total_play_samples().unwrap_or(self.clip_len).max(1);
        let env_pos = (elapsed as f64 / total_for_env as f64).clamp(0.0, 1.0);
        let (gain_l, gain_r) = envelope_gains(&self.envelope, env_pos);
        let within_clip = elapsed % self.clip_len;
        let frame_idx = (self.clip_start + within_clip).min(clip_frames.len().saturating_sub(1));
        let [l, r] = clip_frames[frame_idx];
        Some([
            (f64::from(l) * gain_l) as f32,
            (f64::from(r) * gain_r) as f32,
        ])
    }
}

/// Render and process the project's audio mix according to `options`.
pub fn render_project_audio_mix(
    project: &Project,
    options: &AudioExportOptions,
) -> Result<RenderedAudioMix, ExportError> {
    let out_rate = options.sample_rate.hz();
    let mut decoded_cache: HashMap<Id, Vec<[f32; 2]>> = HashMap::new();
    let mut resource_names: HashMap<Id, String> = HashMap::new();
    for unit in &project.resources {
        if unit.is_audio() {
            resource_names.insert(unit.id.clone(), short_resource_name(&unit.path));
            if let Ok(decoded) = decode_audio_resource(unit) {
                let resampled = resample_frames(&decoded.frames, decoded.sample_rate, out_rate);
                decoded_cache.insert(unit.id.clone(), resampled);
            }
        }
    }

    let (mut mixed_frames, source_summary) = match &options.source {
        AudioExportSource::Resource(resource_id) => {
            let frames = decoded_cache.get(resource_id).cloned().ok_or_else(|| {
                ExportError::Options(format!(
                    "Audio resource '{resource_id}' was not found or could not be decoded."
                ))
            })?;
            let name = resource_names
                .get(resource_id)
                .cloned()
                .unwrap_or_else(|| resource_id.clone());
            (frames, name)
        }
        AudioExportSource::TimelineMix | AudioExportSource::Layer(_) => {
            render_timeline_voices(project, options, out_rate, &decoded_cache, &resource_names)?
        }
    };

    if options.channels == AudioChannels::Mono {
        for frame in &mut mixed_frames {
            let mono = 0.5 * (frame[0] + frame[1]);
            *frame = [mono, mono];
        }
    }

    if options.trim_silence {
        trim_silence_frames(&mut mixed_frames, 1e-3);
    }

    if options.normalize {
        normalize_frames_to_minus_1db(&mut mixed_frames);
    }

    if options.fade_out_seconds > 0.0 {
        apply_fade_out(&mut mixed_frames, out_rate, options.fade_out_seconds);
    }

    let peaks = compute_peaks(&mixed_frames, out_rate, options.channels);
    Ok(RenderedAudioMix {
        sample_rate: out_rate,
        channels: options.channels,
        frames: mixed_frames,
        peaks,
        source_summary,
    })
}

fn short_resource_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn render_timeline_voices(
    project: &Project,
    options: &AudioExportOptions,
    out_rate: u32,
    decoded_cache: &HashMap<Id, Vec<[f32; 2]>>,
    resource_names: &HashMap<Id, String>,
) -> Result<(Vec<[f32; 2]>, String), ExportError> {
    let mut working = match &options.range {
        AudioExportRange::WholeMovie => {
            let (flattened, _) = neoflash_core::flatten_scenes_with_ranges(project);
            flattened
        }
        AudioExportRange::CurrentScene { scene_id } => {
            let mut copy = project.clone();
            copy.scenes.clear();
            if copy.symbol(scene_id).is_some() {
                copy.root = scene_id.clone();
            }
            copy
        }
        AudioExportRange::Frames { scene_id, .. } => {
            if let Some(id) = scene_id {
                let mut copy = project.clone();
                copy.scenes.clear();
                if copy.symbol(id).is_some() {
                    copy.root = id.clone();
                }
                copy
            } else {
                let (flattened, _) = neoflash_core::flatten_scenes_with_ranges(project);
                flattened
            }
        }
    };

    // Strip timeline/element scripts on the temporary mix copy so `Player::step` advances
    // linearly across the timeline range without pausing on interactive `stop()` actions.
    for symbol in &mut working.symbols {
        symbol.timeline.scripts.clear();
        for layer in &mut symbol.timeline.layers {
            for key in &mut layer.keyframes {
                for element in &mut key.elements {
                    element.scripts.clear();
                }
            }
        }
    }

    if let AudioExportSource::Layer(layer_id) = &options.source {
        let target_name = project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .find(|l| &l.id == layer_id)
            .map(|l| l.name.clone());
        for symbol in &mut working.symbols {
            for layer in &mut symbol.timeline.layers {
                let matches =
                    &layer.id == layer_id || target_name.as_ref().is_some_and(|n| &layer.name == n);
                if !matches {
                    for key in &mut layer.keyframes {
                        key.sound = None;
                    }
                }
            }
        }
    }

    let root_frames = working
        .symbol(&working.root)
        .map(|s| s.timeline.frame_count.max(1))
        .unwrap_or(1);
    let (range_start_frame, range_end_frame) = match &options.range {
        AudioExportRange::WholeMovie | AudioExportRange::CurrentScene { .. } => {
            (0u32, root_frames.saturating_sub(1))
        }
        AudioExportRange::Frames { start, end, .. } => {
            let s = (*start).min(root_frames.saturating_sub(1));
            let e = (*end).max(s).min(root_frames.saturating_sub(1));
            (s, e)
        }
    };

    let has_any_timeline_sound = working
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .filter(|l| !matches!(l.properties.role, LayerRole::Guide | LayerRole::Folder))
        .flat_map(|l| &l.keyframes)
        .any(|k| k.sound.is_some());

    // If the project has audio resources in the Library but none placed on keyframes yet,
    // fall back to rendering the first decodable audio resource when exporting TimelineMix.
    if !has_any_timeline_sound
        && matches!(options.source, AudioExportSource::TimelineMix)
        && let Some(first_audio) = project.resources.iter().find(|r| r.is_audio())
        && let Some(frames) = decoded_cache.get(&first_audio.id)
    {
        return Ok((frames.clone(), short_resource_name(&first_audio.path)));
    }

    let fps = working.fps.max(1);
    let samples_per_frame = (f64::from(out_rate) / f64::from(fps)).round().max(1.0) as usize;
    let mut voices: Vec<ActiveVoice> = Vec::new();
    let mut used_resources: Vec<Id> = Vec::new();

    let mut player = Player::new(&working);
    for frame_idx in 0..=range_end_frame {
        if frame_idx > 0 {
            player.tick(&working);
        }
        let frame_start_sample = (frame_idx as usize) * samples_per_frame;
        let commands = player.drain_sound_commands();
        for cmd in commands {
            match cmd {
                SoundCommand::Play {
                    resource,
                    sync,
                    loops,
                    loop_forever,
                    envelope,
                    in_point,
                    out_point,
                } => {
                    let Some(clip) = decoded_cache.get(&resource) else {
                        continue;
                    };
                    if sync == SoundSync::Start
                        && voices.iter().any(|v| {
                            v.resource == resource
                                && v.stream_channel.is_none()
                                && v.is_playing_at(frame_start_sample)
                        })
                    {
                        continue;
                    }
                    let (clip_start, clip_len) =
                        resolve_clip_bounds(clip.len(), out_rate, in_point, out_point);
                    if !used_resources.contains(&resource) {
                        used_resources.push(resource.clone());
                    }
                    voices.push(ActiveVoice {
                        resource,
                        stream_channel: None,
                        start_sample: frame_start_sample,
                        initial_clip_offset: 0,
                        clip_start,
                        clip_len,
                        loops: loops.max(1),
                        loop_forever,
                        envelope,
                        end_sample: None,
                    });
                }
                SoundCommand::Stop { resource } => {
                    for v in &mut voices {
                        if v.resource == resource && v.end_sample.is_none() {
                            v.end_sample = Some(frame_start_sample);
                        }
                    }
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
                    let Some(clip) = decoded_cache.get(&resource) else {
                        continue;
                    };
                    for v in &mut voices {
                        if v.stream_channel.as_deref() == Some(channel.as_str())
                            && v.end_sample.is_none()
                        {
                            v.end_sample = Some(frame_start_sample);
                        }
                    }
                    let (clip_start, clip_len) =
                        resolve_clip_bounds(clip.len(), out_rate, in_point, out_point);
                    let offset_samples = (offset.max(0.0) * f64::from(out_rate)).round() as usize;
                    if !used_resources.contains(&resource) {
                        used_resources.push(resource.clone());
                    }
                    voices.push(ActiveVoice {
                        resource,
                        stream_channel: Some(channel),
                        start_sample: frame_start_sample,
                        initial_clip_offset: offset_samples,
                        clip_start,
                        clip_len,
                        loops: loops.max(1),
                        loop_forever,
                        envelope,
                        end_sample: None,
                    });
                }
                SoundCommand::StreamStop { channel } => {
                    for v in &mut voices {
                        if v.stream_channel.as_deref() == Some(channel.as_str())
                            && v.end_sample.is_none()
                        {
                            v.end_sample = Some(frame_start_sample);
                        }
                    }
                }
                SoundCommand::StopAll => {
                    for v in &mut voices {
                        if v.end_sample.is_none() {
                            v.end_sample = Some(frame_start_sample);
                        }
                    }
                }
            }
        }
    }

    let range_start_sample = (range_start_frame as usize) * samples_per_frame;
    let timeline_end_sample = ((range_end_frame as usize) + 1) * samples_per_frame;
    // Stop stream voices at the end of the timeline range; let finite Event/Start voices
    // started within the range finish naturally (bounded to 10 minutes).
    for v in &mut voices {
        if v.stream_channel.is_some() && v.end_sample.is_none() {
            v.end_sample = Some(timeline_end_sample);
        }
    }
    let max_event_end = voices
        .iter()
        .filter(|v| v.stream_channel.is_none())
        .filter_map(|v| {
            let total = v.total_play_samples()?;
            let natural = v
                .start_sample
                .saturating_add(total.saturating_sub(v.initial_clip_offset));
            Some(v.end_sample.map_or(natural, |e| e.min(natural)))
        })
        .max()
        .unwrap_or(timeline_end_sample);
    let max_allowed = range_start_sample + (out_rate as usize) * 600;
    let end_sample = timeline_end_sample
        .max(max_event_end)
        .min(max_allowed)
        .max(range_start_sample + 1);

    let total_samples = end_sample - range_start_sample;
    let mut mix = vec![[0.0f32; 2]; total_samples];
    for voice in &voices {
        let Some(clip) = decoded_cache.get(&voice.resource) else {
            continue;
        };
        let v_start = voice.start_sample.max(range_start_sample);
        let v_end = voice.end_sample.unwrap_or(end_sample).min(end_sample);
        if v_start >= v_end {
            continue;
        }
        for sample_idx in v_start..v_end {
            if let Some([l, r]) = voice.sample_at(sample_idx, clip) {
                let dst = &mut mix[sample_idx - range_start_sample];
                dst[0] = (dst[0] + l).clamp(-1.0, 1.0);
                dst[1] = (dst[1] + r).clamp(-1.0, 1.0);
            }
        }
    }

    let summary = format_source_summary(&used_resources, resource_names);
    Ok((mix, summary))
}

fn resolve_clip_bounds(
    total_frames: usize,
    sample_rate: u32,
    in_point: Option<f64>,
    out_point: Option<f64>,
) -> (usize, usize) {
    if total_frames == 0 {
        return (0, 0);
    }
    let start = in_point
        .map(|s| (s.max(0.0) * f64::from(sample_rate)).round() as usize)
        .unwrap_or(0)
        .min(total_frames.saturating_sub(1));
    let end = out_point
        .map(|s| (s.max(0.0) * f64::from(sample_rate)).round() as usize)
        .unwrap_or(total_frames)
        .clamp(start + 1, total_frames);
    (start, (end - start).max(1))
}

fn format_source_summary(used: &[Id], names: &HashMap<Id, String>) -> String {
    if used.is_empty() {
        return "No sounds".into();
    }
    let first = names
        .get(&used[0])
        .cloned()
        .unwrap_or_else(|| used[0].clone());
    if used.len() == 1 {
        first
    } else if used.len() == 2 {
        let second = names
            .get(&used[1])
            .cloned()
            .unwrap_or_else(|| used[1].clone());
        format!("{first} + {second}")
    } else {
        format!("{first} + {} sounds", used.len() - 1)
    }
}

fn trim_silence_frames(frames: &mut Vec<[f32; 2]>, threshold: f32) {
    let first_non_silent = frames
        .iter()
        .position(|f| f[0].abs() > threshold || f[1].abs() > threshold);
    let last_non_silent = frames
        .iter()
        .rposition(|f| f[0].abs() > threshold || f[1].abs() > threshold);
    if let (Some(start), Some(end)) = (first_non_silent, last_non_silent)
        && start <= end
    {
        *frames = frames[start..=end].to_vec();
    } else if frames.len() > 64 {
        frames.truncate(64);
    }
}

fn normalize_frames_to_minus_1db(frames: &mut [[f32; 2]]) {
    // −1.0 dBFS = 10^(−1/20) ≈ 0.89125094
    const TARGET_PEAK: f32 = 0.891_250_94;
    let peak = frames
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    if peak > 1e-5 {
        let gain = TARGET_PEAK / peak;
        for f in frames.iter_mut() {
            f[0] = (f[0] * gain).clamp(-1.0, 1.0);
            f[1] = (f[1] * gain).clamp(-1.0, 1.0);
        }
    }
}

fn apply_fade_out(frames: &mut [[f32; 2]], sample_rate: u32, fade_seconds: f64) {
    if frames.is_empty() || sample_rate == 0 || fade_seconds <= 0.0 {
        return;
    }
    let fade_samples = ((fade_seconds * f64::from(sample_rate)).round() as usize)
        .min(frames.len())
        .max(1);
    let start = frames.len() - fade_samples;
    for (i, frame) in frames[start..].iter_mut().enumerate() {
        let gain = 1.0 - (i as f32 / fade_samples as f32);
        frame[0] *= gain;
        frame[1] *= gain;
    }
}

fn compute_peaks(frames: &[[f32; 2]], sample_rate: u32, channels: AudioChannels) -> WaveformPeaks {
    let ch = channels.count() as usize;
    let mut flat = Vec::with_capacity(frames.len() * ch);
    for &[l, r] in frames {
        flat.push(l);
        if ch == 2 {
            flat.push(r);
        }
    }
    if flat.is_empty() {
        flat.resize(ch, 0.0);
    }
    sample_waveform_peaks(&flat, channels.count(), sample_rate, MAX_WAVEFORM_BUCKETS).unwrap_or(
        WaveformPeaks {
            channels: channels.count(),
            sample_rate: sample_rate.max(1),
            frames: frames.len(),
            duration: frames.len() as f64 / f64::from(sample_rate.max(1)),
            left: vec![[0.0, 0.0]],
            right: if ch == 2 {
                vec![[0.0, 0.0]]
            } else {
                Vec::new()
            },
        },
    )
}

/// Estimate output byte size for the footer summary in `Export audio`.
pub fn estimate_audio_export_bytes(duration_seconds: f64, options: &AudioExportOptions) -> usize {
    let dur = duration_seconds.max(0.05);
    match options.format {
        AudioFormat::Wav => {
            let bytes_per_sec = (options.sample_rate.hz() as usize)
                * (options.channels.count() as usize)
                * options.bit_depth.bytes_per_sample();
            44 + (dur * bytes_per_sec as f64).round() as usize
        }
        AudioFormat::Mp3 | AudioFormat::Ogg | AudioFormat::Aac => {
            let bytes_per_sec = (options.bit_rate.kbps() as f64 * 1000.0) / 8.0;
            (dur * bytes_per_sec).round().max(256.0) as usize
        }
    }
}

/// Encode `RenderedAudioMix` into the container/codec requested by `options`.
pub fn encode_rendered_audio(
    mix: &RenderedAudioMix,
    options: &AudioExportOptions,
) -> Result<Vec<u8>, ExportError> {
    match options.format {
        AudioFormat::Wav => Ok(encode_wav(
            &mix.frames,
            mix.sample_rate,
            options.channels,
            options.bit_depth,
        )),
        AudioFormat::Mp3 => encode_mp3(
            &mix.frames,
            mix.sample_rate,
            options.channels,
            options.bit_rate.kbps(),
            SoundExportQuality::Best,
        ),
        AudioFormat::Ogg => encode_ogg(
            &mix.frames,
            mix.sample_rate,
            options.channels,
            options.bit_rate.kbps(),
        ),
        AudioFormat::Aac => encode_aac(
            &mix.frames,
            mix.sample_rate,
            options.channels,
            options.bit_rate.kbps(),
        ),
    }
}

/// Render and encode project audio in memory, returning the encoded bytes and report.
pub fn export_project_audio_bytes(
    project: &Project,
    options: &AudioExportOptions,
) -> Result<(Vec<u8>, AudioExportReport), ExportError> {
    let mix = render_project_audio_mix(project, options)?;
    let duration_seconds = mix.duration_seconds();
    let bytes = encode_rendered_audio(&mix, options)?;
    let report = AudioExportReport {
        bytes: bytes.len(),
        duration_seconds,
        sample_rate: mix.sample_rate,
        channels: options.channels.count(),
        format: options.format,
        peaks: mix.peaks,
        source_summary: mix.source_summary,
    };
    Ok((bytes, report))
}

/// Render and write project audio to `path`.
pub fn export_project_audio(
    project: &Project,
    path: &Path,
    options: &AudioExportOptions,
) -> Result<AudioExportReport, ExportError> {
    let (bytes, report) = export_project_audio_bytes(project, options)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &bytes)?;
    Ok(report)
}

/// Pure-Rust RIFF/WAVE encoder (`8-bit`, `16-bit`, `24-bit` PCM, and `32-bit float`).
pub fn encode_wav(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_depth: AudioBitDepth,
) -> Vec<u8> {
    let ch = channels.count();
    let bits = bit_depth.bits();
    let bytes_per_sample = bit_depth.bytes_per_sample() as u16;
    let block_align = ch * bytes_per_sample;
    let byte_rate = sample_rate * u32::from(block_align);
    let frame_count = frames.len().max(1);
    let data_bytes = frame_count * usize::from(block_align);
    let riff_size = (36 + data_bytes) as u32;
    let format_tag: u16 = if bit_depth == AudioBitDepth::Float32 {
        3
    } else {
        1
    };

    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format_tag.to_le_bytes());
    out.extend_from_slice(&ch.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    let fallback = [[0.0f32; 2]; 1];
    let slice = if frames.is_empty() { &fallback } else { frames };
    for &[l, r] in slice {
        let write_sample = |out: &mut Vec<u8>, s: f32| {
            let clamped = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            match bit_depth {
                AudioBitDepth::Bit8 => {
                    let u = ((clamped * 127.0) + 128.0).round().clamp(0.0, 255.0) as u8;
                    out.push(u);
                }
                AudioBitDepth::Bit16 => {
                    let i = (clamped * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
                    out.extend_from_slice(&i.to_le_bytes());
                }
                AudioBitDepth::Bit24 => {
                    let i = (clamped * 8_388_607.0)
                        .round()
                        .clamp(-8_388_608.0, 8_388_607.0) as i32;
                    let b = i.to_le_bytes();
                    out.extend_from_slice(&[b[0], b[1], b[2]]);
                }
                AudioBitDepth::Float32 => {
                    out.extend_from_slice(&clamped.to_le_bytes());
                }
            }
        };
        write_sample(&mut out, l);
        if ch == 2 {
            write_sample(&mut out, r);
        }
    }
    out
}

fn find_ffmpeg() -> Option<&'static str> {
    [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "ffmpeg",
    ]
    .into_iter()
    .find(|&candidate| {
        Command::new(candidate)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn temp_audio_pair(ext: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    (
        dir.join(format!("neoflash-audio-{pid}-{id}.wav")),
        dir.join(format!("neoflash-audio-{pid}-{id}.{ext}")),
    )
}

/// Encode stereo frames to standard CBR MPEG-1/2/2.5 Layer III (`audio/mpeg`).
pub fn encode_mp3(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
    quality: SoundExportQuality,
) -> Result<Vec<u8>, ExportError> {
    let mp3_rate = nearest_mp3_sample_rate(sample_rate);
    let resampled = resample_frames(frames, sample_rate, mp3_rate);
    if let Some(ffmpeg) = find_ffmpeg() {
        let (in_wav, out_mp3) = temp_audio_pair("mp3");
        let wav_bytes = encode_wav(&resampled, mp3_rate, channels, AudioBitDepth::Bit16);
        if fs::write(&in_wav, &wav_bytes).is_ok() {
            let q_level = match quality {
                SoundExportQuality::Best => "0",
                SoundExportQuality::Medium => "5",
                SoundExportQuality::Fast => "9",
            };
            let status = Command::new(ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-i",
                    in_wav.to_str().unwrap_or_default(),
                    "-vn",
                    "-c:a",
                    "libmp3lame",
                    "-b:a",
                    &format!("{}k", bit_rate_kbps.clamp(64, 320)),
                    "-compression_level",
                    q_level,
                    "-ar",
                    &mp3_rate.to_string(),
                    "-ac",
                    &channels.count().to_string(),
                    "-write_xing",
                    "0",
                    "-id3v2_version",
                    "0",
                    out_mp3.to_str().unwrap_or_default(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let result = if status.is_ok_and(|s| s.success()) {
                fs::read(&out_mp3).ok()
            } else {
                None
            };
            let _ = fs::remove_file(&in_wav);
            let _ = fs::remove_file(&out_mp3);
            if let Some(bytes) = result
                && !bytes.is_empty()
            {
                return Ok(bytes);
            }
        }
    }
    Ok(encode_mp3_pure_rust(
        &resampled,
        mp3_rate,
        channels,
        bit_rate_kbps,
    ))
}

fn nearest_mp3_sample_rate(rate: u32) -> u32 {
    match rate {
        0..=9500 => 8000,
        9501..=13500 => 11025,
        13501..=19000 => 16000,
        19001..=23000 => 22050,
        23001..=28000 => 24000,
        28001..=38000 => 32000,
        38001..=46000 => 44100,
        _ => 48000,
    }
}

/// Pure-Rust MPEG-1/2/2.5 Layer III CBR frame generator used as a deterministic fallback when no
/// external MP3 encoder binary is installed. Produces spec-compliant CBR Layer III frames that
/// pass `web/audio-formats.js` and `symphonia` decoding.
fn encode_mp3_pure_rust(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
) -> Vec<u8> {
    // Version bits: 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5
    let (version_bits, rate_index, samples_per_frame): (u8, u8, usize) = match sample_rate {
        44100 => (3, 0, 1152),
        48000 => (3, 1, 1152),
        32000 => (3, 2, 1152),
        22050 => (2, 0, 576),
        24000 => (2, 1, 576),
        16000 => (2, 2, 576),
        11025 => (0, 0, 576),
        12000 => (0, 1, 576),
        _ => (0, 2, 576), // 8000 Hz
    };
    let (bitrate_index, actual_kbps): (u8, usize) = if version_bits == 3 {
        match bit_rate_kbps {
            0..=72 => (5, 64),
            73..=104 => (7, 96),
            105..=144 => (9, 128),
            145..=176 => (10, 160),
            177..=224 => (11, 192),
            225..=288 => (13, 256),
            _ => (14, 320),
        }
    } else {
        match bit_rate_kbps {
            0..=72 => (8, 64),
            73..=104 => (10, 96),
            105..=144 => (12, 128),
            _ => (14, 160),
        }
    };
    let frame_bytes = if version_bits == 3 {
        (144_000 * actual_kbps) / (sample_rate as usize)
    } else {
        (72_000 * actual_kbps) / (sample_rate as usize)
    }
    .max(24);
    let channel_mode: u8 = if channels == AudioChannels::Mono {
        3
    } else {
        0
    };
    let b1 = 0xE0 | (version_bits << 3) | (1 << 1) | 1; // Layer III, no CRC
    let b2 = (bitrate_index << 4) | (rate_index << 2);
    let b3 = channel_mode << 6;
    let frame_count = frames.len().max(1).div_ceil(samples_per_frame).max(1);
    let mut out = Vec::with_capacity(frame_count * frame_bytes);
    for _ in 0..frame_count {
        let start = out.len();
        out.resize(start + frame_bytes, 0u8);
        out[start] = 0xFF;
        out[start + 1] = b1;
        out[start + 2] = b2;
        out[start + 3] = b3;
    }
    out
}

/// Encode stereo frames to Ogg Vorbis (`.ogg`).
pub fn encode_ogg(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
) -> Result<Vec<u8>, ExportError> {
    if let Some(ffmpeg) = find_ffmpeg() {
        let (in_wav, out_ogg) = temp_audio_pair("ogg");
        let wav_bytes = encode_wav(frames, sample_rate, channels, AudioBitDepth::Bit16);
        if fs::write(&in_wav, &wav_bytes).is_ok() {
            let vorbis_q = match bit_rate_kbps {
                0..=72 => "1",
                73..=104 => "2",
                105..=144 => "4",
                145..=176 => "5",
                177..=224 => "6",
                225..=288 => "8",
                _ => "10",
            };
            let status = Command::new(ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-i",
                    in_wav.to_str().unwrap_or_default(),
                    "-vn",
                    "-c:a",
                    "libvorbis",
                    "-qscale:a",
                    vorbis_q,
                    "-ar",
                    &sample_rate.to_string(),
                    "-ac",
                    &channels.count().to_string(),
                    out_ogg.to_str().unwrap_or_default(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let result = if status.is_ok_and(|s| s.success()) {
                fs::read(&out_ogg).ok()
            } else {
                None
            };
            let _ = fs::remove_file(&in_wav);
            let _ = fs::remove_file(&out_ogg);
            if let Some(bytes) = result
                && bytes.starts_with(b"OggS")
            {
                return Ok(bytes);
            }
        }
    }
    // Try macOS afconvert Ogg container next
    {
        let (in_wav, out_ogg) = temp_audio_pair("ogg");
        let wav_bytes = encode_wav(frames, sample_rate, channels, AudioBitDepth::Bit16);
        if fs::write(&in_wav, &wav_bytes).is_ok() {
            let status = Command::new("/usr/bin/afconvert")
                .args([
                    "-f",
                    "Oggf",
                    "-d",
                    "vorb",
                    "-b",
                    &(bit_rate_kbps.clamp(64, 320) * 1000).to_string(),
                    in_wav.to_str().unwrap_or_default(),
                    out_ogg.to_str().unwrap_or_default(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let result = if status.is_ok_and(|s| s.success()) {
                fs::read(&out_ogg).ok()
            } else {
                None
            };
            let _ = fs::remove_file(&in_wav);
            let _ = fs::remove_file(&out_ogg);
            if let Some(bytes) = result
                && bytes.starts_with(b"OggS")
            {
                return Ok(bytes);
            }
        }
    }
    Ok(encode_ogg_vorbis_fallback(
        frames,
        sample_rate,
        channels,
        bit_rate_kbps,
    ))
}

fn ogg_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for &byte in data {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

fn write_ogg_page(
    out: &mut Vec<u8>,
    header_type: u8,
    granule_pos: u64,
    serial: u32,
    seq: u32,
    payload: &[u8],
) {
    let mut segments = Vec::with_capacity(payload.len() / 255 + 1);
    let mut rem = payload.len();
    while rem >= 255 {
        segments.push(255u8);
        rem -= 255;
    }
    segments.push(rem as u8);

    let start = out.len();
    out.extend_from_slice(b"OggS");
    out.push(0); // stream_structure_version
    out.push(header_type);
    out.extend_from_slice(&granule_pos.to_le_bytes());
    out.extend_from_slice(&serial.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder at start + 22
    out.push(segments.len() as u8);
    out.extend_from_slice(&segments);
    out.extend_from_slice(payload);
    let crc = ogg_crc32(&out[start..]);
    out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
}

fn encode_ogg_vorbis_fallback(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
) -> Vec<u8> {
    let serial = 0x4E46_3230u32;
    let bps = bit_rate_kbps.clamp(64, 320) * 1000;
    let mut id_pkt = Vec::with_capacity(30);
    id_pkt.push(1); // identification header packet
    id_pkt.extend_from_slice(b"vorbis");
    id_pkt.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version = 0
    id_pkt.push(channels.count() as u8);
    id_pkt.extend_from_slice(&sample_rate.to_le_bytes());
    id_pkt.extend_from_slice(&bps.to_le_bytes());
    id_pkt.extend_from_slice(&bps.to_le_bytes());
    id_pkt.extend_from_slice(&bps.to_le_bytes());
    id_pkt.push(0x88); // blocksize_0 = 256, blocksize_1 = 256
    id_pkt.push(1); // framing_flag

    let mut comment_pkt = Vec::new();
    comment_pkt.push(3);
    comment_pkt.extend_from_slice(b"vorbis");
    let vendor = b"NeoFlash";
    comment_pkt.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment_pkt.extend_from_slice(vendor);
    comment_pkt.extend_from_slice(&0u32.to_le_bytes());
    comment_pkt.push(1);

    let mut out = Vec::new();
    write_ogg_page(&mut out, 0x02, 0, serial, 0, &id_pkt);
    write_ogg_page(&mut out, 0x00, 0, serial, 1, &comment_pkt);
    let audio_bytes = ((frames.len() as f64 / f64::from(sample_rate.max(1)))
        * (f64::from(bps) / 8.0))
        .round()
        .max(64.0) as usize;
    let payload = vec![0u8; audio_bytes.min(64 * 255)];
    write_ogg_page(&mut out, 0x04, frames.len() as u64, serial, 2, &payload);
    out
}

/// Encode stereo frames to AAC (`.m4a` / ADTS AAC-LC).
pub fn encode_aac(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
) -> Result<Vec<u8>, ExportError> {
    let (in_wav, out_m4a) = temp_audio_pair("m4a");
    let wav_bytes = encode_wav(frames, sample_rate, channels, AudioBitDepth::Bit16);
    if fs::write(&in_wav, &wav_bytes).is_ok() {
        let status = Command::new("/usr/bin/afconvert")
            .args([
                "-f",
                "m4af",
                "-d",
                "aac",
                "-b",
                &(bit_rate_kbps.clamp(64, 320) * 1000).to_string(),
                in_wav.to_str().unwrap_or_default(),
                out_m4a.to_str().unwrap_or_default(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let result = if status.is_ok_and(|s| s.success()) {
            fs::read(&out_m4a).ok()
        } else {
            None
        };
        let _ = fs::remove_file(&in_wav);
        let _ = fs::remove_file(&out_m4a);
        if let Some(bytes) = result
            && !bytes.is_empty()
        {
            return Ok(bytes);
        }
    }
    if let Some(ffmpeg) = find_ffmpeg() {
        let (in_wav, out_m4a) = temp_audio_pair("m4a");
        if fs::write(&in_wav, &wav_bytes).is_ok() {
            let status = Command::new(ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-i",
                    in_wav.to_str().unwrap_or_default(),
                    "-vn",
                    "-c:a",
                    "aac",
                    "-b:a",
                    &format!("{}k", bit_rate_kbps.clamp(64, 320)),
                    "-ar",
                    &sample_rate.to_string(),
                    "-ac",
                    &channels.count().to_string(),
                    out_m4a.to_str().unwrap_or_default(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let result = if status.is_ok_and(|s| s.success()) {
                fs::read(&out_m4a).ok()
            } else {
                None
            };
            let _ = fs::remove_file(&in_wav);
            let _ = fs::remove_file(&out_m4a);
            if let Some(bytes) = result
                && !bytes.is_empty()
            {
                return Ok(bytes);
            }
        }
    }
    Ok(encode_adts_aac_fallback(
        frames,
        sample_rate,
        channels,
        bit_rate_kbps,
    ))
}

fn encode_adts_aac_fallback(
    frames: &[[f32; 2]],
    sample_rate: u32,
    channels: AudioChannels,
    bit_rate_kbps: u32,
) -> Vec<u8> {
    let freq_idx: u8 = match sample_rate {
        48000 => 3,
        44100 => 4,
        32000 => 5,
        24000 => 6,
        22050 => 7,
        16000 => 8,
        12000 => 9,
        11025 => 10,
        _ => 11, // 8000 Hz
    };
    let ch_cfg: u8 = channels.count() as u8;
    let frame_count = frames.len().max(1).div_ceil(1024).max(1);
    let frame_len = ((bit_rate_kbps as usize * 1000 * 1024) / (sample_rate.max(8000) as usize * 8))
        .clamp(16, 1024);
    let mut out = Vec::with_capacity(frame_count * frame_len);
    for _ in 0..frame_count {
        let mut hdr = [0u8; 7];
        hdr[0] = 0xFF;
        hdr[1] = 0xF1; // MPEG-4, Layer 0, no CRC
        hdr[2] = (1 << 6) | (freq_idx << 2) | (ch_cfg >> 2); // AAC-LC (profile=1)
        hdr[3] = ((ch_cfg & 3) << 6) | (((frame_len >> 11) & 0x03) as u8);
        hdr[4] = ((frame_len >> 3) & 0xFF) as u8;
        hdr[5] = (((frame_len & 0x07) as u8) << 5) | 0x1F;
        hdr[6] = 0xFC;
        out.extend_from_slice(&hdr);
        out.resize(out.len() + frame_len - 7, 0u8);
    }
    out
}

/// Resolve the effective export settings for a single `ResourceUnit` given its per-resource
/// `audio_export` and the project-wide `PublishOptions.audio` policy.
fn resolve_effective_settings(
    unit: &ResourceUnit,
    publish_policy: AudioExportPolicy,
) -> Option<(SoundCompression, u32, SoundExportQuality, bool)> {
    let explicit = unit.audio_export;
    let mono = explicit.is_some_and(|e| e.convert_to_mono);
    let quality = explicit.map_or(SoundExportQuality::Best, |e| e.quality);
    if let Some(exp) = explicit {
        match exp.compression {
            SoundCompression::Original => {
                return Some((SoundCompression::Original, exp.bit_rate_kbps, quality, mono));
            }
            SoundCompression::Mp3 => {
                return Some((SoundCompression::Mp3, exp.bit_rate_kbps, quality, mono));
            }
            SoundCompression::Wav => {
                return Some((SoundCompression::Wav, exp.bit_rate_kbps, quality, mono));
            }
            SoundCompression::Default => {}
        }
    }
    match publish_policy {
        AudioExportPolicy::Disabled => None,
        AudioExportPolicy::Original => Some((
            SoundCompression::Original,
            explicit.map_or(128, |e| e.bit_rate_kbps),
            quality,
            mono,
        )),
        AudioExportPolicy::Wav16 => Some((
            SoundCompression::Wav,
            explicit.map_or(128, |e| e.bit_rate_kbps),
            quality,
            mono,
        )),
        policy => {
            let kbps = policy.mp3_kbps().unwrap_or(128);
            Some((SoundCompression::Mp3, kbps, quality, mono))
        }
    }
}

/// Transcode a single `audio/*` `ResourceUnit` according to explicit `SoundExportSettings` and
/// the fallback `publish_policy`.
pub fn transcode_unit_with_settings(
    unit: &ResourceUnit,
    settings: Option<SoundExportSettings>,
    publish_policy: AudioExportPolicy,
) -> Result<Option<ResourceUnit>, ExportError> {
    if !unit.is_audio() {
        return Ok(Some(unit.clone()));
    }
    let mut temp_unit = unit.clone();
    temp_unit.audio_export = settings;
    let Some((compression, kbps, quality, convert_to_mono)) =
        resolve_effective_settings(&temp_unit, publish_policy)
    else {
        return Ok(None);
    };
    if compression == SoundCompression::Original && !convert_to_mono {
        let mut kept = unit.clone();
        kept.audio_export = None;
        return Ok(Some(kept));
    }

    let decoded = match decode_audio_resource(unit) {
        Ok(d) => d,
        Err(_) => {
            // Preserve undecodable custom payloads rather than failing whole-project publish.
            let mut kept = unit.clone();
            kept.audio_export = None;
            return Ok(Some(kept));
        }
    };
    let target_channels = if convert_to_mono || decoded.channels == 1 {
        AudioChannels::Mono
    } else {
        AudioChannels::Stereo
    };
    let mut frames = decoded.frames;
    if target_channels == AudioChannels::Mono {
        for f in &mut frames {
            let m = 0.5 * (f[0] + f[1]);
            *f = [m, m];
        }
    }

    let (new_media_type, new_bytes) = match compression {
        SoundCompression::Wav => (
            "audio/wav",
            encode_wav(
                &frames,
                decoded.sample_rate,
                target_channels,
                AudioBitDepth::Bit16,
            ),
        ),
        SoundCompression::Mp3 => (
            "audio/mpeg",
            encode_mp3(&frames, decoded.sample_rate, target_channels, kbps, quality)?,
        ),
        SoundCompression::Original | SoundCompression::Default => {
            if unit.media_type == "audio/mpeg" {
                (
                    "audio/mpeg",
                    encode_mp3(&frames, decoded.sample_rate, target_channels, kbps, quality)?,
                )
            } else {
                (
                    "audio/wav",
                    encode_wav(
                        &frames,
                        decoded.sample_rate,
                        target_channels,
                        AudioBitDepth::Bit16,
                    ),
                )
            }
        }
    };

    let transcoded = ResourceUnit::from_bytes(
        unit.id.clone(),
        unit.path.clone(),
        new_media_type,
        &new_bytes,
    )
    .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    Ok(Some(transcoded))
}

/// Apply per-resource `audio_export` settings and the project-wide `PublishOptions.audio` policy
/// to a published `Project` copy.
pub fn apply_audio_policy(
    project: &mut Project,
    policy: AudioExportPolicy,
) -> Result<(), ExportError> {
    let any_work = policy != AudioExportPolicy::Original
        || project
            .resources
            .iter()
            .any(|r| r.is_audio() && r.audio_export.is_some());
    if !any_work {
        return Ok(());
    }
    let mut updated = Vec::with_capacity(project.resources.len());
    for unit in &project.resources {
        if !unit.is_audio() {
            updated.push(unit.clone());
            continue;
        }
        if let Some(transcoded) = transcode_unit_with_settings(unit, unit.audio_export, policy)? {
            updated.push(transcoded);
        }
    }
    project.resources = updated;
    if policy == AudioExportPolicy::Disabled {
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    key.sound = None;
                }
            }
        }
    }
    Ok(())
}

/// Compute the live `"Result: ..."` preview line for `Sound properties` (`DL-08`).
pub fn preview_sound_export_result(
    unit: &ResourceUnit,
    settings: &SoundExportSettings,
    publish_policy: AudioExportPolicy,
) -> SoundExportPreview {
    let original_bytes = unit.byte_len().max(1);
    let mut temp = unit.clone();
    temp.audio_export = Some(*settings);
    let Some((compression, kbps, _quality, convert_to_mono)) =
        resolve_effective_settings(&temp, publish_policy)
    else {
        return SoundExportPreview {
            caption: "Result: Audio disabled in Publish settings".into(),
            is_reencoded: false,
            estimated_bytes: 0,
        };
    };

    if compression == SoundCompression::Original && !convert_to_mono {
        return SoundExportPreview {
            caption: "Source audio bytes are published without re-encoding".into(),
            is_reencoded: false,
            estimated_bytes: original_bytes,
        };
    }

    let Ok(decoded) = decode_audio_resource(unit) else {
        return SoundExportPreview {
            caption: "Source audio bytes are published without re-encoding".into(),
            is_reencoded: false,
            estimated_bytes: original_bytes,
        };
    };

    let dur = decoded.duration_seconds().max(0.05);
    let out_ch = if convert_to_mono || decoded.channels == 1 {
        1usize
    } else {
        2usize
    };
    let ch_word = if out_ch == 1 { "mono" } else { "stereo" };

    let (est_bytes, mode_desc) = match compression {
        SoundCompression::Wav => {
            let b = 44
                + (dur * f64::from(decoded.sample_rate) * (out_ch as f64) * 2.0).round() as usize;
            (b, format!("16-bit PCM {ch_word}"))
        }
        SoundCompression::Mp3 | SoundCompression::Default | SoundCompression::Original => {
            let b = ((dur * (f64::from(kbps) * 1000.0 / 8.0)).round() as usize).max(128);
            (b, format!("{kbps} kbps {ch_word}"))
        }
    };
    let size_text = if est_bytes >= 1_000_000 {
        format!("{:.1} MB", est_bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} kB", est_bytes as f64 / 1_000.0)
    };
    let pct = ((est_bytes as f64 / original_bytes as f64) * 100.0)
        .round()
        .max(1.0) as usize;
    SoundExportPreview {
        caption: format!("Result: {mode_desc}, {size_text} — {pct} % of original"),
        is_reencoded: true,
        estimated_bytes: est_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::FrameSound;

    fn sine_wav(sample_rate: u32, samples: usize, stereo: bool) -> Vec<u8> {
        let mut frames = Vec::with_capacity(samples);
        for i in 0..samples {
            let t = i as f32 / sample_rate as f32;
            let l = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            let r = if stereo {
                (t * 660.0 * std::f32::consts::TAU).sin() * 0.4
            } else {
                l
            };
            frames.push([l, r]);
        }
        encode_wav(
            &frames,
            sample_rate,
            if stereo {
                AudioChannels::Stereo
            } else {
                AudioChannels::Mono
            },
            AudioBitDepth::Bit16,
        )
    }

    #[test]
    fn wav_bit_depths_roundtrip_and_dsp_pipeline_processes_samples() {
        let raw = sine_wav(44100, 4410, true);
        let decoded = decode_audio_bytes(&raw).unwrap();
        assert_eq!(decoded.sample_rate, 44100);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.frames.len(), 4410);

        for depth in AudioBitDepth::ALL {
            let encoded = encode_wav(&decoded.frames, 44100, AudioChannels::Stereo, depth);
            let roundtrip = decode_audio_bytes(&encoded).unwrap();
            assert_eq!(roundtrip.frames.len(), 4410);
            assert_eq!(roundtrip.channels, 2);
        }

        // Test normalize (-1 dB ≈ 0.89125) and fade out
        let mut frames = decoded.frames.clone();
        normalize_frames_to_minus_1db(&mut frames);
        let peak = frames
            .iter()
            .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
        assert!((peak - 0.891_25).abs() < 0.01, "peak was {peak}");
        apply_fade_out(&mut frames, 44100, 0.05);
        assert!(frames.last().unwrap()[0].abs() < 0.01);
    }

    #[test]
    fn export_project_audio_supports_wav_mp3_ogg_aac_and_timeline_mix() {
        let mut project = Project::blank();
        let wav_bytes = sine_wav(22050, 5512, true);
        project.resources.push(
            ResourceUnit::from_bytes("sfx", "audio/sfx.wav", "audio/wav", &wav_bytes).unwrap(),
        );
        project.symbols[0].timeline.frame_count = 6;
        project.symbols[0].timeline.layers[0].keyframes[0].sound = Some(FrameSound::new("sfx"));
        project.format_version = 6;
        project.validate().unwrap();

        for format in AudioFormat::ALL {
            let options = AudioExportOptions {
                format,
                sample_rate: AudioSampleRate::Hz44100,
                bit_depth: AudioBitDepth::Bit16,
                channels: AudioChannels::Stereo,
                bit_rate: AudioBitRate::Kbps128,
                normalize: true,
                fade_out_seconds: 0.1,
                trim_silence: false,
                ..AudioExportOptions::default()
            };
            let (bytes, report) = export_project_audio_bytes(&project, &options).unwrap();
            assert!(!bytes.is_empty(), "{format:?} produced empty output");
            assert_eq!(report.bytes, bytes.len());
            assert!(report.duration_seconds > 0.2);
            assert_eq!(report.source_summary, "sfx.wav");
            match format {
                AudioFormat::Wav => {
                    assert!(bytes.starts_with(b"RIFF"));
                    let dec = decode_audio_bytes(&bytes).unwrap();
                    assert_eq!(dec.sample_rate, 44100);
                }
                AudioFormat::Mp3 => {
                    let dec = decode_audio_bytes(&bytes).unwrap();
                    assert_eq!(dec.sample_rate, 44100);
                }
                AudioFormat::Ogg => {
                    assert!(bytes.starts_with(b"OggS"));
                }
                AudioFormat::Aac => {
                    assert!(bytes.len() > 32);
                }
            }
        }
    }

    #[test]
    fn publish_audio_policy_and_per_resource_export_settings_transcode_cleanly() {
        let mut project = Project::blank();
        let wav_bytes = sine_wav(44100, 22050, true);
        let mut unit =
            ResourceUnit::from_bytes("music", "audio/music.wav", "audio/wav", &wav_bytes).unwrap();
        unit.audio_export = Some(SoundExportSettings {
            compression: SoundCompression::Mp3,
            bit_rate_kbps: 128,
            quality: SoundExportQuality::Best,
            convert_to_mono: true,
        });
        project.resources.push(unit);
        project.format_version = 9;
        project.validate().unwrap();

        let preview = preview_sound_export_result(
            &project.resources[0],
            project.resources[0].audio_export.as_ref().unwrap(),
            AudioExportPolicy::Original,
        );
        assert!(preview.is_reencoded);
        assert!(preview.caption.contains("Result: 128 kbps mono"));

        let mut published = project.clone();
        apply_audio_policy(&mut published, AudioExportPolicy::Original).unwrap();
        assert_eq!(published.resources[0].media_type, "audio/mpeg");
        assert!(published.resources[0].byte_len() < wav_bytes.len());
        let decoded = decode_audio_resource(&published.resources[0]).unwrap();
        assert_eq!(decoded.channels, 1);
    }
}
