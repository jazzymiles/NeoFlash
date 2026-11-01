//! Bounded legacy bitmap and sound decoding. Wire-layout facts are documented
//! by JPEXS FLA Convert 0923d708 (BitmapRecordDecoder/XflBitmapWriter and
//! FlaSoundFormat/MediaPayloadRecordDecoder/XflMediaWriter); this reader uses
//! independent checked slices and the shared Rust image/compression libraries.
use super::contents::ContentsSoundSettings;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_PIXELS: usize = 4096 * 4096;
const MAX_BYTES: usize = MAX_PIXELS * 4;

#[derive(Clone, Debug)]
pub struct DecodedBinaryBitmap {
    pub width: u32,
    pub height: u32,
    /// Straight alpha RGBA, suitable for the shared authored bitmap pipeline.
    pub rgba: Vec<u8>,
    pub bounds_words: [i32; 4],
    pub original_bytes: Vec<u8>,
}
impl DecodedBinaryBitmap {
    pub fn png_bytes(&self) -> Result<Vec<u8>, BinaryMediaError> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &self.rgba,
                self.width,
                self.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|_| error("fla_bitmap_encode", 0, "PNG encoding failed."))?;
        Ok(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code} at media byte {offset}: {message}")]
pub struct BinaryMediaError {
    pub code: &'static str,
    pub offset: usize,
    pub message: &'static str,
}
fn error(code: &'static str, offset: usize, message: &'static str) -> BinaryMediaError {
    BinaryMediaError {
        code,
        offset,
        message,
    }
}
type Result<T, E = BinaryMediaError> = std::result::Result<T, E>;
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(size)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| {
                error(
                    "fla_bitmap_truncated",
                    self.at,
                    "Bitmap field exceeds remaining bytes.",
                )
            })?;
        let result = &self.bytes[self.at..end];
        self.at = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn bounds(&mut self) -> Result<[i32; 4]> {
        Ok([self.i32()?, self.i32()?, self.i32()?, self.i32()?])
    }
}
fn dimensions(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(error(
            "fla_bitmap_limit",
            0,
            "Bitmap dimensions must be in 1..=4096.",
        ));
    }
    Ok(width as usize * height as usize)
}

