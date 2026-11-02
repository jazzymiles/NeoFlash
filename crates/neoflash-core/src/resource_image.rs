//! Bounded PNG/JPEG decoding, shared between immutable authoring and playback.
use crate::{MAX_BITMAP_DIMENSION, ResourceError, ResourceUnit};
use image::{ImageFormat, ImageReader, Limits};
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
struct Cached {
    source: Weak<str>,
    image: Arc<DecodedImage>,
}
const CACHE_BYTES: usize = 64 * 1024 * 1024;
fn cache() -> &'static Mutex<VecDeque<Cached>> {
    static CACHE: OnceLock<Mutex<VecDeque<Cached>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}
impl ResourceUnit {
    pub(crate) fn image_dimensions(&self) -> Result<(u32, u32), ResourceError> {
        let bytes = self.decoded_bytes()?;
        let format =
            image::guess_format(&bytes).map_err(|e| ResourceError(format!("image format: {e}")))?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
            return Err(ResourceError(
                "only PNG and JPEG images are supported".into(),
            ));
        }
        let (w, h) = ImageReader::with_format(Cursor::new(bytes), format)
            .into_dimensions()
            .map_err(|e| ResourceError(format!("image dimensions: {e}")))?;
        if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
            return Err(ResourceError("image dimensions must be in 1..=4096".into()));
        }
        Ok((w, h))
    }

    pub fn decode_image(&self) -> Result<Arc<DecodedImage>, ResourceError> {
        {
            let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|item| item.source.strong_count() > 0);
            if let Some(index) = cache
                .iter()
                .position(|item| item.source.ptr_eq(&Arc::downgrade(&self.data_base64)))
            {
                let item = cache.remove(index).expect("cache index");
                let image = item.image.clone();
                cache.push_back(item);
                return Ok(image);
            }
        }
        let bytes = self.decoded_bytes()?;
        let result = Arc::new(decode_encoded_image(&bytes)?);
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        let mut bytes: usize = cache.iter().map(|item| item.image.rgba.len()).sum();
        while bytes + result.rgba.len() > CACHE_BYTES {
            let Some(item) = cache.pop_front() else { break };
            bytes -= item.image.rgba.len();
        }
        cache.push_back(Cached {
            source: Arc::downgrade(&self.data_base64),
            image: result.clone(),
        });
        Ok(result)
    }
}

/// Decode a portable encoded image without filesystem access or the authoring cache.
pub(crate) fn encoded_image_dimensions(bytes: &[u8]) -> Result<(u32, u32), ResourceError> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(ResourceError(
            "encoded image must be within 1..=16 MiB".into(),
        ));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return gif_dimensions(bytes);
    }
    let format =
        image::guess_format(bytes).map_err(|e| ResourceError(format!("image format: {e}")))?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(ResourceError(
            "only PNG, JPEG, and GIF images are supported".into(),
        ));
    }
    let (w, h) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|e| ResourceError(format!("image dimensions: {e}")))?;
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(ResourceError("image dimensions must be in 1..=4096".into()));
    }
    Ok((w, h))
}
pub(crate) fn decode_encoded_image(bytes: &[u8]) -> Result<DecodedImage, ResourceError> {
    encoded_image_dimensions(bytes)?;
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return decode_gif(bytes);
    }
    let format = image::guess_format(bytes).map_err(|e| ResourceError(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_BITMAP_DIMENSION);
    limits.max_image_height = Some(MAX_BITMAP_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| ResourceError(format!("bounded image decode: {e}")))?
        .into_rgba8();
    Ok(DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}

fn gif_dimensions(bytes: &[u8]) -> Result<(u32, u32), ResourceError> {
    if bytes.len() < 13 {
        return Err(ResourceError("truncated GIF header".into()));
    }
    let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(ResourceError("image dimensions must be in 1..=4096".into()));
    }
    Ok((w, h))
}

