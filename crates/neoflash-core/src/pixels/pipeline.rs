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
        let result = filter_rgba(w, h, &rgba, std::slice::from_ref(filter))
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
                for c in 0..4 {
                    d[c] = (u32::from(source[c])
                        + (u32::from(d[c]) * (255 - u32::from(source[3])) + 127) / 255)
                        .min(255) as u8;
                }
                if !b.transparent {
                    d[3] = 255;
                }
            }
        }
        if x1 > x0 && y1 > y0 {
            b.revision += 1;
        }
        Ok(())
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
