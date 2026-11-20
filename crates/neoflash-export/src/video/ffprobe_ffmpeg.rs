use std::{
    env, fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use neoflash_core::inspect_video_resource;

use super::decoder::{
    FfmpegMediaBacking, OnDemandVideoBackend, PamFrameSet, StagedMediaFile,
    checked_decoded_video_duration_ms, checked_decoded_video_payload_len,
    checked_on_demand_video_bounds, finish_pam_rgba_frames, parse_ffmpeg_fps_from_stderr,
    parse_pam_rgba_frames, read_bounded_process_diagnostics, read_bounded_video_file,
    should_use_eager_video_frames, try_decode_nfvf_box,
};
use super::*;

/// Locate an `ffprobe` binary (`NEOFLASH_FFPROBE`, sibling of `ffmpeg`, bundled `Resources/media/bin/ffprobe`, `PATH`, or system fallbacks).
pub fn detect_ffprobe(ffmpeg: &Path) -> Option<PathBuf> {
    if let Ok(custom) = env::var("NEOFLASH_FFPROBE") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    for sibling_name in ["ffprobe", "ffprobe.exe"] {
        let sibling = ffmpeg.with_file_name(sibling_name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Some(bundled) = detect_bundled_ffprobe() {
        return Some(bundled);
    }
    if env::var("NEOFLASH_DISABLE_SYSTEM_FFMPEG").ok().as_deref() == Some("1") {
        return None;
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in ["ffprobe", "ffprobe.exe"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for fallback in [
        "/opt/homebrew/bin/ffprobe",
        "/usr/local/bin/ffprobe",
        "/usr/bin/ffprobe",
        r"C:\Program Files\ffmpeg\bin\ffprobe.exe",
    ] {
        let path = PathBuf::from(fallback);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct VideoProbeMetadata {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fps: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ProbedStreamMetadata {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fps: f32,
    pub(super) frame_count: Option<usize>,
    pub(super) duration_ms: Option<u64>,
}

pub(super) fn parse_ffprobe_rate(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        if denominator == 0.0 {
            return None;
        }
        let rate = numerator / denominator;
        return (rate.is_finite() && rate > 0.0 && rate <= f32::MAX as f64).then_some(rate as f32);
    }
    trimmed
        .parse::<f32>()
        .ok()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
}

fn parse_ffprobe_raw_fields(text: &str) -> Result<ProbedStreamMetadata, String> {
    let mut width = None;
    let mut height = None;
    let mut avg_fps = None;
    let mut fallback_fps = None;
    let mut duration_ms = None;
    let mut frame_count = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        match key {
            "width" => width = value.parse::<u32>().ok(),
            "height" => height = value.parse::<u32>().ok(),
            "avg_frame_rate" => avg_fps = parse_ffprobe_rate(value),
            "r_frame_rate" => fallback_fps = parse_ffprobe_rate(value),
            "duration" if duration_ms.is_none() => {
                duration_ms = value
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                    .map(|seconds| (seconds * 1000.0).ceil() as u64);
            }
            "nb_frames" => frame_count = value.parse::<usize>().ok().filter(|count| *count > 0),
            _ => {}
        }
    }
    let width = width.ok_or_else(|| "ffprobe did not report video width.".to_owned())?;
    let height = height.ok_or_else(|| "ffprobe did not report video height.".to_owned())?;
    let fps = avg_fps.or(fallback_fps).ok_or_else(|| {
        "ffprobe did not report a usable video frame rate; timing cannot be preserved.".to_owned()
    })?;
    if let Some(duration_ms) = duration_ms
        && duration_ms > MAX_DECODED_VIDEO_DURATION_MS
    {
        return Err(format!(
            "Video duration {duration_ms} ms exceeds the supported {MAX_DECODED_VIDEO_DURATION_MS} ms budget."
        ));
    }
    if frame_count.is_none()
        && let Some(duration_ms) = duration_ms
    {
        let estimate = ((duration_ms as f64 / 1000.0) * f64::from(fps)).ceil();
        if estimate.is_finite() && estimate > 0.0 && estimate <= usize::MAX as f64 {
            frame_count = Some(estimate as usize);
        }
    }
    Ok(ProbedStreamMetadata {
        width,
        height,
        fps,
        frame_count,
        duration_ms,
    })
}

#[cfg(test)]
pub(super) fn parse_ffprobe_metadata(text: &str) -> Result<VideoProbeMetadata, String> {
    let raw = parse_ffprobe_raw_fields(text)?;
    let bounded_frame_count = raw.frame_count.unwrap_or(1);
    checked_decoded_video_payload_len(raw.width, raw.height, bounded_frame_count)?;
    Ok(VideoProbeMetadata {
        width: raw.width,
        height: raw.height,
        fps: raw.fps,
    })
}

fn parse_ffprobe_stream_metadata(text: &str) -> Result<ProbedStreamMetadata, String> {
    let raw = parse_ffprobe_raw_fields(text)?;
    checked_on_demand_video_bounds(raw.width, raw.height, raw.frame_count.unwrap_or(1))?;
    Ok(raw)
}

pub(super) fn probe_stream_metadata(
    ffmpeg: &Path,
    input: &Path,
) -> Result<Option<ProbedStreamMetadata>, String> {
    let Some(ffprobe) = detect_ffprobe(ffmpeg) else {
        return Ok(None);
    };
    let mut cmd = Command::new(&ffprobe);
    configure_media_subprocess_command(&mut cmd, &ffprobe);
    let mut child = cmd
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,avg_frame_rate,r_frame_rate,nb_frames,duration:format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=0")
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to launch ffprobe: {error}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffprobe metadata.".into());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffprobe diagnostics.".into());
        }
    };
    let stdout_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stdout));
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(format!("Failed waiting for ffprobe: {error}"));
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "ffprobe metadata reader panicked.".to_owned())?
        .map_err(|error| format!("Failed reading ffprobe metadata: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "ffprobe diagnostics reader panicked.".to_owned())?
        .map_err(|error| format!("Failed reading ffprobe diagnostics: {error}"))?;
    if !status.success() {
        let diagnostics = String::from_utf8_lossy(&stderr.tail);
        return Err(format!(
            "ffprobe could not inspect the video safely: {}",
            diagnostics.trim()
        ));
    }
    let metadata = String::from_utf8_lossy(&stdout.head);
    parse_ffprobe_stream_metadata(&metadata).map(Some)
}

pub(super) fn decode_ffmpeg_pam_window(
    ffmpeg: &Path,
    input: &Path,
    seek_sec: Option<f64>,
    max_frames: usize,
    expected_dims: Option<(u32, u32)>,
) -> Result<PamFrameSet, String> {
    let mut cmd = Command::new(ffmpeg);
    configure_media_subprocess_command(&mut cmd, ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-nostdin")
        .arg("-max_pixels")
        .arg(
            u64::from(MAX_DECODED_VIDEO_WIDTH)
                .saturating_mul(u64::from(MAX_DECODED_VIDEO_HEIGHT))
                .to_string(),
        );
    if let Some(sec) = seek_sec {
        cmd.arg("-ss").arg(format!("{:.6}", sec.max(0.0)));
    }
    cmd.arg("-i").arg(input);
    if max_frames > 0 {
        cmd.arg("-frames:v").arg(max_frames.to_string());
    }
    let mut child = cmd
        .arg("-f")
        .arg("image2pipe")
        .arg("-c:v")
        .arg("pam")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run ffmpeg for windowed video decode: {e}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffmpeg windowed frames.".into());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffmpeg windowed diagnostics.".into());
        }
    };
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));
    let mut stdout = BufReader::new(stdout);
    let parsed = parse_pam_rgba_frames(&mut stdout);
    let allow_eof_empty = seek_sec.is_some()
        && parsed
            .as_ref()
            .is_err_and(|msg| msg.contains("0 decodable frames"));
    if parsed.is_err() && !allow_eof_empty {
        let _ = child.kill();
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(format!("Failed waiting for ffmpeg window decode: {error}"));
        }
    };
    let stderr_capture = stderr_reader
        .join()
        .map_err(|_| "ffmpeg diagnostics reader panicked.".to_owned())?
        .map_err(|e| format!("Failed reading ffmpeg diagnostics: {e}"))?;
    if !status.success() {
        let stderr_tail = String::from_utf8_lossy(&stderr_capture.tail);
        let last_err = stderr_tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("corrupt or unsupported video stream");
        return Err(format!(
            "Video failed codec/container decoding: {}",
            last_err.trim()
        ));
    }
    if allow_eof_empty {
        let (w, h) = expected_dims.unwrap_or((1, 1));
        return Ok(PamFrameSet {
            width: w,
            height: h,
            images: Vec::new(),
        });
    }
    let raw = parsed?;
    if let Some((exp_w, exp_h)) = expected_dims
        && (raw.width != exp_w || raw.height != exp_h)
    {
        return Err(format!(
            "Video changed dimensions between probe ({exp_w}x{exp_h}) and decode ({}x{}).",
            raw.width, raw.height
        ));
    }
    Ok(raw)
}

