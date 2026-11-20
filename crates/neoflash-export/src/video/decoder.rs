use std::{
    collections::VecDeque,
    env, fs,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use neoflash_core::{DecodedImage, ResolvedVideoPlacement, VideoSyncMode};

use super::ffprobe_ffmpeg::{decode_ffmpeg_pam_window, parse_ffprobe_rate};
use super::*;

pub(super) fn read_bounded_video_file_with_limit(
    path: &Path,
    label: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Failed to open external video '{label}': {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect external video '{label}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("External video '{label}' is not a regular file."));
    }
    if metadata.len() > limit as u64 {
        return Err(format!(
            "External video '{label}' is {} bytes, exceeding the {} MiB input budget.",
            metadata.len(),
            limit / (1024 * 1024)
        ));
    }
    let capacity = usize::try_from(metadata.len()).unwrap_or(limit).min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read external video '{label}': {error}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "External video '{label}' grew beyond the {} MiB input budget while it was being read.",
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

pub(super) fn read_bounded_video_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    read_bounded_video_file_with_limit(path, label, MAX_VIDEO_INPUT_BYTES)
}

/// A single decoded straight-RGBA8 video frame with its presentation timestamp (`T-201`).
#[derive(Clone, Debug)]
pub struct DecodedVideoFrame {
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub image: Arc<DecodedImage>,
}

#[derive(Debug)]
pub(super) enum FfmpegMediaBacking {
    ExternalPath(PathBuf),
    Staged(StagedMediaFile),
}

impl FfmpegMediaBacking {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::ExternalPath(path) => path,
            Self::Staged(staged) => staged.path(),
        }
    }
}

#[derive(Debug)]
pub(super) enum OnDemandVideoBackend {
    Nfvf {
        payload: Arc<[u8]>,
        frames_offset: usize,
        frame_bytes: usize,
    },
    Avi {
        bytes: Arc<[u8]>,
        container: neoflash_core::AviIndexedContainer,
    },
    Ffmpeg {
        ffmpeg: PathBuf,
        backing: FfmpegMediaBacking,
    },
}

#[derive(Debug)]
pub(super) struct WindowedFrameRing {
    entries: VecDeque<(usize, DecodedVideoFrame)>,
    resident_bytes: usize,
    effective_frame_count: usize,
    pub(super) window_fetches: usize,
}

/// Bounded on-demand / windowed ring decoder for long-form timeline video (`T-229`).
#[derive(Debug)]
pub struct OnDemandVideoDecoder {
    width: u32,
    height: u32,
    fps: f32,
    frame_dur_ms: u32,
    frame_bytes: usize,
    frame_count: usize,
    max_ring_frames: usize,
    max_ring_bytes: usize,
    pub(super) window_fetch_frames: usize,
    backing_ram_bytes: usize,
    backend: OnDemandVideoBackend,
    pub(super) ring: Mutex<WindowedFrameRing>,
}

