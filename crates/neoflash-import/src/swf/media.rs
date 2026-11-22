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
use std::collections::BTreeMap;
use std::io::Read;

const MAX_SWF_BITMAP_DIM: u32 = 4096;
const MAX_ZLIB_RASTER_BYTES: usize = 64 * 1024 * 1024;
const MAX_SWF_VIDEO_DIM: u16 = 4096;
const MAX_SWF_VIDEO_FRAMES: usize = 65_535;

#[derive(Clone, Debug)]
pub(crate) struct SwfVideoStreamDef {
    pub character_id: u16,
    pub num_frames: u16,
    pub raw_width: u16,
    pub raw_height: u16,
    pub deblocking: u8,
    pub smoothing: bool,
    pub codec_id: u8,
    pub frames: BTreeMap<u16, Vec<u8>>,
    pub total_frame_bytes: usize,
    pub budget_exceeded: bool,
}

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

/// Preserve every exposure of large, equal-sized JPEG dictionaries within the existing
/// decoded-pixel ceiling. Read dimensions from the tag headers before decoding any frames.
pub(crate) fn compact_jpeg_dimensions(
    tags: &[u8],
    count: usize,
    pixel_budget: usize,
) -> Option<[u32; 2]> {
    if count < 2 {
        return None;
    }
    let mut reader = SwfSlice::new(tags);
    let mut dimensions = None;
    let mut found = 0usize;
    while reader.remaining() >= 2 {
        let header = reader.u16()?;
        let code = header >> 6;
        let len = (header & 63) as usize;
        let len = if len == 63 {
            reader.u32()? as usize
        } else {
            len
        };
        let payload = reader.take(len)?;
        match code {
            0 => break,
            21 => {
                let jpeg = payload.get(2..)?;
                if !jpeg.starts_with(&[0xff, 0xd8, 0xff]) {
                    return None;
                }
                let size = image::ImageReader::with_format(
                    std::io::Cursor::new(jpeg),
                    image::ImageFormat::Jpeg,
                )
                .into_dimensions()
                .ok()?;
                if size.0 == 0
                    || size.1 == 0
                    || size.0 > MAX_SWF_BITMAP_DIM
                    || size.1 > MAX_SWF_BITMAP_DIM
                    || dimensions.is_some_and(|d| d != size)
                {
                    return None;
                }
                dimensions = Some(size);
                found += 1;
            }
            6 | 20 | 35 | 36 | 90 => return None,
            _ => {}
        }
    }
    let (w, h) = dimensions?;
    let pixels = (w as usize).checked_mul(h as usize)?.checked_mul(found)?;
    if found != count || pixels <= pixel_budget {
        return None;
    }
    let scale = (pixel_budget as f64 / pixels as f64).sqrt();
    Some([
        (f64::from(w) * scale).floor().max(1.0) as u32,
        (f64::from(h) * scale).floor().max(1.0) as u32,
    ])
}

