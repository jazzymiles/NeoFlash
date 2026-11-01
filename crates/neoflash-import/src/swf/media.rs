//! SWF bitmap (`JPEGTables`, `DefineBits`, `DefineBitsJPEG2/3/4`,
//! `DefineBitsLossless` 1/2) and sound (`DefineSound`, `StartSound`,
//! `DefineButtonSound`, `SoundStreamHead` 1/2 + `SoundStreamBlock`) decoders.

use super::bits::SwfSlice;
use super::shape::SwfBitmapRef;
use crate::ImportReport;
use crate::legacy::contents::ContentsSoundSettings;
use crate::legacy::media::decode_binary_sound;
use flate2::read::ZlibDecoder;
use image::{ExtendedColorType, ImageEncoder};
use neoflash_core::{
    Color, FrameSound, Id, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES,
    ResourceUnit, SoundEffect, SoundSync,
};
use std::io::Read;

const MAX_SWF_BITMAP_DIM: u32 = 4096;
const MAX_ZLIB_RASTER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct SwfSoundAsset {
    pub resource_id: Id,
    pub duration_44: u64,
}

pub(crate) struct SwfStreamSoundCollector {
    pub playback_rate_flag: u8,
    pub stream_compression: u8,
    pub stream_rate_flag: u8,
    pub stream_is_16_bit: bool,
    pub stream_is_stereo: bool,
    pub start_frame: Option<u32>,
    pub total_samples: u32,
    pub payload: Vec<u8>,
}

/// Decode `DefineBits` (6), `DefineBitsJPEG2` (21), `DefineBitsJPEG3` (35), or `DefineBitsJPEG4` (90).
pub(crate) fn decode_jpeg_bitmap_tag(
    tag_code: u16,
    payload: &[u8],
    jpeg_tables: Option<&[u8]>,
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u16, SwfBitmapRef)> {
    let mut r = SwfSlice::new(payload);
    let bitmap_id = r.u16()?;
    let (img_data, alpha_zlib) = match tag_code {
        6 => (r.rest(), None),
        21 => (r.rest(), None),
        35 => {
            let alpha_offset = r.u32()? as usize;
            let img = r.take(alpha_offset)?;
            (img, Some(r.rest()))
        }
        90 => {
            let alpha_offset = r.u32()? as usize;
            let _deblock = r.u16()?;
            let img = r.take(alpha_offset)?;
            (img, Some(r.rest()))
        }
        _ => return None,
    };

    let normalized =
        normalize_swf_image_stream(img_data, if tag_code == 6 { jpeg_tables } else { None });
    let decoded = match image::load_from_memory(&normalized) {
        Ok(img) => img,
        Err(err) => {
            report.warning(
                "swf_bitmap_skipped",
                format!("{location}#bitmap:{bitmap_id}"),
                format!("Cannot decode SWF bitmap #{bitmap_id}: {err}."),
            );
            return None;
        }
    };
    let (w, h) = (decoded.width(), decoded.height());
    if w == 0 || h == 0 || w > MAX_SWF_BITMAP_DIM || h > MAX_SWF_BITMAP_DIM {
        report.warning(
            "swf_bitmap_skipped",
            format!("{location}#bitmap:{bitmap_id}"),
            format!("SWF bitmap #{bitmap_id} dimensions {w}x{h} exceed 1..=4096 limit."),
        );
        return None;
    }

    let mut rgba = decoded.into_rgba8().into_raw();
    if let Some(alpha_compressed) = alpha_zlib
        && !alpha_compressed.is_empty()
    {
        let pixel_count = (w as usize) * (h as usize);
        match decompress_bounded_zlib(alpha_compressed, pixel_count) {
            Ok(alpha_plane) if alpha_plane.len() >= pixel_count => {
                // SWF DefineBitsJPEG3/4 stores premultiplied RGB against the separate alpha plane;
                // un-premultiply RGB so straight-alpha PNG rendering stays faithful.
                for (px, &a) in rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(&alpha_plane[..pixel_count])
                {
                    if a == 0 {
                        *px = [0, 0, 0, 0];
                    } else if a < 255 {
                        let af = f32::from(a);
                        px[0] = ((f32::from(px[0]) * 255.0 / af).round() as u32).min(255) as u8;
                        px[1] = ((f32::from(px[1]) * 255.0 / af).round() as u32).min(255) as u8;
                        px[2] = ((f32::from(px[2]) * 255.0 / af).round() as u32).min(255) as u8;
                        px[3] = a;
                    } else {
                        px[3] = 255;
                    }
                }
            }
            _ => {
                report.warning(
                    "swf_bitmap_alpha",
                    format!("{location}#bitmap:{bitmap_id}"),
                    format!("Truncated or corrupt JPEG alpha plane on SWF bitmap #{bitmap_id}; opaque RGB retained."),
                );
            }
        }
    }

    let avg_color = compute_average_color(&rgba);

    // Preserve clean self-contained JPEG/PNG directly when there is no separate alpha plane,
    // otherwise encode composited straight-alpha RGBA to PNG.
    let (media_type, ext, final_bytes) = if alpha_zlib.is_none()
        && normalized.starts_with(&[0xFF, 0xD8, 0xFF])
        && image::load_from_memory_with_format(&normalized, image::ImageFormat::Jpeg).is_ok()
    {
        ("image/jpeg", "jpg", normalized)
    } else if alpha_zlib.is_none() && normalized.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("image/png", "png", normalized)
    } else {
        let png = encode_rgba_png(&rgba, w, h)?;
        ("image/png", "png", png)
    };

    let resource_id = register_media_resource(
        format!("swf-bitmap-{bitmap_id}"),
        format!("bitmaps/bitmap-{bitmap_id}.{ext}"),
        media_type,
        &final_bytes,
        resources,
        resource_bytes_total,
        location,
        report,
    )?;

    Some((
        bitmap_id,
        SwfBitmapRef {
            resource_id,
            width: w,
            height: h,
            avg_color,
        },
    ))
}

