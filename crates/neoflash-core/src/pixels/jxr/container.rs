use super::bitstream::BitReader;
use super::decode::*;
use super::encode::*;
use super::*;

// ============================================================================
// Public ITU-T Rec. T.832 `.jxr` Container Encoder & Decoder
// ============================================================================

/// Encodes big-endian straight `ARGB8` pixels (`[A, R, G, B]` per pixel) into a spec-conformant
/// ITU-T Rec. T.832 / ISO/IEC 29199-2 `.jxr` container.
///
/// Uses `GUID_PKPixelFormat24bppBGR` when `transparent == false` and `GUID_PKPixelFormat32bppBGRA`
/// with a planar `ALPHA_OFFSET` (`0xBCC2`) / `ALPHA_BYTE_COUNT` (`0xBCC3`) codestream when
/// `transparent == true` (matching `jxrlib` / Flash `BitmapData.encode` baseline conventions).
pub fn encode_jxr_argb(
    width: u32,
    height: u32,
    transparent: bool,
    argb_be: &[u8],
    quantization: u8,
    color_space: &str,
    trim_flex_bits: u8,
) -> Result<Vec<u8>, BitmapError> {
    encode_jxr_with_alpha_mode(
        width,
        height,
        if transparent {
            JxrAlphaMode::Planar
        } else {
            JxrAlphaMode::None
        },
        argb_be,
        quantization,
        color_space,
        trim_flex_bits,
    )
}

/// Encodes big-endian straight `ARGB8` pixels into a `.jxr` container using the requested alpha layout
/// (`None` -> `24bppBGR`, `Planar` -> `32bppBGRA` with `ALPHA_OFFSET` / `ALPHA_BYTE_COUNT`,
/// `Interleaved` -> `32bppBGRA` with `ALPHA_IMAGE_PLANE_FLAG = 1`).
pub fn encode_jxr_with_alpha_mode(
    width: u32,
    height: u32,
    alpha_mode: JxrAlphaMode,
    argb_be: &[u8],
    quantization: u8,
    color_space: &str,
    trim_flex_bits: u8,
) -> Result<Vec<u8>, BitmapError> {
    if width == 0 || height == 0 {
        return Err(error(
            "JPEG-XR encoding requires a nonempty clipped rectangle",
        ));
    }
    if width > MAX_BITMAP_DIMENSION || height > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR dimensions must be in 1..=4096"));
    }
    if trim_flex_bits > 15 {
        return Err(error("JPEGXREncoderOptions.trimFlexBits requires 0..=15"));
    }
    let subsampling = ChromaSubsampling::from_color_space(color_space)?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| error("JPEG-XR pixel buffer size overflow"))?;
    if argb_be.len() != expected {
        return Err(error("JPEG-XR ARGB input buffer length mismatch"));
    }

    let has_alpha = alpha_mode != JxrAlphaMode::None;
    let interleaved = alpha_mode == JxrAlphaMode::Interleaved;
    let primary_stream = encode_primary_codestream(
        width,
        height,
        argb_be,
        subsampling,
        quantization,
        trim_flex_bits,
        interleaved,
    );
    let planar_alpha_stream = if alpha_mode == JxrAlphaMode::Planar {
        Some(encode_planar_alpha_codestream(
            width,
            height,
            argb_be,
            quantization,
            trim_flex_bits,
        ))
    } else {
        None
    };

    let has_planar_tags = planar_alpha_stream.is_some();
    let num_entries: u16 = if has_planar_tags { 10 } else { 8 };
    let ifd_offset: u32 = 8;
    let ifd_bytes = 2 + usize::from(num_entries) * 12 + 4;
    let guid_offset = (ifd_offset as usize + ifd_bytes) as u32;
    let image_offset = guid_offset + 16;
    let image_byte_count = primary_stream.len() as u32;
    let (alpha_offset, alpha_byte_count) = match &planar_alpha_stream {
        Some(alpha_bytes) => (image_offset + image_byte_count, alpha_bytes.len() as u32),
        None => (0u32, 0u32),
    };

    let total_len = image_offset as usize
        + primary_stream.len()
        + planar_alpha_stream.as_ref().map_or(0, Vec::len);
    let mut out = Vec::with_capacity(total_len);

    // 8-byte little-endian ITU-T T.832 Annex A container header
    out.extend_from_slice(&JXR_MAGIC);
    out.extend_from_slice(&ifd_offset.to_le_bytes());

    // IFD entry count + sorted 12-byte entries
    out.extend_from_slice(&num_entries.to_le_bytes());
    let mut write_ifd_entry = |tag: u16, field_type: u16, count: u32, val_or_off: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&val_or_off.to_le_bytes());
    };

    write_ifd_entry(TAG_PIXEL_FORMAT, TIFF_TYPE_BYTE, 16, guid_offset);
    write_ifd_entry(TAG_TRANSFORMATION, TIFF_TYPE_LONG, 1, 0);
    write_ifd_entry(TAG_IMAGE_WIDTH, TIFF_TYPE_LONG, 1, width);
    write_ifd_entry(TAG_IMAGE_HEIGHT, TIFF_TYPE_LONG, 1, height);
    write_ifd_entry(TAG_WIDTH_RESOLUTION, TIFF_TYPE_FLOAT, 1, 96.0_f32.to_bits());
    write_ifd_entry(
        TAG_HEIGHT_RESOLUTION,
        TIFF_TYPE_FLOAT,
        1,
        96.0_f32.to_bits(),
    );
    write_ifd_entry(TAG_IMAGE_OFFSET, TIFF_TYPE_LONG, 1, image_offset);
    write_ifd_entry(TAG_IMAGE_BYTE_COUNT, TIFF_TYPE_LONG, 1, image_byte_count);
    if has_planar_tags {
        write_ifd_entry(TAG_ALPHA_OFFSET, TIFF_TYPE_LONG, 1, alpha_offset);
        write_ifd_entry(TAG_ALPHA_BYTE_COUNT, TIFF_TYPE_LONG, 1, alpha_byte_count);
    }

    // Next IFD offset = 0
    out.extend_from_slice(&0u32.to_le_bytes());

    // 16-byte WIC PIXEL_FORMAT GUID
    let guid = if has_alpha {
        &GUID_PIXEL_FORMAT_32BPP_BGRA
    } else {
        &GUID_PIXEL_FORMAT_24BPP_BGR
    };
    out.extend_from_slice(guid);

    // Primary + optional planar alpha codestreams
    out.extend_from_slice(&primary_stream);
    if let Some(alpha_bytes) = &planar_alpha_stream {
        out.extend_from_slice(alpha_bytes);
    }

    Ok(out)
}