/// Decode `DefineBits` (6), `DefineBitsJPEG2` (21), `DefineBitsJPEG3` (35), or `DefineBitsJPEG4` (90).
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_jpeg_bitmap_tag(
    tag_code: u16,
    payload: &[u8],
    jpeg_tables: Option<&[u8]>,
    compact_size: Option<[u32; 2]>,
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

    if let Some([rw, rh]) = compact_size {
        let reduced = decoded
            .resize_exact(rw, rh, image::imageops::FilterType::Triangle)
            .into_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&reduced)
            .ok()?;
        let id = register_media_resource(
            format!("swf-bitmap-{bitmap_id}"),
            format!("bitmaps/bitmap-{bitmap_id}.jpg"),
            "image/jpeg",
            &jpeg,
            resources,
            resource_bytes_total,
            location,
            report,
        )?;
        let avg_color = compute_average_color(
            &image::DynamicImage::ImageRgb8(reduced)
                .into_rgba8()
                .into_raw(),
        );
        report.warning("swf_bitmap_sequence_downscaled",format!("{location}#bitmap:{bitmap_id}"),format!("Reduced the stored image from {w}×{h} to {rw}×{rh} to retain the complete JPEG sequence within the 128 MiB decoded-pixel budget. Stage placement and timing are preserved."));
        return Some((
            bitmap_id,
            SwfBitmapRef {
                resource_id: id,
                width: w,
                height: h,
                stored_size: [rw, rh],
                avg_color,
            },
        ));
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
    let (media_type, ext, final_bytes) =
        if alpha_zlib.is_none() && normalized.starts_with(&[0xFF, 0xD8, 0xFF]) {
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
            stored_size: [w, h],
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
            stored_size: [width, height],
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
    read_sound_info(r, sound, |message| {
        report.warning("swf_frame_sound", location, message)
    })
}

pub(super) fn read_sound_info(
    r: &mut SwfSlice<'_>,
    sound: &SwfSoundAsset,
    mut warn: impl FnMut(String),
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

    let in_raw = if has_in_point { Some(r.u32()?) } else { None };
    let out_raw = if has_out_point { Some(r.u32()?) } else { None };
    let mut trim_invalid = false;
    let in_point = match in_raw {
        Some(v) if v > 0 => {
            let sec = f64::from(v) / 44_100.0;
            if sec.is_finite() && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec) {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        }
        _ => None,
    };
    let out_point = match out_raw {
        Some(v) if v > 0 && v < 0x3fff_ffff => {
            let sec = f64::from(v) / 44_100.0;
            if sec.is_finite()
                && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec)
                && in_point.is_none_or(|inp| sec > inp)
            {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        }
        _ => None,
    };
    if trim_invalid {
        warn("SWF SOUNDINFO in/out trim points were out of range and omitted.".to_owned());
    }

    let raw_loops = if has_loops { u32::from(r.u16()?) } else { 1 };
    let (loops, loop_forever) = if raw_loops >= 0x7FFF {
        (1, true)
    } else if raw_loops > FrameSound::MAX_LOOPS {
        warn(format!(
            "SWF sound repeat count {raw_loops} clamped to {}.",
            FrameSound::MAX_LOOPS
        ));
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
        classify_swf_envelope(pts, in_raw, out_raw, sound.duration_44, &mut warn)
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
        in_point,
        out_point,
    })
}

fn classify_swf_envelope(
    mut pts: Vec<(u64, u16, u16)>,
    in_raw: Option<u32>,
    out_raw: Option<u32>,
    duration_44: u64,
    warn: &mut impl FnMut(String),
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
        warn(format!(
            "SWF custom sound envelope truncated to {} control points.",
            FrameSound::MAX_ENVELOPE_POINTS
        ));
    }
    let start_44 = in_raw
        .filter(|&v| v > 0 && (f64::from(v) / 44_100.0) <= FrameSound::MAX_SOUND_TRIM_SECONDS)
        .map(u64::from)
        .unwrap_or(0);
    let out_44 = out_raw
        .filter(|&v| {
            v > 0
                && v < 0x3fff_ffff
                && u64::from(v) > start_44
                && (f64::from(v) / 44_100.0) <= FrameSound::MAX_SOUND_TRIM_SECONDS
        })
        .map(u64::from);
    let end_44 = out_44.unwrap_or(duration_44);
    let span_44 = end_44.saturating_sub(start_44);
    let min_mark = pts.first().map_or(0, |p| p.0);
    let max_mark = pts.last().map_or(0, |p| p.0);
    // Flash authoring and NeoFlash's SWF exporter write `pos44` in sample coordinates anchored at
    // `start_44 ..= end_44`; synthetic callers may also pass playback-relative `0 ..= span_44`.
    let base_44 = if start_44 > 0 && min_mark >= start_44 {
        start_44
    } else if start_44 > 0 && max_mark <= span_44 {
        0
    } else {
        start_44
    };
    let denom = span_44.max(max_mark.saturating_sub(base_44));
    let mut level_clamped = false;
    let mut prev_pos = 0.0_f64;
    let mut out = Vec::with_capacity(pts.len());
    for (pos44, l, r) in pts {
        if l > 32_768 || r > 32_768 {
            level_clamped = true;
        }
        let rel = pos44.saturating_sub(base_44);
        let pos = if denom == 0 {
            0.0
        } else {
            (rel as f64 / denom as f64).clamp(0.0, 1.0)
        }
        .max(prev_pos);
        prev_pos = pos;
        out.push([
            pos,
            (f64::from(l) / 32_768.0).clamp(0.0, 1.0),
            (f64::from(r) / 32_768.0).clamp(0.0, 1.0),
        ]);
    }
    if level_clamped {
        warn("SWF sound envelope level exceeded 100% (32768) and was clamped to 1.0.".to_owned());
    }
    (SoundEffect::Custom, Some(out))
}