impl OnDemandVideoDecoder {
    pub(super) fn new(
        width: u32,
        height: u32,
        fps: f32,
        frame_bytes: usize,
        frame_count: usize,
        backing_ram_bytes: usize,
        backend: OnDemandVideoBackend,
    ) -> Self {
        let safe_fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            24.0
        };
        let frame_dur_ms = ((1000.0 / f64::from(safe_fps)).round() as u32).max(1);
        let max_ring_frames = (MAX_WINDOWED_RING_STREAM_BYTES / frame_bytes.max(1))
            .clamp(1, MAX_WINDOWED_RING_FRAMES)
            .min(frame_count.max(1));
        let max_ring_bytes = max_ring_frames.saturating_mul(frame_bytes).max(frame_bytes);
        let window_fetch_frames = max_ring_frames.clamp(1, 16);
        Self {
            width,
            height,
            fps: safe_fps,
            frame_dur_ms,
            frame_bytes,
            frame_count,
            max_ring_frames,
            max_ring_bytes,
            window_fetch_frames,
            backing_ram_bytes,
            backend,
            ring: Mutex::new(WindowedFrameRing {
                entries: VecDeque::with_capacity(max_ring_frames),
                resident_bytes: 0,
                effective_frame_count: frame_count,
                window_fetches: 0,
            }),
        }
    }

    pub fn logical_frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn max_ring_frames(&self) -> usize {
        self.max_ring_frames
    }

    /// Pinned upper-bound CPU memory reservation for this stream (ring capacity + any in-RAM
    /// compressed container bytes).
    pub fn pinned_budget_bytes(&self) -> Option<usize> {
        const ARC_ALLOCATION_OVERHEAD: usize = 2 * std::mem::size_of::<usize>();
        let per_ring_entry = self
            .frame_bytes
            .checked_add(std::mem::size_of::<DecodedImage>())?
            .checked_add(ARC_ALLOCATION_OVERHEAD)?
            .checked_add(std::mem::size_of::<(usize, DecodedVideoFrame)>())?;
        let ring_budget = self.max_ring_frames.checked_mul(per_ring_entry)?;
        std::mem::size_of::<Self>()
            .checked_add(ARC_ALLOCATION_OVERHEAD)?
            .checked_add(ring_budget)?
            .checked_add(self.backing_ram_bytes)
    }

    pub fn resident_ring_frames(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.entries.len())
    }

    pub fn resident_ring_bytes(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.resident_bytes)
    }

    pub fn window_fetch_count(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.window_fetches)
    }

    fn timestamp_ms_for_index(&self, idx: usize) -> u32 {
        match &self.backend {
            OnDemandVideoBackend::Avi { container, .. } => container
                .frames
                .get(idx)
                .map_or(0, |entry| entry.timestamp_ms),
            OnDemandVideoBackend::Nfvf { .. } | OnDemandVideoBackend::Ffmpeg { .. } => {
                ((idx as f64 * 1000.0) / f64::from(self.fps)).round() as u32
            }
        }
    }

    fn choose_index_for_target_ms(&self, target_ms: u32) -> Option<usize> {
        let effective_count = self
            .ring
            .lock()
            .map_or(self.frame_count, |r| r.effective_frame_count);
        if effective_count == 0 {
            return None;
        }
        match &self.backend {
            OnDemandVideoBackend::Avi { container, .. } => container
                .frame_index_at_ms(target_ms)
                .map(|idx| idx.min(effective_count - 1)),
            OnDemandVideoBackend::Nfvf { .. } | OnDemandVideoBackend::Ffmpeg { .. } => {
                let mut lo = 0usize;
                let mut hi = effective_count;
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if self.timestamp_ms_for_index(mid) <= target_ms {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                Some(lo.saturating_sub(1).min(effective_count - 1))
            }
        }
    }

    pub(super) fn decode_window_frames(
        &self,
        start_idx: usize,
        count: usize,
    ) -> Result<Vec<(usize, DecodedVideoFrame)>, String> {
        if count == 0 || start_idx >= self.frame_count {
            return Ok(Vec::new());
        }
        let bounded_count = count.min(self.frame_count - start_idx);
        match &self.backend {
            OnDemandVideoBackend::Nfvf {
                payload,
                frames_offset,
                frame_bytes,
            } => {
                let mut out = Vec::with_capacity(bounded_count);
                for offset in 0..bounded_count {
                    let idx = start_idx + offset;
                    let byte_start =
                        frames_offset
                            .checked_add(idx.checked_mul(*frame_bytes).ok_or_else(|| {
                                "nfvf window frame byte offset overflow.".to_owned()
                            })?)
                            .ok_or_else(|| "nfvf window payload offset overflow.".to_owned())?;
                    let byte_end = byte_start
                        .checked_add(*frame_bytes)
                        .ok_or_else(|| "nfvf window end offset overflow.".to_owned())?;
                    let slice = payload
                        .get(byte_start..byte_end)
                        .ok_or_else(|| "Truncated nfvf window slice.".to_owned())?;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image: Arc::new(DecodedImage {
                                width: self.width,
                                height: self.height,
                                rgba: slice.to_vec().into(),
                            }),
                        },
                    ));
                }
                Ok(out)
            }
            OnDemandVideoBackend::Avi { bytes, container } => {
                let mut out = Vec::with_capacity(bounded_count);
                for offset in 0..bounded_count {
                    let idx = start_idx + offset;
                    let decoded = container.decode_frame(bytes, idx)?;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image: Arc::new(decoded),
                        },
                    ));
                }
                Ok(out)
            }
            OnDemandVideoBackend::Ffmpeg { ffmpeg, backing } => {
                let seek_sec = ((start_idx as f64 - 0.05) / f64::from(self.fps)).max(0.0);
                let raw = decode_ffmpeg_pam_window(
                    ffmpeg,
                    backing.path(),
                    Some(seek_sec),
                    bounded_count,
                    Some((self.width, self.height)),
                )?;
                let mut out = Vec::with_capacity(raw.images.len());
                for (offset, image) in raw.images.into_iter().enumerate() {
                    let idx = start_idx + offset;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image,
                        },
                    ));
                }
                Ok(out)
            }
        }
    }

    pub(super) fn insert_decoded_batch(
        &self,
        ring: &mut WindowedFrameRing,
        decoded: Vec<(usize, DecodedVideoFrame)>,
    ) {
        for (idx, frame) in decoded {
            let img_bytes = frame.image.rgba.len();
            if let Some(pos) = ring
                .entries
                .iter()
                .position(|(existing, _)| *existing == idx)
                && let Some((_, old)) = ring.entries.remove(pos)
            {
                ring.resident_bytes = ring.resident_bytes.saturating_sub(old.image.rgba.len());
            }
            while (!ring.entries.is_empty())
                && (ring.entries.len() >= self.max_ring_frames
                    || ring.resident_bytes.saturating_add(img_bytes) > self.max_ring_bytes)
            {
                if let Some((_, evicted)) = ring.entries.pop_front() {
                    ring.resident_bytes =
                        ring.resident_bytes.saturating_sub(evicted.image.rgba.len());
                }
            }
            ring.resident_bytes = ring.resident_bytes.saturating_add(img_bytes);
            ring.entries.push_back((idx, frame));
        }
    }

    fn frame_at_index(&self, target_idx: usize) -> Option<(usize, DecodedVideoFrame)> {
        {
            let mut ring = self.ring.lock().ok()?;
            if let Some(pos) = ring.entries.iter().position(|(idx, _)| *idx == target_idx) {
                let entry = ring.entries.remove(pos)?;
                let out = entry.clone();
                ring.entries.push_back(entry);
                return Some(out);
            }
        }

        let fetched = self
            .decode_window_frames(target_idx, self.window_fetch_frames)
            .ok()?;
        if !fetched.is_empty() {
            let mut ring = self.ring.lock().ok()?;
            ring.window_fetches = ring.window_fetches.saturating_add(1);
            self.insert_decoded_batch(&mut ring, fetched);
            if let Some((_, frame)) = ring.entries.iter().find(|(idx, _)| *idx == target_idx) {
                return Some((target_idx, frame.clone()));
            }
        } else if target_idx > 0 {
            // Audio-padded containers can report a format duration 1-2 frames past the final video
            // packet. Step back one window to clamp cleanly to the true final video frame.
            let fallback_start = target_idx.saturating_sub(self.window_fetch_frames.max(4));
            let tail = self
                .decode_window_frames(fallback_start, target_idx - fallback_start + 1)
                .ok()?;
            if let Some((last_idx, _)) = tail.last().cloned() {
                let mut ring = self.ring.lock().ok()?;
                ring.window_fetches = ring.window_fetches.saturating_add(1);
                ring.effective_frame_count = ring.effective_frame_count.min(last_idx + 1);
                self.insert_decoded_batch(&mut ring, tail);
                if let Some((_, frame)) = ring.entries.iter().find(|(idx, _)| *idx == last_idx) {
                    return Some((last_idx, frame.clone()));
                }
            }
        }
        None
    }
}

