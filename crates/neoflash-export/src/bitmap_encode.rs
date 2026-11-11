use crate::ExportError;
use base64::{Engine, engine::general_purpose::STANDARD};
use neoflash_core::{ElementKind, Project};

/// Pure-Rust FIPS 180-4 SHA-256 digest for content-addressed asset caching and deduplication.
#[must_use]
pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Lowercase 64-character hex string for [`sha256_bytes`].
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256_bytes(data);
    let mut s = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(s, "{byte:02x}");
    }
    s
}

/// Summary of bitmap re-encoding, content-hash cache reuse, and deduplication during publish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitmapPublishStats {
    pub inspected: usize,
    pub reencoded: usize,
    pub cache_hits: usize,
    pub deduplicated: usize,
    pub saved_bytes: isize,
}

type BitmapCacheKey = ([u8; 32], u8, u8);
type BitmapCacheVal = (String, std::sync::Arc<str>, usize, bool);

static BITMAP_ENCODE_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<BitmapCacheKey, BitmapCacheVal>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Applies personal Library [`neoflash_core::BitmapExportSettings`] (`Photo JPEG` custom or
/// document-default quality, `Lossless PNG`, `WebP`, and `Allow smoothing`) and global publish
/// JPEG quality on a derived publish copy, reusing cached re-encodes by SHA-256 content digest
/// and sharing `Arc<str>` payloads across identical bitmaps (`T-251`).
pub fn apply_bitmap_export_settings(
    project: &mut Project,
    default_quality: u8,
) -> (BitmapPublishStats, Vec<String>) {
    let quality = default_quality.clamp(1, 100);
    let mut stats = BitmapPublishStats::default();
    let mut warnings = Vec::new();
    let mut smoothing_overrides: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();
    let mut seen_outputs: std::collections::HashMap<[u8; 32], std::sync::Arc<str>> =
        std::collections::HashMap::new();

    for resource in &mut project.resources {
        if !resource.media_type.starts_with("image/") {
            continue;
        }
        stats.inspected += 1;
        if let Some(settings) = resource.bitmap_export {
            smoothing_overrides.insert(resource.id.clone(), settings.allow_smoothing);
        }
        let target: Option<(u8, u8)> = match resource.bitmap_export {
            Some(settings) => match settings.compression {
                neoflash_core::BitmapCompression::PhotoJpeg => {
                    if !settings.use_imported_jpeg_data {
                        Some((1, settings.jpeg_quality.clamp(1, 100)))
                    } else if quality != 80 || resource.media_type != "image/jpeg" {
                        Some((1, quality))
                    } else {
                        None
                    }
                }
                neoflash_core::BitmapCompression::LosslessPng => {
                    (resource.media_type != "image/png").then_some((2, 100))
                }
                neoflash_core::BitmapCompression::Webp => {
                    (resource.media_type != "image/webp").then_some((3, 100))
                }
            },
            None => {
                if quality != 80 && resource.media_type == "image/jpeg" {
                    Some((1, quality))
                } else if matches!(
                    resource.media_type.as_str(),
                    "image/bmp" | "image/x-bmp" | "image/tiff"
                ) {
                    Some((2, 100))
                } else {
                    None
                }
            }
        };

        let Ok(raw) = STANDARD.decode(resource.data_base64.as_ref()) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let raw_len = raw.len();

        let Some((mode_tag, target_q)) = target else {
            let digest = sha256_bytes(&raw);
            if let Some(shared_arc) = seen_outputs.get(&digest) {
                resource.data_base64 = shared_arc.clone();
                stats.deduplicated += 1;
            } else {
                seen_outputs.insert(digest, resource.data_base64.clone());
            }
            continue;
        };

        let src_digest = sha256_bytes(&raw);
        let cache_key: BitmapCacheKey = (src_digest, mode_tag, target_q);

        if let Some((cached_mime, cached_b64, out_len, cached_has_alpha)) = BITMAP_ENCODE_CACHE
            .lock()
            .ok()
            .and_then(|guard| guard.get(&cache_key).cloned())
        {
            if mode_tag == 1 && cached_has_alpha {
                warnings.push(format!(
                    "Bitmap '{}' has translucent alpha pixels; Photo JPEG flattens alpha onto opaque RGB (switch its Library Bitmap Properties to Lossless PNG or WebP to preserve transparency).",
                    resource.path
                ));
            }
            stats.cache_hits += 1;
            stats.saved_bytes += raw_len as isize - out_len as isize;
            let out_digest = sha256_bytes(cached_b64.as_bytes());
            if let Some(shared) = seen_outputs.get(&out_digest) {
                resource.data_base64 = shared.clone();
                stats.deduplicated += 1;
            } else {
                seen_outputs.insert(out_digest, cached_b64.clone());
                resource.data_base64 = cached_b64;
            }
            resource.media_type = cached_mime;
            continue;
        }

        let Ok(img) = image::load_from_memory(&raw) else {
            continue;
        };
        let has_alpha = img.to_rgba8().pixels().any(|px| px[3] < 255);
        if mode_tag == 1 && has_alpha {
            warnings.push(format!(
                "Bitmap '{}' has translucent alpha pixels; Photo JPEG flattens alpha onto opaque RGB (switch its Library Bitmap Properties to Lossless PNG or WebP to preserve transparency).",
                resource.path
            ));
        }
        let mut out = Vec::new();
        let out_mime = match mode_tag {
            1 => {
                let rgb = img.to_rgb8();
                let mut encoder =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, target_q);
                if encoder
                    .encode(
                        rgb.as_raw(),
                        rgb.width(),
                        rgb.height(),
                        image::ExtendedColorType::Rgb8,
                    )
                    .is_err()
                {
                    continue;
                }
                "image/jpeg"
            }
            2 => {
                let rgba = img.to_rgba8();
                let encoder = image::codecs::png::PngEncoder::new(&mut out);
                if image::ImageEncoder::write_image(
                    encoder,
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .is_err()
                {
                    continue;
                }
                "image/png"
            }
            3 => {
                let rgba = img.to_rgba8();
                let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
                if encoder
                    .encode(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .is_err()
                {
                    continue;
                }
                "image/webp"
            }
            _ => continue,
        };
        if out.len() <= neoflash_core::MAX_RESOURCE_BYTES {
            let b64_arc: std::sync::Arc<str> = STANDARD.encode(&out).into();
            let out_len = out.len();
            stats.reencoded += 1;
            stats.saved_bytes += raw_len as isize - out_len as isize;
            if let Ok(mut guard) = BITMAP_ENCODE_CACHE.lock() {
                if guard.len() >= 256 {
                    guard.clear();
                }
                guard.insert(
                    cache_key,
                    (out_mime.to_owned(), b64_arc.clone(), out_len, has_alpha),
                );
            }
            let out_digest = sha256_bytes(b64_arc.as_bytes());
            if let Some(shared) = seen_outputs.get(&out_digest) {
                resource.data_base64 = shared.clone();
                stats.deduplicated += 1;
            } else {
                seen_outputs.insert(out_digest, b64_arc.clone());
                resource.data_base64 = b64_arc;
            }
            resource.media_type = out_mime.to_owned();
        }
    }

    if !smoothing_overrides.is_empty() {
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    for el in &mut key.elements {
                        if let ElementKind::Bitmap {
                            resource_id,
                            smoothing,
                            ..
                        } = &mut el.kind
                            && let Some(&allow) = smoothing_overrides.get(resource_id)
                        {
                            *smoothing = allow;
                        }
                    }
                    for bf in &mut key.bitmap_fills {
                        if let Some(&allow) = smoothing_overrides.get(&bf.resource_id) {
                            bf.smoothing = allow;
                        }
                    }
                }
            }
        }
    }

    (stats, warnings)
}

