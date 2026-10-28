//! Bounded legacy bitmap decoding. Wire-layout facts are documented by JPEXS
//! FLA Convert 0923d708, BitmapRecordDecoder/XflBitmapWriter; this reader uses
//! independent checked slices and the shared Rust image/compression libraries.
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
}
