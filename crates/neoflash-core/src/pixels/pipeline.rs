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
        if blend_mode == "shader" {
            return Err(error(
                "BlendMode.SHADER requires blendShader bytecode evaluation",
            ));
        }
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

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rgba_shader(
        &mut self,
        r: &PixelRef,
        w: u32,
        h: u32,
        fg_rgba: &[u8],
        rect: Option<BitmapRect>,
        expected_revision: &str,
        bytecode_base64: &str,
        precision_hint: &str,
        params_json: &str,
    ) -> Result<(), BitmapError> {
        raster_budget(w, h)?;
        let b_check = self.live(r)?;
        if b_check.width != w || b_check.height != h || fg_rgba.len() != w as usize * h as usize * 4
        {
            return Err(error(
                "draw raster must match destination dimensions and RGBA byte length",
            ));
        }
        if b_check.revision.to_string() != expected_revision {
            return Err(error("draw destination changed during raster capture"));
        }
        let (x0, y0, x1, y1) = rect
            .unwrap_or(BitmapRect {
                x: 0,
                y: 0,
                width: w as i32,
                height: h as i32,
            })
            .clipped(b_check);
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        let mut bx0 = x1;
        let mut by0 = y1;
        let mut bx1 = x0;
        let mut by1 = y0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w as usize + x) * 4;
                if fg_rgba[i + 3] > 0 {
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x + 1);
                    by1 = by1.max(y + 1);
                }
            }
        }
        if bx0 >= bx1 || by0 >= by1 {
            return Ok(());
        }
        let sub_w = bx1 - bx0;
        let sub_h = by1 - by0;
        let mut bg_sub = Vec::with_capacity(sub_w * sub_h * 4);
        let mut fg_sub = Vec::with_capacity(sub_w * sub_h * 4);
        let dst_pixels = b_check.pixels.as_ref().unwrap();
        for y in by0..by1 {
            for x in bx0..bx1 {
                let i = (y * w as usize + x) * 4;
                bg_sub.extend_from_slice(&straight(&dst_pixels[i..i + 4]));
                fg_sub.extend_from_slice(&fg_rgba[i..i + 4]);
            }
        }
        let blended = crate::as3_fte_3d::evaluate_blend_shader_rgba(
            &bg_sub,
            &fg_sub,
            sub_w as u32,
            sub_h as u32,
            bytecode_base64,
            precision_hint,
            params_json,
        )
        .map_err(|e| BitmapError(e.to_string()))?;
        let b = self.live_mut(r)?;
        let transparent = b.transparent;
        let dst_mut = b.pixels.as_mut().unwrap();
        for sy in 0..sub_h {
            for sx in 0..sub_w {
                let si = (sy * sub_w + sx) * 4;
                if fg_sub[si + 3] == 0 {
                    continue;
                }
                let di = ((by0 + sy) * w as usize + (bx0 + sx)) * 4;
                let p = &blended[si..si + 4];
                dst_mut[di..di + 4].copy_from_slice(&premul(
                    u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
                    transparent,
                ));
            }
        }
        b.revision += 1;
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
        "" | "normal" | "layer" => {
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

        let clips_by_id: std::collections::HashMap<&str, &crate::RuntimeClip> =
            snapshot.clips.iter().map(|c| (c.id.as_str(), c)).collect();

        let clip_alpha_at = |clip_ids: &[String], deferred: &[String], px: f64, py: f64| -> f64 {
            if clip_ids.is_empty() {
                return 1.0;
            }
            let mut total = 1.0;
            for id in clip_ids {
                if deferred.contains(id) {
                    continue;
                }
                if let Some(rc) = clips_by_id.get(id.as_str())
                    && let Some(alpha_comps) = &rc.alpha_components
                    && alpha_comps.iter().any(|ac| ac.fill_bitmap.is_some())
                {
                    let mut comp_complement = 1.0;
                    for ac in alpha_comps {
                        let ca = if let Some(bp) = &ac.fill_bitmap {
                            if let Some(inv_m) = invert_affine(ac.matrix) {
                                let lx = inv_m[0] * px + inv_m[2] * py + inv_m[4];
                                let ly = inv_m[1] * px + inv_m[3] * py + inv_m[5];
                                if crate::hit_test_path(
                                    &ac.commands,
                                    ac.fill_rule,
                                    [lx, ly],
                                    true,
                                    0.0,
                                    Default::default(),
                                )
                                .unwrap_or(false)
                                {
                                    let rgba = sample_bitmap_paint(bp, lx, ly);
                                    let ct_a = (f64::from(rgba[3])
                                        * bp.color_transform.multipliers[3]
                                        + bp.color_transform.offsets[3])
                                        .clamp(0.0, 255.0)
                                        / 255.0;
                                    ct_a * ac.alpha.clamp(0.0, 1.0)
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            }
                        } else {
                            coverage
                                .as_ref()
                                .map_or(1.0, |cov| cov.alpha(std::slice::from_ref(id), [px, py]))
                        };
                        comp_complement *= 1.0 - ca.clamp(0.0, 1.0);
                    }
                    let union_a = 1.0 - comp_complement;
                    total *= if rc.invert { 1.0 - union_a } else { union_a };
                } else {
                    total *= coverage
                        .as_ref()
                        .map_or(1.0, |cov| cov.alpha(std::slice::from_ref(id), [px, py]));
                }
                if total <= 0.0 {
                    return 0.0;
                }
            }
            total
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
                        if let Some(spec_json) = child.composite_operation.strip_prefix("shader:") {
                            #[derive(serde::Deserialize)]
                            struct InlineShaderSpec {
                                #[serde(default)]
                                bytecode_base64: String,
                                #[serde(default)]
                                precision_hint: String,
                                #[serde(default)]
                                params_json: String,
                            }
                            let spec: InlineShaderSpec =
                                serde_json::from_str(spec_json).map_err(|e| {
                                    BitmapError(format!("Invalid shader blend spec: {e}"))
                                })?;
                            let mut bx0 = ctx.width;
                            let mut by0 = ctx.height;
                            let mut bx1 = 0u32;
                            let mut by1 = 0u32;
                            for py in 0..ctx.height {
                                for px in 0..ctx.width {
                                    let i = (py as usize * ctx.width as usize + px as usize) * 4;
                                    if child_buf[i + 3] == 0 {
                                        continue;
                                    }
                                    let ca = (ctx.clip_alpha_at)(
                                        &child.clip_ids,
                                        &node.clip_ids,
                                        f64::from(px) + 0.5,
                                        f64::from(py) + 0.5,
                                    )
                                    .clamp(0.0, 1.0);
                                    if ca > 0.0 {
                                        bx0 = bx0.min(px);
                                        by0 = by0.min(py);
                                        bx1 = bx1.max(px + 1);
                                        by1 = by1.max(py + 1);
                                    }
                                }
                            }
                            if bx0 < bx1 && by0 < by1 {
                                let bw = bx1 - bx0;
                                let bh = by1 - by0;
                                let sub_pixels = bw as usize * bh as usize;
                                let mut bg_straight = Vec::with_capacity(sub_pixels * 4);
                                let mut fg_straight = Vec::with_capacity(sub_pixels * 4);
                                let mut mask_alphas = Vec::with_capacity(sub_pixels);
                                for py in by0..by1 {
                                    for px in bx0..bx1 {
                                        let i =
                                            (py as usize * ctx.width as usize + px as usize) * 4;
                                        bg_straight.extend_from_slice(&straight(&buf[i..i + 4]));
                                        let ca = (ctx.clip_alpha_at)(
                                            &child.clip_ids,
                                            &node.clip_ids,
                                            f64::from(px) + 0.5,
                                            f64::from(py) + 0.5,
                                        )
                                        .clamp(0.0, 1.0);
                                        mask_alphas.push(ca);
                                        if ca > 0.0 {
                                            fg_straight
                                                .extend_from_slice(&straight(&child_buf[i..i + 4]));
                                        } else {
                                            fg_straight.extend_from_slice(&[0, 0, 0, 0]);
                                        }
                                    }
                                }
                                let blended = crate::as3_fte_3d::evaluate_blend_shader_rgba(
                                    &bg_straight,
                                    &fg_straight,
                                    bw,
                                    bh,
                                    &spec.bytecode_base64,
                                    &spec.precision_hint,
                                    &spec.params_json,
                                )
                                .map_err(|e| BitmapError(e.to_string()))?;
                                for sy in 0..bh as usize {
                                    for sx in 0..bw as usize {
                                        let si = sy * bw as usize + sx;
                                        let ca = mask_alphas[si];
                                        if ca <= 0.0 || fg_straight[si * 4 + 3] == 0 {
                                            continue;
                                        }
                                        let di = ((by0 as usize + sy) * ctx.width as usize
                                            + (bx0 as usize + sx))
                                            * 4;
                                        let p = &blended[si * 4..si * 4 + 4];
                                        let sh_pre = premul(
                                            u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
                                            true,
                                        );
                                        if ca >= 1.0 {
                                            buf[di..di + 4].copy_from_slice(&sh_pre);
                                        } else {
                                            for c in 0..4 {
                                                let b_val = f64::from(buf[di + c]);
                                                let s_val = f64::from(sh_pre[c]);
                                                buf[di + c] = (b_val * (1.0 - ca) + s_val * ca)
                                                    .round()
                                                    .clamp(0.0, 255.0)
                                                    as u8;
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
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

    /// Validates `JPEGXREncoderOptions` arguments (`quantization`, `color_space`, `trim_flex_bits`)
    /// and reports an explicit diagnostic because ITU-T T.832 `.jxr` entropy coding is outside
    /// the HTML5/WASM profile.
    pub fn encode_jpegxr_argb(
        cw: u32,
        ch: u32,
        _transparent: bool,
        argb_be: &[u8],
        _quantization: u8,
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
        if !matches!(color_space, "4:4:4" | "4:2:2" | "4:2:0" | "auto") {
            return Err(error(
                "JPEGXREncoderOptions.colorSpace requires 'auto', '4:4:4', '4:2:2' or '4:2:0'",
            ));
        }
        let w = cw as usize;
        let h = ch as usize;
        if argb_be.len() != w.saturating_mul(h).saturating_mul(4) {
            return Err(error("JPEG-XR ARGB input buffer length mismatch"));
        }
        Err(error(
            "JPEGXREncoderOptions (ITU-T T.832 .jxr) encoding is not supported in the HTML5/WASM profile; use PNGEncoderOptions or JPEGEncoderOptions",
        ))
    }
}

#[cfg(test)]
mod jpegxr_tests {
    use super::*;

    #[test]
    fn jpegxr_validates_options_then_reports_unsupported_html5_profile() {
        let argb = vec![255u8; 4 * 4 * 4];
        let trim_err = BitmapRect::encode_jpegxr_argb(4, 4, true, &argb, 20, "auto", 16)
            .expect_err("trimFlexBits > 15 rejected");
        assert!(trim_err.0.contains("trimFlexBits"));

        let cs_err = BitmapRect::encode_jpegxr_argb(4, 4, true, &argb, 20, "invalid", 0)
            .expect_err("invalid colorSpace rejected");
        assert!(cs_err.0.contains("colorSpace"));

        for cs in ["auto", "4:4:4", "4:2:2", "4:2:0"] {
            let err = BitmapRect::encode_jpegxr_argb(4, 4, true, &argb, 20, cs, 0)
                .expect_err("fake .jxr bitstream must not be emitted");
            assert_eq!(
                err.0,
                "JPEGXREncoderOptions (ITU-T T.832 .jxr) encoding is not supported in the HTML5/WASM profile; use PNGEncoderOptions or JPEGEncoderOptions"
            );
        }
    }

    #[test]
    fn pixel_arena_draw_rgba_shader_blends_bg_and_fg_and_preserves_unmasked_pixels_with_invert_shader()
     {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let mut arena = PixelArena::default();
        let dst = arena.create(4, 1, true, 0xFF40_2010).unwrap();
        let rev = arena.info(&dst).unwrap().revision;
        // Invert shader: dst = float4(1.0 - bg.rgb, bg.a), ignoring fg.rgb/fg.a!
        // Unmasked pixels (fg.a == 0) at x=0, x=2, x=3 MUST remain 0xFF402010;
        // only covered pixel x=1 (fg.a == 255) is inverted to (255-64, 255-32, 255-16, 255) = (0xBF, 0xDF, 0xEF, 0xFF).
        let invert_pb_json = r#"{"op":"invert","inputs":[{"name":"bg"},{"name":"fg"}]}"#;
        let invert_b64 = STANDARD.encode(invert_pb_json.as_bytes());
        let fg_rgba = [
            0u8, 0, 0, 0, // x=0: unmasked
            255, 255, 255, 255, // x=1: covered
            0, 0, 0, 0, // x=2: unmasked
            0, 0, 0, 0, // x=3: unmasked
        ];
        arena
            .draw_rgba_shader(&dst, 4, 1, &fg_rgba, None, &rev, &invert_b64, "full", "{}")
            .unwrap();
        let out_rgba = arena.pixels(&dst).unwrap();
        assert_eq!(
            &out_rgba[0..4],
            &[0x40, 0x20, 0x10, 0xFF],
            "x=0 unmasked preserved"
        );
        assert_eq!(
            &out_rgba[4..8],
            &[0xBF, 0xDF, 0xEF, 0xFF],
            "x=1 inverted bg"
        );
        assert_eq!(
            &out_rgba[8..12],
            &[0x40, 0x20, 0x10, 0xFF],
            "x=2 unmasked preserved"
        );
        assert_eq!(
            &out_rgba[12..16],
            &[0x40, 0x20, 0x10, 0xFF],
            "x=3 unmasked preserved"
        );

        assert!(
            arena
                .draw_rgba_blended(
                    &dst,
                    4,
                    1,
                    &fg_rgba,
                    None,
                    &arena.info(&dst).unwrap().revision,
                    "shader"
                )
                .is_err()
        );
    }
}