/// Pre-scan SWF tags (including `DefineSprite` tag 39 timelines) to collect `DefineVideoStream` (60)
/// headers and all `VideoFrame` (61) packets within resource memory budgets.
pub(crate) fn collect_swf_video_streams(tag_bytes: &[u8]) -> BTreeMap<u16, SwfVideoStreamDef> {
    let mut streams = BTreeMap::new();
    let mut cumulative_bytes = 0usize;
    collect_video_tags_slice(tag_bytes, true, &mut streams, &mut cumulative_bytes);
    streams
}

fn collect_video_tags_slice(
    tags: &[u8],
    is_root: bool,
    streams: &mut BTreeMap<u16, SwfVideoStreamDef>,
    cumulative_bytes: &mut usize,
) {
    let mut r = SwfSlice::new(tags);
    while r.remaining() >= 2 {
        let Some(tag_and_len) = r.u16() else { break };
        let code = tag_and_len >> 6;
        let short_len = (tag_and_len & 0x3F) as usize;
        let len = if short_len == 0x3F {
            let Some(l) = r.u32() else { break };
            l as usize
        } else {
            short_len
        };
        let Some(payload) = r.take(len) else { break };
        match code {
            0 => break,
            39 if is_root && payload.len() >= 4 => {
                collect_video_tags_slice(&payload[4..], false, streams, cumulative_bytes);
            }
            60 if is_root && payload.len() >= 8 => {
                let character_id = u16::from_le_bytes([payload[0], payload[1]]);
                let num_frames = u16::from_le_bytes([payload[2], payload[3]]);
                let raw_width = u16::from_le_bytes([payload[4], payload[5]]);
                let raw_height = u16::from_le_bytes([payload[6], payload[7]]);
                let flags = payload.get(8).copied().unwrap_or(0);
                let codec_id = payload.get(9).copied().unwrap_or(0);
                streams.insert(
                    character_id,
                    SwfVideoStreamDef {
                        character_id,
                        num_frames,
                        raw_width,
                        raw_height,
                        deblocking: (flags >> 1) & 0x07,
                        smoothing: (flags & 0x01) != 0,
                        codec_id,
                        frames: BTreeMap::new(),
                        total_frame_bytes: 0,
                        budget_exceeded: false,
                    },
                );
            }
            61 if payload.len() >= 4 => {
                let stream_id = u16::from_le_bytes([payload[0], payload[1]]);
                let frame_num = u16::from_le_bytes([payload[2], payload[3]]);
                let data = &payload[4..];
                if let Some(def) = streams.get_mut(&stream_id) {
                    if def.budget_exceeded {
                        continue;
                    }
                    let prev_len = def.frames.get(&frame_num).map_or(0, Vec::len);
                    let next_stream_bytes = def
                        .total_frame_bytes
                        .saturating_sub(prev_len)
                        .saturating_add(data.len());
                    let next_cum_bytes = cumulative_bytes
                        .saturating_sub(prev_len)
                        .saturating_add(data.len());
                    if data.len() > MAX_RESOURCE_BYTES
                        || next_stream_bytes > MAX_RESOURCE_BYTES
                        || next_cum_bytes > MAX_RESOURCE_TOTAL_BYTES
                        || (def.frames.len() >= MAX_SWF_VIDEO_FRAMES
                            && !def.frames.contains_key(&frame_num))
                    {
                        def.budget_exceeded = true;
                    } else {
                        def.total_frame_bytes = next_stream_bytes;
                        *cumulative_bytes = next_cum_bytes;
                        def.frames.insert(frame_num, data.to_vec());
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn register_video_resource(
    video_id: u16,
    flv_bytes: &[u8],
    resources: &mut Vec<ResourceUnit>,
    resource_bytes_total: &mut usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<Id> {
    register_media_resource(
        format!("swf-video-{video_id}"),
        format!("videos/video-{video_id}.flv"),
        "video/x-flv",
        flv_bytes,
        resources,
        resource_bytes_total,
        location,
        report,
    )
}

/// Mux a validated non-empty `DefineVideoStream` + `VideoFrame` sequence (`codec_id` `2..=7`)
/// into a spec-compliant FLV v1 (`video/x-flv`) byte stream.
pub(crate) fn mux_swf_video_stream_to_flv(
    def: &SwfVideoStreamDef,
    fps: f64,
) -> Result<Vec<u8>, String> {
    if !matches!(def.codec_id, 2..=7) {
        return Err(format!(
            "Unsupported SWF DefineVideoStream codec_id {} on character #{}.",
            def.codec_id, def.character_id
        ));
    }
    if def.frames.is_empty() {
        return Err("SWF DefineVideoStream has 0 embedded VideoFrame tags.".to_owned());
    }
    let width = def.raw_width.clamp(1, MAX_SWF_VIDEO_DIM);
    let height = def.raw_height.clamp(1, MAX_SWF_VIDEO_DIM);
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps.clamp(1.0, 1000.0)
    } else {
        24.0
    };
    let max_frame = def.frames.keys().next_back().copied().unwrap_or(0);
    let total_frames = u32::from(def.num_frames).max(u32::from(max_frame).saturating_add(1));
    let duration_secs = f64::from(total_frames) / safe_fps;

    let mut flv = Vec::with_capacity(
        def.total_frame_bytes
            .saturating_add(256 + def.frames.len() * 20)
            .min(MAX_RESOURCE_BYTES),
    );
    // 9-byte FLV v1 header (video flag 0x01) + 4-byte PreviousTagSize0
    flv.extend_from_slice(&[b'F', b'L', b'V', 0x01, 0x01, 0x00, 0x00, 0x00, 0x09]);
    flv.extend_from_slice(&0u32.to_be_bytes());

    let meta = build_flv_on_metadata(
        duration_secs,
        f64::from(width),
        f64::from(height),
        safe_fps,
        def.codec_id,
        def.deblocking,
    );
    push_swf_flv_tag(&mut flv, 18, 0, &[], &meta)?;

    let vp6_crop = ((((16 - (width % 16)) % 16) as u8) << 4) | (((16 - (height % 16)) % 16) as u8);
    let mut avc_nal_len_size = 4usize;

    for (idx, (&frame_num, raw)) in def.frames.iter().enumerate() {
        if raw.is_empty() {
            return Err(format!(
                "Empty SWF VideoFrame #{frame_num} on stream #{}.",
                def.character_id
            ));
        }
        let ts_ms = ((f64::from(frame_num) * 1000.0) / safe_fps)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        let pkt = strip_optional_flv_video_prefix(raw, def.codec_id, width, height);
        match def.codec_id {
            2 => {
                let frame_type =
                    validate_h263_packet_frame_type(pkt, idx == 0).ok_or_else(|| {
                        format!(
                            "Malformed Sorenson H.263 VideoFrame #{frame_num} on stream #{}.",
                            def.character_id
                        )
                    })?;
                let hdr = (frame_type << 4) | 2;
                push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr], pkt)?;
            }
            3 => {
                let is_kf = is_screen_video_keyframe(pkt, width, height).ok_or_else(|| {
                    format!(
                        "Malformed ScreenVideo VideoFrame #{frame_num} on stream #{}.",
                        def.character_id
                    )
                })?;
                let frame_type = if is_kf { 1u8 } else { 2u8 };
                let hdr = (frame_type << 4) | 3;
                push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr], pkt)?;
            }
            4 => {
                if pkt.len() < 2 {
                    return Err(format!(
                        "Truncated On2 VP6 VideoFrame #{frame_num} on stream #{}.",
                        def.character_id
                    ));
                }
                let frame_type = if (pkt[0] & 0x80) == 0 { 1u8 } else { 2u8 };
                let hdr = (frame_type << 4) | 4;
                push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr, vp6_crop], pkt)?;
            }
            5 => {
                if pkt.len() < 5 {
                    return Err(format!(
                        "Truncated On2 VP6A VideoFrame #{frame_num} on stream #{}.",
                        def.character_id
                    ));
                }
                let alpha_off =
                    (usize::from(pkt[0]) << 16) | (usize::from(pkt[1]) << 8) | usize::from(pkt[2]);
                if alpha_off == 0 || 3usize.saturating_add(alpha_off) > pkt.len() {
                    return Err(format!(
                        "Invalid On2 VP6A alpha offset on VideoFrame #{frame_num} of stream #{}.",
                        def.character_id
                    ));
                }
                let frame_type = if (pkt[3] & 0x80) == 0 { 1u8 } else { 2u8 };
                let hdr = (frame_type << 4) | 5;
                push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr, vp6_crop], pkt)?;
            }
            6 => {
                let is_kf = is_screen_video_v2_keyframe(pkt, width, height).ok_or_else(|| {
                    format!(
                        "Malformed ScreenVideo v2 VideoFrame #{frame_num} on stream #{}.",
                        def.character_id
                    )
                })?;
                let frame_type = if is_kf || idx == 0 { 1u8 } else { 2u8 };
                let hdr = (frame_type << 4) | 6;
                push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr], pkt)?;
            }
            7 => {
                if is_wrapped_swf_avc_packet(pkt, &mut avc_nal_len_size) {
                    let is_seq_or_idr = pkt[0] == 0
                        || (pkt[0] == 1 && avc_chain_contains_idr(&pkt[4..], avc_nal_len_size));
                    let frame_type = if is_seq_or_idr || idx == 0 { 1u8 } else { 2u8 };
                    let hdr = (frame_type << 4) | 7;
                    push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr], pkt)?;
                } else if idx == 0 && is_valid_swf_avc_config(pkt) {
                    avc_nal_len_size = usize::from(pkt[4] & 0x03) + 1;
                    let hdr = (1u8 << 4) | 7;
                    push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr, 0, 0, 0, 0], pkt)?;
                } else if is_valid_swf_nalu_chain(pkt, avc_nal_len_size) {
                    let is_idr = avc_chain_contains_idr(pkt, avc_nal_len_size);
                    let frame_type = if is_idr || idx == 0 { 1u8 } else { 2u8 };
                    let hdr = (frame_type << 4) | 7;
                    push_swf_flv_tag(&mut flv, 9, ts_ms, &[hdr, 1, 0, 0, 0], pkt)?;
                } else {
                    return Err(format!(
                        "Malformed AVC VideoFrame #{frame_num} on stream #{}.",
                        def.character_id
                    ));
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(flv)
}

