//! AS2-only bitmap operations over the shared pixel arena.
//!
//! Channel/palette/merge/compare formulas, Park-Miller noise and the dissolve
//! permutation adapt Ruffle 07d3e430631333f80f754df01a968bdcd80d98c5 under its
//! MIT option. See docs/AS2-BITMAP-BATCH.md and THIRD_PARTY_NOTICES.txt.
//! No foreign engine/storage is retained. All source/destination overlap uses
//! one immutable clipped source snapshot; arena premultiplication remains shared.
use super::as2_perlin::PerlinSettings;
use super::*;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum As2BitmapCommand {
    CopyAlpha {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        mask: PixelRef,
        alpha_point: [i32; 2],
        merge: bool,
    },
    PerlinNoise {
        dst: PixelRef,
        settings: PerlinSettings,
    },
    Displacement {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        settings: DisplacementSettings,
    },
    FilterBounds {
        src: PixelRef,
        rect: BitmapRect,
    },
    CopyChannel {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        source_channel: u8,
        dest_channel: u8,
    },
    FloodFill {
        dst: PixelRef,
        x: i32,
        y: i32,
        color: u32,
    },
    ColorBounds {
        src: PixelRef,
        mask: u32,
        color: u32,
        find_color: bool,
    },
    Compare {
        dst: PixelRef,
        src: PixelRef,
    },
    Merge {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        multipliers: [i32; 4],
    },
    PaletteMap {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        tables: [Option<Vec<u32>>; 4],
    },
    Threshold {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        operation: String,
        threshold: u32,
        color: u32,
        mask: u32,
        copy_source: bool,
    },
    HitTest {
        src: PixelRef,
        point: [i32; 2],
        threshold: u8,
        other: HitTarget,
    },
    Noise {
        dst: PixelRef,
        seed: i32,
        low: u8,
        high: u8,
        channels: u8,
        grayscale: bool,
    },
    PixelDissolve {
        dst: PixelRef,
        src: PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        seed: i32,
        num_pixels: i32,
        fill_color: u32,
    },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplacementSettings {
    pub map_bitmap: Option<PixelRef>,
    pub map_point: [i32; 2],
    pub component_x: u8,
    pub component_y: u8,
    pub scale_x: f64,
    pub scale_y: f64,
    pub mode: String,
    pub color: u32,
    pub alpha: f64,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HitTarget {
    Point {
        point: [i32; 2],
    },
    Rectangle {
        rect: BitmapRect,
    },
    Bitmap {
        pixel_ref: PixelRef,
        point: [i32; 2],
        threshold: u8,
    },
}
fn argb(p: [u8; 4]) -> u32 {
    u32::from_be_bytes([p[3], p[0], p[1], p[2]])
}
fn rgba(value: u32) -> [u8; 4] {
    [
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
        (value >> 24) as u8,
    ]
}
fn channel(value: u8) -> Result<usize, BitmapError> {
    match value {
        1 => Ok(0),
        2 => Ok(1),
        4 => Ok(2),
        8 => Ok(3),
        _ => Err(error(
            "channel must be 1 (red), 2 (green), 4 (blue) or 8 (alpha)",
        )),
    }
}
#[derive(Clone, Copy)]
struct Region {
    sx: usize,
    sy: usize,
    dx: usize,
    dy: usize,
    width: usize,
    height: usize,
}
fn region(source: &Buffer, dest: &Buffer, rect: BitmapRect, point: [i32; 2]) -> Region {
    let x0 = 0_i64.max(-i64::from(rect.x)).max(-i64::from(point[0]));
    let y0 = 0_i64.max(-i64::from(rect.y)).max(-i64::from(point[1]));
    let x1 = i64::from(rect.width.max(0))
        .min(i64::from(source.width) - i64::from(rect.x))
        .min(i64::from(dest.width) - i64::from(point[0]));
    let y1 = i64::from(rect.height.max(0))
        .min(i64::from(source.height) - i64::from(rect.y))
        .min(i64::from(dest.height) - i64::from(point[1]));
    Region {
        sx: (i64::from(rect.x) + x0) as usize,
        sy: (i64::from(rect.y) + y0) as usize,
        dx: (i64::from(point[0]) + x0) as usize,
        dy: (i64::from(point[1]) + y0) as usize,
        width: (x1 - x0).max(0) as usize,
        height: (y1 - y0).max(0) as usize,
    }
}
fn snapshot(source: &Buffer, area: Region) -> Vec<[u8; 4]> {
    let mut result = Vec::with_capacity(area.width * area.height);
    for y in 0..area.height {
        for x in 0..area.width {
            let offset = ((area.sy + y) * source.width as usize + area.sx + x) * 4;
            result.push(straight(
                &source.pixels.as_ref().unwrap()[offset..offset + 4],
            ));
        }
    }
    result
}
impl PixelArena {
    pub(crate) fn as2_command(&mut self, command: As2BitmapCommand) -> Result<Value, BitmapError> {
        match command {
            As2BitmapCommand::CopyAlpha {
                dst,
                src,
                rect,
                point,
                mask,
                alpha_point,
                merge,
            } => {
                self.copy_alpha(&dst, &src, rect, point, &mask, alpha_point, merge)?;
                Ok(Value::Null)
            }
            As2BitmapCommand::PerlinNoise { dst, settings } => {
                let b = self.live(&dst)?;
                let output = super::as2_perlin::generate(b.width, b.height, &settings)?;
                let b = self.live_mut(&dst)?;
                for (pixel, next) in b
                    .pixels
                    .as_mut()
                    .unwrap()
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(output)
                {
                    pixel.copy_from_slice(&premul(argb(next), b.transparent));
                }
                b.revision += 1;
                Ok(Value::Null)
            }

            As2BitmapCommand::Displacement {
                dst,
                src,
                rect,
                point,
                settings,
            } => {
                self.displacement(&dst, &src, rect, point, &settings)?;
                Ok(Value::Null)
            }
            As2BitmapCommand::FilterBounds { src, rect } => {
                let b = self.live(&src)?;
                let (x0, y0, x1, y1) =
                    clip(rect.x, rect.y, rect.width, rect.height, b.width, b.height);
                Ok(json!(BitmapRect {
                    x: x0 as i32,
                    y: y0 as i32,
                    width: (x1 - x0) as i32,
                    height: (y1 - y0) as i32
                }))
            }
            As2BitmapCommand::CopyChannel {
                dst,
                src,
                rect,
                point,
                source_channel,
                dest_channel,
            } => {
                let from = channel(source_channel)?;
                let to = channel(dest_channel)?;
                self.map_region(&dst, &src, rect, point, |source, mut dest| {
                    dest[to] = source[from];
                    Some(dest)
                })?;
                Ok(Value::Null)
            }
            As2BitmapCommand::FloodFill { dst, x, y, color } => {
                self.flood_fill(&dst, x, y, color)?;
                Ok(Value::Null)
            }
            As2BitmapCommand::ColorBounds {
                src,
                mask,
                color,
                find_color,
            } => Ok(json!(self.color_bounds(&src, mask, color, find_color)?)),
            As2BitmapCommand::Compare { dst, src } => self.compare_buffers(&dst, &src),
            As2BitmapCommand::Merge {
                dst,
                src,
                rect,
                point,
                multipliers,
            } => {
                let weights = multipliers.map(|v| v.clamp(0, 256) as u32);
                self.map_region(&dst, &src, rect, point, |source, dest| {
                    Some(std::array::from_fn(|c| {
                        ((u32::from(source[c]) * weights[c]
                            + u32::from(dest[c]) * (256 - weights[c]))
                            / 256) as u8
                    }))
                })?;
                Ok(Value::Null)
            }
            As2BitmapCommand::PaletteMap {
                dst,
                src,
                rect,
                point,
                tables,
            } => {
                if tables.iter().flatten().any(|values| values.len() != 256) {
                    return Err(error(
                        "paletteMap tables must contain exactly 256 uint entries, or null for identity",
                    ));
                }
                let shifts = [16, 8, 0, 24];
                self.map_region(&dst, &src, rect, point, |source, _| {
                    Some(rgba((0..4).fold(0_u32, |value, c| {
                        value.wrapping_add(
                            tables[c]
                                .as_ref()
                                .map_or(u32::from(source[c]) << shifts[c], |table| {
                                    table[source[c] as usize]
                                }),
                        )
                    })))
                })?;
                Ok(Value::Null)
            }
            As2BitmapCommand::Threshold {
                dst,
                src,
                rect,
                point,
                operation,
                threshold,
                color,
                mask,
                copy_source,
            } => {
                if !["<", "<=", ">", ">=", "==", "!="].contains(&operation.as_str()) {
                    return Err(error("threshold operation must be <, <=, >, >=, == or !="));
                }
                let threshold = threshold & mask;
                let mut matched = 0;
                self.map_region(&dst, &src, rect, point, |source, _| {
                    let value = argb(source) & mask;
                    let matches = match operation.as_str() {
                        "<" => value < threshold,
                        "<=" => value <= threshold,
                        ">" => value > threshold,
                        ">=" => value >= threshold,
                        "==" => value == threshold,
                        _ => value != threshold,
                    };
                    if matches {
                        matched += 1;
                        Some(rgba(color))
                    } else if copy_source {
                        Some(source)
                    } else {
                        None
                    }
                })?;
                Ok(json!(matched))
            }
            As2BitmapCommand::HitTest {
                src,
                point,
                threshold,
                other,
            } => Ok(json!(self.hit_test(&src, point, threshold, other)?)),
            As2BitmapCommand::Noise {
                dst,
                seed,
                low,
                high,
                channels,
                grayscale,
            } => {
                self.noise(&dst, seed, low, high, channels, grayscale)?;
                Ok(Value::Null)
            }
            As2BitmapCommand::PixelDissolve {
                dst,
                src,
                rect,
                point,
                seed,
                num_pixels,
                fill_color,
            } => Ok(json!(self.pixel_dissolve(
                &dst, &src, rect, point, seed, num_pixels, fill_color
            )?)),
        }
    }
    fn displacement(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        settings: &DisplacementSettings,
    ) -> Result<(), BitmapError> {
        if ![0, 1, 2, 4, 8].contains(&settings.component_x)
            || ![0, 1, 2, 4, 8].contains(&settings.component_y)
            || !settings.scale_x.is_finite()
            || settings.scale_x.abs() > 8192.0
            || !settings.scale_y.is_finite()
            || settings.scale_y.abs() > 8192.0
            || !settings.alpha.is_finite()
            || !(0.0..=1.0).contains(&settings.alpha)
            || settings.color > 0xffffff
            || !["wrap", "clamp", "ignore", "color"].contains(&settings.mode.as_str())
        {
            return Err(error(
                "displacement requires valid channels/mode, finite scales within +/-8192, RGB color and alpha0..1",
            ));
        }
        let source = self.live(src)?;
        let dest = self.live(dst)?;
        let area = region(source, dest, rect, point);
        if area.width * area.height > crate::MAX_FILTER_PIXELS {
            return Err(error(
                "displacement work exceeds the 4 Mi-pixel filter budget",
            ));
        }
        let map = settings
            .map_bitmap
            .as_ref()
            .map(|map| self.live(map))
            .transpose()?;
        let fill = (settings.color & 0xffffff) | (((settings.alpha * 255.0).round() as u32) << 24);
        let mut output = Vec::with_capacity(area.width * area.height);
        let fetch = |x: usize, y: usize| {
            let offset = (y * source.width as usize + x) * 4;
            straight(&source.pixels.as_ref().unwrap()[offset..offset + 4])
        };
        for y in 0..area.height {
            for x in 0..area.width {
                let sx = area.sx + x;
                let sy = area.sy + y;
                let mut mx = sx as i64;
                let mut my = sy as i64;
                if let Some(map) = map {
                    let map_x = sx as i64 - i64::from(settings.map_point[0]);
                    let map_y = sy as i64 - i64::from(settings.map_point[1]);
                    if map_x >= 0
                        && map_y >= 0
                        && map_x < i64::from(map.width)
                        && map_y < i64::from(map.height)
                    {
                        let index = (map_y as usize * map.width as usize + map_x as usize) * 4;
                        let pixel = straight(&map.pixels.as_ref().unwrap()[index..index + 4]);
                        let sample =
                            |value: u8| channel(value).map_or(128.0, |c| f64::from(pixel[c]));
                        mx = (sx as f64
                            + (sample(settings.component_x) - 128.0) * settings.scale_x / 256.0)
                            .floor() as i64;
                        my = (sy as f64
                            + (sample(settings.component_y) - 128.0) * settings.scale_y / 256.0)
                            .floor() as i64;
                    }
                }
                let outside = mx < 0
                    || my < 0
                    || mx >= i64::from(source.width)
                    || my >= i64::from(source.height);
                let pixel = match settings.mode.as_str() {
                    "color" if outside => rgba(fill),
                    "ignore" if outside => fetch(sx, sy),
                    "clamp" => fetch(
                        mx.clamp(0, i64::from(source.width) - 1) as usize,
                        my.clamp(0, i64::from(source.height) - 1) as usize,
                    ),
                    _ => fetch(
                        mx.rem_euclid(i64::from(source.width)) as usize,
                        my.rem_euclid(i64::from(source.height)) as usize,
                    ),
                };
                output.push(premul(argb(pixel), dest.transparent));
            }
        }
        let dest = self.live_mut(dst)?;
        for (at, pixel) in output.into_iter().enumerate() {
            let offset =
                ((area.dy + at / area.width) * dest.width as usize + area.dx + at % area.width) * 4;
            dest.pixels.as_mut().unwrap()[offset..offset + 4].copy_from_slice(&pixel);
        }
        if area.width > 0 && area.height > 0 {
            dest.revision += 1;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn copy_alpha(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        mask: &PixelRef,
        alpha_point: [i32; 2],
        merge: bool,
    ) -> Result<(), BitmapError> {
        let area = region(self.live(src)?, self.live(dst)?, rect, point);
        let opaque = !self.live(dst)?.transparent;
        let mask = self.live(mask)?;
        let mask_width = mask.width;
        let mask_height = mask.height;
        let alpha: Vec<u8> = mask
            .pixels
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .collect();
        let mut index = 0;
        self.map_region(dst, src, rect, point, |mut source, dest| {
            let mx = i64::from(alpha_point[0]) + area.sx as i64 - i64::from(rect.x)
                + (index % area.width) as i64;
            let my = i64::from(alpha_point[1]) + area.sy as i64 - i64::from(rect.y)
                + (index / area.width) as i64;
            index += 1;
            let mask_alpha = if mx >= 0
                && my >= 0
                && mx < i64::from(mask_width)
                && my < i64::from(mask_height)
            {
                alpha[my as usize * mask_width as usize + mx as usize]
            } else {
                0
            };
            source[3] = ((u32::from(source[3]) * u32::from(mask_alpha) + 127) / 255) as u8;
            if merge || opaque {
                let s = premul(argb(source), true);
                let d = premul(argb(dest), !opaque);
                let inverse = 255 - u32::from(s[3]);
                let mut output: [u8; 4] = std::array::from_fn(|c| {
                    (u32::from(s[c]) + (u32::from(d[c]) * inverse + 127) / 255).min(255) as u8
                });
                if opaque {
                    output[3] = 255;
                }
                Some(straight(&output))
            } else {
                Some(source)
            }
        })
    }
    fn map_region(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        mut edit: impl FnMut([u8; 4], [u8; 4]) -> Option<[u8; 4]>,
    ) -> Result<(), BitmapError> {
        let area = region(self.live(src)?, self.live(dst)?, rect, point);
        if area.width == 0 || area.height == 0 {
            return Ok(());
        }
        let source = snapshot(self.live(src)?, area);
        let dest = self.live_mut(dst)?;
        let mut changed = false;
        for (at, source) in source.into_iter().enumerate() {
            let offset =
                ((area.dy + at / area.width) * dest.width as usize + area.dx + at % area.width) * 4;
            let pixel = &mut dest.pixels.as_mut().unwrap()[offset..offset + 4];
            if let Some(next) = edit(source, straight(pixel)) {
                pixel.copy_from_slice(&premul(argb(next), dest.transparent));
                changed = true;
            }
        }
        if changed {
            dest.revision += 1;
        }
        Ok(())
    }
    fn flood_fill(
        &mut self,
        dst: &PixelRef,
        x: i32,
        y: i32,
        color: u32,
    ) -> Result<(), BitmapError> {
        let b = self.live_mut(dst)?;
        if x < 0 || y < 0 || x as u32 >= b.width || y as u32 >= b.height {
            return Ok(());
        }
        let start = y as usize * b.width as usize + x as usize;
        let replacement = premul(color, b.transparent);
        let pixels = b.pixels.as_mut().unwrap().as_chunks_mut::<4>().0;
        let original = pixels[start];
        if original == replacement {
            return Ok(());
        }
        // Mark when enqueued so the bounded u32 stack has at most one entry per pixel.
        pixels[start] = replacement;
        let mut pending = vec![start as u32];
        let width = b.width as usize;
        while let Some(index) = pending.pop() {
            let index = index as usize;
            let x = index % width;
            let y = index / width;
            let adjacent = [
                (x > 0).then(|| index - 1),
                (x + 1 < width).then_some(index + 1),
                (y > 0).then(|| index - width),
                (y + 1 < b.height as usize).then_some(index + width),
            ];
            for next in adjacent.into_iter().flatten() {
                if pixels[next] == original {
                    pixels[next] = replacement;
                    pending.push(next as u32);
                }
            }
        }
        b.revision += 1;
        Ok(())
    }
    fn color_bounds(
        &self,
        src: &PixelRef,
        mask: u32,
        color: u32,
        find: bool,
    ) -> Result<BitmapRect, BitmapError> {
        let b = self.live(src)?;
        let mut left = b.width;
        let mut top = b.height;
        let mut right = 0;
        let mut bottom = 0;
        for (index, pixel) in b
            .pixels
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if ((argb(straight(pixel)) & mask) == color) == find {
                let x = index as u32 % b.width;
                let y = index as u32 / b.width;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
        if right == 0 || bottom == 0 {
            return Ok(BitmapRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        }
        Ok(BitmapRect {
            x: left as i32,
            y: top as i32,
            width: (right - left) as i32,
            height: (bottom - top) as i32,
        })
    }
    fn compare_buffers(&mut self, left: &PixelRef, right: &PixelRef) -> Result<Value, BitmapError> {
        let a = self.live(left)?;
        let b = self.live(right)?;
        if a.width != b.width {
            return Ok(json!(-3));
        }
        if a.height != b.height {
            return Ok(json!(-4));
        }
        let dimensions = (a.width, a.height);
        let mut different = false;
        let mut pixels = Vec::with_capacity(a.pixels.as_ref().unwrap().len());
        for (a, b) in a
            .pixels
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.pixels.as_ref().unwrap().as_chunks::<4>().0)
        {
            let a = straight(a);
            let b = straight(b);
            let diff = if a == b {
                [0; 4]
            } else {
                different = true;
                if a[..3] != b[..3] {
                    [
                        a[0].wrapping_sub(b[0]),
                        a[1].wrapping_sub(b[1]),
                        a[2].wrapping_sub(b[2]),
                        255,
                    ]
                } else {
                    let alpha = a[3].wrapping_sub(b[3]);
                    [255, 255, 255, alpha]
                }
            };
            pixels.extend_from_slice(&premul(argb(diff), true));
        }
        if !different {
            return Ok(json!(0));
        }
        let reference = self.create(dimensions.0, dimensions.1, true, 0)?;
        self.live_mut(&reference)?.pixels = Some(pixels);
        Ok(json!(reference))
    }
    fn hit_test(
        &self,
        src: &PixelRef,
        origin: [i32; 2],
        threshold: u8,
        other: HitTarget,
    ) -> Result<bool, BitmapError> {
        let first = self.live(src)?;
        let hit = |x: i64, y: i64, limit: u8| {
            x >= 0
                && y >= 0
                && x < i64::from(first.width)
                && y < i64::from(first.height)
                && first.pixels.as_ref().unwrap()
                    [(y as usize * first.width as usize + x as usize) * 4 + 3]
                    >= limit
        };
        match other {
            HitTarget::Point { point } => Ok(hit(
                i64::from(point[0]) - i64::from(origin[0]),
                i64::from(point[1]) - i64::from(origin[1]),
                threshold.max(1),
            )),
            HitTarget::Rectangle { rect } => {
                let left =
                    (i64::from(rect.x) - i64::from(origin[0])).clamp(0, i64::from(first.width));
                let top =
                    (i64::from(rect.y) - i64::from(origin[1])).clamp(0, i64::from(first.height));
                let right = (i64::from(rect.x) + i64::from(rect.width.max(0))
                    - i64::from(origin[0]))
                .clamp(0, i64::from(first.width));
                let bottom = (i64::from(rect.y) + i64::from(rect.height.max(0))
                    - i64::from(origin[1]))
                .clamp(0, i64::from(first.height));
                for y in top..bottom {
                    for x in left..right {
                        if hit(x, y, threshold) {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            HitTarget::Bitmap {
                pixel_ref,
                point,
                threshold: second_threshold,
            } => {
                let second = self.live(&pixel_ref)?;
                let dx = i64::from(point[0]) - i64::from(origin[0]);
                let dy = i64::from(point[1]) - i64::from(origin[1]);
                let left = dx.max(0);
                let top = dy.max(0);
                let right = (dx + i64::from(second.width)).min(i64::from(first.width));
                let bottom = (dy + i64::from(second.height)).min(i64::from(first.height));
                for y in top..bottom {
                    for x in left..right {
                        if hit(x, y, threshold)
                            && second.pixels.as_ref().unwrap()[((y - dy) as usize
                                * second.width as usize
                                + (x - dx) as usize)
                                * 4
                                + 3]
                                >= second_threshold
                        {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
        }
    }
    fn noise(
        &mut self,
        dst: &PixelRef,
        seed: i32,
        low: u8,
        high: u8,
        channels: u8,
        grayscale: bool,
    ) -> Result<(), BitmapError> {
        if low > high || channels > 15 {
            return Err(error("noise requires low <= high and channel mask 0..15"));
        }
        let b = self.live_mut(dst)?;
        let mut random = Lehmer::new(seed);
        for pixel in b.pixels.as_mut().unwrap().as_chunks_mut::<4>().0 {
            let mut value = [0, 0, 0, 255];
            if grayscale {
                let gray = random.range(low, high);
                value[..3].fill(gray);
            } else {
                for (c, value) in value[..3].iter_mut().enumerate() {
                    if channels & (1 << c) != 0 {
                        *value = random.range(low, high);
                    }
                }
            }
            if b.transparent && channels & 8 != 0 {
                value[3] = random.range(low, high);
            }
            pixel.copy_from_slice(&premul(argb(value), b.transparent));
        }
        b.revision += 1;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn pixel_dissolve(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: BitmapRect,
        point: [i32; 2],
        seed: i32,
        count: i32,
        fill: u32,
    ) -> Result<i32, BitmapError> {
        if count < 0 {
            return Err(error("pixelDissolve numPixels must be nonnegative"));
        }
        let area = region(self.live(src)?, self.live(dst)?, rect, point);
        let length = (area.width * area.height) as u32;
        if length == 0 {
            return Ok(0);
        }
        let source = if dst == src {
            None
        } else {
            Some(snapshot(self.live(src)?, area))
        };
        let count = (count as u32).min(length);
        let bits = (32 - (length.max(2) - 1).leading_zeros()).div_ceil(2) * 2;
        let half = bits / 2;
        let permutation = 1_u32 << bits;
        let mask = (1_u32 << half) - 1;
        let mut cursor = i64::from(seed).rem_euclid(i64::from(permutation)) as u32;
        let b = self.live_mut(dst)?;
        let write = |b: &mut Buffer, index: u32| {
            let x = index as usize % area.width;
            let y = index as usize / area.width;
            let offset = ((area.dy + y) * b.width as usize + area.dx + x) * 4;
            let value = source
                .as_ref()
                .map_or(fill, |pixels| argb(pixels[index as usize]));
            b.pixels.as_mut().unwrap()[offset..offset + 4]
                .copy_from_slice(&premul(value, b.transparent));
        };
        write(b, 0);
        for _ in 0..count {
            let mut index = 0;
            for _ in 0..permutation {
                cursor = (cursor + 1) % permutation;
                let low = cursor & mask;
                let high = cursor >> half;
                index = ((high ^ (low * low + 1) & mask) << half) | low;
                if (index > 0 && index < length) || length == 1 {
                    break;
                }
            }
            if length == 1 {
                index = 0;
            }
            write(b, index);
        }
        b.revision += 1;
        Ok(cursor as i32)
    }
}
struct Lehmer(u32);
impl Lehmer {
    fn new(seed: i32) -> Self {
        Self(if seed <= 0 {
            (-i64::from(seed) + 1) as u32
        } else {
            seed as u32
        })
    }
    fn range(&mut self, low: u8, high: u8) -> u8 {
        self.0 = (u64::from(self.0) * 16807 % 2147483647) as u32;
        low + (self.0 % (u32::from(high) - u32::from(low) + 1)) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_bitmap_pixel_family() {
        let mut arena = PixelArena::default();
        arena.reset();
        let source = arena.create(4, 4, true, 0xff102030).unwrap();
        let dest = arena.create(4, 4, true, 0xffa0b0c0).unwrap();
        let rect = BitmapRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        };
        arena
            .as2_command(As2BitmapCommand::CopyChannel {
                dst: dest.clone(),
                src: source.clone(),
                rect,
                point: [0, 0],
                source_channel: 1,
                dest_channel: 4,
            })
            .unwrap();
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xffa0b010);
        arena
            .as2_command(As2BitmapCommand::Merge {
                dst: dest.clone(),
                src: source.clone(),
                rect,
                point: [0, 0],
                multipliers: [256, 0, 128, 256],
            })
            .unwrap();
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xff10b020);
        let red = (0..256).map(|v| (255 - v) << 16).collect();
        arena
            .as2_command(As2BitmapCommand::PaletteMap {
                dst: dest.clone(),
                src: source.clone(),
                rect,
                point: [0, 0],
                tables: [Some(red), None, None, None],
            })
            .unwrap();
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xffef2030);
        let count = arena
            .as2_command(As2BitmapCommand::Threshold {
                dst: dest.clone(),
                src: source.clone(),
                rect: BitmapRect {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 2,
                },
                point: [1, 1],
                operation: "==".into(),
                threshold: 0xff102030,
                color: 0xff112233,
                mask: u32::MAX,
                copy_source: false,
            })
            .unwrap();
        assert_eq!(count, json!(4));
        assert_eq!(
            arena
                .color_bounds(&dest, u32::MAX, 0xff112233, true)
                .unwrap(),
            BitmapRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2
            }
        );
        arena.flood_fill(&dest, 1, 1, 0xff778899).unwrap();
        assert_eq!(arena.get(&dest, 2, 2, true).unwrap(), 0xff778899);
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xffef2030);
        assert!(
            arena
                .hit_test(&dest, [10, 20], 1, HitTarget::Point { point: [11, 21] })
                .unwrap()
        );
        assert!(
            !arena
                .hit_test(
                    &dest,
                    [10, 20],
                    1,
                    HitTarget::Rectangle {
                        rect: BitmapRect {
                            x: 0,
                            y: 0,
                            width: 1,
                            height: 1
                        }
                    }
                )
                .unwrap()
        );
        assert!(
            arena
                .hit_test(
                    &dest,
                    [0, 0],
                    255,
                    HitTarget::Bitmap {
                        pixel_ref: source.clone(),
                        point: [2, 2],
                        threshold: 255
                    }
                )
                .unwrap()
        );
        let same = arena.clone_buffer(&dest).unwrap();
        assert_eq!(arena.compare_buffers(&dest, &same).unwrap(), json!(0));
        arena.set(&same, 0, 0, 0xff000000, true).unwrap();
        let difference: PixelRef =
            serde_json::from_value(arena.compare_buffers(&dest, &same).unwrap()).unwrap();
        assert_eq!(arena.get(&difference, 0, 0, true).unwrap(), 0xffef2030);
        arena.noise(&dest, 7, 16, 64, 7, true).unwrap();
        arena.noise(&same, 7, 16, 64, 7, true).unwrap();
        assert_eq!(arena.compare_buffers(&dest, &same).unwrap(), json!(0));
        let pixel = rgba(arena.get(&dest, 0, 0, true).unwrap());
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert!((16..=64).contains(&pixel[0]));
        let resume = arena
            .pixel_dissolve(&dest, &source, rect, [0, 0], 0, 8, 0)
            .unwrap();
        assert!(resume > 0);
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xff102030);
        arena
            .pixel_dissolve(&dest, &dest, rect, [0, 0], resume, 16, 0xff123456)
            .unwrap();
        assert_eq!(arena.get(&dest, 3, 3, true).unwrap(), 0xff123456);
        let before = arena.pixels(&dest).unwrap();
        assert!(
            arena
                .as2_command(As2BitmapCommand::PaletteMap {
                    dst: dest.clone(),
                    src: source.clone(),
                    rect,
                    point: [0, 0],
                    tables: [Some(vec![0]), None, None, None]
                })
                .is_err()
        );
        assert_eq!(arena.pixels(&dest).unwrap(), before);
        // One immutable source snapshot prevents overlapping writes from feeding later reads.
        arena.set(&dest, 0, 0, 0xff010203, true).unwrap();
        arena.set(&dest, 1, 0, 0xff040506, true).unwrap();
        arena
            .as2_command(As2BitmapCommand::CopyChannel {
                dst: dest.clone(),
                src: dest.clone(),
                rect: BitmapRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                point: [1, 0],
                source_channel: 1,
                dest_channel: 1,
            })
            .unwrap();
        assert_eq!(arena.get(&dest, 2, 0, true).unwrap() >> 16 & 255, 4);
        let mask = arena.create(4, 4, true, 0x80000000).unwrap();
        arena
            .copy_alpha(&dest, &source, rect, [0, 0], &mask, [0, 0], false)
            .unwrap();
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap() >> 24, 128);
        let procedural = PerlinSettings {
            base: [4.0, 4.0],
            octaves: 3,
            seed: 19,
            stitch: true,
            fractal: true,
            channels: 7,
            grayscale: true,
            offsets: vec![[0.5, 0.25]],
        };
        let one = super::super::as2_perlin::generate(16, 16, &procedural).unwrap();
        let two = super::super::as2_perlin::generate(16, 16, &procedural).unwrap();
        assert_eq!(one, two);
        assert!(one.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(
            one.iter()
                .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[3] == 255)
        );
        arena.dispose(&source).unwrap();
        assert!(arena.compare_buffers(&source, &dest).is_err());
    }
}

#[cfg(test)]
mod displacement_tests {
    use super::*;
    #[test]
    fn as2_displacement_filter_family() {
        let mut arena = PixelArena::default();
        arena.reset();
        let source = arena.create(3, 1, true, 0xff000000).unwrap();
        arena.set(&source, 0, 0, 0xffff0000, true).unwrap();
        arena.set(&source, 1, 0, 0xff00ff00, true).unwrap();
        arena.set(&source, 2, 0, 0xff0000ff, true).unwrap();
        let map = arena.create(3, 1, true, 0xffff8080).unwrap();
        let dest = arena.create(3, 1, true, 0).unwrap();
        let rect = BitmapRect {
            x: 0,
            y: 0,
            width: 3,
            height: 1,
        };
        let mut settings = DisplacementSettings {
            map_bitmap: Some(map.clone()),
            map_point: [0, 0],
            component_x: 1,
            component_y: 0,
            scale_x: 3.0,
            scale_y: 0.0,
            mode: "wrap".into(),
            color: 0x112233,
            alpha: 1.0,
        };
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.get(&dest, 0, 0, true).unwrap(), 0xff00ff00);
        assert_eq!(arena.get(&dest, 2, 0, true).unwrap(), 0xffff0000);
        settings.mode = "color".into();
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.get(&dest, 2, 0, true).unwrap(), 0xff112233);
        settings.mode = "clamp".into();
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.get(&dest, 2, 0, true).unwrap(), 0xff0000ff);
        settings.mode = "ignore".into();
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.get(&dest, 2, 0, true).unwrap(), 0xff0000ff);
        settings.map_point = [10, 10];
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.compare_buffers(&dest, &source).unwrap(), json!(0));
        settings.map_bitmap = None;
        arena
            .displacement(&dest, &source, rect, [0, 0], &settings)
            .unwrap();
        assert_eq!(arena.compare_buffers(&dest, &source).unwrap(), json!(0));
        let before = arena.pixels(&dest).unwrap();
        settings.mode = "unsupported".into();
        assert!(
            arena
                .displacement(&dest, &source, rect, [0, 0], &settings)
                .is_err()
        );
        assert_eq!(arena.pixels(&dest).unwrap(), before);
    }
}
