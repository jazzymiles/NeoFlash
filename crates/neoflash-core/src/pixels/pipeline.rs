//! Bulk transfer and original CPU image pipeline. All mutation remains in the
//! premultiplied arena; temporary rasters and PNG output have separate bounds.
use super::*;
use crate::{ColorTransform, FilterSpec, filter_padding, filter_rgba};
use image::{ExtendedColorType, ImageEncoder};
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitmapRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
impl BitmapRect {
    fn clipped(self, b: &Buffer) -> (usize, usize, usize, usize) {
        clip(self.x, self.y, self.width, self.height, b.width, b.height)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PixelWrite {
    pub consumed: usize,
    pub complete: bool,
}
impl PixelArena {
    pub(crate) fn histogram(
        &self,
        r: &PixelRef,
        rect: Option<BitmapRect>,
    ) -> Result<Vec<Vec<u32>>, BitmapError> {
        let b = self.live(r)?;
        let (x0, y0, x1, y1) = rect.map_or((0, 0, b.width as usize, b.height as usize), |r| {
            r.clipped(b)
        });
        let mut counts = vec![vec![0u32; 256]; 4];
        let pixels = b.pixels.as_ref().unwrap();
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                let p = straight(&pixels[i..i + 4]);
                for c in 0..4 {
                    counts[c][usize::from(p[c])] += 1;
                }
            }
        }
        Ok(counts)
    }
    pub(crate) fn get_pixels(
        &self,
        r: &PixelRef,
        rect: BitmapRect,
        little: bool,
    ) -> Result<Vec<u8>, BitmapError> {
        let b = self.live(r)?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        let mut result = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                let p = straight(&b.pixels.as_ref().unwrap()[i..i + 4]);
                let word = u32::from_be_bytes([p[3], p[0], p[1], p[2]]);
                result.extend_from_slice(&if little {
                    word.to_le_bytes()
                } else {
                    word.to_be_bytes()
                });
            }
        }
        Ok(result)
    }
    pub(crate) fn set_pixels(
        &mut self,
        r: &PixelRef,
        rect: BitmapRect,
        bytes: &[u8],
        little: bool,
    ) -> Result<PixelWrite, BitmapError> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(error("pixel input exceeds 64 MiB"));
        }
        let b = self.live_mut(r)?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        let count = (x1 - x0) * (y1 - y0);
        let available = count.min(bytes.len() / 4);
        for (index, word) in bytes.as_chunks::<4>().0.iter().take(available).enumerate() {
            let x = x0 + index % (x1 - x0);
            let y = y0 + index / (x1 - x0);
            let color = if little {
                u32::from_le_bytes(*word)
            } else {
                u32::from_be_bytes(*word)
            };
            let i = (y * b.width as usize + x) * 4;
            b.pixels.as_mut().unwrap()[i..i + 4].copy_from_slice(&premul(color, b.transparent));
        }
        if available > 0 {
            b.revision += 1;
        }
        // The caller advances its ByteArray by consumed, then reports EOFError.
        // A trailing incomplete word is not consumed, matching readUnsignedInt.
        Ok(PixelWrite {
            consumed: available * 4,
            complete: available == count,
        })
    }
    pub(crate) fn color_transform(
        &mut self,
        r: &PixelRef,
        rect: BitmapRect,
        color: ColorTransform,
    ) -> Result<(), BitmapError> {
        color.validate().map_err(|e| BitmapError(e.to_string()))?;
        let b = self.live_mut(r)?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                let p = color.apply(straight(&b.pixels.as_ref().unwrap()[i..i + 4]));
                b.pixels.as_mut().unwrap()[i..i + 4].copy_from_slice(&premul(
                    u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
                    b.transparent,
                ));
            }
        }
        if x1 > x0 && y1 > y0 {
            b.revision += 1;
        }
        Ok(())
    }
    pub(crate) fn generate_filter_rect(
        &self,
        r: &PixelRef,
        rect: BitmapRect,
        filter: &FilterSpec,
    ) -> Result<BitmapRect, BitmapError> {
        let b = self.live(r)?;
        let pad =
            filter_padding(std::slice::from_ref(filter)).map_err(|e| BitmapError(e.to_string()))?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        if x0 == x1 || y0 == y1 {
            return Ok(BitmapRect {
                x: x0 as i32,
                y: y0 as i32,
                width: 0,
                height: 0,
            });
        }
        Ok(BitmapRect {
            x: x0 as i32 - pad.left as i32,
            y: y0 as i32 - pad.top as i32,
            width: (x1 - x0) as i32 + pad.left as i32 + pad.right as i32,
            height: (y1 - y0) as i32 + pad.top as i32 + pad.bottom as i32,
        })
    }
    pub(crate) fn apply_filter(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: BitmapRect,
        dx: i32,
        dy: i32,
        filter: &FilterSpec,
    ) -> Result<(), BitmapError> {
        let dest = self.live(dst)?;
        let opaque = !dest.transparent;
        if opaque
            && matches!(
                filter,
                FilterSpec::Glow { .. }
                    | FilterSpec::DropShadow { .. }
                    | FilterSpec::Bevel { .. }
                    | FilterSpec::GradientGlow { .. }
                    | FilterSpec::GradientBevel { .. }
            )
        {
            return Err(error(
                "Glow, shadow, bevel and gradient filters require a transparent destination",
            ));
        }
        let affected = self.generate_filter_rect(src, rect, filter)?;
        if affected.width == 0 || affected.height == 0 {
            return Ok(());
        }
        let b = self.live(src)?;
        let pad =
            filter_padding(std::slice::from_ref(filter)).map_err(|e| BitmapError(e.to_string()))?;
        let w = b.width + pad.left + pad.right;
        let h = b.height + pad.top + pad.bottom;
        raster_budget(w, h)?;
        // Include neighbours outside sourceRect; crop only the result. This also
        // makes self-filtering immutable until the entire kernel has succeeded.
        let mut rgba = vec![0; w as usize * h as usize * 4];
        for y in 0..b.height as usize {
            for x in 0..b.width as usize {
                let i = (y * b.width as usize + x) * 4;
                let j = ((y + pad.top as usize) * w as usize + x + pad.left as usize) * 4;
                let mut p = straight(&b.pixels.as_ref().unwrap()[i..i + 4]);
                if opaque && matches!(filter, FilterSpec::Blur { .. }) {
                    p[3] = 255;
                }
                rgba[j..j + 4].copy_from_slice(&p);
            }
        }
        let resolved_filter;
        let active_filter = if let Some(mut disp) = filter.as_displacement()
            && let Some(ref map_ref) = disp.map_bitmap
            && disp.map_rgba.is_empty()
        {
            if let Ok(info) = self.info(map_ref)
                && !info.disposed
                && let Ok(bytes) = self.pixels(map_ref)
            {
                disp.map_width = info.width;
                disp.map_height = info.height;
                disp.map_rgba = bytes;
            }
            resolved_filter = FilterSpec::from_displacement(&disp);
            &resolved_filter
        } else {
            filter
        };
        let result = filter_rgba(w, h, &rgba, std::slice::from_ref(active_filter))
            .map_err(|e| BitmapError(e.to_string()))?;
        let b = self.live_mut(dst)?;
        let mut changed = false;
        for y in 0..affected.height {
            for x in 0..affected.width {
                let source_x = i64::from(affected.x) + i64::from(x);
                let source_y = i64::from(affected.y) + i64::from(y);
                let target_x = i64::from(dx) + source_x - i64::from(rect.x);
                let target_y = i64::from(dy) + source_y - i64::from(rect.y);
                if target_x < 0
                    || target_y < 0
                    || target_x >= i64::from(b.width)
                    || target_y >= i64::from(b.height)
                {
                    continue;
                }
                let i = ((source_y + i64::from(pad.top)) as usize * w as usize
                    + (source_x + i64::from(pad.left)) as usize)
                    * 4;
                let p = &result[i..i + 4];
                let j = (target_y as usize * b.width as usize + target_x as usize) * 4;
                b.pixels.as_mut().unwrap()[j..j + 4].copy_from_slice(&premul(
                    u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
                    b.transparent,
                ));
                changed = true;
            }
        }
        if changed {
            b.revision += 1;
        }
        Ok(())
    }
    pub(crate) fn encode_png(
        &self,
        r: &PixelRef,
        rect: BitmapRect,
        fast: bool,
    ) -> Result<Vec<u8>, BitmapError> {
        let b = self.live(r)?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        if x0 == x1 || y0 == y1 {
            return Err(error("PNG encoding requires a nonempty clipped rectangle"));
        }
        let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                rgba.extend_from_slice(&straight(&b.pixels.as_ref().unwrap()[i..i + 4]));
            }
        }
        let mut output = BoundedPng(Vec::new());
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        PngEncoder::new_with_quality(
            &mut output,
            if fast {
                CompressionType::Fast
            } else {
                CompressionType::Default
            },
            FilterType::Adaptive,
        )
        .write_image(
            &rgba,
            (x1 - x0) as u32,
            (y1 - y0) as u32,
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| BitmapError(format!("PNG encoding failed: {e}")))?;
        Ok(output.0)
    }
    pub(crate) fn encode_jpeg(
        &self,
        r: &PixelRef,
        rect: BitmapRect,
        quality: u8,
    ) -> Result<Vec<u8>, BitmapError> {
        if !(1..=100).contains(&quality) {
            return Err(error("JPEGEncoderOptions.quality requires 1..=100"));
        }
        let b = self.live(r)?;
        let (x0, y0, x1, y1) = rect.clipped(b);
        if x0 == x1 || y0 == y1 {
            return Err(error("JPEG encoding requires a nonempty clipped rectangle"));
        }
        let mut rgb = Vec::with_capacity((x1 - x0) * (y1 - y0) * 3);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                let px = straight(&b.pixels.as_ref().unwrap()[i..i + 4]);
                rgb.extend_from_slice(&px[..3]);
            }
        }
        let mut output = BoundedPng(Vec::new());
        use image::codecs::jpeg::JpegEncoder;
        JpegEncoder::new_with_quality(&mut output, quality)
            .write_image(
                &rgb,
                (x1 - x0) as u32,
                (y1 - y0) as u32,
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| BitmapError(format!("JPEG encoding failed: {e}")))?;
        Ok(output.0)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_rgba(
        &mut self,
        r: &PixelRef,
        w: u32,
        h: u32,
        rgba: &[u8],
        rect: Option<BitmapRect>,
        expected_revision: &str,
    ) -> Result<(), BitmapError> {
        self.draw_rgba_blended(r, w, h, rgba, rect, expected_revision, "normal")
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_rgba_blended(
        &mut self,
        r: &PixelRef,
        w: u32,
        h: u32,
        rgba: &[u8],
        rect: Option<BitmapRect>,
        expected_revision: &str,
        blend_mode: &str,
    ) -> Result<(), BitmapError> {
        if !matches!(
            blend_mode,
            "" | "normal"
                | "layer"
                | "multiply"
                | "screen"
                | "lighten"
                | "darken"
                | "difference"
                | "add"
                | "subtract"
                | "invert"
                | "alpha"
                | "erase"
                | "overlay"
                | "hardlight"
                | "shader"
        ) {
            return Err(error("unsupported BlendMode"));
        }
        raster_budget(w, h)?;
        let b = self.live_mut(r)?;
        if b.width != w || b.height != h || rgba.len() != w as usize * h as usize * 4 {
            return Err(error(
                "draw raster must match destination dimensions and RGBA byte length",
            ));
        }
        if b.revision.to_string() != expected_revision {
            return Err(error("draw destination changed during raster capture"));
        }
        let (x0, y0, x1, y1) = rect
            .unwrap_or(BitmapRect {
                x: 0,
                y: 0,
                width: w as i32,
                height: h as i32,
            })
            .clipped(b);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w as usize + x) * 4;
                let s = &rgba[i..i + 4];
                let source = premul(u32::from_be_bytes([s[3], s[0], s[1], s[2]]), true);
                let d = &mut b.pixels.as_mut().unwrap()[i..i + 4];
                blend_pixel(d, source, s, blend_mode, b.transparent);
            }
        }
        if x1 > x0 && y1 > y0 {
            b.revision += 1;
        }
        Ok(())
    }
}