fn strip_optional_flv_video_prefix(raw: &[u8], codec_id: u8, width: u16, height: u16) -> &[u8] {
    if raw.len() >= 5 && (raw[0] & 0x0F) == codec_id && matches!(raw[0] >> 4, 1..=3) {
        match codec_id {
            2 if raw[1] == 0 && raw[2] == 0 && (raw[3] & 0x80) != 0 => return &raw[1..],
            3 | 6
                if (u16::from_be_bytes([raw[1], raw[2]]) & 0x0FFF) == width
                    && (u16::from_be_bytes([raw[3], raw[4]]) & 0x0FFF) == height =>
            {
                return &raw[1..];
            }
            _ => {}
        }
    }
    raw
}

fn validate_h263_packet_frame_type(pkt: &[u8], default_keyframe: bool) -> Option<u8> {
    if pkt.len() < 4 || pkt[0] != 0 || pkt[1] != 0 || (pkt[2] & 0x80) == 0 {
        return None;
    }
    let mut r = SwfSlice::new(pkt);
    if r.ub(17)? != 1 {
        return None;
    }
    let _version = r.ub(5)?;
    let _temporal_ref = r.ub(8)?;
    let pic_size = r.ub(3)?;
    match pic_size {
        0 => {
            r.ub(8)?;
            r.ub(8)?;
        }
        1 => {
            r.ub(16)?;
            r.ub(16)?;
        }
        2..=6 => {}
        _ => return None,
    }
    let pic_type = r.ub(2)?;
    Some(match pic_type {
        0 => 1,
        1 => 2,
        2 => 3,
        _ if default_keyframe => 1,
        _ => 2,
    })
}