#[derive(Debug)]
pub(super) struct ParsedJxrContainer {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) has_alpha_guid: bool,
    pub(super) is_premultiplied: bool,
    pub(super) image_offset: usize,
    pub(super) image_end: usize,
    pub(super) alpha_offset: usize,
    pub(super) alpha_end: usize,
}

pub(super) fn parse_jxr_container(bytes: &[u8]) -> Result<ParsedJxrContainer, BitmapError> {
    if bytes.len() < 16 || bytes[..4] != JXR_MAGIC {
        return Err(error(
            "not a valid little-endian ITU-T T.832 .jxr container",
        ));
    }
    let ifd_offset = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if ifd_offset < 8 || ifd_offset + 2 > bytes.len() {
        return Err(error("JPEG-XR IFD offset out of bounds"));
    }
    let num_entries = u16::from_le_bytes([bytes[ifd_offset], bytes[ifd_offset + 1]]) as usize;
    let entries_end = ifd_offset
        .checked_add(2)
        .and_then(|v| v.checked_add(num_entries.checked_mul(12)?))
        .and_then(|v| v.checked_add(4))
        .ok_or_else(|| error("JPEG-XR IFD table overflow"))?;
    if entries_end > bytes.len() {
        return Err(error("truncated JPEG-XR IFD table"));
    }

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut has_alpha_guid = false;
    let mut is_premultiplied = false;
    let mut saw_pixel_format = false;
    let mut image_offset: Option<usize> = None;
    let mut image_byte_count: Option<usize> = None;
    let mut alpha_offset: usize = 0;
    let mut alpha_byte_count: usize = 0;

    let read_u32_tag = |ftype: u16, count: u32, raw_val: [u8; 4]| -> Result<u32, BitmapError> {
        if count != 1 {
            return Err(error("JPEG-XR scalar IFD tag requires count == 1"));
        }
        match ftype {
            TIFF_TYPE_BYTE => Ok(u32::from(raw_val[0])),
            TIFF_TYPE_SHORT => Ok(u32::from(u16::from_le_bytes([raw_val[0], raw_val[1]]))),
            TIFF_TYPE_LONG => Ok(u32::from_le_bytes(raw_val)),
            _ => Err(error("unsupported TIFF field type for JPEG-XR integer tag")),
        }
    };

    for i in 0..num_entries {
        let pos = ifd_offset + 2 + i * 12;
        let tag = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        let ftype = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
        let count = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]);
        let raw_val = [
            bytes[pos + 8],
            bytes[pos + 9],
            bytes[pos + 10],
            bytes[pos + 11],
        ];
        let val_u32 = u32::from_le_bytes(raw_val);

        match tag {
            TAG_PIXEL_FORMAT => {
                if ftype != TIFF_TYPE_BYTE || count != 16 {
                    return Err(error("JPEG-XR PIXEL_FORMAT tag requires 16 BYTEs"));
                }
                let goff = val_u32 as usize;
                if goff + 16 > bytes.len() {
                    return Err(error("JPEG-XR PIXEL_FORMAT GUID offset out of bounds"));
                }
                let guid = &bytes[goff..goff + 16];
                if guid == GUID_PIXEL_FORMAT_32BPP_RGB {
                    has_alpha_guid = false;
                } else if guid == GUID_PIXEL_FORMAT_32BPP_RGBA {
                    has_alpha_guid = true;
                } else if guid[..15] == GUID_PREFIX_WIC {
                    match guid[15] {
                        0x08 | 0x0C | 0x0D | 0x0E => {
                            // 8bppGray, 24bppBGR, 24bppRGB, 32bppBGR
                            has_alpha_guid = false;
                        }
                        0x0F | 0x1E => {
                            // 32bppBGRA, 32bppRGBA (legacy subtype)
                            has_alpha_guid = true;
                        }
                        0x10 | 0x1F => {
                            // 32bppPBGRA, 32bppPRGBA
                            has_alpha_guid = true;
                            is_premultiplied = true;
                        }
                        _ => return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID subtype")),
                    }
                } else {
                    return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID"));
                }
                saw_pixel_format = true;
            }
            TAG_IMAGE_WIDTH => width = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_HEIGHT => height = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_OFFSET | TAG_IMAGE_OFFSET_ALT => {
                image_offset = Some(read_u32_tag(ftype, count, raw_val)? as usize);
            }
            TAG_IMAGE_BYTE_COUNT | TAG_IMAGE_BYTE_COUNT_ALT => {
                image_byte_count = Some(read_u32_tag(ftype, count, raw_val)? as usize);
            }
            TAG_ALPHA_OFFSET | TAG_ALPHA_OFFSET_ALT => {
                alpha_offset = read_u32_tag(ftype, count, raw_val)? as usize;
            }
            TAG_ALPHA_BYTE_COUNT | TAG_ALPHA_BYTE_COUNT_ALT => {
                alpha_byte_count = read_u32_tag(ftype, count, raw_val)? as usize;
            }
            _ => {}
        }
    }

    if !saw_pixel_format {
        return Err(error("missing JPEG-XR PIXEL_FORMAT IFD tag (0xBC01)"));
    }
    let image_offset =
        image_offset.ok_or_else(|| error("missing JPEG-XR IMAGE_OFFSET (0xBCC0)"))?;
    let image_byte_count =
        image_byte_count.ok_or_else(|| error("missing JPEG-XR IMAGE_BYTE_COUNT (0xBCC1)"))?;
    if image_offset >= bytes.len() {
        return Err(error("JPEG-XR IMAGE_OFFSET out of bounds"));
    }
    // Note: jxrlib's ReadContainer permits byte_count == 0 (unbounded to EOF) or clamped to EOF.
    let image_end = if image_byte_count == 0 {
        if alpha_offset > image_offset && alpha_offset <= bytes.len() {
            alpha_offset
        } else {
            bytes.len()
        }
    } else {
        let end = image_offset
            .checked_add(image_byte_count)
            .ok_or_else(|| error("JPEG-XR IMAGE_OFFSET overflow"))?;
        if end > bytes.len() {
            return Err(error("JPEG-XR primary codestream range out of bounds"));
        }
        end
    };
    if image_end.saturating_sub(image_offset) < 16 {
        return Err(error("JPEG-XR primary codestream too short"));
    }

    let alpha_end = if alpha_offset > 0 {
        if alpha_offset >= bytes.len() || alpha_byte_count < 16 {
            return Err(error("JPEG-XR planar alpha codestream range out of bounds"));
        }
        // Note: jxrlib's JXRGlueJxr.c WriteContainerPost writes deAlphaByteCount = nCbAlpha + nOffAlpha;
        // clamp to bytes.len() when alpha_offset + alpha_byte_count overshoots EOF.
        let end = bytes
            .len()
            .min(alpha_offset.saturating_add(alpha_byte_count));
        if end.saturating_sub(alpha_offset) < 16 {
            return Err(error("JPEG-XR planar alpha codestream too short"));
        }
        end
    } else {
        0
    };

    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            let mut br = BitReader::new(&bytes[image_offset..image_end]);
            let hdr = read_wmi_image_header(&mut br)?;
            if hdr.orientation >= 4 {
                (hdr.height, hdr.width)
            } else {
                (hdr.width, hdr.height)
            }
        }
    };
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR image dimensions must be in 1..=4096"));
    }

    Ok(ParsedJxrContainer {
        width: w,
        height: h,
        has_alpha_guid,
        is_premultiplied,
        image_offset,
        image_end,
        alpha_offset,
        alpha_end,
    })
}

