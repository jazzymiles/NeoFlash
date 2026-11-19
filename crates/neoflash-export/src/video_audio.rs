//! Embedded video soundtracks for native playback and offline exports.
use neoflash_core::{ResolvedVideoPlacement, SoundCommand, VideoSyncMode};
use std::{collections::HashMap, fs, path::Path, process::Command};

/// Extract the first sound track, preserving native AVI PCM/MP3 when available.
/// A video with no audio track is a normal silent placement.
pub fn embedded_soundtrack(bytes: &[u8]) -> Result<Option<(Vec<u8>, &'static str)>, String> {
    if super::video::has_nfvf_box_header(bytes) {
        return Ok(None);
    }
    if neoflash_core::is_avi_bytes(bytes)
        && let Ok(avi) = neoflash_core::index_avi_container(bytes)
        && matches!(avi.info.audio_format_tag, None | Some(1 | 0x0055))
        && let Ok((data, mime)) = avi.extract_audio(bytes)
    {
        return Ok(data.zip(mime));
    }
    let helper = super::detect_ffmpeg().ok_or("Decoding this video soundtrack requires the bundled FFmpeg helper, NEOFLASH_FFMPEG or ffmpeg on PATH.")?;
    Ok(extract_wav_with_resolved_ffmpeg(bytes, &helper)?.map(|wav| (wav, "audio/wav")))
}

pub fn extract_wav_with_resolved_ffmpeg(
    bytes: &[u8],
    helper: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let dir = tempfile::Builder::new()
        .prefix("neoflash-video-audio-")
        .tempdir()
        .map_err(|e| format!("Cannot stage the video soundtrack: {e}"))?;
    let input = dir.path().join("input_video.bin");
    let output = dir.path().join("extracted.wav");
    fs::write(&input, bytes).map_err(|e| format!("Cannot stage video bytes: {e}"))?;
    // Use the same ten-minute decoded PCM budget as native sound previews.
    const MAX_WAV_BYTES: u64 = 44_100 * 4 * 600 + 65_536;
    let mut cmd = Command::new(helper);
    super::configure_media_subprocess_command(&mut cmd, helper);
    let result = cmd
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:a:0?",
            "-vn",
            "-acodec",
            "pcm_s16le",
            "-ar",
            "44100",
            "-ac",
            "2",
            "-fs",
        ])
        .arg(MAX_WAV_BYTES.to_string())
        .arg(&output)
        .output()
        .map_err(|e| format!("Could not start FFmpeg for the video soundtrack: {e}"))?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr);
        if detail.contains("does not contain any stream") {
            return Ok(None);
        }
        return Err(format!(
            "Could not decode video soundtrack: {}",
            detail.trim()
        ));
    }
    let size = fs::metadata(&output).map_err(|e| e.to_string())?.len();
    if size >= MAX_WAV_BYTES {
        return Err("Decoded video soundtrack exceeds the ten-minute PCM buffer budget.".into());
    }
    fs::read(output).map(Some).map_err(|e| e.to_string())
}

/// Convert the video scheduler's active channels into the same sound commands
/// used by frame sounds. A seek restarts one channel; normal ticks play continuously.
#[derive(Default)]
pub struct VideoSoundChannels {
    active: HashMap<String, ResolvedVideoPlacement>,
    external_audio: bool,
}

impl VideoSoundChannels {
    pub fn external_audio_warning(&self) -> Option<&'static str> {
        self.external_audio.then_some("The mix omits sound from external video. Embed the video or put its soundtrack on an audio layer.")
    }

    pub fn sync<'a>(
        &mut self,
        placements: impl Iterator<Item = &'a ResolvedVideoPlacement>,
        fps: f64,
    ) -> Vec<SoundCommand> {
        let mut next = HashMap::new();
        let mut commands = Vec::new();
        for p in placements.filter(|p| p.playing && !p.stopped && !p.muted) {
            if p.source.as_ref().is_some_and(|s| !s.mode.is_embedded()) {
                self.external_audio = true;
                continue;
            }
            let channel = format!("video:{}", p.channel);
            let continuous = self.active.remove(&channel).is_some_and(|old| {
                old.resource == p.resource
                    && old.in_point_ms == p.in_point_ms
                    && old.out_point_ms == p.out_point_ms
                    && old.loop_video == p.loop_video
                    && old.sync == p.sync
                    && p.time_ms >= old.time_ms
                    && (p.sync == VideoSyncMode::Event
                        || p.time_ms - old.time_ms <= (1000.0 / fps.max(1.0)).ceil() as u32 + 1)
            });
            if !continuous {
                commands.push(SoundCommand::StreamStart {
                    channel: channel.clone(),
                    resource: p.resource.clone(),
                    offset: f64::from(p.time_ms.saturating_sub(p.in_point_ms)) / 1000.0,
                    loops: 1,
                    loop_forever: p.loop_video,
                    envelope: Vec::new(),
                    in_point: Some(f64::from(p.in_point_ms) / 1000.0),
                    out_point: p.out_point_ms.map(|end| f64::from(end) / 1000.0),
                });
            }
            next.insert(channel, p.clone());
        }
        for channel in self.active.keys() {
            commands.push(SoundCommand::StreamStop {
                channel: channel.clone(),
            });
        }
        self.active = next;
        commands
    }
}