/// Inspect a `SCREENVIDEOPACKET` (`codec_id == 3`) header and block directory, returning
/// `Some(true)` if every macroblock carries intra data (`DataSize > 0`), `Some(false)` if any
/// block is an inter-frame delta skip (`DataSize == 0`), or `None` if malformed.
pub(crate) fn is_screen_video_keyframe(
    raw: &[u8],
    expected_w: u16,
    expected_h: u16,
) -> Option<bool> {
    let pkt = strip_optional_flv_video_prefix(raw, 3, expected_w, expected_h);
    if pkt.len() < 4 {
        return None;
    }
    let h0 = u16::from_be_bytes([pkt[0], pkt[1]]);
    let h1 = u16::from_be_bytes([pkt[2], pkt[3]]);
    let block_w = usize::from(((h0 >> 12) & 0x0F) + 1) * 16;
    let img_w = h0 & 0x0FFF;
    let block_h = usize::from(((h1 >> 12) & 0x0F) + 1) * 16;
    let img_h = h1 & 0x0FFF;
    if img_w == 0
        || img_h == 0
        || (expected_w > 0 && img_w != expected_w)
        || (expected_h > 0 && img_h != expected_h)
    {
        return None;
    }
    let cols = usize::from(img_w).div_ceil(block_w);
    let rows = usize::from(img_h).div_ceil(block_h);
    let total_blocks = cols.checked_mul(rows)?;
    let mut pos = 4usize;
    let mut all_present = true;
    for _ in 0..total_blocks {
        if pos + 2 > pkt.len() {
            return None;
        }
        let data_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
        pos += 2;
        if data_size == 0 {
            all_present = false;
        } else {
            pos = pos.checked_add(data_size)?;
            if pos > pkt.len() {
                return None;
            }
        }
    }
    Some(all_present)
}