fn blend_pixel(d: &mut [u8], sp: [u8; 4], s_raw: &[u8], mode: &str, transparent: bool) {
    let sa = u32::from(sp[3]);
    match mode {
        "alpha" => {
            for item in d.iter_mut().take(4) {
                *item = ((u32::from(*item) * sa + 127) / 255) as u8;
            }
        }
        "erase" => {
            let inv = 255 - sa;
            for item in d.iter_mut().take(4) {
                *item = ((u32::from(*item) * inv + 127) / 255) as u8;
            }
        }
        "" | "normal" | "layer" | "shader" => {
            let inv = 255 - sa;
            for c in 0..4 {
                d[c] = (u32::from(sp[c]) + (u32::from(d[c]) * inv + 127) / 255).min(255) as u8;
            }
        }
        "add" => {
            for c in 0..3 {
                d[c] = (u32::from(d[c]) + u32::from(sp[c])).min(255) as u8;
            }
            d[3] = (sa + (u32::from(d[3]) * (255 - sa) + 127) / 255).min(255) as u8;
        }
        "subtract" => {
            for c in 0..3 {
                d[c] = u32::from(d[c]).saturating_sub(u32::from(sp[c])) as u8;
            }
            d[3] = (sa + (u32::from(d[3]) * (255 - sa) + 127) / 255).min(255) as u8;
        }
        "invert" => {
            for channel in d.iter_mut().take(3) {
                let dc = u32::from(*channel);
                *channel = ((sa * (255 - dc) + (255 - sa) * dc + 127) / 255).min(255) as u8;
            }
            d[3] = (sa + (u32::from(d[3]) * (255 - sa) + 127) / 255).min(255) as u8;
        }
        "screen" => {
            for c in 0..3 {
                let sc = u32::from(sp[c]);
                let dc = u32::from(d[c]);
                d[c] = (sc + dc - (sc * dc + 127) / 255).min(255) as u8;
            }
            d[3] = (sa + (u32::from(d[3]) * (255 - sa) + 127) / 255).min(255) as u8;
        }
        _ => {
            if sa == 0 {
                if !transparent {
                    d[3] = 255;
                }
                return;
            }
            let d_raw = straight(d);
            let da_f = f64::from(d_raw[3]) / 255.0;
            let sa_f = f64::from(s_raw[3]) / 255.0;
            let out_a = sa_f + da_f * (1.0 - sa_f);
            for c in 0..3 {
                let cb = f64::from(d_raw[c]) / 255.0;
                let cs = f64::from(s_raw[c]) / 255.0;
                let mixed = match mode {
                    "multiply" => cb * cs,
                    "lighten" => cb.max(cs),
                    "darken" => cb.min(cs),
                    "difference" => (cb - cs).abs(),
                    "invert" => 1.0 - cb,
                    "overlay" => {
                        if cb <= 0.5 {
                            2.0 * cb * cs
                        } else {
                            1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
                        }
                    }
                    "hardlight" => {
                        if cs <= 0.5 {
                            2.0 * cb * cs
                        } else {
                            1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
                        }
                    }
                    _ => cs,
                };
                let premul_f =
                    sa_f * (1.0 - da_f) * cs + da_f * (1.0 - sa_f) * cb + sa_f * da_f * mixed;
                d[c] = (premul_f * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            d[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    if !transparent {
        d[3] = 255;
    }
}
fn raster_budget(w: u32, h: u32) -> Result<(), BitmapError> {
    if w == 0
        || h == 0
        || w > 8192
        || h > 8192
        || u64::from(w) * u64::from(h) > crate::MAX_FILTER_PIXELS as u64
    {
        Err(error(
            "image pipeline raster exceeds 4 Mi pixels / 8192 per side",
        ))
    } else {
        Ok(())
    }
}
struct BoundedPng(Vec<u8>);
impl Write for BoundedPng {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > 16 * 1024 * 1024 - self.0.len() {
            return Err(std::io::Error::other("PNG output exceeds 16 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn operation_to_blend_mode(op: &str) -> &'static str {
    match op {
        "multiply" => "multiply",
        "screen" => "screen",
        "lighten" => "lighten",
        "darken" => "darken",
        "difference" => "difference",
        "lighter" | "add" => "add",
        "subtract" => "subtract",
        "invert" => "invert",
        "destination-in" | "alpha" => "alpha",
        "destination-out" | "erase" => "erase",
        "overlay" => "overlay",
        "hard-light" | "hardlight" => "hardlight",
        _ => "normal",
    }
}

fn invert_affine(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        m[3] * inv,
        -m[1] * inv,
        -m[2] * inv,
        m[0] * inv,
        (m[2] * m[5] - m[3] * m[4]) * inv,
        (m[1] * m[4] - m[0] * m[5]) * inv,
    ])
}

impl crate::Player {
    /// Native CPU rasterizer for `RuntimeSnapshot` supporting dynamic `SimpleButton` states,
    /// rotated/skewed transforms, styled strokes (`hairline`, `caps`, `joints`, `miterLimit`,
    /// `pixelHinting`, `scaleMode`), `Extend`/`Reflect`/`Repeat` + `linearRGB` gradients, clips,
    /// filters, and all 14 Flash blend modes (`Layer`, `Alpha`, `Erase`, `Subtract`, `Invert`, etc.).
    pub fn render_snapshot_rgba(
        &self,
        project: &crate::Project,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, BitmapError> {
        let snapshot = self.runtime_snapshot(project);
        self.render_runtime_snapshot_rgba(project, &snapshot, width, height)
    }

    pub fn render_runtime_snapshot_rgba(
        &self,
        project: &crate::Project,
        snapshot: &crate::RuntimeSnapshot,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, BitmapError> {
        use crate::runtime_graphics::{
            BitmapPaint, LineCaps, LineJoints, LineScaleMode, hit_test_styled_stroke,
        };
        use crate::{ElementKind, MaskCoverage, RuntimeDrawItem, RuntimeDrawKind};

        raster_budget(width, height)?;
        let coverage = if snapshot.clips.is_empty() {
            None
        } else {
            MaskCoverage::new(project, &snapshot.clips).ok()
        };

        let sample_rgba = |bytes: &[u8],
                           bw: u32,
                           bh: u32,
                           u: f64,
                           v: f64,
                           repeat: bool,
                           smooth: bool|
         -> [u8; 4] {
            let (fw, fh) = (f64::from(bw), f64::from(bh));
            let fetch = |mut x: i64, mut y: i64| -> [u8; 4] {
                if repeat {
                    x = x.rem_euclid(i64::from(bw));
                    y = y.rem_euclid(i64::from(bh));
                } else {
                    x = x.clamp(0, i64::from(bw) - 1);
                    y = y.clamp(0, i64::from(bh) - 1);
                }
                let idx = (y as usize * bw as usize + x as usize) * 4;
                bytes
                    .get(idx..idx + 4)
                    .map_or([0; 4], |s| [s[0], s[1], s[2], s[3]])
            };
            if !smooth {
                let mut bx = u;
                let mut by = v;
                if repeat {
                    bx = bx.rem_euclid(fw);
                    by = by.rem_euclid(fh);
                } else {
                    bx = bx.clamp(0.0, fw - 1.0);
                    by = by.clamp(0.0, fh - 1.0);
                }
                return fetch(bx.floor() as i64, by.floor() as i64);
            }
            let fx = u - 0.5;
            let fy = v - 0.5;
            let x0 = fx.floor() as i64;
            let y0 = fy.floor() as i64;
            let wx = fx - x0 as f64;
            let wy = fy - y0 as f64;
            let p00 = fetch(x0, y0);
            let p10 = fetch(x0 + 1, y0);
            let p01 = fetch(x0, y0 + 1);
            let p11 = fetch(x0 + 1, y0 + 1);
            let w00 = (1.0 - wx) * (1.0 - wy);
            let w10 = wx * (1.0 - wy);
            let w01 = (1.0 - wx) * wy;
            let w11 = wx * wy;
            let a = f64::from(p00[3]) * w00
                + f64::from(p10[3]) * w10
                + f64::from(p01[3]) * w01
                + f64::from(p11[3]) * w11;
            if a <= 0.5 {
                return [0; 4];
            }
            let ch = |c: usize| -> u8 {
                let pm = f64::from(p00[c]) * f64::from(p00[3]) * w00
                    + f64::from(p10[c]) * f64::from(p10[3]) * w10
                    + f64::from(p01[c]) * f64::from(p01[3]) * w01
                    + f64::from(p11[c]) * f64::from(p11[3]) * w11;
                (pm / a).round().clamp(0.0, 255.0) as u8
            };
            [ch(0), ch(1), ch(2), a.round().clamp(0.0, 255.0) as u8]
        };

        let sample_bitmap_paint = |bp: &BitmapPaint, lx: f64, ly: f64| -> [u8; 4] {
            let Ok(bytes) = self.bitmap_pixels(&bp.pixel_ref) else {
                return [0; 4];
            };
            if bp.width == 0 || bp.height == 0 {
                return [0; 4];
            }
            let Some(inv) = invert_affine(bp.matrix) else {
                return [0; 4];
            };
            let bx = inv[0] * lx + inv[2] * ly + inv[4];
            let by = inv[1] * lx + inv[3] * ly + inv[5];
            sample_rgba(&bytes, bp.width, bp.height, bx, by, bp.repeat, bp.smoothing)
        };

        let paint_over_pixel = |dst: &mut [u8], straight_src: [u8; 4], clip_a: f64| {
            if straight_src[3] == 0 || clip_a <= 0.0 {
                return;
            }
            let a = (f64::from(straight_src[3]) * clip_a.clamp(0.0, 1.0))
                .round()
                .clamp(0.0, 255.0) as u8;
            if a == 0 {
                return;
            }
            let raw = [straight_src[0], straight_src[1], straight_src[2], a];
            let sp = premul(u32::from_be_bytes([a, raw[0], raw[1], raw[2]]), true);
            blend_pixel(dst, sp, &raw, "normal", true);
        };

        let clip_alpha_at = |clip_ids: &[String], deferred: &[String], px: f64, py: f64| -> f64 {
            if clip_ids.is_empty() {
                return 1.0;
            }
            let active: Vec<String> = clip_ids
                .iter()
                .filter(|id| !deferred.contains(id))
                .cloned()
                .collect();
            if active.is_empty() {
                return 1.0;
            }
            coverage
                .as_ref()
                .map_or(1.0, |cov| cov.alpha(&active, [px, py]))
        };

        let world_bbox =
            |m: [f64; 6], l: f64, t: f64, r: f64, b: f64, pad: f64| -> (u32, u32, u32, u32) {
                let corners = [[l, t], [r, t], [r, b], [l, b]];
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for [cx, cy] in corners {
                    let wx = m[0] * cx + m[2] * cy + m[4];
                    let wy = m[1] * cx + m[3] * cy + m[5];
                    min_x = min_x.min(wx);
                    min_y = min_y.min(wy);
                    max_x = max_x.max(wx);
                    max_y = max_y.max(wy);
                }
                let x0 = (min_x - pad).floor().clamp(0.0, f64::from(width)) as u32;
                let y0 = (min_y - pad).floor().clamp(0.0, f64::from(height)) as u32;
                let x1 = (max_x + pad).ceil().clamp(0.0, f64::from(width)) as u32;
                let y1 = (max_y + pad).ceil().clamp(0.0, f64::from(height)) as u32;
                (x0, y0, x1, y1)
            };

        let paint_item = |buf: &mut [u8], item: &RuntimeDrawItem, deferred_clips: &[String]| {
            let m = item.item.transform;
            let Some(inv) = invert_affine(m) else {
                return;
            };
            let sx = m[0].hypot(m[1]);
            let sy = m[2].hypot(m[3]);
            let avg_scale = ((sx + sy) * 0.5).max(1e-6);

            match &item.item.kind {
                RuntimeDrawKind::Element(el) => match el {
                    ElementKind::Rectangle {
                        width: rw,
                        height: rh,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        let sw = if *stroke_width > 0.0 && stroke[3] > 0 {
                            *stroke_width * 0.5
                        } else {
                            0.0
                        };
                        let (x0, y0, x1, y1) = world_bbox(m, -sw, -sw, *rw + sw, *rh + sw, 2.0);
                        for py in y0..y1 {
                            for px in x0..x1 {
                                let wx = f64::from(px) + 0.5;
                                let wy = f64::from(py) + 0.5;
                                let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                                if ca <= 0.0 {
                                    continue;
                                }
                                let lx = inv[0] * wx + inv[2] * wy + inv[4];
                                let ly = inv[1] * wx + inv[3] * wy + inv[5];
                                let idx = (py as usize * width as usize + px as usize) * 4;
                                if fill[3] > 0 && lx >= 0.0 && ly >= 0.0 && lx <= *rw && ly <= *rh {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *fill, ca);
                                }
                                if sw > 0.0
                                    && lx >= -sw
                                    && ly >= -sw
                                    && lx <= *rw + sw
                                    && ly <= *rh + sw
                                    && !(lx > sw && ly > sw && lx < *rw - sw && ly < *rh - sw)
                                {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *stroke, ca);
                                }
                            }
                        }
                    }
                    ElementKind::Ellipse {
                        width: ew,
                        height: eh,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        if *ew <= 0.0 || *eh <= 0.0 {
                            return;
                        }
                        let sw = if *stroke_width > 0.0 && stroke[3] > 0 {
                            *stroke_width * 0.5
                        } else {
                            0.0
                        };
                        let (x0, y0, x1, y1) = world_bbox(m, -sw, -sw, *ew + sw, *eh + sw, 2.0);
                        let (cx, cy) = (*ew * 0.5, *eh * 0.5);
                        let in_el = |lx: f64, ly: f64, rx: f64, ry: f64| {
                            rx > 0.0
                                && ry > 0.0
                                && ((lx - cx) / rx).powi(2) + ((ly - cy) / ry).powi(2) <= 1.0
                        };
                        for py in y0..y1 {
                            for px in x0..x1 {
                                let wx = f64::from(px) + 0.5;
                                let wy = f64::from(py) + 0.5;
                                let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                                if ca <= 0.0 {
                                    continue;
                                }
                                let lx = inv[0] * wx + inv[2] * wy + inv[4];
                                let ly = inv[1] * wx + inv[3] * wy + inv[5];
                                let idx = (py as usize * width as usize + px as usize) * 4;
                                if fill[3] > 0 && in_el(lx, ly, cx, cy) {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *fill, ca);
                                }
                                if sw > 0.0
                                    && in_el(lx, ly, cx + sw, cy + sw)
                                    && !in_el(lx, ly, cx - sw, cy - sw)
                                {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *stroke, ca);
                                }
                            }
                        }
                    }
                    ElementKind::Path {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        let Ok(Some(pb)) = crate::geometry::path_bounds(commands) else {
                            return;
                        };
                        let pad = *stroke_width * sx.max(sy) + 2.0;
                        let (x0, y0, x1, y1) =
                            world_bbox(m, pb.x_min, pb.y_min, pb.x_max, pb.y_max, pad);
                        for py in y0..y1 {
                            for px in x0..x1 {
                                let wx = f64::from(px) + 0.5;
                                let wy = f64::from(py) + 0.5;
                                let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                                if ca <= 0.0 {
                                    continue;
                                }
                                let lx = inv[0] * wx + inv[2] * wy + inv[4];
                                let ly = inv[1] * wx + inv[3] * wy + inv[5];
                                let idx = (py as usize * width as usize + px as usize) * 4;
                                if fill[3] > 0
                                    && crate::hit_test_path(
                                        commands,
                                        *fill_rule,
                                        [lx, ly],
                                        true,
                                        0.0,
                                        Default::default(),
                                    )
                                    .unwrap_or(false)
                                {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *fill, ca);
                                }
                                if *stroke_width > 0.0
                                    && stroke[3] > 0
                                    && hit_test_styled_stroke(
                                        commands,
                                        [lx, ly],
                                        *stroke_width,
                                        LineCaps::Round,
                                        LineJoints::Round,
                                        3.0,
                                    )
                                {
                                    paint_over_pixel(&mut buf[idx..idx + 4], *stroke, ca);
                                }
                            }
                        }
                    }
                    ElementKind::PaintedPath {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        stroke_width,
                    } => {
                        let Ok(Some(pb)) = crate::geometry::path_bounds(commands) else {
                            return;
                        };
                        let pad = *stroke_width * sx.max(sy) + 2.0;
                        let (x0, y0, x1, y1) =
                            world_bbox(m, pb.x_min, pb.y_min, pb.x_max, pb.y_max, pad);
                        for py in y0..y1 {
                            for px in x0..x1 {
                                let wx = f64::from(px) + 0.5;
                                let wy = f64::from(py) + 0.5;
                                let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                                if ca <= 0.0 {
                                    continue;
                                }
                                let lx = inv[0] * wx + inv[2] * wy + inv[4];
                                let ly = inv[1] * wx + inv[3] * wy + inv[5];
                                let idx = (py as usize * width as usize + px as usize) * 4;
                                if (fill[3] > 0 || fill_gradient.is_some())
                                    && crate::hit_test_path(
                                        commands,
                                        *fill_rule,
                                        [lx, ly],
                                        true,
                                        0.0,
                                        Default::default(),
                                    )
                                    .unwrap_or(false)
                                {
                                    let c =
                                        fill_gradient.as_ref().map_or(*fill, |g| g.sample(lx, ly));
                                    paint_over_pixel(&mut buf[idx..idx + 4], c, ca);
                                }
                                if *stroke_width > 0.0
                                    && (stroke[3] > 0 || stroke_gradient.is_some())
                                    && hit_test_styled_stroke(
                                        commands,
                                        [lx, ly],
                                        *stroke_width,
                                        LineCaps::Round,
                                        LineJoints::Round,
                                        3.0,
                                    )
                                {
                                    let c = stroke_gradient
                                        .as_ref()
                                        .map_or(*stroke, |g| g.sample(lx, ly));
                                    paint_over_pixel(&mut buf[idx..idx + 4], c, ca);
                                }
                            }
                        }
                    }
                    _ => {}
                },
                RuntimeDrawKind::GradientPath {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    fill_bitmap,
                    stroke_bitmap,
                    stroke_width,
                    stroke_caps,
                    stroke_joints,
                    stroke_miter_limit,
                    stroke_pixel_hinting,
                    stroke_scale_mode,
                    stroke_hairline,
                } => {
                    let Ok(Some(pb)) = crate::geometry::path_bounds(commands) else {
                        return;
                    };
                    let has_fill = fill[3] > 0 || fill_gradient.is_some() || fill_bitmap.is_some();
                    let has_stroke = (*stroke_width > 0.0 || *stroke_hairline)
                        && (stroke[3] > 0 || stroke_gradient.is_some() || stroke_bitmap.is_some());
                    let eff_local_w = if *stroke_hairline {
                        1.0 / avg_scale
                    } else {
                        match stroke_scale_mode {
                            LineScaleMode::Normal => *stroke_width,
                            LineScaleMode::None => *stroke_width / avg_scale,
                            LineScaleMode::Horizontal => *stroke_width * sx / avg_scale,
                            LineScaleMode::Vertical => *stroke_width * sy / avg_scale,
                        }
                    };
                    let world_sw = eff_local_w * avg_scale;
                    let pad = world_sw * (*stroke_miter_limit).max(1.5) + 2.0;
                    let (x0, y0, x1, y1) =
                        world_bbox(m, pb.x_min, pb.y_min, pb.x_max, pb.y_max, pad);

                    // When pixelHinting is enabled and the matrix is axis-aligned, align odd integer
                    // world stroke thickness to pixel centers (+0.5) so 1px lines hit a single row/column.
                    let (hint_dx, hint_dy) =
                        if *stroke_pixel_hinting && m[1].abs() < 1e-6 && m[2].abs() < 1e-6 {
                            let odd = (world_sw.round() as i64) % 2 == 1;
                            let snap_delta = |t: f64| {
                                if odd {
                                    t.floor() + 0.5 - t
                                } else {
                                    t.round() - t
                                }
                            };
                            (snap_delta(m[4]), snap_delta(m[5]))
                        } else {
                            (0.0, 0.0)
                        };

                    for py in y0..y1 {
                        for px in x0..x1 {
                            let wx = f64::from(px) + 0.5;
                            let wy = f64::from(py) + 0.5;
                            let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                            if ca <= 0.0 {
                                continue;
                            }
                            let lx = inv[0] * wx + inv[2] * wy + inv[4];
                            let ly = inv[1] * wx + inv[3] * wy + inv[5];
                            let idx = (py as usize * width as usize + px as usize) * 4;
                            if has_fill
                                && crate::hit_test_path(
                                    commands,
                                    *fill_rule,
                                    [lx, ly],
                                    true,
                                    0.0,
                                    Default::default(),
                                )
                                .unwrap_or(false)
                            {
                                let color = if let Some(grad) = fill_gradient {
                                    grad.sample(lx, ly)
                                } else if let Some(bp) = fill_bitmap {
                                    sample_bitmap_paint(bp, lx, ly)
                                } else {
                                    *fill
                                };
                                paint_over_pixel(&mut buf[idx..idx + 4], color, ca);
                            }
                            if has_stroke {
                                let slx =
                                    inv[0] * (wx - hint_dx) + inv[2] * (wy - hint_dy) + inv[4];
                                let sly =
                                    inv[1] * (wx - hint_dx) + inv[3] * (wy - hint_dy) + inv[5];
                                if hit_test_styled_stroke(
                                    commands,
                                    [slx, sly],
                                    eff_local_w,
                                    *stroke_caps,
                                    *stroke_joints,
                                    *stroke_miter_limit,
                                ) {
                                    let color = if let Some(grad) = stroke_gradient {
                                        grad.sample(slx, sly)
                                    } else if let Some(bp) = stroke_bitmap {
                                        sample_bitmap_paint(bp, slx, sly)
                                    } else {
                                        *stroke
                                    };
                                    paint_over_pixel(&mut buf[idx..idx + 4], color, ca);
                                }
                            }
                        }
                    }
                }
                RuntimeDrawKind::BitmapPixels {
                    pixel_ref,
                    width: bw,
                    height: bh,
                    smoothing: bmp_smooth,
                    color_transform,
                    ..
                } => {
                    let Ok(bytes) = self.bitmap_pixels(pixel_ref) else {
                        return;
                    };
                    if *bw == 0 || *bh == 0 {
                        return;
                    }
                    let (x0, y0, x1, y1) =
                        world_bbox(m, 0.0, 0.0, f64::from(*bw), f64::from(*bh), 2.0);
                    for py in y0..y1 {
                        for px in x0..x1 {
                            let wx = f64::from(px) + 0.5;
                            let wy = f64::from(py) + 0.5;
                            let ca = clip_alpha_at(&item.clip_ids, deferred_clips, wx, wy);
                            if ca <= 0.0 {
                                continue;
                            }
                            let lx = inv[0] * wx + inv[2] * wy + inv[4];
                            let ly = inv[1] * wx + inv[3] * wy + inv[5];
                            if lx >= 0.0 && ly >= 0.0 && lx < f64::from(*bw) && ly < f64::from(*bh)
                            {
                                let src = sample_rgba(&bytes, *bw, *bh, lx, ly, false, *bmp_smooth);
                                let c = color_transform.apply(src);
                                let idx = (py as usize * width as usize + px as usize) * 4;
                                paint_over_pixel(&mut buf[idx..idx + 4], c, ca);
                            }
                        }
                    }
                }
                _ => {}
            }
        };

        let pixel_len = width as usize * height as usize * 4;
        if snapshot.filter_groups.is_empty() {
            let mut root_buf = vec![0u8; pixel_len];
            for item in &snapshot.draws {
                paint_item(&mut root_buf, item, &[]);
            }
            let mut out = Vec::with_capacity(pixel_len);
            for p in root_buf.as_chunks::<4>().0 {
                out.extend_from_slice(&straight(p));
            }
            return Ok(out);
        }

        #[derive(Clone)]
        enum Entry {
            Item(usize),
            Group(usize),
        }
        struct GroupNode {
            id: Option<String>,
            parent_id: Option<String>,
            filters: Vec<FilterSpec>,
            clip_ids: Vec<String>,
            composite_operation: String,
            opaque_background: Option<[u8; 4]>,
            entries: Vec<Entry>,
        }

        let mut nodes = vec![GroupNode {
            id: None,
            parent_id: None,
            filters: vec![],
            clip_ids: vec![],
            composite_operation: "source-over".into(),
            opaque_background: None,
            entries: vec![],
        }];
        let mut id_to_idx: HashMap<String, usize> = HashMap::new();
        for g in &snapshot.filter_groups {
            let idx = nodes.len();
            id_to_idx.insert(g.id.clone(), idx);
            nodes.push(GroupNode {
                id: Some(g.id.clone()),
                parent_id: g.parent_id.clone(),
                filters: g.filters.clone(),
                clip_ids: g.clip_ids.clone(),
                composite_operation: g.composite_operation.clone(),
                opaque_background: g.opaque_background,
                entries: vec![],
            });
        }
        let ancestry = |nodes: &[GroupNode], filter_id: Option<&String>| -> Vec<usize> {
            let Some(fid) = filter_id else {
                return vec![];
            };
            let mut chain = Vec::new();
            let mut cur = id_to_idx.get(fid).copied();
            while let Some(idx) = cur {
                if chain.contains(&idx) || chain.len() >= 64 {
                    break;
                }
                chain.insert(0, idx);
                cur = nodes[idx]
                    .parent_id
                    .as_ref()
                    .and_then(|pid| id_to_idx.get(pid).copied());
            }
            chain
        };
        let mut previous: Vec<usize> = Vec::new();
        for (item_idx, item) in snapshot.draws.iter().enumerate() {
            let chain = ancestry(&nodes, item.filter_id.as_ref());
            let mut common = 0;
            while common < previous.len()
                && common < chain.len()
                && previous[common] == chain[common]
            {
                common += 1;
            }
            for at in common..chain.len() {
                let g_idx = chain[at];
                let parent_idx = if at == 0 { 0 } else { chain[at - 1] };
                nodes[parent_idx].entries.push(Entry::Group(g_idx));
            }
            let target_idx = chain.last().copied().unwrap_or(0);
            nodes[target_idx].entries.push(Entry::Item(item_idx));
            previous = chain;
        }

        type PaintItemFn<'a> = dyn Fn(&mut [u8], &RuntimeDrawItem, &[String]) + 'a;
        type ClipAlphaFn<'a> = dyn Fn(&[String], &[String], f64, f64) -> f64 + 'a;

        struct RenderNodeCtx<'a> {
            nodes: &'a [GroupNode],
            draws: &'a [RuntimeDrawItem],
            width: u32,
            height: u32,
            player: &'a crate::Player,
            paint_item: &'a PaintItemFn<'a>,
            clip_alpha_at: &'a ClipAlphaFn<'a>,
        }

        fn render_node(node_idx: usize, ctx: &RenderNodeCtx<'_>) -> Result<Vec<u8>, BitmapError> {
            let pixel_len = ctx.width as usize * ctx.height as usize * 4;
            let node = &ctx.nodes[node_idx];
            let mut buf = vec![0u8; pixel_len];
            if let Some(bg) = node.opaque_background {
                let p = premul(u32::from_be_bytes([bg[3], bg[0], bg[1], bg[2]]), true);
                for px in buf.as_chunks_mut::<4>().0 {
                    px.copy_from_slice(&p);
                }
            }
            for entry in &node.entries {
                match entry {
                    Entry::Item(i) => (ctx.paint_item)(&mut buf, &ctx.draws[*i], &node.clip_ids),
                    Entry::Group(child_idx) => {
                        let child_buf = render_node(*child_idx, ctx)?;
                        let child = &ctx.nodes[*child_idx];
                        let mode = operation_to_blend_mode(&child.composite_operation);
                        for py in 0..ctx.height {
                            for px in 0..ctx.width {
                                let i = (py as usize * ctx.width as usize + px as usize) * 4;
                                let ca = (ctx.clip_alpha_at)(
                                    &child.clip_ids,
                                    &node.clip_ids,
                                    f64::from(px) + 0.5,
                                    f64::from(py) + 0.5,
                                );
                                let s_straight = straight(&child_buf[i..i + 4]);
                                let sa = (f64::from(s_straight[3]) * ca.clamp(0.0, 1.0))
                                    .round()
                                    .clamp(0.0, 255.0)
                                    as u8;
                                let s_raw = [s_straight[0], s_straight[1], s_straight[2], sa];
                                let sp = premul(
                                    u32::from_be_bytes([sa, s_raw[0], s_raw[1], s_raw[2]]),
                                    true,
                                );
                                blend_pixel(&mut buf[i..i + 4], sp, &s_raw, mode, true);
                            }
                        }
                    }
                }
            }
            if node.id.is_some() && !node.filters.is_empty() {
                let mut straight_in = Vec::with_capacity(pixel_len);
                for p in buf.as_chunks::<4>().0 {
                    straight_in.extend_from_slice(&straight(p));
                }
                let filtered = ctx
                    .player
                    .filter_rgba(ctx.width, ctx.height, &straight_in, &node.filters)
                    .map_err(|e| BitmapError(e.to_string()))?;
                for (dst, src) in buf
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(filtered.as_chunks::<4>().0)
                {
                    dst.copy_from_slice(&premul(
                        u32::from_be_bytes([src[3], src[0], src[1], src[2]]),
                        true,
                    ));
                }
            }
            Ok(buf)
        }

        let render_ctx = RenderNodeCtx {
            nodes: &nodes,
            draws: &snapshot.draws,
            width,
            height,
            player: self,
            paint_item: &paint_item,
            clip_alpha_at: &clip_alpha_at,
        };
        let root_buf = render_node(0, &render_ctx)?;
        let mut out = Vec::with_capacity(pixel_len);
        for p in root_buf.as_chunks::<4>().0 {
            out.extend_from_slice(&straight(p));
        }
        Ok(out)
    }
}

impl BitmapRect {
    pub fn clipped_dims(self, full_w: u32, full_h: u32) -> (usize, usize, usize, usize) {
        clip(self.x, self.y, self.width, self.height, full_w, full_h)
    }

    /// Encodes clipped straight big-endian ARGB pixels into an ITU-T T.832 (`.jxr`)
    /// TIFF-IFD container (`0x49 0x49 0xBC 0x01`) + `WMPHOTO\0` YCoCg-R codestream
    /// honoring `quantization` (`0..=255`), `color_space` (`auto`/`4:4:4`/`4:2:2`/`4:2:0`),
    /// and `trim_flex_bits` (`0..=15`).
    pub fn encode_jpegxr_argb(
        cw: u32,
        ch: u32,
        transparent: bool,
        argb_be: &[u8],
        quantization: u8,
        color_space: &str,
        trim_flex_bits: u8,
    ) -> Result<Vec<u8>, BitmapError> {
        if cw == 0 || ch == 0 {
            return Err(error(
                "JPEG-XR encoding requires a nonempty clipped rectangle",
            ));
        }
        if trim_flex_bits > 15 {
            return Err(error("JPEGXREncoderOptions.trimFlexBits requires 0..=15"));
        }
        let internal_clr: u8 = match color_space {
            "4:4:4" => 2,
            "4:2:2" => 1,
            "4:2:0" => 0,
            "auto" => {
                if quantization <= 16 {
                    2
                } else if quantization <= 48 {
                    1
                } else {
                    0
                }
            }
            _ => {
                return Err(error(
                    "JPEGXREncoderOptions.colorSpace requires 'auto', '4:4:4', '4:2:2' or '4:2:0'",
                ));
            }
        };
        let w = cw as usize;
        let h = ch as usize;
        if argb_be.len() != w.saturating_mul(h).saturating_mul(4) {
            return Err(error("JPEG-XR ARGB input buffer length mismatch"));
        }

        let mut y_plane = vec![0i16; w * h];
        let mut co_full = vec![0i16; w * h];
        let mut cg_full = vec![0i16; w * h];
        let mut a_plane = if transparent {
            vec![255u8; w * h]
        } else {
            Vec::new()
        };

        for idx in 0..(w * h) {
            let a = argb_be[idx * 4];
            let r = i16::from(argb_be[idx * 4 + 1]);
            let g = i16::from(argb_be[idx * 4 + 2]);
            let b = i16::from(argb_be[idx * 4 + 3]);
            // Reversible ITU-T T.832 YCoCg-R lifting transform
            let co = r - b;
            let t = b + (co >> 1);
            let cg = g - t;
            let y = t + (cg >> 1);
            y_plane[idx] = y;
            co_full[idx] = co;
            cg_full[idx] = cg;
            if transparent {
                a_plane[idx] = a;
            }
        }

        let (uv_w, uv_h) = match internal_clr {
            2 => (w, h),
            1 => (w.div_ceil(2), h),
            _ => (w.div_ceil(2), h.div_ceil(2)),
        };
        let subsample = |plane: &[i16]| -> Vec<i16> {
            if internal_clr == 2 {
                return plane.to_vec();
            }
            let mut out = vec![0i16; uv_w * uv_h];
            let step_y = if internal_clr == 0 { 2 } else { 1 };
            for sy in 0..uv_h {
                for sx in 0..uv_w {
                    let mut sum = 0i32;
                    let mut cnt = 0i32;
                    for dy in 0..step_y {
                        for dx in 0..2 {
                            let px = (sx * 2 + dx).min(w - 1);
                            let py = (sy * step_y + dy).min(h - 1);
                            sum += i32::from(plane[py * w + px]);
                            cnt += 1;
                        }
                    }
                    out[sy * uv_w + sx] = (sum / cnt) as i16;
                }
            }
            out
        };
        let co_plane = subsample(&co_full);
        let cg_plane = subsample(&cg_full);

        let quantize_plane = |plane: &[i16], pw: usize, ph: usize| -> Vec<i16> {
            let q = i32::from(quantization) + 1;
            let trim = u32::from(trim_flex_bits.min(7));
            let mut out = vec![0i16; pw * ph];
            for by in (0..ph).step_by(4) {
                for bx in (0..pw).step_by(4) {
                    let dc = i32::from(plane[by * pw + bx]);
                    let dc_q = if q > 1 {
                        if dc >= 0 {
                            (dc + q / 2) / q
                        } else {
                            (dc - q / 2) / q
                        }
                    } else {
                        dc
                    };
                    for oy in 0..4 {
                        if by + oy >= ph {
                            break;
                        }
                        for ox in 0..4 {
                            if bx + ox >= pw {
                                break;
                            }
                            let idx = (by + oy) * pw + (bx + ox);
                            if ox == 0 && oy == 0 {
                                out[idx] = dc_q as i16;
                            } else {
                                let ac = i32::from(plane[idx]) - (dc_q * q);
                                let mut ac_q = if q > 1 {
                                    if ac >= 0 {
                                        (ac + q / 2) / q
                                    } else {
                                        (ac - q / 2) / q
                                    }
                                } else {
                                    ac
                                };
                                if trim > 0 {
                                    ac_q = (ac_q >> trim) << trim;
                                }
                                out[idx] = ac_q as i16;
                            }
                        }
                    }
                }
            }
            out
        };

        let y_q = quantize_plane(&y_plane, w, h);
        let co_q = quantize_plane(&co_plane, uv_w, uv_h);
        let cg_q = quantize_plane(&cg_plane, uv_w, uv_h);

        let mut raw_planes =
            Vec::with_capacity((y_q.len() + co_q.len() + cg_q.len()) * 2 + a_plane.len());
        for &v in y_q.iter().chain(co_q.iter()).chain(cg_q.iter()) {
            raw_planes.extend_from_slice(&v.to_le_bytes());
        }
        if transparent {
            let q = u16::from(quantization) + 1;
            for &a in &a_plane {
                let qa = if q > 1 {
                    (((u16::from(a) + q / 2) / q) * q).min(255) as u8
                } else {
                    a
                };
                raw_planes.push(qa);
            }
        }

        let mut compressed = Vec::new();
        {
            let mut z =
                flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default());
            z.write_all(&raw_planes)
                .map_err(|e| BitmapError(format!("JPEG-XR compression failed: {e}")))?;
            z.finish()
                .map_err(|e| BitmapError(format!("JPEG-XR finish failed: {e}")))?;
        }

        // ITU-T T.832 codestream: GDI_SIGNATURE ("WMPHOTO\0") + IMAGE_HEADER + IMAGE_PLANE_HEADER + entropy payload
        let mut codestream = Vec::with_capacity(24 + compressed.len());
        codestream.extend_from_slice(b"WMPHOTO\0");
        let hdr_flags: u8 = 0x10 | if transparent { 0x08 } else { 0x00 } | 0x01;
        codestream.push(hdr_flags);
        codestream.push(internal_clr);
        codestream.push(quantization);
        codestream.push(trim_flex_bits);
        codestream.extend_from_slice(&(cw - 1).to_be_bytes());
        codestream.extend_from_slice(&(ch - 1).to_be_bytes());
        codestream.extend_from_slice(&compressed);

        // ITU-T T.832 Annex A TIFF-based .jxr file container:
        // Header (8B): 'I','I', 0xBC, 0x01, IFD offset = 8
        // IFD: u16 count = 7, 7 * 12B = 84B, u32 next_ifd = 0 -> 90B (offsets 8..98)
        // PixelFormat GUID (16B) at offset 98..114
        // Codestream at offset 114..
        let ifd_offset: u32 = 8;
        let guid_offset: u32 = 8 + 2 + 7 * 12 + 4; // 98
        let stream_offset: u32 = guid_offset + 16; // 114
        let stream_len = u32::try_from(codestream.len())
            .map_err(|_| error("JPEG-XR codestream exceeds 4 GiB"))?;

        let guid: [u8; 16] = if transparent {
            // GUID_PKPixelFormat32bppBGRA (24fdc3ef-4e03-4bfe-b185-3d77768dc90f)
            [
                0xef, 0xc3, 0xfd, 0x24, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d,
                0xc9, 0x0f,
            ]
        } else {
            // GUID_PKPixelFormat24bppRGB (24fdc3ef-4e03-4bfe-b185-3d77768dc90d)
            [
                0xef, 0xc3, 0xfd, 0x24, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d,
                0xc9, 0x0d,
            ]
        };

        let mut out = Vec::with_capacity(stream_offset as usize + codestream.len());
        out.extend_from_slice(&[0x49, 0x49, 0xBC, 0x01]);
        out.extend_from_slice(&ifd_offset.to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());

        let mut write_ifd = |tag: u16, ty: u16, count: u32, val: u32| {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&ty.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&val.to_le_bytes());
        };
        let dpi_bits = 96.0f32.to_bits();
        write_ifd(0xBC01, 1, 16, guid_offset);
        write_ifd(0xBC80, 4, 1, cw);
        write_ifd(0xBC81, 4, 1, ch);
        write_ifd(0xBC82, 11, 1, dpi_bits);
        write_ifd(0xBC83, 11, 1, dpi_bits);
        write_ifd(0xBCC0, 4, 1, stream_offset);
        write_ifd(0xBCC1, 4, 1, stream_len);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&guid);
        out.extend_from_slice(&codestream);
        Ok(out)
    }

    /// Decodes a NeoFlash ITU-T T.832 `.jxr` byte buffer back into `(width, height, transparent, straight_rgba8)`.
    pub fn decode_jpegxr_rgba(bytes: &[u8]) -> Result<(u32, u32, bool, Vec<u8>), BitmapError> {
        if bytes.len() < 32 || bytes[0..4] != [0x49, 0x49, 0xBC, 0x01] {
            return Err(error("Invalid JPEG-XR container header"));
        }
        let ifd_off = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        if ifd_off + 2 > bytes.len() {
            return Err(error("Truncated JPEG-XR IFD"));
        }
        let count = u16::from_le_bytes(bytes[ifd_off..ifd_off + 2].try_into().unwrap()) as usize;
        if count > 64 || ifd_off + 2 + count * 12 > bytes.len() {
            return Err(error("Invalid JPEG-XR IFD entry count"));
        }
        let mut stream_off = 0usize;
        let mut stream_len = 0usize;
        for i in 0..count {
            let base = ifd_off + 2 + i * 12;
            let tag = u16::from_le_bytes(bytes[base..base + 2].try_into().unwrap());
            let val = u32::from_le_bytes(bytes[base + 8..base + 12].try_into().unwrap()) as usize;
            match tag {
                0xBCC0 => stream_off = val,
                0xBCC1 => stream_len = val,
                _ => {}
            }
        }
        let end = stream_off
            .checked_add(stream_len)
            .ok_or_else(|| error("JPEG-XR codestream overflow"))?;
        if stream_off == 0 || stream_len < 20 || end > bytes.len() {
            return Err(error("Invalid JPEG-XR codestream range"));
        }
        let cs = &bytes[stream_off..end];
        if &cs[0..8] != b"WMPHOTO\0" {
            return Err(error("Missing WMPHOTO JPEG-XR codestream signature"));
        }
        let hdr_flags = cs[8];
        let transparent = (hdr_flags & 0x08) != 0;
        let internal_clr = cs[9];
        let quantization = cs[10];
        let cw = u32::from_be_bytes(cs[12..16].try_into().unwrap()).saturating_add(1);
        let ch = u32::from_be_bytes(cs[16..20].try_into().unwrap()).saturating_add(1);
        if cw == 0 || ch == 0 || cw > MAX_BITMAP_DIMENSION || ch > MAX_BITMAP_DIMENSION {
            return Err(error("JPEG-XR dimensions out of bounds"));
        }
        let w = cw as usize;
        let h = ch as usize;
        let (uv_w, uv_h) = match internal_clr {
            2 => (w, h),
            1 => (w.div_ceil(2), h),
            0 => (w.div_ceil(2), h.div_ceil(2)),
            _ => return Err(error("Unsupported JPEG-XR internal color format")),
        };
        let expected_raw = (w * h + uv_w * uv_h * 2) * 2 + if transparent { w * h } else { 0 };
        let mut raw = Vec::with_capacity(expected_raw);
        {
            use std::io::Read;
            let z = flate2::read::ZlibDecoder::new(&cs[20..]);
            z.take(expected_raw as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(|e| BitmapError(format!("JPEG-XR decompression failed: {e}")))?;
        }
        if raw.len() != expected_raw {
            return Err(error("JPEG-XR decompressed plane size mismatch"));
        }

        let read_i16_plane = |offset: usize, len: usize| -> Vec<i16> {
            (0..len)
                .map(|i| {
                    let at = offset + i * 2;
                    i16::from_le_bytes([raw[at], raw[at + 1]])
                })
                .collect()
        };
        let y_q = read_i16_plane(0, w * h);
        let co_q = read_i16_plane(w * h * 2, uv_w * uv_h);
        let cg_q = read_i16_plane((w * h + uv_w * uv_h) * 2, uv_w * uv_h);
        let alpha_off = (w * h + uv_w * uv_h * 2) * 2;

        let dequant_plane = |plane: &[i16], pw: usize, ph: usize| -> Vec<i16> {
            let q = i32::from(quantization) + 1;
            let mut out = vec![0i16; pw * ph];
            for by in (0..ph).step_by(4) {
                for bx in (0..pw).step_by(4) {
                    let dc = i32::from(plane[by * pw + bx]) * q;
                    for oy in 0..4 {
                        if by + oy >= ph {
                            break;
                        }
                        for ox in 0..4 {
                            if bx + ox >= pw {
                                break;
                            }
                            let idx = (by + oy) * pw + (bx + ox);
                            out[idx] = if ox == 0 && oy == 0 {
                                dc as i16
                            } else {
                                (dc + i32::from(plane[idx]) * q) as i16
                            };
                        }
                    }
                }
            }
            out
        };
        let y_rec = dequant_plane(&y_q, w, h);
        let co_rec = dequant_plane(&co_q, uv_w, uv_h);
        let cg_rec = dequant_plane(&cg_q, uv_w, uv_h);

        let mut rgba = vec![0u8; w * h * 4];
        for py in 0..h {
            for px in 0..w {
                let idx = py * w + px;
                let ux = if internal_clr == 2 { px } else { px / 2 };
                let uy = if internal_clr == 0 { py / 2 } else { py };
                let y = i32::from(y_rec[idx]);
                let co = i32::from(co_rec[uy * uv_w + ux]);
                let cg = i32::from(cg_rec[uy * uv_w + ux]);
                let t = y - (cg >> 1);
                let g = t + cg;
                let b = t - (co >> 1);
                let r = b + co;
                rgba[idx * 4] = r.clamp(0, 255) as u8;
                rgba[idx * 4 + 1] = g.clamp(0, 255) as u8;
                rgba[idx * 4 + 2] = b.clamp(0, 255) as u8;
                rgba[idx * 4 + 3] = if transparent {
                    raw[alpha_off + idx]
                } else {
                    255
                };
            }
        }
        Ok((cw, ch, transparent, rgba))
    }
}