/// Decode `DefineBitsLossless` (20) or `DefineBitsLossless2` (36) into a PNG `ResourceUnit`.
pub(crate) fn decode_lossless_bitmap_tag(
    tag_code: u16,
    payload: &[u8],
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u16, SwfBitmapRef)> {
    let mut r = SwfSlice::new(payload);
    let bitmap_id = r.u16()?;
    let format = r.u8()?;
    let width = u32::from(r.u16()?);
    let height = u32::from(r.u16()?);
    if width == 0 || height == 0 || width > MAX_SWF_BITMAP_DIM || height > MAX_SWF_BITMAP_DIM {
        report.warning(
            "swf_bitmap_skipped",
            format!("{location}#bitmap:{bitmap_id}"),
            format!("Lossless SWF bitmap #{bitmap_id} dimensions {width}x{height} out of 1..=4096 range."),
        );
        return None;
    }
    let has_alpha = tag_code == 36;
    let color_table_size = if format == 3 {
        usize::from(r.u8()?) + 1
    } else {
        0
    };
    let zlib_data = r.rest();
    let (w, h) = (width as usize, height as usize);
    let expected_raw = match format {
        3 => color_table_size * if has_alpha { 4 } else { 3 } + ((w + 3) & !3) * h,
        4 if !has_alpha => ((w * 2 + 3) & !3) * h,
        5 => w * h * 4,
        _ => {
            report.warning(
                "swf_bitmap_skipped",
                format!("{location}#bitmap:{bitmap_id}"),
                format!("Unsupported DefineBitsLossless format {format} on bitmap #{bitmap_id}."),
            );
            return None;
        }
    };
    if expected_raw > MAX_ZLIB_RASTER_BYTES {
        report.warning(
            "swf_bitmap_skipped",
            format!("{location}#bitmap:{bitmap_id}"),
            format!("Lossless bitmap #{bitmap_id} exceeds 64 MiB raster budget."),
        );
        return None;
    }
    let raw = match decompress_bounded_zlib(zlib_data, expected_raw) {
        Ok(buf) if buf.len() >= expected_raw => buf,
        _ => {
            report.warning(
                "swf_bitmap_skipped",
                format!("{location}#bitmap:{bitmap_id}"),
                format!("Truncated or corrupt zlib stream in lossless bitmap #{bitmap_id}."),
            );
            return None;
        }
    };

    let mut rgba = Vec::with_capacity(w * h * 4);
    match format {
        3 => {
            let entry_bytes = if has_alpha { 4 } else { 3 };
            let pal_end = color_table_size * entry_bytes;
            let mut palette = Vec::with_capacity(color_table_size);
            for i in 0..color_table_size {
                let base = i * entry_bytes;
                if has_alpha {
                    palette.push(unpremultiply_rgba([
                        raw[base],
                        raw[base + 1],
                        raw[base + 2],
                        raw[base + 3],
                    ]));
                } else {
                    palette.push([raw[base], raw[base + 1], raw[base + 2], 255]);
                }
            }
            let stride = (w + 3) & !3;
            let indices = &raw[pal_end..];
            for y in 0..h {
                let row = &indices[y * stride..y * stride + w];
                for &idx in row {
                    let c = palette
                        .get(usize::from(idx))
                        .copied()
                        .unwrap_or([0, 0, 0, 0]);
                    rgba.extend_from_slice(&c);
                }
            }
        }
        4 => {
            let stride = (w * 2 + 3) & !3;
            for y in 0..h {
                let row = &raw[y * stride..y * stride + w * 2];
                for &[hi, lo] in row.as_chunks::<2>().0 {
                    let pix = u16::from_be_bytes([hi, lo]);
                    let r5 = ((pix >> 10) & 0x1f) as u8;
                    let g5 = ((pix >> 5) & 0x1f) as u8;
                    let b5 = (pix & 0x1f) as u8;
                    rgba.extend_from_slice(&[
                        (r5 << 3) | (r5 >> 2),
                        (g5 << 3) | (g5 >> 2),
                        (b5 << 3) | (b5 >> 2),
                        255,
                    ]);
                }
            }
        }
        5 => {
            for &[a, cr, cg, cb] in raw[..w * h * 4].as_chunks::<4>().0 {
                if has_alpha {
                    rgba.extend_from_slice(&unpremultiply_rgba([cr, cg, cb, a]));
                } else {
                    rgba.extend_from_slice(&[cr, cg, cb, 255]);
                }
            }
        }
        _ => return None,
    }

    let avg_color = compute_average_color(&rgba);
    let png = encode_rgba_png(&rgba, width, height)?;
    let resource_id = register_media_resource(
        format!("swf-bitmap-{bitmap_id}"),
        format!("bitmaps/bitmap-{bitmap_id}.png"),
        "image/png",
        &png,
        resources,
        resource_bytes_total,
        location,
        report,
    )?;

    Some((
        bitmap_id,
        SwfBitmapRef {
            resource_id,
            width,
            height,
            avg_color,
        },
    ))
}