fn decode_ffmpeg_backing(
    name: &str,
    ffmpeg: PathBuf,
    backing: FfmpegMediaBacking,
) -> Result<DecodedVideoStream, String> {
    let probed_metadata = probe_stream_metadata(&ffmpeg, backing.path())?;
    if let Some(meta) = probed_metadata
        && let Some(frame_count) = meta.frame_count
    {
        let frame_bytes = checked_on_demand_video_bounds(meta.width, meta.height, frame_count)?;
        if !should_use_eager_video_frames(frame_bytes, frame_count) {
            let duration_ms = match meta.duration_ms {
                Some(dur_ms) if dur_ms > 0 => u32::try_from(dur_ms).unwrap_or(u32::MAX),
                _ => checked_decoded_video_duration_ms(frame_count, meta.fps)?,
            };
            let decoder = OnDemandVideoDecoder::new(
                meta.width,
                meta.height,
                meta.fps,
                frame_bytes,
                frame_count,
                0,
                OnDemandVideoBackend::Ffmpeg { ffmpeg, backing },
            );
            let initial = decoder
                .decode_window_frames(0, decoder.window_fetch_frames)
                .map_err(|e| format!("Video '{name}' initial window decode failed: {e}"))?;
            if initial.is_empty() {
                return Err(format!("Video '{name}' produced 0 decodable frames."));
            }
            if let Ok(mut ring) = decoder.ring.lock() {
                ring.window_fetches = 1;
                decoder.insert_decoded_batch(&mut ring, initial);
            }
            return Ok(DecodedVideoStream {
                width: meta.width,
                height: meta.height,
                fps: meta.fps,
                duration_ms,
                frames: Vec::new(),
                on_demand: Some(Arc::new(decoder)),
            });
        }
    }

    if let Some(meta) = probed_metadata {
        checked_decoded_video_payload_len(meta.width, meta.height, meta.frame_count.unwrap_or(1))?;
    }

    let mut cmd = Command::new(&ffmpeg);
    configure_media_subprocess_command(&mut cmd, &ffmpeg);
    let mut child = cmd
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-max_pixels")
        .arg(
            u64::from(MAX_DECODED_VIDEO_WIDTH)
                .saturating_mul(u64::from(MAX_DECODED_VIDEO_HEIGHT))
                .to_string(),
        )
        .arg("-i")
        .arg(backing.path())
        .arg("-f")
        .arg("image2pipe")
        .arg("-c:v")
        .arg("pam")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run ffmpeg for video '{name}': {e}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Failed to capture ffmpeg frames for video '{name}'."
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Failed to capture ffmpeg diagnostics for video '{name}'."
            ));
        }
    };
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));

    // Parse frames as ffmpeg emits them. This avoids retaining the complete raw
    // PAM pipe and then copying it a second time into decoded frames.
    let mut stdout = BufReader::new(stdout);
    let parsed = parse_pam_rgba_frames(&mut stdout);
    if parsed.is_err() {
        let _ = child.kill();
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(format!(
                "Failed waiting for ffmpeg video decode '{name}': {error}"
            ));
        }
    };
    let stderr_capture = stderr_reader
        .join()
        .map_err(|_| format!("ffmpeg diagnostics reader panicked for video '{name}'."))?
        .map_err(|e| format!("Failed reading ffmpeg diagnostics for video '{name}': {e}"))?;

    let stderr_tail = String::from_utf8_lossy(&stderr_capture.tail);
    if !status.success() {
        let last_err = stderr_tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("corrupt or unsupported video stream");
        return Err(format!(
            "Video '{name}' failed codec/container decoding: {}",
            last_err.trim()
        ));
    }
    let raw = parsed.map_err(|e| format!("Video '{name}' frame extraction failed: {e}"))?;
    if let Some(metadata) = probed_metadata
        && (raw.width != metadata.width || raw.height != metadata.height)
    {
        return Err(format!(
            "Video '{name}' changed dimensions between probe ({}x{}) and decode ({}x{}).",
            metadata.width, metadata.height, raw.width, raw.height
        ));
    }
    let stderr_head = String::from_utf8_lossy(&stderr_capture.head);
    let fps = probed_metadata
        .map(|metadata| metadata.fps)
        .or_else(|| parse_ffmpeg_fps_from_stderr(&stderr_head))
        .ok_or_else(|| {
            format!(
                "Video '{name}' frame rate could not be determined safely; timing was not guessed."
            )
        })?;
    finish_pam_rgba_frames(raw, fps)
        .map_err(|e| format!("Video '{name}' frame extraction failed: {e}"))
}