/// Reads `(width, height)` from an ITU-T T.832 `.jxr` container without decoding macroblocks.
pub fn jxr_dimensions(bytes: &[u8]) -> Result<(u32, u32), BitmapError> {
    let parsed = parse_jxr_container(bytes)?;
    Ok((parsed.width, parsed.height))
}

/// Decodes an ITU-T T.832 `.jxr` container into straight `RGBA8` (`DecodedImage`).
pub fn decode_jxr_to_rgba(bytes: &[u8]) -> Result<DecodedImage, BitmapError> {
    let container = parse_jxr_container(bytes)?;
    let primary_slice = &bytes[container.image_offset..container.image_end];
    let mut decoded = decode_codestream(primary_slice)?;
    if let Some(y_samples) = decoded.y_only_samples.take() {
        decoded.rgba = Vec::with_capacity(y_samples.len() * 4);
        for y in y_samples {
            decoded.rgba.extend_from_slice(&[y, y, y, 255]);
        }
    }
    if decoded.width != container.width || decoded.height != container.height {
        return Err(error(
            "JPEG-XR IFD dimensions do not match IMAGE_HEADER dimensions",
        ));
    }

    if !container.has_alpha_guid {
        for px in decoded.rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    } else if !decoded.has_interleaved_alpha && container.alpha_offset > 0 {
        let alpha_slice = &bytes[container.alpha_offset..container.alpha_end];
        let alpha_dec = decode_codestream(alpha_slice)?;
        let Some(alpha_samples) = alpha_dec.y_only_samples else {
            return Err(error(
                "JPEG-XR planar alpha stream did not decode as Y_ONLY",
            ));
        };
        if alpha_dec.width != decoded.width
            || alpha_dec.height != decoded.height
            || alpha_samples.len() * 4 != decoded.rgba.len()
        {
            return Err(error("JPEG-XR planar alpha dimensions mismatch"));
        }
        for (px, &a) in decoded
            .rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&alpha_samples)
        {
            px[3] = a;
        }
    }

    if container.is_premultiplied {
        for px in decoded.rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            if a == 0 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            } else if a < 255 {
                px[0] = ((u32::from(px[0]) * 255 + a / 2) / a).min(255) as u8;
                px[1] = ((u32::from(px[1]) * 255 + a / 2) / a).min(255) as u8;
                px[2] = ((u32::from(px[2]) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }

    Ok(DecodedImage {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba.into(),
    })
}
