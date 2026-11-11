//! Sequential, bounded RIFF/AVI container inspection and pure-Rust MJPEG/DIB/PCM decoding (`T-247`).
//!
//! Decodes standard RIFF `AVI ` containers (`hdrl`/`avih`, `strl`/`strh`/`strf`, `movi` and nested
//! `rec ` lists) sequentially without byte scanning or resynchronization. Supports pure-Rust frame
//! decoding for MJPEG (`MJPG`/`JPEG`), PNG (`PNG `), and uncompressed DIB (`BI_RGB` 8/24/32-bit),
//! plus PCM (`0x0001`) WAV audio extraction for native Stage preview, export, and HTML5 transcode.

use crate::MAX_BITMAP_DIMENSION;
use crate::resource_image::{DecodedImage, decode_encoded_image};
use std::sync::Arc;

const MAX_AVI_BYTES: usize = 512 * 1024 * 1024;
const MAX_AVI_FRAMES: usize = 36_000;
const MAX_AVI_DECODED_BYTES: usize = 256 * 1024 * 1024;
const MAX_AVI_REC_DEPTH: usize = 16;

/// Parsed container and elementary-stream metadata from an AVI `hdrl` list.
#[derive(Clone, Debug, PartialEq)]
pub struct AviHeaderInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frame_count: u32,
    pub video_fourcc: [u8; 4],
    pub bit_count: u16,
    pub bottom_up: bool,
    pub video_stream_index: u8,
    pub audio_stream_index: Option<u8>,
    pub audio_format_tag: Option<u16>,
    pub audio_channels: u16,
    pub audio_sample_rate: u32,
    pub audio_block_align: u16,
    pub audio_bits_per_sample: u16,
}

impl AviHeaderInfo {
    pub fn video_fourcc_label(&self) -> String {
        if self.video_fourcc == [0, 0, 0, 0] {
            return "BI_RGB".to_owned();
        }
        if self.video_fourcc.iter().all(|b| (0x20..=0x7E).contains(b)) {
            let trimmed = std::str::from_utf8(&self.video_fourcc).unwrap_or("").trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
        format!(
            "0x{:02X}{:02X}{:02X}{:02X}",
            self.video_fourcc[0], self.video_fourcc[1], self.video_fourcc[2], self.video_fourcc[3]
        )
    }

    pub fn duration_ms(&self) -> u32 {
        if self.fps <= 0.0 || self.frame_count == 0 {
            return 0;
        }
        (((self.frame_count as f64 / self.fps) * 1000.0).round() as u64).min(u32::MAX as u64) as u32
    }

    pub fn supports_pure_rust_video(&self) -> bool {
        let fcc_upper = [
            self.video_fourcc[0].to_ascii_uppercase(),
            self.video_fourcc[1].to_ascii_uppercase(),
            self.video_fourcc[2].to_ascii_uppercase(),
            self.video_fourcc[3].to_ascii_uppercase(),
        ];
        match &fcc_upper {
            b"MJPG" | b"JPEG" | b"PNG " | b"MPNG" => true,
            b"\0\0\0\0" | b"DIB " | b"RGB " | b"RAW " => {
                matches!(self.bit_count, 8 | 24 | 32)
            }
            _ => false,
        }
    }
}

/// Byte-offset index entry for a single frame inside an AVI `movi` list (`T-259` / `T-229`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AviFrameEntry {
    pub timestamp_ms: u32,
    /// `Some((start, end))` into the container byte slice (resolved across 0-byte drop/hold chunks).
    pub chunk_range: Option<(usize, usize)>,
}

/// Sequential chunk-offset index of an AVI container for $O(1)$ on-demand frame decoding.
#[derive(Clone, Debug)]
pub struct AviIndexedContainer {
    pub info: AviHeaderInfo,
    palette: Vec<[u8; 4]>,
    is_encoded_image: bool,
    pub frames: Vec<AviFrameEntry>,
    audio_ranges: Vec<(usize, usize)>,
}

/// Decoded video frames (`(timestamp_ms, DecodedImage)`) and optional canonical WAV/MP3 audio bytes.
#[derive(Clone, Debug)]
pub struct DecodedAviMedia {
    pub info: AviHeaderInfo,
    pub frames: Vec<(u32, Arc<DecodedImage>)>,
    pub audio_bytes: Option<Vec<u8>>,
    pub audio_mime: Option<&'static str>,
}

/// Returns `true` when `bytes` begins with a RIFF `AVI ` or `AVIX` signature.
pub fn is_avi_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && (&bytes[8..12] == b"AVI " || &bytes[8..12] == b"AVIX")
}