/// Decode `DefineSound` (tag 14) into a WAV or MP3 `ResourceUnit`.
pub(crate) fn decode_define_sound_tag(
    payload: &[u8],
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u16, SwfSoundAsset)> {
    let mut r = SwfSlice::new(payload);
    let sound_id = r.u16()?;
    let format = r.ub(4)? as u8;
    let rate_code = r.ub(2)? as u8;
    let is_16_bit = r.bit()?;
    let is_stereo = r.bit()?;
    let sample_count = r.u32()?;
    let mut data = r.rest();

    // In SWF DefineSound, MP3 (`format == 2`) prepends a 2-byte signed `SeekSamples` header.
    if format == 2 {
        if data.len() < 2 {
            report.warning(
                "swf_media_not_imported",
                format!("{location}#sound:{sound_id}"),
                format!("Truncated MP3 SeekSamples header on SWF sound #{sound_id}."),
            );
            return None;
        }
        data = &data[2..];
    }

    if matches!(format, 0 | 3) {
        let block = (if is_16_bit { 2usize } else { 1 }) * (if is_stereo { 2 } else { 1 });
        let max_pcm = (sample_count as usize).saturating_mul(block);
        if max_pcm > 0 && data.len() > max_pcm {
            data = &data[..max_pcm];
        }
    }
    let format_flags = (u16::from(format) << 4)
        | (u16::from(rate_code) << 2)
        | (if is_16_bit { 0x02 } else { 0 })
        | (if is_stereo { 0x01 } else { 0 });
    let settings = ContentsSoundSettings {
        format_flags,
        sample_count,
        export_format: None,
        export_bits: None,
        encoded_byte_count: Some(data.len() as u32),
        imported_mp3_quality: None,
        secondary_format_flags: None,
        secondary_sample_count: None,
        use_8k_sample_rate: None,
    };

    let decoded = match decode_binary_sound(data, 5, &settings) {
        Ok(s) => s,
        Err(err) => {
            report.warning(
                "swf_media_not_imported",
                format!("{location}#sound:{sound_id}"),
                format!(
                    "SWF sound #{sound_id} (codec {format}) could not be imported: {}.",
                    err.message
                ),
            );
            return None;
        }
    };
    if decoded.short_read {
        report.warning(
            "swf_sound_short_read",
            format!("{location}#sound:{sound_id}"),
            format!("SWF sound #{sound_id} ended early; available audio samples were retained."),
        );
    }

    let resource_id = register_media_resource(
        format!("swf-sound-{sound_id}"),
        format!("sounds/sound-{sound_id}.{}", decoded.extension),
        decoded.media_type,
        &decoded.bytes,
        resources,
        resource_bytes_total,
        location,
        report,
    )?;

    Some((
        sound_id,
        SwfSoundAsset {
            resource_id,
            duration_44: u64::from(decoded.duration_44),
        },
    ))
}