/// Decoded timeline video stream with deterministic timestamp lookup (`T-201` / `T-229`).
#[derive(Clone, Debug)]
pub struct DecodedVideoStream {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration_ms: u32,
    pub frames: Vec<DecodedVideoFrame>,
    pub on_demand: Option<Arc<OnDemandVideoDecoder>>,
}

impl DecodedVideoStream {
    pub fn logical_frame_count(&self) -> usize {
        self.on_demand
            .as_ref()
            .map_or(self.frames.len(), |d| d.logical_frame_count())
    }

    pub fn resident_cpu_bytes(&self) -> usize {
        if let Some(decoder) = &self.on_demand {
            decoder.resident_ring_bytes()
        } else {
            self.frames.iter().map(|f| f.image.rgba.len()).sum()
        }
    }

    /// Resolve `(frame_index, DecodedVideoFrame)` for `placement`, honoring `in_point_ms`,
    /// `out_point_ms`, `loop_video`, and retained `stopped` frames (`T-201` / `T-229`).
    pub fn frame_at_placement(
        &self,
        placement: &ResolvedVideoPlacement,
    ) -> Option<(usize, DecodedVideoFrame)> {
        let index = self.frame_index_at_placement(placement)?;
        if let Some(decoder) = &self.on_demand {
            decoder.frame_at_index(index)
        } else {
            self.frames.get(index).cloned().map(|frame| (index, frame))
        }
    }