/// Encode an RGBA8 buffer to lossless PNG bytes (`T-260` shared export helper).
pub(crate) fn encode_rgba_png(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ExportError> {
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return Err(ExportError::InvalidProject(format!(
            "Invalid RGBA buffer dimensions {width}x{height} ({} bytes)",
            rgba.len()
        )));
    }
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(
        encoder,
        rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| ExportError::InvalidProject(format!("PNG encode failed: {e}")))?;
    Ok(out)
}

/// Composite an RGBA8 buffer over `bg` and encode as JPEG at `quality` (`1..=100`) (`T-260`).
pub(crate) fn encode_rgba_jpeg_composited(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    bg: [u8; 4],
) -> Result<Vec<u8>, ExportError> {
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return Err(ExportError::InvalidProject(format!(
            "Invalid RGBA buffer dimensions {width}x{height} ({} bytes)",
            rgba.len()
        )));
    }
    let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for px in rgba.as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        let inv = 255 - a;
        rgb.push(((u16::from(px[0]) * a + u16::from(bg[0]) * inv + 127) / 255) as u8);
        rgb.push(((u16::from(px[1]) * a + u16::from(bg[1]) * inv + 127) / 255) as u8);
        rgb.push(((u16::from(px[2]) * a + u16::from(bg[2]) * inv + 127) / 255) as u8);
    }
    let mut out = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    encoder
        .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| ExportError::InvalidProject(format!("JPEG encode failed: {e}")))?;
    Ok(out)
}

/// Encode an RGBA8 buffer to lossless WebP bytes (`T-260` shared export helper).
pub(crate) fn encode_rgba_webp_lossless(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ExportError> {
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return Err(ExportError::InvalidProject(format!(
            "Invalid RGBA buffer dimensions {width}x{height} ({} bytes)",
            rgba.len()
        )));
    }
    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
    encoder
        .encode(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::InvalidProject(format!("WebP encode failed: {e}")))?;
    Ok(out)
}