/// Parse `SoundStreamHead` (18) or `SoundStreamHead2` (45).
pub(crate) fn parse_sound_stream_head(payload: &[u8]) -> Option<SwfStreamSoundCollector> {
    let mut r = SwfSlice::new(payload);
    let _reserved = r.ub(4)?;
    let playback_rate_flag = r.ub(2)? as u8;
    let _playback_16 = r.bit()?;
    let _playback_stereo = r.bit()?;
    let stream_compression = r.ub(4)? as u8;
    let stream_rate_flag = r.ub(2)? as u8;
    let stream_is_16_bit = r.bit()?;
    let stream_is_stereo = r.bit()?;
    let _avg_sample_count = r.u16()?;
    if stream_compression == 2 && r.remaining() >= 2 {
        let _latency_seek = r.i16()?;
    }
    Some(SwfStreamSoundCollector {
        playback_rate_flag,
        stream_compression,
        stream_rate_flag,
        stream_is_16_bit,
        stream_is_stereo,
        start_frame: None,
        total_samples: 0,
        payload: Vec::new(),
    })
}

/// Append a `SoundStreamBlock` (tag 19) to an active `SwfStreamSoundCollector`.
pub(crate) fn append_sound_stream_block(
    collector: &mut SwfStreamSoundCollector,
    current_frame: u32,
    block: &[u8],
) {
    if block.is_empty() {
        return;
    }
    if collector.start_frame.is_none() {
        collector.start_frame = Some(current_frame);
    }
    if collector.stream_compression == 2 {
        // MP3 SoundStreamBlock: `u16 sample_count`, `i16 seek_samples`, followed by MPEG frames.
        if block.len() > 4 {
            let samples = u16::from_le_bytes([block[0], block[1]]);
            collector.total_samples = collector.total_samples.saturating_add(u32::from(samples));
            collector.payload.extend_from_slice(&block[4..]);
        }
    } else if matches!(collector.stream_compression, 0 | 3) {
        // Raw PCM stream block: concatenate directly; sample count derived from byte length.
        let bytes_per_frame = (if collector.stream_is_16_bit {
            2usize
        } else {
            1
        }) * (if collector.stream_is_stereo { 2 } else { 1 });
        collector.total_samples = collector
            .total_samples
            .saturating_add((block.len() / bytes_per_frame.max(1)) as u32);
        collector.payload.extend_from_slice(block);
    }
    // Note: ADPCM SoundStreamBlocks each reset their own 2-bit `code_size` header per block, so
    // only single-block ADPCM or PCM/MP3 multi-block streams concatenate directly; handled in finalize.
    else if collector.stream_compression == 1 && collector.payload.is_empty() {
        collector.payload.extend_from_slice(block);
    }
}