    /// Resolve the presentation sample without loading its pixels.
    pub(crate) fn frame_index_at_placement(
        &self,
        placement: &ResolvedVideoPlacement,
    ) -> Option<usize> {
        let has_any_frames = !self.frames.is_empty()
            || self
                .on_demand
                .as_ref()
                .is_some_and(|d| d.logical_frame_count() > 0);
        if !has_any_frames || (placement.sync == VideoSyncMode::Stop && !placement.stopped) {
            return None;
        }
        let effective_end = match (
            placement.out_point_ms,
            Some(self.duration_ms).filter(|&d| d > 0),
        ) {
            (Some(out), Some(dur)) => Some(out.min(dur)),
            (Some(out), None) => Some(out),
            (None, Some(dur)) => Some(dur),
            (None, None) => None,
        };
        let mut target_ms = placement.effective_time_ms(Some(self.duration_ms));
        if !placement.loop_video
            && let Some(end_ms) = effective_end
            && end_ms > placement.in_point_ms
        {
            target_ms = target_ms.min(end_ms.saturating_sub(1));
        }
        if let Some(decoder) = &self.on_demand {
            return decoder.choose_index_for_target_ms(target_ms);
        }
        Some(
            self.frames
                .partition_point(|frame| frame.timestamp_ms <= target_ms)
                .saturating_sub(1),
        )
    }
}

/// Encode straight-RGBA8 frames into NeoFlash's deterministic `nfvf` test-fixture payload inside
/// an ISO-BMFF box sequence (`T-201`). This is intentionally not an interoperable H.264 encoder;
/// production MP4 output uses `ffmpeg`. The private payload gives tests a small, lossless stream
/// that `decode_video_stream_bytes` can decode without an external codec binary.
#[doc(hidden)]
pub fn encode_deterministic_mp4_rgba_frames(
    width: u32,
    height: u32,
    fps: u16,
    frames: &[&[u8]],
) -> Vec<u8> {
    let w = width.max(1);
    let h = height.max(1);
    let safe_fps = fps.max(1);
    let frame_byte_len = (w as usize) * (h as usize) * 4;
    let count = frames.len().min(u16::MAX as usize);
    let payload_len = 13usize + count * frame_byte_len;
    let box_size = (8usize + payload_len) as u32;

    let mut out = Vec::with_capacity(32 + box_size as usize);
    out.extend_from_slice(MP4_FTYP_AVC1_BOX);
    out.extend_from_slice(&box_size.to_be_bytes());
    out.extend_from_slice(NFVF_FOURCC);
    out.push(1u8); // version 1
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&safe_fps.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    for frame in frames.iter().take(count) {
        if frame.len() >= frame_byte_len {
            out.extend_from_slice(&frame[..frame_byte_len]);
        } else {
            out.extend_from_slice(frame);
            out.resize(out.len() + (frame_byte_len - frame.len()), 0);
        }
    }
    out
}

pub(super) fn checked_decoded_video_payload_len(
    width: u32,
    height: u32,
    frame_count: usize,
) -> Result<(usize, usize), String> {
    if width == 0
        || height == 0
        || width > MAX_DECODED_VIDEO_WIDTH
        || height > MAX_DECODED_VIDEO_HEIGHT
    {
        return Err(format!(
            "Decoded video dimensions {width}x{height} exceed the supported 1..={MAX_DECODED_VIDEO_WIDTH} by 1..={MAX_DECODED_VIDEO_HEIGHT} bounds."
        ));
    }
    if frame_count == 0 || frame_count > MAX_DECODED_VIDEO_FRAMES {
        return Err(format!(
            "Decoded video frame count {frame_count} exceeds the supported 1..={MAX_DECODED_VIDEO_FRAMES} budget."
        ));
    }
    let frame_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Decoded video frame dimensions overflow the RGBA byte size.".to_owned())?;
    let total_bytes = frame_bytes
        .checked_mul(frame_count)
        .ok_or_else(|| "Decoded video stream RGBA byte size overflow.".to_owned())?;
    if total_bytes > MAX_DECODED_VIDEO_STREAM_BYTES {
        return Err(format!(
            "Decoded video requires {total_bytes} RGBA bytes, exceeding the {} MiB retained-frame budget. Shorten or resize the source video.",
            MAX_DECODED_VIDEO_STREAM_BYTES / (1024 * 1024)
        ));
    }
    Ok((frame_bytes, total_bytes))
}