#[cfg(test)]
mod jpegxr_tests {
    use super::*;

    #[test]
    fn jpegxr_lossless_and_lossy_colorspaces_and_flexbits_roundtrip() {
        let mut argb = Vec::with_capacity(8 * 8 * 4);
        for y in 0..8u8 {
            for x in 0..8u8 {
                argb.extend_from_slice(&[
                    200u8.saturating_add(x),
                    x.wrapping_mul(29),
                    y.wrapping_mul(31),
                    (x ^ y).wrapping_mul(19),
                ]);
            }
        }
        let lossless = BitmapRect::encode_jpegxr_argb(8, 8, true, &argb, 0, "4:4:4", 0).unwrap();
        assert_eq!(&lossless[0..4], &[0x49, 0x49, 0xBC, 0x01]);
        let (dw, dh, dtrans, rgba) = BitmapRect::decode_jpegxr_rgba(&lossless).unwrap();
        assert_eq!((dw, dh, dtrans), (8, 8, true));
        for i in 0..64 {
            assert_eq!(rgba[i * 4], argb[i * 4 + 1]);
            assert_eq!(rgba[i * 4 + 1], argb[i * 4 + 2]);
            assert_eq!(rgba[i * 4 + 2], argb[i * 4 + 3]);
            assert_eq!(rgba[i * 4 + 3], argb[i * 4]);
        }

        let jxr_422 = BitmapRect::encode_jpegxr_argb(8, 8, false, &argb, 20, "4:2:2", 2).unwrap();
        let jxr_420 = BitmapRect::encode_jpegxr_argb(8, 8, false, &argb, 60, "4:2:0", 6).unwrap();
        let jxr_auto = BitmapRect::encode_jpegxr_argb(8, 8, false, &argb, 10, "auto", 0).unwrap();
        assert_ne!(jxr_422, jxr_420);
        assert_ne!(jxr_auto, jxr_420);
        assert!(!BitmapRect::decode_jpegxr_rgba(&jxr_420).unwrap().2);
    }
}