/// Finalize a collected timeline stream sound into `(start_frame, FrameSound)` if non-empty.
pub(crate) fn finalize_stream_sound(
    stream_uid: &str,
    collector: SwfStreamSoundCollector,
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u32, FrameSound)> {
    let start_frame = collector.start_frame?;
    if collector.payload.is_empty() {
        return None;
    }
    let _ = collector.playback_rate_flag;
    let format_flags = (u16::from(collector.stream_compression) << 4)
        | (u16::from(collector.stream_rate_flag) << 2)
        | (if collector.stream_is_16_bit { 0x02 } else { 0 })
        | (if collector.stream_is_stereo { 0x01 } else { 0 });
    let sample_count = collector.total_samples.max(1);
    let settings = ContentsSoundSettings {
        format_flags,
        sample_count,
        export_format: None,
        export_bits: None,
        encoded_byte_count: Some(collector.payload.len() as u32),
        imported_mp3_quality: None,
        secondary_format_flags: None,
        secondary_sample_count: None,
        use_8k_sample_rate: None,
    };
    let decoded = match decode_binary_sound(&collector.payload, 5, &settings) {
        Ok(s) => s,
        Err(err) => {
            report.warning(
                "swf_media_not_imported",
                format!("{location}#stream:{stream_uid}"),
                format!(
                    "SWF timeline stream sound could not be imported: {}.",
                    err.message
                ),
            );
            return None;
        }
    };
    let resource_id = register_media_resource(
        format!("swf-stream-{stream_uid}"),
        format!("sounds/stream-{stream_uid}.{}", decoded.extension),
        decoded.media_type,
        &decoded.bytes,
        resources,
        resource_bytes_total,
        location,
        report,
    )?;
    let mut fs = FrameSound::new(resource_id);
    fs.sync = SoundSync::Stream;
    Some((start_frame, fs))
}

/// Parse SWF `SOUNDINFO` (used in `StartSound` tag 15 after `sound_id: u16`, and in `DefineButtonSound` tag 17)
/// into a format-6/7 `FrameSound`.
pub(crate) fn parse_sound_info(
    r: &mut SwfSlice<'_>,
    sound: &SwfSoundAsset,
    location: &str,
    report: &mut ImportReport,
) -> Option<FrameSound> {
    r.align();
    let _reserved = r.ub(2)?;
    let sync_stop = r.bit()?;
    let sync_no_multiple = r.bit()?;
    let has_envelope = r.bit()?;
    let has_loops = r.bit()?;
    let has_out_point = r.bit()?;
    let has_in_point = r.bit()?;
    r.align();

    let in_point = if has_in_point { Some(r.u32()?) } else { None };
    let out_point = if has_out_point { Some(r.u32()?) } else { None };
    if in_point.is_some_and(|v| v > 0) || out_point.is_some() {
        report.warning(
            "swf_frame_sound",
            location,
            "SWF SOUNDINFO in/out trim points are not applied yet; full audio resource plays.",
        );
    }

    let raw_loops = if has_loops { u32::from(r.u16()?) } else { 1 };
    let (loops, loop_forever) = if raw_loops >= 0x7FFF {
        (1, true)
    } else if raw_loops > FrameSound::MAX_LOOPS {
        report.warning(
            "swf_frame_sound",
            location,
            format!(
                "SWF sound repeat count {raw_loops} clamped to {}.",
                FrameSound::MAX_LOOPS
            ),
        );
        (FrameSound::MAX_LOOPS, false)
    } else {
        (raw_loops.max(1), false)
    };

    let (effect, envelope) = if has_envelope {
        let env_count = usize::from(r.u8()?);
        let mut pts = Vec::with_capacity(env_count);
        for _ in 0..env_count {
            let pos44 = u64::from(r.u32()?);
            let left = r.u16()?;
            let right = r.u16()?;
            pts.push((pos44, left, right));
        }
        classify_swf_envelope(pts, sound.duration_44, location, report)
    } else {
        (SoundEffect::None, None)
    };

    let sync = if sync_stop {
        SoundSync::Stop
    } else if sync_no_multiple {
        SoundSync::Start
    } else {
        SoundSync::Event
    };

    Some(FrameSound {
        resource: sound.resource_id.clone(),
        sync,
        loops,
        loop_forever,
        effect,
        envelope,
    })
}