pub(super) fn checked_decoded_video_duration_ms(
    frame_count: usize,
    fps: f32,
) -> Result<u32, String> {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        f64::from(fps)
    } else {
        24.0
    };
    let duration_ms = (frame_count as f64 * 1000.0 / safe_fps).round().max(1.0);
    if !duration_ms.is_finite() || duration_ms > MAX_DECODED_VIDEO_DURATION_MS as f64 {
        return Err(format!(
            "Decoded video duration exceeds the supported {} minute retained-frame budget.",
            MAX_DECODED_VIDEO_DURATION_MS / 60_000
        ));
    }
    Ok(duration_ms as u32)
}

pub(super) fn checked_on_demand_video_bounds(
    width: u32,
    height: u32,
    frame_count: usize,
) -> Result<usize, String> {
    if width == 0
        || height == 0
        || width > MAX_DECODED_VIDEO_WIDTH
        || height > MAX_DECODED_VIDEO_HEIGHT
    {
        return Err(format!(
            "Decoded video dimensions {width}x{height} exceed the supported 1..={MAX_DECODED_VIDEO_WIDTH} by 1..={MAX_DECODED_VIDEO_HEIGHT} bounds."
        ));
    }
    if frame_count == 0 || frame_count > MAX_DECODED_VIDEO_BATCH_FRAMES {
        return Err(format!(
            "Decoded video frame count {frame_count} exceeds the supported 1..={MAX_DECODED_VIDEO_BATCH_FRAMES} budget."
        ));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Decoded video frame dimensions overflow the RGBA byte size.".to_owned())
}

pub(super) fn should_use_eager_video_frames(frame_bytes: usize, frame_count: usize) -> bool {
    frame_count <= MAX_EAGER_VIDEO_FRAMES
        && frame_bytes
            .checked_mul(frame_count)
            .is_some_and(|total| total <= MAX_EAGER_VIDEO_STREAM_BYTES)
}

pub(crate) fn has_nfvf_box_header(bytes: &[u8]) -> bool {
    let mut offsets = vec![0usize];
    if bytes.starts_with(LEGACY_STUB_MP4_HEADER) && bytes.len() > 32 {
        offsets.push(32);
    }
    for start_offset in offsets {
        let mut pos = start_offset;
        while pos + 8 <= bytes.len() {
            let box_size =
                u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                    as usize;
            if &bytes[pos + 4..pos + 8] == NFVF_FOURCC {
                return true;
            }
            if box_size < 8 || pos.saturating_add(box_size) > bytes.len() {
                break;
            }
            pos += box_size;
        }
    }
    false
}

pub(super) fn try_decode_nfvf_box(bytes: &[u8]) -> Option<Result<DecodedVideoStream, String>> {
    let mut offsets = vec![0usize];
    if bytes.starts_with(LEGACY_STUB_MP4_HEADER) && bytes.len() > 32 {
        offsets.push(32);
    }
    for start_offset in offsets {
        let mut pos = start_offset;
        while pos + 8 <= bytes.len() {
            let box_size =
                u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                    as usize;
            let fourcc = &bytes[pos + 4..pos + 8];
            if fourcc == NFVF_FOURCC {
                if box_size < 21 || pos + box_size > bytes.len() {
                    return Some(Err(
                        "Truncated or corrupt nfvf video frame box in MP4 container.".into(),
                    ));
                }
                let payload = &bytes[pos + 8..pos + box_size];
                let version = payload[0];
                if version != 1 {
                    return Some(Err(format!("Unsupported nfvf box version {version}.")));
                }
                let w = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
                let h = u32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]);
                let fps_u16 = u16::from_be_bytes([payload[9], payload[10]]);
                let count = u16::from_be_bytes([payload[11], payload[12]]) as usize;
                if fps_u16 == 0 {
                    return Some(Err("Invalid dimensions or frame count in nfvf box.".into()));
                }
                let frame_bytes = match checked_on_demand_video_bounds(w, h, count) {
                    Ok(value) => value,
                    Err(e) => return Some(Err(e)),
                };
                let expected_payload = match count
                    .checked_mul(frame_bytes)
                    .and_then(|n| n.checked_add(13))
                {
                    Some(len) => len,
                    None => {
                        return Some(Err("nfvf video payload byte length overflow.".into()));
                    }
                };
                if payload.len() != expected_payload {
                    return Some(Err(
                        "Corrupt or truncated frame payload in MP4 video.".into()
                    ));
                }
                let fps = f32::from(fps_u16);
                let duration_ms = match checked_decoded_video_duration_ms(count, fps) {
                    Ok(duration_ms) => duration_ms,
                    Err(e) => return Some(Err(e)),
                };
                if should_use_eager_video_frames(frame_bytes, count) {
                    let frame_dur_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
                    let mut frames = Vec::with_capacity(count);
                    for idx in 0..count {
                        let start = 13 + idx * frame_bytes;
                        let rgba = payload[start..start + frame_bytes].to_vec();
                        let timestamp_ms = ((idx as f64 * 1000.0) / f64::from(fps)).round() as u32;
                        frames.push(DecodedVideoFrame {
                            timestamp_ms,
                            duration_ms: frame_dur_ms,
                            image: Arc::new(DecodedImage {
                                width: w,
                                height: h,
                                rgba: rgba.into(),
                            }),
                        });
                    }
                    return Some(Ok(DecodedVideoStream {
                        width: w,
                        height: h,
                        fps,
                        duration_ms,
                        frames,
                        on_demand: None,
                    }));
                }
                let backing: Arc<[u8]> = payload.to_vec().into();
                let backing_len = backing.len();
                let decoder = OnDemandVideoDecoder::new(
                    w,
                    h,
                    fps,
                    frame_bytes,
                    count,
                    backing_len,
                    OnDemandVideoBackend::Nfvf {
                        payload: backing,
                        frames_offset: 13,
                        frame_bytes,
                    },
                );
                return Some(Ok(DecodedVideoStream {
                    width: w,
                    height: h,
                    fps,
                    duration_ms,
                    frames: Vec::new(),
                    on_demand: Some(Arc::new(decoder)),
                }));
            }
            if box_size < 8 || pos + box_size > bytes.len() {
                break;
            }
            pos += box_size;
        }
    }
    None
}