fn decode_video_stream_with_ffmpeg(
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<DecodedVideoStream, String> {
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        format!(
            "Video '{name}' requires ffmpeg to decode compressed video frames, but ffmpeg was not found."
        )
    })?;
    let ext = if mime.contains("webm") || name.ends_with(".webm") {
        "webm"
    } else if mime.contains("ogg") || name.ends_with(".ogv") || name.ends_with(".ogg") {
        "ogv"
    } else {
        "mp4"
    };
    let staged_video = StagedMediaFile::create(name, ext, bytes)?;
    decode_ffmpeg_backing(name, ffmpeg, FfmpegMediaBacking::Staged(staged_video))
}

fn decode_avi_stream_bytes(name: &str, bytes: &[u8]) -> Option<Result<DecodedVideoStream, String>> {
    let container = neoflash_core::index_avi_container(bytes).ok()?;
    if !container.info.supports_pure_rust_video() || container.frames.is_empty() {
        return None;
    }
    let count = container.frames.len();
    let frame_bytes =
        match checked_on_demand_video_bounds(container.info.width, container.info.height, count) {
            Ok(fb) => fb,
            Err(e) => return Some(Err(format!("Video '{name}' failed decoding: {e}"))),
        };
    let fps = (container.info.fps as f32).max(1.0);
    let duration_ms = container.info.duration_ms();
    if u64::from(duration_ms) > MAX_DECODED_VIDEO_DURATION_MS {
        return Some(Err(format!(
            "Video '{name}' duration {duration_ms} ms exceeds the supported {MAX_DECODED_VIDEO_DURATION_MS} ms budget."
        )));
    }
    if should_use_eager_video_frames(frame_bytes, count) {
        if let Ok(avi) = neoflash_core::decode_avi_bytes(bytes) {
            let default_dur_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
            let frames = avi
                .frames
                .into_iter()
                .map(|(timestamp_ms, image)| DecodedVideoFrame {
                    timestamp_ms,
                    duration_ms: default_dur_ms,
                    image,
                })
                .collect();
            return Some(Ok(DecodedVideoStream {
                width: avi.info.width,
                height: avi.info.height,
                fps,
                duration_ms,
                frames,
                on_demand: None,
            }));
        }
        return None;
    }

    let backing: Arc<[u8]> = bytes.to_vec().into();
    let backing_len = backing.len();
    let w = container.info.width;
    let h = container.info.height;
    let decoder = OnDemandVideoDecoder::new(
        w,
        h,
        fps,
        frame_bytes,
        count,
        backing_len,
        OnDemandVideoBackend::Avi {
            bytes: backing,
            container,
        },
    );
    let initial = match decoder.decode_window_frames(0, decoder.window_fetch_frames) {
        Ok(frames) if !frames.is_empty() => frames,
        Ok(_) => {
            return Some(Err(format!(
                "Video '{name}' produced 0 decodable AVI frames."
            )));
        }
        Err(e) => return Some(Err(format!("Video '{name}' AVI decode failed: {e}"))),
    };
    if let Ok(mut ring) = decoder.ring.lock() {
        ring.window_fetches = 1;
        decoder.insert_decoded_batch(&mut ring, initial);
    }
    Some(Ok(DecodedVideoStream {
        width: w,
        height: h,
        fps,
        duration_ms,
        frames: Vec::new(),
        on_demand: Some(Arc::new(decoder)),
    }))
}