fn decode_gif(bytes: &[u8]) -> Result<DecodedImage, ResourceError> {
    let (width, height) = gif_dimensions(bytes)?;
    let packed = bytes[10];
    let has_gct = (packed & 0x80) != 0;
    let gct_size = if has_gct {
        1usize << ((packed & 0x07) + 1)
    } else {
        0
    };
    let mut pos = 13usize;
    let mut gct = vec![[0u8, 0, 0, 255]; 256];
    if has_gct {
        let gct_bytes = gct_size * 3;
        if pos + gct_bytes > bytes.len() {
            return Err(ResourceError("truncated GIF global color table".into()));
        }
        for (i, entry) in gct.iter_mut().enumerate().take(gct_size) {
            let off = pos + i * 3;
            *entry = [bytes[off], bytes[off + 1], bytes[off + 2], 255];
        }
        pos += gct_bytes;
    }

    let mut transparent_idx: Option<u8> = None;
    let mut canvas = vec![0u8; (width as usize) * (height as usize) * 4];
    let bg_index = bytes[11] as usize;
    if has_gct && bg_index < gct_size {
        let bg = gct[bg_index];
        for px in canvas.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&bg);
        }
    }

    while pos < bytes.len() {
        let introducer = bytes[pos];
        pos += 1;
        match introducer {
            0x3B => break, // GIF trailer
            0x21 => {
                // Extension block
                if pos >= bytes.len() {
                    break;
                }
                let label = bytes[pos];
                pos += 1;
                if label == 0xF9 && pos < bytes.len() {
                    let block_size = bytes[pos] as usize;
                    pos += 1;
                    if pos + block_size <= bytes.len() && block_size >= 4 {
                        let gce_packed = bytes[pos];
                        if (gce_packed & 0x01) != 0 {
                            transparent_idx = Some(bytes[pos + 3]);
                        }
                    }
                    pos = (pos + block_size).min(bytes.len());
                    if pos < bytes.len() && bytes[pos] == 0 {
                        pos += 1;
                    }
                } else {
                    while pos < bytes.len() {
                        let sub_len = bytes[pos] as usize;
                        pos += 1;
                        if sub_len == 0 {
                            break;
                        }
                        pos = (pos + sub_len).min(bytes.len());
                    }
                }
            }
            0x2C => {
                // Image descriptor
                if pos + 9 > bytes.len() {
                    return Err(ResourceError("truncated GIF image descriptor".into()));
                }
                let left = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as u32;
                let top = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]) as u32;
                let img_w = u16::from_le_bytes([bytes[pos + 4], bytes[pos + 5]]) as u32;
                let img_h = u16::from_le_bytes([bytes[pos + 6], bytes[pos + 7]]) as u32;
                let img_packed = bytes[pos + 8];
                pos += 9;
                let has_lct = (img_packed & 0x80) != 0;
                let interlaced = (img_packed & 0x40) != 0;
                let mut palette = gct.clone();
                if has_lct {
                    let lct_size = 1usize << ((img_packed & 0x07) + 1);
                    let lct_bytes = lct_size * 3;
                    if pos + lct_bytes > bytes.len() {
                        return Err(ResourceError("truncated GIF local color table".into()));
                    }
                    for (i, entry) in palette.iter_mut().enumerate().take(lct_size) {
                        let off = pos + i * 3;
                        *entry = [bytes[off], bytes[off + 1], bytes[off + 2], 255];
                    }
                    pos += lct_bytes;
                }
                if pos >= bytes.len() {
                    return Err(ResourceError("missing GIF LZW min code size".into()));
                }
                let lzw_min_code_size = bytes[pos];
                pos += 1;
                let mut lzw_data = Vec::new();
                while pos < bytes.len() {
                    let sub_len = bytes[pos] as usize;
                    pos += 1;
                    if sub_len == 0 {
                        break;
                    }
                    let end = (pos + sub_len).min(bytes.len());
                    lzw_data.extend_from_slice(&bytes[pos..end]);
                    pos = end;
                }
                let indices = decode_gif_lzw(
                    lzw_min_code_size,
                    &lzw_data,
                    (img_w as usize).saturating_mul(img_h as usize),
                )?;
                let mut row_map = Vec::with_capacity(img_h as usize);
                if interlaced {
                    for (start, step) in [(0u32, 8u32), (4, 8), (2, 4), (1, 2)] {
                        let mut y = start;
                        while y < img_h {
                            row_map.push(y);
                            y += step;
                        }
                    }
                } else {
                    row_map.extend(0..img_h);
                }
                for (src_row, &dst_rel_y) in row_map.iter().enumerate() {
                    let dst_y = top + dst_rel_y;
                    if dst_y >= height {
                        continue;
                    }
                    for rel_x in 0..img_w {
                        let dst_x = left + rel_x;
                        if dst_x >= width {
                            continue;
                        }
                        let idx_pos = src_row * (img_w as usize) + (rel_x as usize);
                        let Some(&color_idx) = indices.get(idx_pos) else {
                            continue;
                        };
                        let dst_off = ((dst_y * width + dst_x) as usize) * 4;
                        if Some(color_idx) == transparent_idx {
                            canvas[dst_off..dst_off + 4].copy_from_slice(&[0, 0, 0, 0]);
                        } else {
                            canvas[dst_off..dst_off + 4]
                                .copy_from_slice(&palette[color_idx as usize]);
                        }
                    }
                }
                break; // First frame decoded
            }
            _ => break,
        }
    }

    Ok(DecodedImage {
        width,
        height,
        rgba: canvas.into(),
    })
}

