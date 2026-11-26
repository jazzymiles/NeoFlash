//! Session-owned premultiplied RGBA8 buffers. Public transfer is straight RGBA.
//! NeoFlash uses nearest-integer c*a/255 and c*255/a (zero RGB at alpha zero).
//! This is an explicit integer profile, not exact historical Flash rounding.
//! Copy/scroll snapshots at most one 64 MiB clipped rectangle for overlap safety;
//! this temporary memory is additional to the 128 MiB resident arena budget.
use serde::{Deserialize, Serialize};
mod as2;
mod as2_perlin;
pub mod jxr;
mod pipeline;
pub use as2::As2BitmapCommand;
pub use pipeline::{BitmapRect, PixelWrite};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};
pub const MAX_BITMAP_DIMENSION: u32 = 4096;
pub const MAX_BITMAP_BYTES: usize = 128 * 1024 * 1024;
const MAX_BUFFERS: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelRef {
    pub id: String,
    pub generation: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelSnapping {
    #[default]
    Auto,
    Always,
    Never,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelDescriptor {
    pub pixel_ref: PixelRef,
    pub width: u32,
    pub height: u32,
    pub transparent: bool,
    pub revision: String,
    pub disposed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("BitmapData: {0}")]
pub struct BitmapError(pub String);
fn error(s: &str) -> BitmapError {
    BitmapError(s.into())
}
struct Buffer {
    width: u32,
    height: u32,
    transparent: bool,
    revision: u64,
    pixels: Option<Vec<u8>>,
    lock_depth: u32,
    locked_premul: Option<Vec<u8>>,
    // Render-only brush handles can retain encoded Library data when the live pixel arena is full.
    encoded_paint: Option<crate::ResourceUnit>,
}
impl Buffer {
    fn require_pixels(&self) -> Result<(), BitmapError> {
        if self.pixels.is_some() {
            Ok(())
        } else if self.encoded_paint.is_some() {
            Err(error("authored paint is read-only"))
        } else {
            Err(error("buffer is disposed"))
        }
    }
}
#[derive(Default)]
pub(crate) struct PixelArena {
    generation: u64,
    next: u64,
    bytes: usize,
    buffers: HashMap<PixelRef, Buffer>,
}
fn premul(argb: u32, transparent: bool) -> [u8; 4] {
    let a = if transparent { argb >> 24 } else { 255 };
    [
        ((argb >> 16 & 255) * a + 127) / 255,
        ((argb >> 8 & 255) * a + 127) / 255,
        ((argb & 255) * a + 127) / 255,
        a,
    ]
    .map(|v| v as u8)
}
fn straight(p: &[u8]) -> [u8; 4] {
    let a = u32::from(p[3]);
    let channel = |v: u8| {
        (u32::from(v) * 255 + a / 2)
            .checked_div(a)
            .unwrap_or(0)
            .min(255) as u8
    };
    [channel(p[0]), channel(p[1]), channel(p[2]), p[3]]
}
impl PixelArena {
    pub(crate) fn reset(&mut self) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        self.generation = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("pixel generation exhausted");
        self.next = 0;
        self.bytes = 0;
        self.buffers.clear();
    }
    pub(crate) fn reserve_import(&self, w: u32, h: u32) -> Result<(), BitmapError> {
        if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
            return Err(error("dimensions must be in 1..=4096"));
        }
        if self.buffers.len() >= MAX_BUFFERS
            || self.bytes + w as usize * h as usize * 4 > MAX_BITMAP_BYTES
        {
            return Err(error(
                "buffer budget exceeded (256 records / 128 MiB live pixels)",
            ));
        }
        Ok(())
    }
    pub(crate) fn release(&mut self, reference: &PixelRef) -> Result<(), BitmapError> {
        let buffer = self
            .buffers
            .remove(reference)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        if let Some(pixels) = buffer.pixels {
            self.bytes -= pixels.len();
        }
        if let Some(snap) = buffer.locked_premul {
            self.bytes -= snap.len();
        }
        Ok(())
    }
    pub(crate) fn create(
        &mut self,
        w: u32,
        h: u32,
        transparent: bool,
        color: u32,
    ) -> Result<PixelRef, BitmapError> {
        if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
            return Err(error("dimensions must be in 1..=4096"));
        }
        let bytes = w as usize * h as usize * 4;
        if self.buffers.len() >= MAX_BUFFERS || self.bytes + bytes > MAX_BITMAP_BYTES {
            return Err(error(
                "buffer budget exceeded (256 records / 128 MiB live pixels)",
            ));
        }
        let pixel = premul(color, transparent);
        let mut pixels = vec![0; bytes];
        for p in pixels.as_chunks_mut::<4>().0 {
            p.copy_from_slice(&pixel);
        }
        Ok(self.insert_buffer(Buffer {
            width: w,
            height: h,
            transparent,
            revision: 1,
            pixels: Some(pixels),
            lock_depth: 0,
            locked_premul: None,
            encoded_paint: None,
        }))
    }
    fn insert_buffer(&mut self, buffer: Buffer) -> PixelRef {
        self.next += 1;
        let reference = PixelRef {
            id: self.next.to_string(),
            generation: self.generation.to_string(),
        };
        self.bytes += buffer.pixels.as_ref().map_or(0, Vec::len);
        self.buffers.insert(reference.clone(), buffer);
        reference
    }
    fn buffer(&self, r: &PixelRef) -> Result<&Buffer, BitmapError> {
        self.buffers
            .get(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))
    }
    fn live(&self, r: &PixelRef) -> Result<&Buffer, BitmapError> {
        let b = self.buffer(r)?;
        b.require_pixels()?;
        Ok(b)
    }
    fn live_mut(&mut self, r: &PixelRef) -> Result<&mut Buffer, BitmapError> {
        let Self { bytes, buffers, .. } = self;
        let b = buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        b.require_pixels()?;
        if b.lock_depth > 0
            && b.locked_premul.is_none()
            && let Some(px) = &b.pixels
        {
            if *bytes + px.len() > MAX_BITMAP_BYTES {
                return Err(error(
                    "buffer budget exceeded (256 records / 128 MiB live pixels)",
                ));
            }
            *bytes += px.len();
            b.locked_premul = Some(px.clone());
        }
        Ok(b)
    }
    pub(crate) fn lock(&mut self, r: &PixelRef) -> Result<String, BitmapError> {
        let b = self
            .buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        if b.pixels.is_none() && b.encoded_paint.is_none() {
            return Err(error("buffer is disposed"));
        }
        b.lock_depth = b.lock_depth.saturating_add(1);
        Ok(b.revision.to_string())
    }
    pub(crate) fn unlock(&mut self, r: &PixelRef) -> Result<String, BitmapError> {
        let Self { bytes, buffers, .. } = self;
        let b = buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        if b.pixels.is_none() && b.encoded_paint.is_none() {
            return Err(error("buffer is disposed"));
        }
        if b.lock_depth > 0 {
            b.lock_depth -= 1;
            if b.lock_depth == 0
                && let Some(snap) = b.locked_premul.take()
            {
                *bytes -= snap.len();
            }
        }
        Ok(b.revision.to_string())
    }
    pub(crate) fn locked_pixels(&self, r: &PixelRef) -> Result<Vec<u8>, BitmapError> {
        let b = self.buffer(r)?;
        if let Some(premul_snap) = &b.locked_premul {
            let mut result = Vec::with_capacity(premul_snap.len());
            for pixel in premul_snap.as_chunks::<4>().0 {
                result.extend_from_slice(&straight(pixel));
            }
            return Ok(result);
        }
        self.pixels(r)
    }
    pub(crate) fn info(&self, r: &PixelRef) -> Result<PixelDescriptor, BitmapError> {
        let b = self.buffer(r)?;
        Ok(PixelDescriptor {
            pixel_ref: r.clone(),
            width: b.width,
            height: b.height,
            transparent: b.transparent,
            revision: b.revision.to_string(),
            disposed: b.pixels.is_none() && b.encoded_paint.is_none(),
        })
    }
    pub(crate) fn pixels(&self, r: &PixelRef) -> Result<Vec<u8>, BitmapError> {
        if let Some(resource) = &self.buffer(r)?.encoded_paint {
            let image = resource
                .decode_image()
                .map_err(|e| BitmapError(e.to_string()))?;
            let mut result = image.rgba.to_vec();
            for p in result.as_chunks_mut::<4>().0 {
                let argb = u32::from_be_bytes([p[3], p[0], p[1], p[2]]);
                *p = straight(&premul(argb, true));
            }
            return Ok(result);
        }
        let b = self.live(r)?;
        let source = b.pixels.as_ref().unwrap();
        let mut result = Vec::with_capacity(source.len());
        for pixel in source.as_chunks::<4>().0 {
            result.extend_from_slice(&straight(pixel));
        }
        Ok(result)
    }
    pub(crate) fn import_authored(
        &mut self,
        resource: &crate::ResourceUnit,
        placed: bool,
    ) -> Result<PixelRef, BitmapError> {
        let (width, height) = resource
            .image_dimensions()
            .map_err(|e| BitmapError(e.to_string()))?;
        if !placed && self.bytes + width as usize * height as usize * 4 > MAX_BITMAP_BYTES {
            if self.buffers.len() >= MAX_BUFFERS {
                return Err(error("authored images exceed the 256 buffer budget"));
            }
            return Ok(self.insert_buffer(Buffer {
                width,
                height,
                transparent: true,
                revision: 1,
                pixels: None,
                lock_depth: 0,
                locked_premul: None,
                encoded_paint: Some(resource.clone()),
            }));
        }
        self.reserve_import(width, height)?;
        let image = resource
            .decode_image()
            .map_err(|e| BitmapError(e.to_string()))?;
        self.import(&image)
    }
    pub(crate) fn import(&mut self, image: &crate::DecodedImage) -> Result<PixelRef, BitmapError> {
        let r = self.create(image.width, image.height, true, 0)?;
        let b = self.live_mut(&r)?;
        for (p, s) in b
            .pixels
            .as_mut()
            .unwrap()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(image.rgba.as_chunks::<4>().0)
        {
            let c = u32::from_be_bytes([s[3], s[0], s[1], s[2]]);
            p.copy_from_slice(&premul(c, true));
        }
        Ok(r)
    }
    pub(crate) fn get(
        &self,
        r: &PixelRef,
        x: i32,
        y: i32,
        alpha: bool,
    ) -> Result<u32, BitmapError> {
        let b = self.live(r)?;
        if x < 0 || y < 0 || x as u32 >= b.width || y as u32 >= b.height {
            return Ok(0);
        }
        let i = (y as usize * b.width as usize + x as usize) * 4;
        let p = straight(&b.pixels.as_ref().unwrap()[i..i + 4]);
        Ok(u32::from_be_bytes([
            if alpha { p[3] } else { 0 },
            p[0],
            p[1],
            p[2],
        ]))
    }
    pub(crate) fn set(
        &mut self,
        r: &PixelRef,
        x: i32,
        y: i32,
        c: u32,
        alpha: bool,
    ) -> Result<(), BitmapError> {
        let b = self.live_mut(r)?;
        if x < 0 || y < 0 || x as u32 >= b.width || y as u32 >= b.height {
            return Ok(());
        }
        let i = (y as usize * b.width as usize + x as usize) * 4;
        let p = &mut b.pixels.as_mut().unwrap()[i..i + 4];
        let c = if alpha {
            c
        } else {
            c & 0xffffff | u32::from(p[3]) << 24
        };
        p.copy_from_slice(&premul(c, b.transparent));
        b.revision += 1;
        Ok(())
    }
    pub(crate) fn fill(
        &mut self,
        r: &PixelRef,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        c: u32,
    ) -> Result<(), BitmapError> {
        let b = self.live_mut(r)?;
        let (x0, y0, x1, y1) = clip(x, y, w, h, b.width, b.height);
        let pixel = premul(c, b.transparent);
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * b.width as usize + x) * 4;
                b.pixels.as_mut().unwrap()[i..i + 4].copy_from_slice(&pixel)
            }
        }
        b.revision += 1;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn copy(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
        dx: i32,
        dy: i32,
        merge: bool,
    ) -> Result<(), BitmapError> {
        let s = self.live(src)?;
        let d = self.live(dst)?;
        // Transparent source onto an opaque destination always composites.
        let merge = merge || (s.transparent && !d.transparent);
        let x0 = 0_i64.max(-i64::from(sx)).max(-i64::from(dx));
        let y0 = 0_i64.max(-i64::from(sy)).max(-i64::from(dy));
        let x1 = i64::from(w)
            .min(i64::from(s.width) - i64::from(sx))
            .min(i64::from(d.width) - i64::from(dx));
        let y1 = i64::from(h)
            .min(i64::from(s.height) - i64::from(sy))
            .min(i64::from(d.height) - i64::from(dy));
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }
        // A bounded rectangle snapshot gives overlap-safe copies, including scroll.
        let mut samples = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            let i = ((i64::from(sy) + y) * i64::from(s.width) + i64::from(sx) + x0) as usize * 4;
            let n = (x1 - x0) as usize * 4;
            samples.extend_from_slice(&s.pixels.as_ref().unwrap()[i..i + n]);
        }
        let d = self.live_mut(dst)?;
        let mut source = samples.as_chunks::<4>().0.iter();
        for y in y0..y1 {
            for x in x0..x1 {
                let s = source.next().unwrap();
                let i = ((i64::from(dy) + y) * i64::from(d.width) + i64::from(dx) + x) as usize * 4;
                let p = &mut d.pixels.as_mut().unwrap()[i..i + 4];
                if merge {
                    let inverse = 255 - u32::from(s[3]);
                    for c in 0..4 {
                        p[c] = (u32::from(s[c]) + (u32::from(p[c]) * inverse + 127) / 255).min(255)
                            as u8;
                    }
                    if !d.transparent {
                        p[3] = 255
                    }
                } else if d.transparent {
                    p.copy_from_slice(s)
                } else {
                    let raw = straight(s);
                    p.copy_from_slice(&[raw[0], raw[1], raw[2], 255]);
                }
            }
        }
        d.revision += 1;
        Ok(())
    }
    pub(crate) fn clone_buffer(&mut self, r: &PixelRef) -> Result<PixelRef, BitmapError> {
        let b = self.buffer(r)?;
        if let Some(resource) = b.encoded_paint.clone() {
            return self.import_authored(&resource, true);
        }
        b.require_pixels()?;
        let n = b.width as usize * b.height as usize * 4;
        if self.buffers.len() >= MAX_BUFFERS || self.bytes + n > MAX_BITMAP_BYTES {
            return Err(error("buffer clone exceeds pixel budget"));
        }
        Ok(self.insert_buffer(Buffer {
            width: b.width,
            height: b.height,
            transparent: b.transparent,
            revision: 1,
            pixels: b.pixels.clone(),
            lock_depth: 0,
            locked_premul: None,
            encoded_paint: None,
        }))
    }
    pub(crate) fn scroll(&mut self, r: &PixelRef, dx: i32, dy: i32) -> Result<(), BitmapError> {
        let b = self.live(r)?;
        let (w, h) = (b.width as i32, b.height as i32);
        self.copy(r, r, 0, 0, w, h, dx, dy, false)
    }
    pub(crate) fn dispose(&mut self, r: &PixelRef) -> Result<(), BitmapError> {
        let Self { bytes, buffers, .. } = self;
        let b = buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        b.lock_depth = 0;
        if let Some(snap) = b.locked_premul.take() {
            *bytes -= snap.len();
        }
        if b.encoded_paint.take().is_some() {
            b.revision += 1;
        }
        if let Some(p) = b.pixels.take() {
            *bytes -= p.len();
            b.revision += 1;
        }
        Ok(())
    }
}
fn clip(x: i32, y: i32, w: i32, h: i32, bw: u32, bh: u32) -> (usize, usize, usize, usize) {
    let x0 = i64::from(x).clamp(0, i64::from(bw));
    let y0 = i64::from(y).clamp(0, i64::from(bh));
    let x1 = (i64::from(x) + i64::from(w.max(0))).clamp(0, i64::from(bw));
    let y1 = (i64::from(y) + i64::from(h.max(0))).clamp(0, i64::from(bh));
    (x0 as usize, y0 as usize, x1 as usize, y1 as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};

    #[test]
    fn authored_paint_keeps_pixels_and_dimensions_when_mutable_arena_is_full() {
        let mut png = Vec::new();
        let rgba = [41, 91, 127, 63, 35, 126, 89, 0];
        PngEncoder::new(&mut png)
            .write_image(&rgba, 2, 1, ColorType::Rgba8.into())
            .unwrap();
        let image =
            crate::ResourceUnit::from_bytes("sample", "assets/sample.png", "image/png", &png)
                .unwrap();
        let mut normal = PixelArena::default();
        let resident = normal.import_authored(&image, true).unwrap();

        let mut full = PixelArena {
            bytes: MAX_BITMAP_BYTES,
            ..PixelArena::default()
        };
        assert!(full.import_authored(&image, true).is_err());
        let reference = full.import_authored(&image, false).unwrap();
        let info = full.info(&reference).unwrap();
        assert_eq!((info.width, info.height, info.disposed), (2, 1, false));
        assert_eq!(
            full.pixels(&reference).unwrap(),
            normal.pixels(&resident).unwrap()
        );
        assert_eq!(full.bytes, MAX_BITMAP_BYTES);
        full.dispose(&reference).unwrap();
        assert!(full.info(&reference).unwrap().disposed);
        assert!(full.pixels(&reference).is_err());
        full.reset();
        assert!(full.info(&reference).is_err());
    }
}