/// Decode a video byte buffer (`MP4`, `WebM`, `Ogg`, `AVI`, or deterministic `nfvf` MP4) into RGBA8 frames (`T-201` / `T-229`).
pub fn decode_video_stream_bytes(
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<DecodedVideoStream, String> {
    if bytes.len() > MAX_VIDEO_INPUT_BYTES {
        return Err(format!(
            "Video '{name}' is {} bytes, exceeding the {} MiB compressed-input budget.",
            bytes.len(),
            MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let diags = inspect_video_resource(name, mime, bytes, None);
    if let Some(err_diag) = diags.iter().find(|d| d.is_error) {
        return Err(err_diag.message.clone());
    }
    if let Some(nfvf_res) = try_decode_nfvf_box(bytes) {
        return nfvf_res.map_err(|e| format!("Video '{name}' failed decoding: {e}"));
    }
    if neoflash_core::is_avi_bytes(bytes)
        && let Some(avi_res) = decode_avi_stream_bytes(name, bytes)
    {
        return avi_res;
    }
    decode_video_stream_with_ffmpeg(name, mime, bytes)
}

pub(super) fn decode_external_video_file(
    candidate: &Path,
    label: &str,
    mime: &str,
) -> Result<DecodedVideoStream, String> {
    let file = fs::File::open(candidate)
        .map_err(|error| format!("Failed to open external video '{label}': {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect external video '{label}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("External video '{label}' is not a regular file."));
    }
    if metadata.len() > MAX_VIDEO_INPUT_BYTES as u64 {
        return Err(format!(
            "External video '{label}' is {} bytes, exceeding the {} MiB input budget.",
            metadata.len(),
            MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let mut prefix = Vec::with_capacity(65_536.min(metadata.len() as usize));
    file.take(65_536)
        .read_to_end(&mut prefix)
        .map_err(|error| format!("Failed to read external video '{label}': {error}"))?;
    if prefix.is_empty() {
        return Err(format!("External video '{label}' is empty."));
    }
    if has_nfvf_box_header(&prefix) || neoflash_core::is_avi_bytes(&prefix) {
        let bytes = read_bounded_video_file(candidate, label)?;
        return decode_video_stream_bytes(label, mime, &bytes);
    }
    // Inspect full container boxes when small, or prefix for large external MP4/WebM files so
    // explicit codec diagnostics (`hvc1`, unsupported container) fire without reading 512 MiB into RAM.
    if metadata.len() <= 4 * 1024 * 1024 {
        let bytes = read_bounded_video_file(candidate, label)?;
        let diags = inspect_video_resource(label, mime, &bytes, None);
        if let Some(err_diag) = diags.iter().find(|d| d.is_error) {
            return Err(err_diag.message.clone());
        }
    } else {
        let diags = inspect_video_resource(label, mime, &prefix, None);
        if let Some(err_diag) = diags
            .iter()
            .find(|d| d.is_error && !d.message.contains("truncated"))
        {
            return Err(err_diag.message.clone());
        }
    }
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        format!(
            "Video '{label}' requires ffmpeg to decode compressed video frames, but ffmpeg was not found."
        )
    })?;
    decode_ffmpeg_backing(
        label,
        ffmpeg,
        FfmpegMediaBacking::ExternalPath(candidate.to_path_buf()),
    )
}