fn decode_gif_lzw(
    min_code_size: u8,
    data: &[u8],
    expected_pixels: usize,
) -> Result<Vec<u8>, ResourceError> {
    if !(2..=8).contains(&min_code_size) {
        return Err(ResourceError("invalid GIF LZW code size".into()));
    }
    let clear_code = 1u16 << min_code_size;
    let end_code = clear_code + 1;
    let mut code_size = (min_code_size + 1) as u32;
    let mut next_code = end_code + 1;
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut length = [0u16; 4096];
    for i in 0..clear_code {
        suffix[i as usize] = i as u8;
        length[i as usize] = 1;
    }

    let mut bit_buf: u32 = 0;
    let mut bit_count: u32 = 0;
    let mut byte_pos = 0usize;
    let mut out = Vec::with_capacity(expected_pixels);
    let mut prev_code: Option<u16> = None;
    let mut first_byte: u8 = 0;

    while out.len() < expected_pixels {
        while bit_count < code_size {
            if byte_pos >= data.len() {
                return Ok(out);
            }
            bit_buf |= (data[byte_pos] as u32) << bit_count;
            bit_count += 8;
            byte_pos += 1;
        }
        let code = (bit_buf & ((1u32 << code_size) - 1)) as u16;
        bit_buf >>= code_size;
        bit_count -= code_size;

        if code == clear_code {
            code_size = (min_code_size + 1) as u32;
            next_code = end_code + 1;
            prev_code = None;
            continue;
        }
        if code == end_code {
            break;
        }

        let start_len = out.len();
        if code < next_code && (code < clear_code || code > end_code) {
            let seq_len = length[code as usize] as usize;
            out.resize(start_len + seq_len, 0);
            let mut cur = code;
            for pos in (start_len..start_len + seq_len).rev() {
                out[pos] = suffix[cur as usize];
                first_byte = out[pos];
                if cur < clear_code {
                    break;
                }
                cur = prefix[cur as usize];
            }
            if let Some(prev) = prev_code
                && (next_code as usize) < 4096
            {
                prefix[next_code as usize] = prev;
                suffix[next_code as usize] = first_byte;
                length[next_code as usize] = length[prev as usize].saturating_add(1);
                next_code += 1;
                if next_code == (1u16 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
            prev_code = Some(code);
        } else if code == next_code {
            let Some(prev) = prev_code else {
                return Err(ResourceError("corrupt GIF LZW stream".into()));
            };
            let seq_len = (length[prev as usize] as usize) + 1;
            out.resize(start_len + seq_len, 0);
            out[start_len + seq_len - 1] = first_byte;
            let mut cur = prev;
            for pos in (start_len..start_len + seq_len - 1).rev() {
                out[pos] = suffix[cur as usize];
                first_byte = out[pos];
                if cur < clear_code {
                    break;
                }
                cur = prefix[cur as usize];
            }
            if (next_code as usize) < 4096 {
                prefix[next_code as usize] = prev;
                suffix[next_code as usize] = first_byte;
                length[next_code as usize] = seq_len as u16;
                next_code += 1;
                if next_code == (1u16 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
            prev_code = Some(code);
        } else {
            return Err(ResourceError("invalid GIF LZW code".into()));
        }
    }

    out.truncate(expected_pixels);
    Ok(out)
}