/// Inspect a `SCREEN2VIDEOPACKET` (`codec_id == 6`) header and block directory.
pub(crate) fn is_screen_video_v2_keyframe(
    raw: &[u8],
    expected_w: u16,
    expected_h: u16,
) -> Option<bool> {
    let pkt = strip_optional_flv_video_prefix(raw, 6, expected_w, expected_h);
    if pkt.len() < 5 {
        return None;
    }
    let h0 = u16::from_be_bytes([pkt[0], pkt[1]]);
    let h1 = u16::from_be_bytes([pkt[2], pkt[3]]);
    let block_w = usize::from(((h0 >> 12) & 0x0F) + 1) * 16;
    let img_w = h0 & 0x0FFF;
    let block_h = usize::from(((h1 >> 12) & 0x0F) + 1) * 16;
    let img_h = h1 & 0x0FFF;
    if img_w == 0
        || img_h == 0
        || (expected_w > 0 && img_w != expected_w)
        || (expected_h > 0 && img_h != expected_h)
    {
        return None;
    }
    let flags = pkt[4];
    let has_iframe = (flags & 0x02) != 0;
    let has_palette = (flags & 0x01) != 0;
    let mut pos = 5usize;
    if has_palette {
        if pos + 2 > pkt.len() {
            return None;
        }
        let pal_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
        pos = pos.checked_add(2)?.checked_add(pal_size)?;
        if pos > pkt.len() {
            return None;
        }
    }
    let cols = usize::from(img_w).div_ceil(block_w);
    let rows = usize::from(img_h).div_ceil(block_h);
    let total_blocks = cols.checked_mul(rows)?;
    let mut all_present = true;
    for _ in 0..total_blocks {
        if pos + 2 > pkt.len() {
            return None;
        }
        let data_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
        pos += 2;
        if data_size == 0 {
            all_present = false;
        } else {
            pos = pos.checked_add(data_size)?;
            if pos > pkt.len() {
                return None;
            }
        }
    }
    Some(has_iframe || all_present)
}