pub(super) fn parse_ffmpeg_fps_from_stderr(stderr: &str) -> Option<f32> {
    for line in stderr.lines() {
        if !line.contains("Video:") {
            continue;
        }
        for part in line.split(',') {
            let trimmed = part.trim();
            if let Some(num_str) = trimmed.strip_suffix("fps")
                && let Some(v) = parse_ffprobe_rate(num_str)
            {
                return Some(v);
            }
        }
    }
    None
}

#[derive(Debug)]
pub(super) struct PamFrameSet {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) images: Vec<Arc<DecodedImage>>,
}

pub(super) fn parse_pam_rgba_frames(reader: &mut impl BufRead) -> Result<PamFrameSet, String> {
    let mut images = Vec::new();
    let mut stream_w = 0u32;
    let mut stream_h = 0u32;
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed reading PAM frame header: {e}"))?;
        if read == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line.trim_end() != "P7" {
            return Err("Invalid PAM magic from ffmpeg.".into());
        }

        let mut w = 0u32;
        let mut h = 0u32;
        let mut depth = 0u32;
        let mut maxval = 0u32;
        let mut header_bytes = read;
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|e| format!("Failed reading PAM frame header: {e}"))?;
            if read == 0 {
                return Err("Truncated PAM video frame header from ffmpeg.".into());
            }
            header_bytes = header_bytes.saturating_add(read);
            if header_bytes > 4096 {
                return Err("PAM video frame header exceeds 4096 bytes.".into());
            }
            let trimmed = line.trim_end();
            if trimmed == "ENDHDR" {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("WIDTH ") {
                w = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("HEIGHT ") {
                h = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("DEPTH ") {
                depth = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("MAXVAL ") {
                maxval = rest.trim().parse::<u32>().unwrap_or(0);
            }
        }

        if depth != 4 || maxval != 255 {
            return Err(format!(
                "Invalid PAM RGBA frame descriptor ({w}x{h}, depth {depth}, maxval {maxval})."
            ));
        }
        if stream_w != 0 && (w != stream_w || h != stream_h) {
            return Err(format!(
                "Video dimensions changed during decode ({}x{} to {w}x{h}).",
                stream_w, stream_h
            ));
        }
        let (payload_len, _) =
            checked_decoded_video_payload_len(w, h, images.len().saturating_add(1))?;
        let mut rgba = vec![0u8; payload_len];
        reader
            .read_exact(&mut rgba)
            .map_err(|e| format!("Truncated PAM video frame payload from ffmpeg: {e}"))?;
        if stream_w == 0 {
            stream_w = w;
            stream_h = h;
        }
        images.push(Arc::new(DecodedImage {
            width: w,
            height: h,
            rgba: rgba.into(),
        }));
    }
    if images.is_empty() {
        return Err("Video stream produced 0 decodable frames.".into());
    }

    Ok(PamFrameSet {
        width: stream_w,
        height: stream_h,
        images,
    })
}

pub(super) fn finish_pam_rgba_frames(
    raw: PamFrameSet,
    fps: f32,
) -> Result<DecodedVideoStream, String> {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        24.0
    };
    let frame_dur_ms = ((1000.0 / f64::from(safe_fps)).round() as u32).max(1);
    let frames = raw
        .images
        .into_iter()
        .enumerate()
        .map(|(idx, image)| DecodedVideoFrame {
            timestamp_ms: ((idx as f64 * 1000.0) / f64::from(safe_fps)).round() as u32,
            duration_ms: frame_dur_ms,
            image,
        })
        .collect::<Vec<_>>();
    let duration_ms = checked_decoded_video_duration_ms(frames.len(), safe_fps)?;
    Ok(DecodedVideoStream {
        width: raw.width,
        height: raw.height,
        fps: safe_fps,
        duration_ms,
        frames,
        on_demand: None,
    })
}

