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
        String::from_utf8_lossy(&self.video_fourcc)
            .trim()
            .to_owned()
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
        matches!(
            &fcc_upper,
            b"\0\0\0\0" | b"DIB " | b"RGB " | b"RAW " | b"MJPG" | b"JPEG" | b"PNG " | b"MPNG"
        )
    }
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
                                    let bi_size = read_u32_le(bytes, sstart).unwrap_or(40) as usize;
                                    let w_raw = read_i32_le(bytes, sstart + 4).unwrap_or(0);
                                    let h_raw = read_i32_le(bytes, sstart + 8).unwrap_or(0);
                                    bih_width = w_raw.unsigned_abs();
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
                                    if bit_count == 8 && slen > bi_size {
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
    let mut rgba = vec![0u8; w * h * 4];
    match info.bit_count {
        24 => {
            let stride = (w * 3).div_ceil(4) * 4;
            if chunk.len() < stride * h {
                return Err("Truncated 24-bit DIB video frame in AVI.".into());
            }
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
        }
        32 => {
            let stride = w * 4;
            if chunk.len() < stride * h {
                return Err("Truncated 32-bit DIB video frame in AVI.".into());
            }
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
        }
        8 => {
            let stride = w.div_ceil(4) * 4;
            if chunk.len() < stride * h {
                return Err("Truncated 8-bit paletted DIB video frame in AVI.".into());
            }
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
        }
        other => {
            return Err(format!(
                "Unsupported uncompressed AVI DIB bit depth {other} (expected 8, 24 or 32)."
            ));
        }
    }
    Ok(DecodedImage {
        width: info.width,
        height: info.height,
        rgba: rgba.into(),
    })
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

/// Decode an AVI byte stream in pure Rust (`MJPEG`, `PNG`, or `BI_RGB` video + `PCM`/`MP3` audio).
pub fn decode_avi_bytes(bytes: &[u8]) -> Result<DecodedAviMedia, String> {
    let (info, palette, movi_range) = parse_avi_structure(bytes)?;
    if !info.supports_pure_rust_video() {
        return Err(format!(
            "AVI codec '{}' requires external ffmpeg decoding.",
            info.video_fourcc_label()
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

    let mut frames: Vec<(u32, Arc<DecodedImage>)> = Vec::new();
    let mut audio_payload = Vec::new();
    let mut decoded_rgba_bytes = 0usize;

    #[allow(clippy::too_many_arguments)]
    fn walk_movi_chunks(
        bytes: &[u8],
        start: usize,
        end: usize,
        info: &AviHeaderInfo,
        palette: &[[u8; 4]],
        is_encoded_image: bool,
        frames: &mut Vec<(u32, Arc<DecodedImage>)>,
        audio_payload: &mut Vec<u8>,
        decoded_rgba_bytes: &mut usize,
    ) -> Result<(), String> {
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
                    info,
                    palette,
                    is_encoded_image,
                    frames,
                    audio_payload,
                    decoded_rgba_bytes,
                )?;
            } else if let Some(sidx) = stream_chunk_index(&id[0..2]) {
                let kind = &id[2..4];
                let chunk = &bytes[cstart..cend];
                if sidx == info.video_stream_index && (kind == b"dc" || kind == b"db") {
                    if frames.len() >= MAX_AVI_FRAMES {
                        return Err(format!(
                            "AVI stream exceeds {MAX_AVI_FRAMES} frames budget."
                        ));
                    }
                    let ts_ms = (((frames.len() as f64) * 1000.0 / info.fps).round() as u64)
                        .min(u32::MAX as u64) as u32;
                    if chunk.is_empty() {
                        // Repeat previous frame (0-byte drop/hold chunk in AVI).
                        if let Some((_, prev)) = frames.last().cloned() {
                            frames.push((ts_ms, prev));
                        }
                    } else {
                        let decoded = if is_encoded_image {
                            decode_encoded_image(chunk).map_err(|e| {
                                format!("AVI frame {} decode failed: {e}", frames.len())
                            })?
                        } else {
                            decode_dib_frame(chunk, info, palette)?
                        };
                        *decoded_rgba_bytes = decoded_rgba_bytes
                            .checked_add(decoded.rgba.len())
                            .ok_or("AVI decoded frame memory overflow")?;
                        if *decoded_rgba_bytes > MAX_AVI_DECODED_BYTES {
                            return Err(format!(
                                "Decoded AVI video exceeds {} MiB frame budget.",
                                MAX_AVI_DECODED_BYTES / (1024 * 1024)
                            ));
                        }
                        frames.push((ts_ms, Arc::new(decoded)));
                    }
                } else if Some(sidx) == info.audio_stream_index
                    && kind == b"wb"
                    && audio_payload.len().saturating_add(chunk.len()) <= 64 * 1024 * 1024
                {
                    audio_payload.extend_from_slice(chunk);
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
        &info,
        &palette,
        is_encoded_image,
        &mut frames,
        &mut audio_payload,
        &mut decoded_rgba_bytes,
    )?;

    if frames.is_empty() {
        return Err("AVI container contains zero decodable video frames.".into());
    }

    let (audio_bytes, audio_mime) = if !audio_payload.is_empty() {
        match info.audio_format_tag {
            Some(1) => {
                // Wrap raw PCM in canonical RIFF WAVE container.
                let wav = wrap_pcm_as_wav(
                    &audio_payload,
                    info.audio_channels.max(1),
                    info.audio_sample_rate.max(8_000),
                    info.audio_bits_per_sample.max(8),
                );
                (Some(wav), Some("audio/wav"))
            }
            Some(0x0055) => (Some(audio_payload), Some("audio/mpeg")),
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    Ok(DecodedAviMedia {
        info,
        frames,
        audio_bytes,
        audio_mime,
    })
}

fn wrap_pcm_as_wav(pcm: &[u8], channels: u16, sample_rate: u32, bits_per_sample: u16) -> Vec<u8> {
    let bytes_per_sample = (bits_per_sample.div_ceil(8)).max(1);
    let block_align = channels.saturating_mul(bytes_per_sample);
    let byte_rate = sample_rate.saturating_mul(block_align as u32);
    let data_len = pcm.len() as u32;
    let riff_len = 36u32.saturating_add(data_len);

    let mut out = Vec::with_capacity(44 + pcm.len());
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
    out
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
    }
}