fn classify_swf_envelope(
    mut pts: Vec<(u64, u16, u16)>,
    duration_44: u64,
    location: &str,
    report: &mut ImportReport,
) -> (SoundEffect, Option<Vec<[f64; 3]>>) {
    if pts.is_empty() || pts.iter().all(|&(_, l, r)| l == 32_768 && r == 32_768) {
        return (SoundEffect::None, None);
    }
    if pts.len() == 1 {
        match (pts[0].1, pts[0].2) {
            (32_768, 0) => return (SoundEffect::LeftChannel, None),
            (0, 32_768) => return (SoundEffect::RightChannel, None),
            _ => {}
        }
    }
    pts.sort_by_key(|p| p.0);
    if pts.len() > FrameSound::MAX_ENVELOPE_POINTS {
        pts.truncate(FrameSound::MAX_ENVELOPE_POINTS);
        report.warning(
            "swf_frame_sound",
            location,
            format!(
                "SWF custom sound envelope truncated to {} control points.",
                FrameSound::MAX_ENVELOPE_POINTS
            ),
        );
    }
    let max_mark = pts.iter().map(|p| p.0).max().unwrap_or(0);
    let denom = duration_44.max(max_mark);
    let mut prev_pos = 0.0_f64;
    let mut out = Vec::with_capacity(pts.len());
    for (pos44, l, r) in pts {
        let pos = if denom == 0 {
            0.0
        } else {
            (pos44 as f64 / denom as f64).clamp(0.0, 1.0)
        }
        .max(prev_pos);
        prev_pos = pos;
        out.push([
            pos,
            (f64::from(l) / 32_768.0).clamp(0.0, 1.0),
            (f64::from(r) / 32_768.0).clamp(0.0, 1.0),
        ]);
    }
    (SoundEffect::Custom, Some(out))
}