/// Decode a complete media stream. Positive encoded_byte_count is the JPEG
/// length in its Contents descriptor; zero selects a native raster envelope.
pub fn decode_binary_bitmap(bytes: &[u8], encoded_byte_count: i32) -> Result<DecodedBinaryBitmap> {
    if bytes.len() > MAX_INPUT || encoded_byte_count < 0 {
        return Err(error(
            "fla_bitmap_limit",
            0,
            "Bitmap input exceeds 8 MiB or has a negative encoded length.",
        ));
    }
    let encoded = if encoded_byte_count == 0 && bytes.starts_with(&[0xff, 0xd8, 0xff, 0xe0]) {
        bytes
            .len()
            .checked_sub(16)
            .ok_or_else(|| error("fla_bitmap_truncated", 0, "JPEG has no bounds trailer."))?
    } else {
        encoded_byte_count as usize
    };
    let mut r = Reader { bytes, at: 0 };
    if encoded > 0 {
        let jpeg = r.take(encoded)?;
        let bounds_words = r.bounds()?;
        if r.at != bytes.len() {
            return Err(error(
                "fla_bitmap_trailing",
                r.at,
                "Bytes remain after JPEG bounds.",
            ));
        }
        if image::guess_format(jpeg).ok() != Some(ImageFormat::Jpeg) {
            return Err(error(
                "fla_bitmap_format",
                0,
                "Encoded media is not a JPEG.",
            ));
        }
        let mut limits = Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(MAX_BYTES as u64);
        let mut reader = ImageReader::with_format(Cursor::new(jpeg), ImageFormat::Jpeg);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|_| error("fla_bitmap_jpeg", 0, "JPEG decoding failed."))?
            .into_rgba8();
        dimensions(image.width(), image.height())?;
        return Ok(DecodedBinaryBitmap {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
            bounds_words,
            original_bytes: bytes.to_vec(),
        });
    }
    let revision = r.u8()?;
    if revision > 3 {
        return Err(error(
            "fla_bitmap_revision",
            0,
            "Raster revision is unsupported.",
        ));
    }
    let format = r.u8()?;
    if format > 5 {
        return Err(error(
            "fla_bitmap_format",
            1,
            "Raster pixel format is unsupported.",
        ));
    }
    let stride = r.u16()? as usize;
    let width = r.u16()? as u32;
    let height = r.u16()? as u32;
    let count = dimensions(width, height)?;
    let bounds_words = r.bounds()?;
    let alpha = if revision >= 3 { r.u8()? != 0 } else { false };
    let depth = if format == 5 { 32 } else { 1usize << format };
    let minimum = (width as usize * depth).div_ceil(8);
    let raster_size = stride
        .checked_mul(height as usize)
        .filter(|&n| n <= MAX_BYTES)
        .ok_or_else(|| error("fla_bitmap_limit", r.at, "Decoded raster exceeds 64 MiB."))?;
    if stride < minimum {
        return Err(error(
            "fla_bitmap_stride",
            r.at,
            "Raster stride is shorter than a row.",
        ));
    }
    let mut palette = Vec::<[u8; 4]>::new();
    if format < 4 {
        let count = r.u16()? as usize;
        if count == 0 || count > 256 {
            return Err(error(
                "fla_bitmap_palette",
                r.at,
                "Palette must contain 1..=256 entries.",
            ));
        }
        for _ in 0..count {
            palette.push(if revision >= 3 {
                let stored_alpha = r.u8()?;
                [
                    r.u8()?,
                    r.u8()?,
                    r.u8()?,
                    if alpha { stored_alpha } else { 255 },
                ]
            } else {
                [
                    (r.u16()? >> 8) as u8,
                    (r.u16()? >> 8) as u8,
                    (r.u16()? >> 8) as u8,
                    255,
                ]
            });
        }
    }
    let compressed = if revision >= 2 {
        r.u8()? != 0
    } else {
        revision == 1
    };
    let pixels = if compressed {
        let mut compressed = Vec::new();
        let mut chunks = 0;
        loop {
            let length = r.u16()? as usize;
            if length == 0 {
                break;
            }
            chunks += 1;
            if chunks > 50_000 {
                return Err(error("fla_bitmap_limit", r.at, "Too many zlib chunks."));
            }
            compressed.extend_from_slice(r.take(length)?);
        }
        let mut decoder = flate2::Decompress::new(true);
        let mut decoded = Vec::with_capacity(raster_size + 1);
        let status = decoder
            .decompress_vec(&compressed, &mut decoded, flate2::FlushDecompress::Finish)
            .map_err(|_| error("fla_bitmap_zlib", r.at, "Bitmap zlib data is invalid."))?;
        if status != flate2::Status::StreamEnd
            || decoded.len() != raster_size
            || decoder.total_in() as usize != compressed.len()
        {
            return Err(error(
                "fla_bitmap_zlib",
                r.at,
                "Bitmap zlib size or trailing-byte check failed.",
            ));
        }
        decoded
    } else {
        r.take(raster_size)?.to_vec()
    };
    if r.at != bytes.len() {
        return Err(error(
            "fla_bitmap_trailing",
            r.at,
            "Bytes remain after the bitmap raster.",
        ));
    }
    let mut rgba = Vec::with_capacity(count * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let row = y * stride;
            let mut pixel = if format == 5 {
                let p = row + x * 4;
                [
                    pixels[p + 1],
                    pixels[p + 2],
                    pixels[p + 3],
                    if alpha { pixels[p] } else { 255 },
                ]
            } else if format == 4 {
                // Historical 5-bit channel lookup differs from linear 255/31.
                const CHANNEL: [u8; 32] = [
                    0, 15, 23, 31, 39, 47, 55, 63, 71, 79, 87, 95, 95, 111, 119, 127, 135, 143,
                    151, 159, 167, 175, 183, 191, 199, 207, 215, 223, 223, 239, 247, 255,
                ];
                let p = row + x * 2;
                let word = u16::from_be_bytes([pixels[p], pixels[p + 1]]);
                [
                    CHANNEL[(word >> 10 & 31) as usize],
                    CHANNEL[(word >> 5 & 31) as usize],
                    CHANNEL[(word & 31) as usize],
                    255,
                ]
            } else {
                let bit = x * depth;
                let index =
                    (pixels[row + bit / 8] as usize >> (8 - depth - bit % 8)) & ((1 << depth) - 1);
                *palette.get(index).ok_or_else(|| {
                    error(
                        "fla_bitmap_palette",
                        r.at,
                        "Pixel refers to absent palette entry.",
                    )
                })?
            };
            let a = pixel[3];
            for channel in &mut pixel[..3] {
                if *channel > a {
                    return Err(error(
                        "fla_bitmap_alpha",
                        r.at,
                        "Archived channels exceed premultiplied alpha.",
                    ));
                }
                *channel = if a == 0 {
                    0
                } else {
                    ((u32::from(*channel) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                };
            }
            rgba.extend_from_slice(&pixel);
        }
    }
    Ok(DecodedBinaryBitmap {
        width,
        height,
        rgba,
        bounds_words,
        original_bytes: bytes.to_vec(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedBinarySound {
    pub media_type: &'static str,
    pub extension: &'static str,
    pub bytes: Vec<u8>,
    pub duration_44: u32,
    pub short_read: bool,
}

fn pcm_byte_count(flags: u32, samples: u32) -> Option<usize> {
    let block =
        usize::from(1 + u16::from((flags & 2) != 0)) * usize::from(1 + u16::from((flags & 1) != 0));
    (samples as usize).checked_mul(block)
}

fn is_mpeg_frame_header(h: [u8; 4]) -> bool {
    h[0] == 0xff
        && (h[1] & 0xe0) == 0xe0
        && ((h[1] >> 3) & 3) != 1
        && ((h[1] >> 1) & 3) != 0
        && ((h[2] >> 4) & 0xf) != 0xf
        && ((h[2] >> 2) & 3) != 3
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    if bytes.len() >= 10
        && &bytes[..3] == b"ID3"
        && (bytes[6] | bytes[7] | bytes[8] | bytes[9]) & 0x80 == 0
    {
        let tag_len = ((bytes[6] as usize) << 21)
            | ((bytes[7] as usize) << 14)
            | ((bytes[8] as usize) << 7)
            | (bytes[9] as usize);
        let footer = if (bytes[5] & 0x10) != 0 { 10 } else { 0 };
        start = (10 + tag_len + footer).min(bytes.len());
    }
    let end = bytes
        .len()
        .saturating_sub(3)
        .min(start.saturating_add(4096));
    (start..end).any(|i| is_mpeg_frame_header(bytes[i..i + 4].try_into().unwrap()))
}

/// Decode a sound media stream into a standalone `audio/wav` or `audio/mpeg`
/// payload. Codecs without a standalone source container (ADPCM, GSM,
/// Nellymoser) return an explicit error so conversion can warn and keep the
/// original stream in provenance without failing the document.
pub fn decode_binary_sound(
    bytes: &[u8],
    subtype_revision: u8,
    settings: &ContentsSoundSettings,
) -> Result<DecodedBinarySound> {
    if bytes.len() > neoflash_core::MAX_RESOURCE_TOTAL_BYTES {
        return Err(error(
            "fla_sound_limit",
            0,
            "Sound media stream exceeds the 64 MiB resource budget.",
        ));
    }
    let flags = u32::from(settings.format_flags) + if subtype_revision < 5 { 4 } else { 0 };
    if (flags & !0xff) != 0 {
        return Err(error(
            "fla_sound_format",
            0,
            "Sound format flags are unsupported.",
        ));
    }
    let codec_id = ((flags >> 4) & 0xf) as u8;
    let rate_code = ((flags >> 2) & 3) as usize;
    let sample_rate_hz = [5512_u32, 11025, 22050, 44100][rate_code];
    let bits_per_sample = if (flags & 2) != 0 { 16_u16 } else { 8_u16 };
    let channels = if (flags & 1) != 0 { 2_u16 } else { 1_u16 };
    let block_align = channels * (bits_per_sample / 8);
    let scale_44 = 1_u64 << (3 - rate_code);

    let encoded = settings.encoded_byte_count.unwrap_or(0) as usize;
    let expected = if codec_id != 0 && encoded > 0 {
        encoded
    } else {
        pcm_byte_count(flags, settings.sample_count)
            .ok_or_else(|| error("fla_sound_limit", 0, "Sound PCM byte count overflows."))?
    };
    let actual = expected.min(bytes.len());
    let short_read = actual != expected;
    let primary = &bytes[..actual];

    if codec_id == 0 {
        if bytes.len() > actual {
            return Err(error(
                "fla_sound_trailing",
                actual,
                "Bytes remain after uncompressed PCM audio.",
            ));
        }
    } else if bytes.len() > actual {
        let secondary_flags = u32::from(settings.secondary_format_flags.unwrap_or(0));
        let secondary_samples = settings.secondary_sample_count.unwrap_or(0);
        let secondary_expected =
            pcm_byte_count(secondary_flags, secondary_samples).ok_or_else(|| {
                error(
                    "fla_sound_limit",
                    actual,
                    "Secondary sound PCM count overflows.",
                )
            })?;
        let remaining_after_marker = bytes.len() - actual - 1;
        if remaining_after_marker > secondary_expected {
            return Err(error(
                "fla_sound_trailing",
                actual + 1 + secondary_expected,
                "Bytes remain after secondary cached audio.",
            ));
        }
    }

    match codec_id {
        0 | 3 => {
            let block = usize::from(block_align);
            let count = (primary.len() / block) * block;
            if count == 0 {
                return Err(error(
                    "fla_sound_empty",
                    0,
                    "Sound media stream contains no complete PCM sample frames.",
                ));
            }
            let pad = count & 1;
            let wav_len = 44usize
                .checked_add(count)
                .and_then(|n| n.checked_add(pad))
                .filter(|&n| n <= neoflash_core::MAX_RESOURCE_BYTES)
                .ok_or_else(|| {
                    error(
                        "fla_sound_limit",
                        0,
                        "WAV output exceeds the 16 MiB resource limit.",
                    )
                })?;
            let riff_chunk_size = (36 + count + pad) as u32;
            let byte_rate = sample_rate_hz * u32::from(block_align);
            let mut wav = Vec::with_capacity(wav_len);
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&riff_chunk_size.to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16_u32.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&channels.to_le_bytes());
            wav.extend_from_slice(&sample_rate_hz.to_le_bytes());
            wav.extend_from_slice(&byte_rate.to_le_bytes());
            wav.extend_from_slice(&block_align.to_le_bytes());
            wav.extend_from_slice(&bits_per_sample.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&(count as u32).to_le_bytes());
            wav.extend_from_slice(&primary[..count]);
            if pad != 0 {
                wav.push(0);
            }
            let actual_samples = (count / block) as u64;
            let duration_44 = (actual_samples * scale_44).min(u64::from(u32::MAX)) as u32;
            Ok(DecodedBinarySound {
                media_type: "audio/wav",
                extension: "wav",
                bytes: wav,
                duration_44,
                short_read,
            })
        }
        2 => {
            if primary.is_empty() {
                return Err(error("fla_sound_empty", 0, "MP3 media stream is empty."));
            }
            if primary.len() > neoflash_core::MAX_RESOURCE_BYTES {
                return Err(error(
                    "fla_sound_limit",
                    0,
                    "MP3 media stream exceeds the 16 MiB resource limit.",
                ));
            }
            if !looks_like_mp3(primary) {
                return Err(error(
                    "fla_sound_format",
                    0,
                    "MP3 media stream has no valid MPEG audio frame header.",
                ));
            }
            let duration_44 =
                (u64::from(settings.sample_count) * scale_44).min(u64::from(u32::MAX)) as u32;
            Ok(DecodedBinarySound {
                media_type: "audio/mpeg",
                extension: "mp3",
                bytes: primary.to_vec(),
                duration_44,
                short_read,
            })
        }
        1 => Err(error(
            "fla_sound_codec_unsupported",
            0,
            "ADPCM sound codec is not supported for editable audio import.",
        )),
        4 => Err(error(
            "fla_sound_codec_unsupported",
            0,
            "GSM sound codec is not supported for editable audio import.",
        )),
        5 | 6 => Err(error(
            "fla_sound_codec_unsupported",
            0,
            "Nellymoser sound codec is not supported for editable audio import.",
        )),
        _ => Err(error(
            "fla_sound_codec_unsupported",
            0,
            "Sound codec is not supported for editable audio import.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn raster(
        format: u8,
        width: u16,
        stride: u16,
        palette: &[[u8; 4]],
        pixels: &[u8],
        compressed: bool,
    ) -> Vec<u8> {
        let mut out = vec![3, format];
        out.extend(stride.to_le_bytes());
        out.extend(width.to_le_bytes());
        out.extend(1_u16.to_le_bytes());
        for word in [0_i32, 0, i32::from(width) * 20, 20] {
            out.extend(word.to_le_bytes());
        }
        out.push(1);
        if format < 4 {
            out.extend((palette.len() as u16).to_le_bytes());
            for color in palette {
                out.extend(color);
            }
        }
        out.push(u8::from(compressed));
        if compressed {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(pixels).unwrap();
            let bytes = encoder.finish().unwrap();
            for part in bytes.chunks(3) {
                out.extend((part.len() as u16).to_le_bytes());
                out.extend(part);
            }
            out.extend(0_u16.to_le_bytes());
        } else {
            out.extend(pixels);
        }
        out
    }

    #[test]
    fn legacy_bitmap_family_preserves_alpha_padding_palette_and_original_bytes() {
        let pixels = [128, 64, 32, 0, 0, 0, 0, 0, 91, 92, 93, 94];
        for compressed in [false, true] {
            let bytes = raster(5, 2, 12, &[], &pixels, compressed);
            let decoded = decode_binary_bitmap(&bytes, 0).unwrap();
            assert_eq!(decoded.rgba, [128, 64, 0, 128, 0, 0, 0, 0]);
            assert_eq!((decoded.width, decoded.height), (2, 1));
            assert_eq!(decoded.original_bytes, bytes);
            assert_eq!(decoded.bounds_words, [0, 0, 40, 20]);
            assert_eq!(
                image::load_from_memory(&decoded.png_bytes().unwrap())
                    .unwrap()
                    .into_rgba8()
                    .into_raw(),
                decoded.rgba
            );
            for n in 0..bytes.len() {
                assert!(
                    decode_binary_bitmap(&bytes[..n], 0).is_err(),
                    "truncation {n}"
                );
            }
        }
        for format in 0..4 {
            let depth = 1 << format;
            let bytes = raster(
                format,
                2,
                2,
                &[[255, 7, 8, 9], [128, 64, 32, 16]],
                &[1 << (8 - depth), 0],
                true,
            );
            let decoded = decode_binary_bitmap(&bytes, 0).unwrap();
            let expected = [128, 64, 32, 128, 7, 8, 9, 255];
            assert_eq!(decoded.rgba, expected);
        }
        let rgb555 = raster(4, 1, 2, &[], &[0x7c, 0x00], false);
        assert_eq!(
            decode_binary_bitmap(&rgb555, 0).unwrap().rgba,
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn malformed_bitmap_sizes_zlib_and_premultiplication_fail_without_partial_output() {
        let valid = raster(5, 1, 4, &[], &[255, 32, 64, 128], true);
        let mut invalid = valid.clone();
        invalid.extend([0]);
        assert_eq!(
            decode_binary_bitmap(&invalid, 0).unwrap_err().code,
            "fla_bitmap_trailing"
        );
        let mut wrong_size = valid.clone();
        wrong_size[2..4].copy_from_slice(&8_u16.to_le_bytes());
        assert_eq!(
            decode_binary_bitmap(&wrong_size, 0).unwrap_err().code,
            "fla_bitmap_zlib"
        );
        let mut no_checksum = valid.clone();
        // Three-byte chunks: remove the final data chunk while retaining the terminator.
        let mut at = 26;
        let mut last = at;
        while u16::from_le_bytes(no_checksum[at..at + 2].try_into().unwrap()) != 0 {
            last = at;
            at += 2 + usize::from(u16::from_le_bytes(
                no_checksum[at..at + 2].try_into().unwrap(),
            ));
        }
        no_checksum.drain(last..at);
        assert_eq!(
            decode_binary_bitmap(&no_checksum, 0).unwrap_err().code,
            "fla_bitmap_zlib"
        );
        assert_eq!(
            decode_binary_bitmap(&raster(5, 1, 4, &[], &[5, 6, 0, 0], false), 0)
                .unwrap_err()
                .code,
            "fla_bitmap_alpha"
        );
        assert_eq!(
            decode_binary_bitmap(&raster(5, 1, 3, &[], &[0; 3], false), 0)
                .unwrap_err()
                .code,
            "fla_bitmap_stride"
        );
        assert_eq!(
            decode_binary_bitmap(&raster(0, 1, 1, &[[255, 0, 0, 0]], &[128], false), 0)
                .unwrap_err()
                .code,
            "fla_bitmap_palette"
        );
    }

    #[test]
    fn legacy_jpeg_requires_bounds_and_checks_descriptor_length() {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[32, 64, 128], 1, 1, ExtendedColorType::Rgb8)
            .unwrap();
        let length = bytes.len() as i32;
        bytes.extend([0; 16]);
        let decoded = decode_binary_bitmap(&bytes, length).unwrap();
        assert_eq!((decoded.width, decoded.height), (1, 1));
        assert_eq!(decoded.original_bytes, bytes);
        assert!(decode_binary_bitmap(&bytes, length + 1).is_err());
        assert!(decode_binary_bitmap(&bytes[..bytes.len() - 1], length).is_err());
    }

    #[test]
    fn binary_sound_wraps_pcm_into_wav_preserves_mp3_and_rejects_unsupported_codecs() {
        use crate::legacy::contents::ContentsSoundSettings;

        fn settings(
            format_flags: u8,
            sample_count: u32,
            encoded_byte_count: Option<u32>,
        ) -> ContentsSoundSettings {
            ContentsSoundSettings {
                format_flags: u16::from(format_flags),
                sample_count,
                encoded_byte_count,
                ..Default::default()
            }
        }

        // 22.05 kHz (rate=2 -> 2<<2=0x08), 16-bit (0x02), stereo (0x01), codec 0 (raw PCM)
        let pcm_stream = vec![0x10_u8, 0x00, 0xf0, 0xff, 0x20, 0x00, 0xe0, 0xff];
        let wav = decode_binary_sound(&pcm_stream, 8, &settings(0x0b, 2, None)).unwrap();
        assert_eq!(wav.media_type, "audio/wav");
        assert_eq!(wav.extension, "wav");
        assert!(!wav.short_read);
        assert_eq!(wav.bytes.len(), 44 + 8);
        assert_eq!(&wav.bytes[0..4], b"RIFF");
        assert_eq!(&wav.bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([wav.bytes[22], wav.bytes[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(wav.bytes[24..28].try_into().unwrap()),
            22_050
        );
        assert_eq!(u16::from_le_bytes([wav.bytes[34], wav.bytes[35]]), 16);
        assert_eq!(&wav.bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav.bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(&wav.bytes[44..], &pcm_stream[..8]);

        // Trailing bytes on codec 0 raw PCM are rejected:
        let mut trailing_pcm = pcm_stream.clone();
        trailing_pcm.push(0x99);
        assert_eq!(
            decode_binary_sound(&trailing_pcm, 8, &settings(0x0b, 2, None))
                .unwrap_err()
                .code,
            "fla_sound_trailing"
        );

        // Pre-rev-5 (+4 added in decoder!) and odd-byte 8-bit mono RIFF padding (codec 3 = LE PCM):
        // Passing format_flags = ((3 << 4) | (3 << 2)) - 4 with subtype_revision = 4 adds +4 back -> 44.1 kHz!
        let odd_pcm = [0x80_u8, 0x90, 0x70];
        let pre_rev5_flags = ((3_u8 << 4) | (3_u8 << 2)).wrapping_sub(4);
        let wav_odd = decode_binary_sound(&odd_pcm, 4, &settings(pre_rev5_flags, 3, None)).unwrap();
        assert_eq!(
            u32::from_le_bytes(wav_odd.bytes[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(wav_odd.bytes.len(), 44 + 4);
        assert_eq!(
            u32::from_le_bytes(wav_odd.bytes[4..8].try_into().unwrap()),
            36 + 4
        );
        assert_eq!(
            u32::from_le_bytes(wav_odd.bytes[40..44].try_into().unwrap()),
            3
        );

        // Empty/incomplete single-sample PCM (< 1 block_align) is rejected, while partial frames set short_read:
        assert_eq!(
            decode_binary_sound(&[0; 2], 8, &settings(0x0b, 2, None))
                .unwrap_err()
                .code,
            "fla_sound_empty"
        );
        assert!(
            decode_binary_sound(&[0; 4], 8, &settings(0x0b, 2, None))
                .unwrap()
                .short_read
        );

        // Embedded MP3 (codec 2 -> 2<<4 = 0x20), raw MPEG sync, ID3v2 prefix, and secondary cached PCM:
        for mp3_bytes in [
            &[0xff_u8, 0xfb, 0x90, 0x64, 0x00, 0x00][..],
            &[
                b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0, 0xff, 0xfb, 0x90, 0x64,
            ][..],
        ] {
            let mut stream = mp3_bytes.to_vec();
            stream.push(0xff); // 1-byte marker
            stream.extend_from_slice(&[0; 8]); // 2 frames of 16-bit stereo secondary PCM
            let mut s = settings(0x2b, 576, Some(mp3_bytes.len() as u32));
            s.secondary_format_flags = Some(0x0b);
            s.secondary_sample_count = Some(2);
            let mp3 = decode_binary_sound(&stream, 8, &s).unwrap();
            assert_eq!(mp3.media_type, "audio/mpeg");
            assert_eq!(mp3.extension, "mp3");
            assert_eq!(mp3.bytes, mp3_bytes);
        }
        assert_eq!(
            decode_binary_sound(&[0x00, 0x01, 0x02, 0x03], 8, &settings(0x2b, 576, Some(4)))
                .unwrap_err()
                .code,
            "fla_sound_format"
        );

        // ADPCM (1), Nellymoser (4, 5, 6), GSM (9) produce explicit unsupported codec error:
        for codec in [1_u8, 4, 5, 6, 9] {
            assert_eq!(
                decode_binary_sound(&[1, 2, 3, 4], 8, &settings(codec << 4, 4, Some(4)))
                    .unwrap_err()
                    .code,
                "fla_sound_codec_unsupported"
            );
        }
    }
}