/// Apply a lossless ScreenVideo v1 (`codec_id == 3`) or ScreenVideo v2 (`codec_id == 6`) packet
/// onto `canvas_rgba` (`width * height * 4` straight RGBA), preserving unchanged (`DataSize == 0`)
/// blocks from prior frames.
pub(crate) fn apply_screen_video_packet(
    codec_id: u8,
    raw: &[u8],
    width: u16,
    height: u16,
    canvas_rgba: &mut [u8],
) -> bool {
    let w = usize::from(width);
    let h = usize::from(height);
    if w == 0
        || h == 0
        || u32::from(width) > MAX_SWF_BITMAP_DIM
        || u32::from(height) > MAX_SWF_BITMAP_DIM
        || canvas_rgba.len() != w.saturating_mul(h).saturating_mul(4)
    {
        return false;
    }
    let pkt = strip_optional_flv_video_prefix(raw, codec_id, width, height);
    let min_hdr = if codec_id == 6 { 5 } else { 4 };
    if pkt.len() < min_hdr {
        return false;
    }
    let h0 = u16::from_be_bytes([pkt[0], pkt[1]]);
    let h1 = u16::from_be_bytes([pkt[2], pkt[3]]);
    let block_w = usize::from(((h0 >> 12) & 0x0F) + 1) * 16;
    let img_w = usize::from(h0 & 0x0FFF);
    let block_h = usize::from(((h1 >> 12) & 0x0F) + 1) * 16;
    let img_h = usize::from(h1 & 0x0FFF);
    if img_w != w || img_h != h || block_w == 0 || block_h == 0 {
        return false;
    }
    let mut pos = 4usize;
    if codec_id == 6 {
        let flags = pkt[4];
        if (flags & 0xFC) != 0 {
            return false;
        }
        pos = 5;
        if (flags & 0x01) != 0 {
            if pos + 2 > pkt.len() {
                return false;
            }
            let pal_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
            let Some(next_pos) = pos.checked_add(2).and_then(|p| p.checked_add(pal_size)) else {
                return false;
            };
            pos = next_pos;
            if pos > pkt.len() {
                return false;
            }
        }
    }

    let cols = w.div_ceil(block_w);
    let rows = h.div_ceil(block_h);
    let mut decoded_ok = false;

    for br in 0..rows {
        let cur_h = block_h.min(h - br * block_h);
        for bc in 0..cols {
            if pos + 2 > pkt.len() {
                return false;
            }
            let cur_w = block_w.min(w - bc * block_w);
            let data_size = usize::from(u16::from_be_bytes([pkt[pos], pkt[pos + 1]]));
            pos += 2;
            if data_size == 0 {
                decoded_ok = true;
                continue;
            }
            if pos + data_size > pkt.len() {
                return false;
            }
            let block_slice = &pkt[pos..pos + data_size];
            pos += data_size;

            let zslice = if codec_id == 6 {
                if block_slice.len() < 2 {
                    return false;
                }
                // SWF19 §13 IMAGEBLOCKV2 header byte:
                // UB[3] Reserved (0), UB[2] ColorDepth (0 = 24-bit BGR),
                // UB[1] HasDiffBlocks, UB[1] ZlibPrimeCompressCurrent, UB[1] ZlibPrimeCompressPrevious.
                let block_flags = block_slice[0];
                let reserved = (block_flags >> 5) & 0x07;
                let color_depth = (block_flags >> 3) & 0x03;
                let has_diff_or_zlib_prime = (block_flags & 0x07) != 0;
                if reserved != 0 || color_depth != 0 || has_diff_or_zlib_prime {
                    return false;
                }
                &block_slice[1..]
            } else {
                block_slice
            };

            let expected_bgr = cur_w * cur_h * 3;
            let Ok(raw_block) = decompress_bounded_zlib(zslice, expected_bgr) else {
                return false;
            };
            if raw_block.len() != expected_bgr {
                return false;
            }
            decoded_ok = true;
            for by in 0..cur_h {
                let bottom_y = br * block_h + by;
                let top_y = h - 1 - bottom_y;
                for bx in 0..cur_w {
                    let si = (by * cur_w + bx) * 3;
                    let dst_x = bc * block_w + bx;
                    let di = (top_y * w + dst_x) * 4;
                    canvas_rgba[di] = raw_block[si + 2];
                    canvas_rgba[di + 1] = raw_block[si + 1];
                    canvas_rgba[di + 2] = raw_block[si];
                    canvas_rgba[di + 3] = 255;
                }
            }
        }
    }
    decoded_ok
}

fn push_swf_flv_tag(
    out: &mut Vec<u8>,
    tag_type: u8,
    timestamp_ms: u32,
    prefix: &[u8],
    payload: &[u8],
) -> Result<(), String> {
    let data_size = prefix.len().saturating_add(payload.len());
    if data_size > 0x00FF_FFFF
        || out
            .len()
            .saturating_add(11)
            .saturating_add(data_size)
            .saturating_add(4)
            > MAX_RESOURCE_BYTES
    {
        return Err("Muxed FLV stream exceeds the 16 MiB single-resource limit.".to_owned());
    }
    out.push(tag_type);
    out.push(((data_size >> 16) & 0xFF) as u8);
    out.push(((data_size >> 8) & 0xFF) as u8);
    out.push((data_size & 0xFF) as u8);
    out.push(((timestamp_ms >> 16) & 0xFF) as u8);
    out.push(((timestamp_ms >> 8) & 0xFF) as u8);
    out.push((timestamp_ms & 0xFF) as u8);
    out.push(((timestamp_ms >> 24) & 0xFF) as u8);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(prefix);
    out.extend_from_slice(payload);
    let prev_size = (11 + data_size) as u32;
    out.extend_from_slice(&prev_size.to_be_bytes());
    Ok(())
}