fn read_u16_le(buf: &[u8], off: usize) -> Option<u16> {
    let slice = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    let slice = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_i32_le(buf: &[u8], off: usize) -> Option<i32> {
    let slice = buf.get(off..off + 4)?;
    Some(i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Inspect an AVI container's `hdrl` list sequentially without decoding `movi` payloads.
pub fn inspect_avi_header(bytes: &[u8]) -> Result<AviHeaderInfo, String> {
    let (info, _, _) = parse_avi_structure(bytes)?;
    Ok(info)
}

#[allow(clippy::type_complexity)]
fn parse_avi_structure(
    bytes: &[u8],
) -> Result<(AviHeaderInfo, Vec<[u8; 4]>, Option<(usize, usize)>), String> {
    if bytes.len() > MAX_AVI_BYTES {
        return Err(format!(
            "AVI container exceeds the {} MiB input budget.",
            MAX_AVI_BYTES / (1024 * 1024)
        ));
    }
    if !is_avi_bytes(bytes) {
        return Err("Missing RIFF AVI header signature.".into());
    }
    let riff_size = read_u32_le(bytes, 4).ok_or("Truncated RIFF size")? as usize;
    let riff_end = (8usize.saturating_add(riff_size)).min(bytes.len());

    let mut avih_width = 0u32;
    let mut avih_height = 0u32;
    let mut micro_sec_per_frame = 0u32;
    let mut total_frames = 0u32;

    let mut found_video = false;
    let mut video_stream_index = 0u8;
    let mut video_fourcc = [0u8; 4];
    let mut video_scale = 0u32;
    let mut video_rate = 0u32;
    let mut video_length = 0u32;
    let mut bih_width = 0u32;
    let mut bih_height = 0u32;
    let mut bit_count = 24u16;
    let mut bottom_up = true;
    let mut palette: Vec<[u8; 4]> = Vec::new();

    let mut audio_stream_index: Option<u8> = None;
    let mut audio_format_tag: Option<u16> = None;
    let mut audio_channels = 0u16;
    let mut audio_sample_rate = 0u32;
    let mut audio_block_align = 0u16;
    let mut audio_bits_per_sample = 0u16;

    let mut movi_range: Option<(usize, usize)> = None;

    let mut pos = 12usize;
    while pos + 8 <= riff_end {
        let id = &bytes[pos..pos + 4];
        let chunk_len = read_u32_le(bytes, pos + 4).ok_or("Truncated AVI chunk header")? as usize;
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(chunk_len)
            .ok_or("AVI chunk size overflow")?;
        if data_end > riff_end {
            break;
        }
        if id == b"LIST" && chunk_len >= 4 {
            let list_type = &bytes[data_start..data_start + 4];
            if list_type == b"hdrl" {
                let mut hpos = data_start + 4;
                let mut stream_idx: u8 = 0;
                while hpos + 8 <= data_end {
                    let hid = &bytes[hpos..hpos + 4];
                    let hlen = read_u32_le(bytes, hpos + 4).ok_or("Truncated hdrl chunk")? as usize;
                    let hstart = hpos + 8;
                    let hend = hstart.checked_add(hlen).ok_or("hdrl chunk overflow")?;
                    if hend > data_end {
                        break;
                    }
                    if hid == b"avih" && hlen >= 40 {
                        micro_sec_per_frame = read_u32_le(bytes, hstart).unwrap_or(0);
                        total_frames = read_u32_le(bytes, hstart + 16).unwrap_or(0);
                        avih_width = read_u32_le(bytes, hstart + 32).unwrap_or(0);
                        avih_height = read_u32_le(bytes, hstart + 36).unwrap_or(0);
                    } else if hid == b"LIST" && hlen >= 4 && &bytes[hstart..hstart + 4] == b"strl" {
                        let mut spos = hstart + 4;
                        let mut current_fcc_type = [0u8; 4];
                        let mut strh_handler = [0u8; 4];
                        while spos + 8 <= hend {
                            let sid = &bytes[spos..spos + 4];
                            let slen = read_u32_le(bytes, spos + 4).ok_or("Truncated strl chunk")?
                                as usize;
                            let sstart = spos + 8;
                            let send = sstart.checked_add(slen).ok_or("strl chunk overflow")?;
                            if send > hend {
                                break;
                            }
                            if sid == b"strh" && slen >= 36 {
                                current_fcc_type.copy_from_slice(&bytes[sstart..sstart + 4]);
                                strh_handler.copy_from_slice(&bytes[sstart + 4..sstart + 8]);
                                if &current_fcc_type == b"vids" && !found_video {
                                    video_scale = read_u32_le(bytes, sstart + 20).unwrap_or(0);
                                    video_rate = read_u32_le(bytes, sstart + 24).unwrap_or(0);
                                    video_length = read_u32_le(bytes, sstart + 32).unwrap_or(0);
                                }
                            } else if sid == b"strf" {
                                if &current_fcc_type == b"vids" && !found_video && slen >= 40 {
                                    found_video = true;
                                    video_stream_index = stream_idx;
                                    let bi_size = read_u32_le(bytes, sstart).unwrap_or(0) as usize;
                                    if bi_size < 40 {
                                        return Err(format!(
                                            "AVI video BITMAPINFOHEADER biSize ({bi_size}) must be at least 40 bytes."
                                        ));
                                    }
                                    let w_raw = read_i32_le(bytes, sstart + 4).unwrap_or(0);
                                    if w_raw <= 0 {
                                        return Err(format!(
                                            "AVI video BITMAPINFOHEADER biWidth ({w_raw}) must be positive."
                                        ));
                                    }
                                    let h_raw = read_i32_le(bytes, sstart + 8).unwrap_or(0);
                                    if h_raw == 0 || h_raw == i32::MIN {
                                        return Err(format!(
                                            "AVI video BITMAPINFOHEADER biHeight ({h_raw}) is invalid."
                                        ));
                                    }
                                    bih_width = w_raw as u32;
                                    bih_height = h_raw.unsigned_abs();
                                    bottom_up = h_raw >= 0;
                                    bit_count = read_u16_le(bytes, sstart + 14).unwrap_or(24);
                                    video_fourcc.copy_from_slice(&bytes[sstart + 16..sstart + 20]);
                                    if video_fourcc == [0, 0, 0, 0]
                                        && strh_handler != [0, 0, 0, 0]
                                        && &strh_handler != b"DIB "
                                    {
                                        // Keep BI_RGB (`[0,0,0,0]`) when strf compression is 0.
                                    }
                                    if bit_count == 8 && slen > bi_size && sstart + bi_size <= send
                                    {
                                        let pal_bytes = &bytes[sstart + bi_size..send];
                                        for entry in pal_bytes.as_chunks::<4>().0.iter().take(256) {
                                            palette.push([entry[2], entry[1], entry[0], 255]);
                                        }
                                    }
                                } else if &current_fcc_type == b"auds"
                                    && audio_stream_index.is_none()
                                    && slen >= 14
                                {
                                    audio_stream_index = Some(stream_idx);
                                    audio_format_tag = read_u16_le(bytes, sstart);
                                    audio_channels = read_u16_le(bytes, sstart + 2).unwrap_or(1);
                                    audio_sample_rate =
                                        read_u32_le(bytes, sstart + 4).unwrap_or(44_100);
                                    audio_block_align =
                                        read_u16_le(bytes, sstart + 12).unwrap_or(2);
                                    audio_bits_per_sample = if slen >= 16 {
                                        read_u16_le(bytes, sstart + 14).unwrap_or(16)
                                    } else {
                                        16
                                    };
                                }
                            }
                            spos = send + (slen & 1);
                        }
                        stream_idx = stream_idx.saturating_add(1);
                    }
                    hpos = hend + (hlen & 1);
                }
            } else if list_type == b"movi" && movi_range.is_none() {
                movi_range = Some((data_start + 4, data_end));
            }
        }
        pos = data_end + (chunk_len & 1);
    }

    if !found_video {
        return Err("AVI container has no video ('vids') stream header.".into());
    }
    let width = if bih_width > 0 { bih_width } else { avih_width };
    let height = if bih_height > 0 {
        bih_height
    } else {
        avih_height
    };
    if width == 0 || height == 0 || width > MAX_BITMAP_DIMENSION || height > MAX_BITMAP_DIMENSION {
        return Err(format!(
            "AVI video dimensions {width}x{height} must be within 1..={MAX_BITMAP_DIMENSION}."
        ));
    }

    let fps = if video_scale > 0 && video_rate > 0 {
        (video_rate as f64) / (video_scale as f64)
    } else if micro_sec_per_frame > 0 {
        1_000_000.0 / (micro_sec_per_frame as f64)
    } else {
        24.0
    }
    .clamp(0.1, 240.0);

    let frame_count = if video_length > 0 {
        video_length
    } else {
        total_frames
    };

    Ok((
        AviHeaderInfo {
            width,
            height,
            fps,
            frame_count,
            video_fourcc,
            bit_count,
            bottom_up,
            video_stream_index,
            audio_stream_index,
            audio_format_tag,
            audio_channels,
            audio_sample_rate,
            audio_block_align,
            audio_bits_per_sample,
        },
        palette,
        movi_range,
    ))
}

fn decode_dib_frame(
    chunk: &[u8],
    info: &AviHeaderInfo,
    palette: &[[u8; 4]],
) -> Result<DecodedImage, String> {
    let w = info.width as usize;
    let h = info.height as usize;
    let frame_rgba_bytes = w
        .checked_mul(h)
        .and_then(|px| px.checked_mul(4))
        .ok_or("AVI DIB frame dimensions overflow.")?;
    if frame_rgba_bytes > MAX_AVI_DECODED_BYTES {
        return Err(format!(
            "Decoded AVI DIB frame exceeds {} MiB frame budget.",
            MAX_AVI_DECODED_BYTES / (1024 * 1024)
        ));
    }
    match info.bit_count {
        24 => {
            let stride = (w * 3).div_ceil(4) * 4;
            if chunk.len() < stride.saturating_mul(h) {
                return Err("Truncated 24-bit DIB video frame in AVI.".into());
            }
            let mut rgba = vec![0u8; frame_rgba_bytes];
            for y in 0..h {
                let src_y = if info.bottom_up { h - 1 - y } else { y };
                let row = &chunk[src_y * stride..src_y * stride + w * 3];
                let dst_row = &mut rgba[y * w * 4..(y + 1) * w * 4];
                for x in 0..w {
                    dst_row[x * 4] = row[x * 3 + 2];
                    dst_row[x * 4 + 1] = row[x * 3 + 1];
                    dst_row[x * 4 + 2] = row[x * 3];
                    dst_row[x * 4 + 3] = 255;
                }
            }
            Ok(DecodedImage {
                width: info.width,
                height: info.height,
                rgba: rgba.into(),
            })
        }
        32 => {
            let stride = w * 4;
            if chunk.len() < stride.saturating_mul(h) {
                return Err("Truncated 32-bit DIB video frame in AVI.".into());
            }
            let mut rgba = vec![0u8; frame_rgba_bytes];
            for y in 0..h {
                let src_y = if info.bottom_up { h - 1 - y } else { y };
                let row = &chunk[src_y * stride..src_y * stride + stride];
                let dst_row = &mut rgba[y * w * 4..(y + 1) * w * 4];
                for x in 0..w {
                    dst_row[x * 4] = row[x * 4 + 2];
                    dst_row[x * 4 + 1] = row[x * 4 + 1];
                    dst_row[x * 4 + 2] = row[x * 4];
                    // Many legacy 32-bit BI_RGB AVI writers leave alpha 0x00; preserve alpha if any pixel has non-zero alpha, else opaque.
                    dst_row[x * 4 + 3] = row[x * 4 + 3];
                }
            }
            if rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
                for px in rgba.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
            Ok(DecodedImage {
                width: info.width,
                height: info.height,
                rgba: rgba.into(),
            })
        }
        8 => {
            let stride = w.div_ceil(4) * 4;
            if chunk.len() < stride.saturating_mul(h) {
                return Err("Truncated 8-bit paletted DIB video frame in AVI.".into());
            }
            let mut rgba = vec![0u8; frame_rgba_bytes];
            for y in 0..h {
                let src_y = if info.bottom_up { h - 1 - y } else { y };
                let row = &chunk[src_y * stride..src_y * stride + w];
                let dst_row = &mut rgba[y * w * 4..(y + 1) * w * 4];
                for (x, &idx) in row.iter().enumerate() {
                    let color = palette
                        .get(idx as usize)
                        .copied()
                        .unwrap_or([idx, idx, idx, 255]);
                    dst_row[x * 4..x * 4 + 4].copy_from_slice(&color);
                }
            }
            Ok(DecodedImage {
                width: info.width,
                height: info.height,
                rgba: rgba.into(),
            })
        }
        other => Err(format!(
            "Unsupported uncompressed AVI DIB bit depth {other} (expected 8, 24 or 32)."
        )),
    }
}

fn stream_chunk_index(two_digits: &[u8]) -> Option<u8> {
    if two_digits.len() != 2 {
        return None;
    }
    let hi = match two_digits[0] {
        b'0'..=b'9' => two_digits[0] - b'0',
        b'a'..=b'f' => two_digits[0] - b'a' + 10,
        b'A'..=b'F' => two_digits[0] - b'A' + 10,
        _ => return None,
    };
    let lo = match two_digits[1] {
        b'0'..=b'9' => two_digits[1] - b'0',
        b'a'..=b'f' => two_digits[1] - b'a' + 10,
        b'A'..=b'F' => two_digits[1] - b'A' + 10,
        _ => return None,
    };
    Some((hi << 4) | lo)
}

impl AviIndexedContainer {
    /// Binary-search the indexed frame entry active at `timestamp_ms`.
    pub fn frame_index_at_ms(&self, timestamp_ms: u32) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        let idx = self
            .frames
            .partition_point(|entry| entry.timestamp_ms <= timestamp_ms);
        Some(idx.saturating_sub(1).min(self.frames.len() - 1))
    }

    /// Decode a single indexed video frame on demand without touching any other frame (`T-229` / `T-259`).
    pub fn decode_frame(&self, bytes: &[u8], frame_index: usize) -> Result<DecodedImage, String> {
        let entry = self
            .frames
            .get(frame_index)
            .ok_or_else(|| format!("AVI frame index {frame_index} is out of bounds."))?;
        let (cstart, cend) = entry
            .chunk_range
            .ok_or_else(|| format!("AVI frame {frame_index} has no backing video payload."))?;
        let chunk = bytes
            .get(cstart..cend)
            .ok_or_else(|| format!("AVI frame {frame_index} slice is out of bounds."))?;
        if self.is_encoded_image {
            decode_encoded_image(chunk)
                .map_err(|e| format!("AVI frame {frame_index} decode failed: {e}"))
        } else {
            decode_dib_frame(chunk, &self.info, &self.palette)
        }
    }

    /// Extract canonical WAV (`0x0001` PCM) or MP3 (`0x0055`) audio bytes from the indexed chunks.
    pub fn extract_audio(
        &self,
        bytes: &[u8],
    ) -> Result<(Option<Vec<u8>>, Option<&'static str>), String> {
        if self.audio_ranges.is_empty() {
            return Ok((None, None));
        }
        let total_len: usize = self
            .audio_ranges
            .iter()
            .map(|(s, e)| e.saturating_sub(*s))
            .sum();
        if total_len == 0 {
            return Ok((None, None));
        }
        let mut payload = Vec::with_capacity(total_len);
        for &(s, e) in &self.audio_ranges {
            if let Some(slice) = bytes.get(s..e) {
                payload.extend_from_slice(slice);
            }
        }
        match self.info.audio_format_tag {
            Some(1) => {
                let wav = wrap_pcm_as_wav(
                    &payload,
                    self.info.audio_channels,
                    self.info.audio_sample_rate,
                    self.info.audio_bits_per_sample,
                )?;
                Ok((Some(wav), Some("audio/wav")))
            }
            Some(0x0055) => Ok((Some(payload), Some("audio/mpeg"))),
            _ => Ok((None, None)),
        }
    }
}

/// Index an AVI container's `movi` frame and audio chunk byte offsets sequentially without
/// decoding video pixels (`T-259` / `T-229`).
pub fn index_avi_container(bytes: &[u8]) -> Result<AviIndexedContainer, String> {
    let (mut info, palette, movi_range) = parse_avi_structure(bytes)?;
    if !info.supports_pure_rust_video() {
        return Err(format!(
            "AVI codec '{}' ({}-bit) requires external ffmpeg decoding.",
            info.video_fourcc_label(),
            info.bit_count
        ));
    }
    let (movi_start, movi_end) =
        movi_range.ok_or_else(|| "AVI container is missing its 'movi' data list.".to_owned())?;

    let fcc_upper = [
        info.video_fourcc[0].to_ascii_uppercase(),
        info.video_fourcc[1].to_ascii_uppercase(),
        info.video_fourcc[2].to_ascii_uppercase(),
        info.video_fourcc[3].to_ascii_uppercase(),
    ];
    let is_encoded_image = matches!(&fcc_upper, b"MJPG" | b"JPEG" | b"PNG " | b"MPNG");

    let mut frames: Vec<AviFrameEntry> = Vec::new();
    let mut last_non_empty_range: Option<(usize, usize)> = None;
    let mut audio_ranges: Vec<(usize, usize)> = Vec::new();
    let mut audio_total_bytes = 0usize;

    #[allow(clippy::too_many_arguments)]
    fn walk_movi_chunks(
        bytes: &[u8],
        start: usize,
        end: usize,
        depth: usize,
        info: &AviHeaderInfo,
        frames: &mut Vec<AviFrameEntry>,
        last_non_empty_range: &mut Option<(usize, usize)>,
        audio_ranges: &mut Vec<(usize, usize)>,
        audio_total_bytes: &mut usize,
    ) -> Result<(), String> {
        if depth > MAX_AVI_REC_DEPTH {
            return Err(format!(
                "AVI 'rec ' list nesting exceeds maximum depth ({MAX_AVI_REC_DEPTH})."
            ));
        }
        let mut pos = start;
        while pos + 8 <= end {
            let id = &bytes[pos..pos + 4];
            let len = read_u32_le(bytes, pos + 4).ok_or("Truncated movi chunk")? as usize;
            let cstart = pos + 8;
            let cend = cstart.checked_add(len).ok_or("movi chunk overflow")?;
            if cend > end {
                break;
            }
            if id == b"LIST" && len >= 4 && &bytes[cstart..cstart + 4] == b"rec " {
                walk_movi_chunks(
                    bytes,
                    cstart + 4,
                    cend,
                    depth + 1,
                    info,
                    frames,
                    last_non_empty_range,
                    audio_ranges,
                    audio_total_bytes,
                )?;
            } else if let Some(sidx) = stream_chunk_index(&id[0..2]) {
                let kind = &id[2..4];
                if sidx == info.video_stream_index && (kind == b"dc" || kind == b"db") {
                    if frames.len() >= MAX_AVI_FRAMES {
                        return Err(format!(
                            "AVI stream exceeds {MAX_AVI_FRAMES} frames budget."
                        ));
                    }
                    let ts_ms = (((frames.len() as f64) * 1000.0 / info.fps).round() as u64)
                        .min(u32::MAX as u64) as u32;
                    if cstart == cend {
                        if let Some(prev_range) = *last_non_empty_range {
                            frames.push(AviFrameEntry {
                                timestamp_ms: ts_ms,
                                chunk_range: Some(prev_range),
                            });
                        }
                    } else {
                        *last_non_empty_range = Some((cstart, cend));
                        frames.push(AviFrameEntry {
                            timestamp_ms: ts_ms,
                            chunk_range: Some((cstart, cend)),
                        });
                    }
                } else if Some(sidx) == info.audio_stream_index
                    && kind == b"wb"
                    && cend > cstart
                    && audio_total_bytes.saturating_add(cend - cstart) <= 64 * 1024 * 1024
                {
                    *audio_total_bytes += cend - cstart;
                    audio_ranges.push((cstart, cend));
                }
            }
            pos = cend + (len & 1);
        }
        Ok(())
    }

    walk_movi_chunks(
        bytes,
        movi_start,
        movi_end,
        0,
        &info,
        &mut frames,
        &mut last_non_empty_range,
        &mut audio_ranges,
        &mut audio_total_bytes,
    )?;

    if frames.is_empty() {
        return Err("AVI container contains zero decodable video frames.".into());
    }
    if info.frame_count == 0 {
        info.frame_count = u32::try_from(frames.len()).unwrap_or(u32::MAX);
    }

    Ok(AviIndexedContainer {
        info,
        palette,
        is_encoded_image,
        frames,
        audio_ranges,
    })
}

/// Decode an AVI byte stream in pure Rust (`MJPEG`, `PNG`, or `BI_RGB` video + `PCM`/`MP3` audio).
pub fn decode_avi_bytes(bytes: &[u8]) -> Result<DecodedAviMedia, String> {
    let indexed = index_avi_container(bytes)?;
    let mut frames: Vec<(u32, Arc<DecodedImage>)> = Vec::with_capacity(indexed.frames.len());
    let mut decoded_rgba_bytes = 0usize;
    let mut prev_decoded: Option<((usize, usize), Arc<DecodedImage>)> = None;

    for (idx, entry) in indexed.frames.iter().enumerate() {
        if let Some(range) = entry.chunk_range
            && let Some((prev_range, ref prev_img)) = prev_decoded
            && prev_range == range
        {
            frames.push((entry.timestamp_ms, Arc::clone(prev_img)));
            continue;
        }
        let decoded = indexed.decode_frame(bytes, idx)?;
        decoded_rgba_bytes = decoded_rgba_bytes
            .checked_add(decoded.rgba.len())
            .ok_or("AVI decoded frame memory overflow")?;
        if decoded_rgba_bytes > MAX_AVI_DECODED_BYTES {
            return Err(format!(
                "Decoded AVI video exceeds {} MiB frame budget.",
                MAX_AVI_DECODED_BYTES / (1024 * 1024)
            ));
        }
        let arc = Arc::new(decoded);
        if let Some(range) = entry.chunk_range {
            prev_decoded = Some((range, Arc::clone(&arc)));
        }
        frames.push((entry.timestamp_ms, arc));
    }

    let (audio_bytes, audio_mime) = indexed.extract_audio(bytes)?;

    Ok(DecodedAviMedia {
        info: indexed.info,
        frames,
        audio_bytes,
        audio_mime,
    })
}

/// Wrap raw linear PCM bytes in a canonical RIFF `WAVE` container with validated channel,
/// sample-rate, bit-depth, and even-byte RIFF padding (`T-259` / `T-260`).
pub fn wrap_pcm_as_wav(
    pcm: &[u8],
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
) -> Result<Vec<u8>, String> {
    if !(1..=8).contains(&channels) {
        return Err(format!(
            "Unsupported PCM channel count {channels} (expected 1..=8)."
        ));
    }
    if !(1_000..=384_000).contains(&sample_rate) {
        return Err(format!(
            "Unsupported PCM sample rate {sample_rate} Hz (expected 1000..=384000)."
        ));
    }
    if !matches!(bits_per_sample, 8 | 16 | 24 | 32) {
        return Err(format!(
            "Unsupported PCM bit depth {bits_per_sample} (expected 8, 16, 24 or 32)."
        ));
    }
    let bytes_per_sample = bits_per_sample / 8;
    let block_align = channels.saturating_mul(bytes_per_sample);
    let byte_rate = sample_rate.saturating_mul(u32::from(block_align));
    let data_len = u32::try_from(pcm.len()).map_err(|_| "PCM audio exceeds 4 GiB WAV limit.")?;
    let pad = pcm.len() & 1;
    let padded_data_len = u32::try_from(pcm.len().saturating_add(pad))
        .map_err(|_| "PCM audio exceeds 4 GiB WAV limit.")?;
    let riff_len = 36u32
        .checked_add(padded_data_len)
        .ok_or("PCM audio exceeds RIFF WAVE size limit.")?;

    let mut out = Vec::with_capacity(44 + pcm.len() + pad);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    if pad == 1 {
        out.push(0);
    }
    Ok(out)
}

/// Build a canonical RIFF/AVI container with `MJPG` video frames and optional 16-bit PCM audio.
pub fn build_minimal_mjpeg_avi(
    width: u32,
    height: u32,
    fps: u32,
    jpeg_frames: &[Vec<u8>],
    pcm_mono_16le: Option<(&[i16], u32)>,
) -> Vec<u8> {
    let fps_clean = fps.max(1);
    let micro_sec = (1_000_000u32 / fps_clean).max(1);
    let n_frames = jpeg_frames.len() as u32;
    let has_audio = pcm_mono_16le.is_some();
    let streams = if has_audio { 2u32 } else { 1u32 };

    let mut avih = Vec::with_capacity(56);
    avih.extend_from_slice(&micro_sec.to_le_bytes());
    avih.extend_from_slice(&0u32.to_le_bytes()); // dwMaxBytesPerSec
    avih.extend_from_slice(&0u32.to_le_bytes()); // dwPaddingGranularity
    avih.extend_from_slice(&0x10u32.to_le_bytes()); // AVIF_HASINDEX
    avih.extend_from_slice(&n_frames.to_le_bytes());
    avih.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
    avih.extend_from_slice(&streams.to_le_bytes());
    avih.extend_from_slice(&0u32.to_le_bytes()); // dwSuggestedBufferSize
    avih.extend_from_slice(&width.to_le_bytes());
    avih.extend_from_slice(&height.to_le_bytes());
    avih.extend_from_slice(&[0u8; 16]); // dwReserved[4]

    let mut v_strh = Vec::with_capacity(56);
    v_strh.extend_from_slice(b"vids");
    v_strh.extend_from_slice(b"MJPG");
    v_strh.extend_from_slice(&0u32.to_le_bytes()); // dwFlags
    v_strh.extend_from_slice(&0u16.to_le_bytes()); // wPriority
    v_strh.extend_from_slice(&0u16.to_le_bytes()); // wLanguage
    v_strh.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
    v_strh.extend_from_slice(&1u32.to_le_bytes()); // dwScale
    v_strh.extend_from_slice(&fps_clean.to_le_bytes()); // dwRate
    v_strh.extend_from_slice(&0u32.to_le_bytes()); // dwStart
    v_strh.extend_from_slice(&n_frames.to_le_bytes()); // dwLength
    v_strh.extend_from_slice(&0u32.to_le_bytes()); // dwSuggestedBufferSize
    v_strh.extend_from_slice(&10_000u32.to_le_bytes()); // dwQuality
    v_strh.extend_from_slice(&0u32.to_le_bytes()); // dwSampleSize
    v_strh.extend_from_slice(&0u16.to_le_bytes()); // rcFrame.left
    v_strh.extend_from_slice(&0u16.to_le_bytes()); // rcFrame.top
    v_strh.extend_from_slice(&(width as u16).to_le_bytes());
    v_strh.extend_from_slice(&(height as u16).to_le_bytes());

    let mut v_strf = Vec::with_capacity(40);
    v_strf.extend_from_slice(&40u32.to_le_bytes());
    v_strf.extend_from_slice(&(width as i32).to_le_bytes());
    v_strf.extend_from_slice(&(height as i32).to_le_bytes());
    v_strf.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    v_strf.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
    v_strf.extend_from_slice(b"MJPG");
    v_strf.extend_from_slice(&(width * height * 3).to_le_bytes());
    v_strf.extend_from_slice(&[0u8; 16]);

    fn make_chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + payload.len() + (payload.len() & 1));
        out.extend_from_slice(id);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        if payload.len() & 1 == 1 {
            out.push(0);
        }
        out
    }

    fn make_list(list_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let len = 4 + body.len();
        let mut out = Vec::with_capacity(8 + len + (len & 1));
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(list_type);
        out.extend_from_slice(body);
        if len & 1 == 1 {
            out.push(0);
        }
        out
    }

    let mut v_strl_body = Vec::new();
    v_strl_body.extend_from_slice(&make_chunk(b"strh", &v_strh));
    v_strl_body.extend_from_slice(&make_chunk(b"strf", &v_strf));
    let v_strl = make_list(b"strl", &v_strl_body);

    let mut hdrl_body = Vec::new();
    hdrl_body.extend_from_slice(&make_chunk(b"avih", &avih));
    hdrl_body.extend_from_slice(&v_strl);

    let mut audio_pcm_bytes = Vec::new();
    if let Some((samples, sample_rate)) = pcm_mono_16le {
        for &s in samples {
            audio_pcm_bytes.extend_from_slice(&s.to_le_bytes());
        }
        let mut a_strh = Vec::with_capacity(56);
        a_strh.extend_from_slice(b"auds");
        a_strh.extend_from_slice(&[0u8; 4]);
        a_strh.extend_from_slice(&0u32.to_le_bytes());
        a_strh.extend_from_slice(&0u16.to_le_bytes());
        a_strh.extend_from_slice(&0u16.to_le_bytes());
        a_strh.extend_from_slice(&0u32.to_le_bytes());
        a_strh.extend_from_slice(&2u32.to_le_bytes()); // dwScale = blockAlign
        a_strh.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // dwRate = byteRate
        a_strh.extend_from_slice(&0u32.to_le_bytes());
        a_strh.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        a_strh.extend_from_slice(&0u32.to_le_bytes());
        a_strh.extend_from_slice(&10_000u32.to_le_bytes());
        a_strh.extend_from_slice(&2u32.to_le_bytes());
        a_strh.extend_from_slice(&[0u8; 8]);

        let mut a_strf = Vec::with_capacity(16);
        a_strf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        a_strf.extend_from_slice(&1u16.to_le_bytes()); // mono
        a_strf.extend_from_slice(&sample_rate.to_le_bytes());
        a_strf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        a_strf.extend_from_slice(&2u16.to_le_bytes());
        a_strf.extend_from_slice(&16u16.to_le_bytes());

        let mut a_strl_body = Vec::new();
        a_strl_body.extend_from_slice(&make_chunk(b"strh", &a_strh));
        a_strl_body.extend_from_slice(&make_chunk(b"strf", &a_strf));
        hdrl_body.extend_from_slice(&make_list(b"strl", &a_strl_body));
    }

    let hdrl = make_list(b"hdrl", &hdrl_body);

    let mut movi_body = Vec::new();
    for frame in jpeg_frames {
        movi_body.extend_from_slice(&make_chunk(b"00dc", frame));
    }
    if !audio_pcm_bytes.is_empty() {
        movi_body.extend_from_slice(&make_chunk(b"01wb", &audio_pcm_bytes));
    }
    let movi = make_list(b"movi", &movi_body);

    let riff_payload_len = 4 + hdrl.len() + movi.len();
    let mut riff = Vec::with_capacity(8 + riff_payload_len);
    riff.extend_from_slice(b"RIFF");
    riff.extend_from_slice(&(riff_payload_len as u32).to_le_bytes());
    riff.extend_from_slice(b"AVI ");
    riff.extend_from_slice(&hdrl);
    riff.extend_from_slice(&movi);
    riff
}