#[derive(Debug)]
pub(super) struct StagedMediaFile {
    path: PathBuf,
}

impl StagedMediaFile {
    pub(super) fn create(name: &str, extension: &str, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_VIDEO_INPUT_BYTES {
            return Err(format!(
                "Temporary media '{name}' is {} bytes, exceeding the {} MiB staging budget.",
                bytes.len(),
                MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
            ));
        }
        let epoch_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..64 {
            let sequence = VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "neoflash-media-stage-{}-{epoch_nanos}-{sequence}.{extension}",
                std::process::id()
            ));
            let mut file = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "Failed to create a secure temporary media file for '{name}': {error}"
                    ));
                }
            };
            let staged = Self { path };
            file.write_all(bytes).map_err(|error| {
                format!("Failed to stage temporary media file for '{name}': {error}")
            })?;
            file.flush().map_err(|error| {
                format!("Failed to flush temporary media file for '{name}': {error}")
            })?;
            return Ok(staged);
        }
        Err(format!(
            "Failed to allocate a unique temporary media file for '{name}' after 64 attempts."
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedMediaFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
pub(super) struct BoundedProcessDiagnostics {
    pub(super) head: Vec<u8>,
    pub(super) tail: Vec<u8>,
}

pub(super) fn read_bounded_process_diagnostics(
    mut reader: impl Read,
) -> std::io::Result<BoundedProcessDiagnostics> {
    let mut head = Vec::with_capacity(MAX_FFMPEG_DIAGNOSTIC_BYTES);
    let mut tail = Vec::with_capacity(MAX_FFMPEG_DIAGNOSTIC_BYTES);
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if head.len() < MAX_FFMPEG_DIAGNOSTIC_BYTES {
            let retain = (MAX_FFMPEG_DIAGNOSTIC_BYTES - head.len()).min(read);
            head.extend_from_slice(&chunk[..retain]);
        }
        let overflow = tail
            .len()
            .saturating_add(read)
            .saturating_sub(MAX_FFMPEG_DIAGNOSTIC_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(&chunk[..read]);
    }
    Ok(BoundedProcessDiagnostics { head, tail })
}