#[allow(clippy::too_many_arguments)]
fn register_media_resource(
    id: String,
    path: String,
    media_type: &str,
    bytes: &[u8],
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<Id> {
    if bytes.is_empty()
        || bytes.len() > MAX_RESOURCE_BYTES
        || resources.len() >= MAX_RESOURCES
        || resource_bytes_total.saturating_add(bytes.len()) > MAX_RESOURCE_TOTAL_BYTES
    {
        report.warning(
            "swf_media_budget",
            format!("{location}#{id}"),
            format!("SWF resource '{path}' ({} bytes) omitted due to 64 MiB / 1024-resource project limit.", bytes.len()),
        );
        return None;
    }
    let unit = ResourceUnit::from_bytes(id.clone(), path, media_type, bytes).ok()?;
    *resource_bytes_total += bytes.len();
    resources.push(unit);
    Some(id)
}

/// Normalize SWF JPEG streams:
/// 1. Prepend shared `JPEGTables` when `DefineBits` lacks quantization/Huffman tables;
/// 2. Strip erroneous `FF D9 FF D8` (`EOI SOI`) or leading `FF D8 FF D9` glue markers
///    emitted by older Flash encoders (SWF19 specification §8 "DefineBitsJPEG2").
fn normalize_swf_image_stream(img: &[u8], jpeg_tables: Option<&[u8]>) -> Vec<u8> {
    if img.starts_with(b"\x89PNG\r\n\x1a\n") || img.starts_with(b"GIF8") {
        return img.to_vec();
    }
    let mut combined = Vec::with_capacity(img.len() + jpeg_tables.map_or(0, <[u8]>::len));
    if let Some(tables) = jpeg_tables
        && !tables.is_empty()
    {
        let mut t = strip_leading_empty_soi_eoi(tables);
        if t.ends_with(&[0xFF, 0xD9]) {
            t = &t[..t.len() - 2];
        }
        combined.extend_from_slice(t);
        let mut body = strip_leading_empty_soi_eoi(img);
        if !combined.is_empty() && body.starts_with(&[0xFF, 0xD8]) {
            body = &body[2..];
        }
        combined.extend_from_slice(body);
    } else {
        combined.extend_from_slice(strip_leading_empty_soi_eoi(img));
    }

    // Remove embedded `FF D9 FF D8` glue sequences inside the header (before SOS `FF DA`).
    let mut cleaned = Vec::with_capacity(combined.len());
    let mut i = 0usize;
    let mut in_scan = false;
    while i < combined.len() {
        if !in_scan && i + 4 <= combined.len() && combined[i..i + 4] == [0xFF, 0xD9, 0xFF, 0xD8] {
            if i == 0 {
                cleaned.extend_from_slice(&[0xFF, 0xD8]);
            }
            i += 4;
            continue;
        }
        if !in_scan && i + 2 <= combined.len() && combined[i..i + 2] == [0xFF, 0xDA] {
            in_scan = true;
        }
        cleaned.push(combined[i]);
        i += 1;
    }
    if !cleaned.starts_with(&[0xFF, 0xD8]) && combined.len() >= 2 {
        let mut with_soi = Vec::with_capacity(cleaned.len() + 2);
        with_soi.extend_from_slice(&[0xFF, 0xD8]);
        with_soi.extend_from_slice(&cleaned);
        return with_soi;
    }
    cleaned
}

fn strip_leading_empty_soi_eoi(mut s: &[u8]) -> &[u8] {
    while s.starts_with(&[0xFF, 0xD8, 0xFF, 0xD9]) || s.starts_with(&[0xFF, 0xD9, 0xFF, 0xD8]) {
        s = &s[4..];
    }
    s
}

fn decompress_bounded_zlib(compressed: &[u8], expected_max: usize) -> Result<Vec<u8>, ()> {
    let cap = expected_max.min(MAX_ZLIB_RASTER_BYTES);
    let mut decoder = ZlibDecoder::new(compressed).take((cap + 64) as u64);
    let mut out = Vec::with_capacity(cap.min(256 * 1024));
    decoder.read_to_end(&mut out).map_err(|_| ())?;
    Ok(out)
}

#[inline]
fn unpremultiply_rgba([r, g, b, a]: [u8; 4]) -> Color {
    if a == 0 {
        [0, 0, 0, 0]
    } else if a == 255 {
        [r, g, b, 255]
    } else {
        let af = f32::from(a);
        [
            ((f32::from(r) * 255.0 / af).round() as u32).min(255) as u8,
            ((f32::from(g) * 255.0 / af).round() as u32).min(255) as u8,
            ((f32::from(b) * 255.0 / af).round() as u32).min(255) as u8,
            a,
        ]
    }
}

fn compute_average_color(rgba: &[u8]) -> Color {
    let pixels = rgba.as_chunks::<4>().0;
    if pixels.is_empty() {
        return [128, 128, 128, 255];
    }
    let step = (pixels.len() / 1024).max(1);
    let (mut sr, mut sg, mut sb, mut sa, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for px in pixels.iter().step_by(step) {
        sr += u64::from(px[0]);
        sg += u64::from(px[1]);
        sb += u64::from(px[2]);
        sa += u64::from(px[3]);
        n += 1;
    }
    [
        (sr / n) as u8,
        (sg / n) as u8,
        (sb / n) as u8,
        (sa / n) as u8,
    ]
}

fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}