fn build_flv_on_metadata(
    duration_secs: f64,
    width: f64,
    height: f64,
    fps: f64,
    codec_id: u8,
    deblocking: u8,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(144);
    body.push(0x02);
    body.extend_from_slice(&10u16.to_be_bytes());
    body.extend_from_slice(b"onMetaData");
    body.push(0x08);
    body.extend_from_slice(&6u32.to_be_bytes());
    for (k, v) in [
        ("duration", duration_secs.max(0.0)),
        ("width", width),
        ("height", height),
        ("framerate", fps),
        ("videocodecid", f64::from(codec_id)),
        ("deblocking", f64::from(deblocking)),
    ] {
        body.extend_from_slice(&(k.len() as u16).to_be_bytes());
        body.extend_from_slice(k.as_bytes());
        body.push(0x00);
        body.extend_from_slice(&v.to_be_bytes());
    }
    body.extend_from_slice(&[0x00, 0x00, 0x09]);
    body
}

fn is_valid_swf_avc_config(buf: &[u8]) -> bool {
    buf.len() >= 7
        && buf[0] == 0x01
        && buf[1] != 0
        && buf[3] != 0
        && (buf[4] & 0xFC) == 0xFC
        && (buf[5] & 0xE0) == 0xE0
}

fn is_valid_swf_nalu_chain(buf: &[u8], len_size: usize) -> bool {
    if buf.len() <= len_size || !matches!(len_size, 1..=4) {
        return false;
    }
    let mut pos = 0usize;
    let mut count = 0usize;
    while pos < buf.len() {
        if pos + len_size > buf.len() || count >= 4096 {
            return false;
        }
        let mut nalu_len = 0usize;
        for &b in &buf[pos..pos + len_size] {
            nalu_len = (nalu_len << 8) | usize::from(b);
        }
        pos += len_size;
        if nalu_len == 0 || pos.checked_add(nalu_len).is_none_or(|end| end > buf.len()) {
            return false;
        }
        let nal_hdr = buf[pos];
        if (nal_hdr & 0x80) != 0 || (nal_hdr & 0x1F) == 0 {
            return false;
        }
        pos += nalu_len;
        count += 1;
    }
    count > 0 && pos == buf.len()
}

fn avc_chain_contains_idr(buf: &[u8], len_size: usize) -> bool {
    if !matches!(len_size, 1..=4) {
        return false;
    }
    let mut pos = 0usize;
    while pos + len_size < buf.len() {
        let mut nalu_len = 0usize;
        for &b in &buf[pos..pos + len_size] {
            nalu_len = (nalu_len << 8) | usize::from(b);
        }
        pos += len_size;
        if nalu_len == 0 || pos.checked_add(nalu_len).is_none_or(|end| end > buf.len()) {
            break;
        }
        if (buf[pos] & 0x1F) == 5 {
            return true;
        }
        pos += nalu_len;
    }
    false
}

fn is_wrapped_swf_avc_packet(slice: &[u8], nal_len_size: &mut usize) -> bool {
    if slice.len() < 4 {
        return false;
    }
    match slice[0] {
        0 => {
            if slice[1..4] == [0, 0, 0] && is_valid_swf_avc_config(&slice[4..]) {
                *nal_len_size = usize::from(slice[8] & 0x03) + 1;
                true
            } else {
                false
            }
        }
        1 => {
            !is_valid_swf_nalu_chain(slice, *nal_len_size)
                && is_valid_swf_nalu_chain(&slice[4..], *nal_len_size)
        }
        2 => slice.len() == 4 && slice[1..4] == [0, 0, 0],
        _ => false,
    }
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

pub(crate) fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .ok()?;
    if png.len() > MAX_RESOURCE_BYTES {
        // Retry with a denser lossless encoding before applying the portable resource limit.
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        png.clear();
        PngEncoder::new_with_quality(&mut png, CompressionType::Default, FilterType::Adaptive)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .ok()?;
    }
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_jpeg_sequence_preserves_all_frames_at_the_decoded_pixel_limit() {
        let image = image::RgbImage::from_pixel(16, 32, image::Rgb([23, 107, 211]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image)
            .unwrap();
        let mut tags = Vec::new();
        for id in 1u16..=3 {
            tags.extend_from_slice(&((21u16 << 6) | 63).to_le_bytes());
            tags.extend_from_slice(&((jpeg.len() + 2) as u32).to_le_bytes());
            tags.extend_from_slice(&id.to_le_bytes());
            tags.extend_from_slice(&jpeg);
        }
        assert_eq!(compact_jpeg_dimensions(&tags, 3, 3 * 16 * 32), None);
        assert_eq!(compact_jpeg_dimensions(&tags, 3, 3 * 8 * 16), Some([8, 16]));
        assert_eq!(compact_jpeg_dimensions(&tags, 4, 3 * 8 * 16), None);
    }
}