/// Probe ISO-BMFF (`.mp4` / `.m4v` / `.mov`) container duration in milliseconds from `moov/mvhd`
/// (versions `0` and `1`) without invoking external tools (`T-260`).
pub fn probe_mp4_duration_ms(bytes: &[u8]) -> Option<u32> {
    const MAX_MP4_BOX_DEPTH: usize = 8;
    fn scan_boxes(buf: &[u8], depth: usize) -> Option<u32> {
        if depth > MAX_MP4_BOX_DEPTH {
            return None;
        }
        let mut pos = 0usize;
        while pos + 8 <= buf.len() {
            let size32 = u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
            let box_type = &buf[pos + 4..pos + 8];
            let (header_len, box_len) = if size32 == 1 {
                if pos + 16 > buf.len() {
                    break;
                }
                let size64 = u64::from_be_bytes([
                    buf[pos + 8],
                    buf[pos + 9],
                    buf[pos + 10],
                    buf[pos + 11],
                    buf[pos + 12],
                    buf[pos + 13],
                    buf[pos + 14],
                    buf[pos + 15],
                ]);
                let Ok(size64) = usize::try_from(size64) else {
                    break;
                };
                (16usize, size64)
            } else if size32 == 0 {
                (8usize, buf.len() - pos)
            } else {
                (8usize, size32 as usize)
            };
            if box_len < header_len || box_len > buf.len() - pos {
                break;
            }
            let payload = &buf[pos + header_len..pos + box_len];
            if box_type == b"moov"
                && let Some(ms) = scan_boxes(payload, depth + 1)
            {
                return Some(ms);
            } else if box_type == b"mvhd" && payload.len() >= 20 {
                let version = payload[0];
                if version == 0 && payload.len() >= 20 {
                    let timescale =
                        u32::from_be_bytes([payload[12], payload[13], payload[14], payload[15]]);
                    let duration =
                        u32::from_be_bytes([payload[16], payload[17], payload[18], payload[19]]);
                    if timescale > 0 && duration > 0 {
                        let ms = (u64::from(duration) * 1000) / u64::from(timescale);
                        return Some(ms.min(u64::from(u32::MAX)) as u32);
                    }
                } else if version == 1 && payload.len() >= 32 {
                    let timescale =
                        u32::from_be_bytes([payload[20], payload[21], payload[22], payload[23]]);
                    let duration = u64::from_be_bytes([
                        payload[24],
                        payload[25],
                        payload[26],
                        payload[27],
                        payload[28],
                        payload[29],
                        payload[30],
                        payload[31],
                    ]);
                    if timescale > 0 && duration > 0 {
                        let ms = duration.saturating_mul(1000) / u64::from(timescale);
                        return Some(ms.min(u64::from(u32::MAX)) as u32);
                    }
                }
            }
            pos += box_len;
        }
        None
    }
    scan_boxes(bytes, 0)
}

/// Probe RIFF `WAVE` container duration in milliseconds from `fmt ` + `data` chunks (`T-260`).
pub fn probe_wav_duration_ms(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut byte_rate = 0u32;
    let mut data_bytes = 0u64;
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let chunk_len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.saturating_add(chunk_len).min(bytes.len());
        let payload = &bytes[payload_start..payload_end];
        if id == b"fmt " && payload.len() >= 12 {
            byte_rate = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
        } else if id == b"data" {
            data_bytes = data_bytes.saturating_add(payload.len() as u64);
        }
        let step = chunk_len.saturating_add(chunk_len & 1);
        let Some(next) = payload_start.checked_add(step) else {
            break;
        };
        if next <= pos {
            break;
        }
        pos = next;
    }
    if byte_rate == 0 || data_bytes == 0 {
        return None;
    }
    let ms = data_bytes.saturating_mul(1000) / u64::from(byte_rate);
    Some(ms.min(u64::from(u32::MAX)) as u32)
}

/// Probe RIFF `AVI` or ISO-BMFF `MP4`/`MOV` video container duration in milliseconds (`T-260`).
pub fn probe_video_bytes_duration_ms(bytes: &[u8]) -> Option<u32> {
    if is_avi_bytes(bytes) {
        return inspect_avi_header(bytes)
            .ok()
            .map(|info| info.duration_ms())
            .filter(|ms| *ms > 0);
    }
    probe_mp4_duration_ms(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    #[test]
    fn mjpeg_avi_with_pcm_audio_roundtrips_pure_rust_decode() {
        let mut f0 = Vec::new();
        RgbImage::from_pixel(16, 12, Rgb([220, 40, 30]))
            .write_to(&mut Cursor::new(&mut f0), ImageFormat::Jpeg)
            .unwrap();
        let mut f1 = Vec::new();
        RgbImage::from_pixel(16, 12, Rgb([20, 180, 90]))
            .write_to(&mut Cursor::new(&mut f1), ImageFormat::Jpeg)
            .unwrap();

        let pcm = vec![0i16, 4000, -4000, 8000, -8000, 0];
        let avi = build_minimal_mjpeg_avi(16, 12, 10, &[f0, f1], Some((&pcm, 22_050)));
        assert!(is_avi_bytes(&avi));

        let info = inspect_avi_header(&avi).expect("valid avi header");
        assert_eq!(info.width, 16);
        assert_eq!(info.height, 12);
        assert!((info.fps - 10.0).abs() < 1e-6);
        assert_eq!(info.frame_count, 2);
        assert_eq!(info.video_fourcc_label(), "MJPG");
        assert!(info.supports_pure_rust_video());

        let decoded = decode_avi_bytes(&avi).expect("pure rust avi decode");
        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].0, 0);
        assert_eq!(decoded.frames[1].0, 100);
        assert_eq!(decoded.frames[0].1.width, 16);
        assert_eq!(decoded.frames[0].1.height, 12);
        let wav = decoded.audio_bytes.expect("wav extracted");
        assert!(wav.starts_with(b"RIFF") && &wav[8..12] == b"WAVE");
        assert_eq!(decoded.audio_mime, Some("audio/wav"));
        assert!(probe_wav_duration_ms(&wav).is_some());
        assert_eq!(probe_video_bytes_duration_ms(&avi), Some(200));
    }

    #[test]
    fn mp4_mvhd_and_project_freshen_helpers_preserve_durations_and_references() {
        let mut mvhd = vec![0u8; 24];
        mvhd[0] = 0; // version 0
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes()); // timescale 1000
        mvhd[16..20].copy_from_slice(&2500u32.to_be_bytes()); // duration 2500 -> 2500ms
        let mut moov = Vec::new();
        moov.extend_from_slice(&((8 + mvhd.len()) as u32).to_be_bytes());
        moov.extend_from_slice(b"mvhd");
        moov.extend_from_slice(&mvhd);
        let mut mp4 = Vec::new();
        mp4.extend_from_slice(&((8 + moov.len()) as u32).to_be_bytes());
        mp4.extend_from_slice(b"moov");
        mp4.extend_from_slice(&moov);
        assert_eq!(probe_mp4_duration_ms(&mp4), Some(2500));
        assert_eq!(probe_video_bytes_duration_ms(&mp4), Some(2500));

        let mut proj = crate::Project::demo();
        let orig_root = proj.root.clone();
        let mut seq = 0u32;
        proj.freshen_all_ids(true, |kind| {
            seq += 1;
            format!("fresh-{kind}-{seq}")
        });
        assert_ne!(proj.root, orig_root);
        assert!(proj.validate().is_ok());
    }
}
