//! Session-owned premultiplied RGBA8 buffers. Public transfer is straight RGBA.
//! NeoFlash uses nearest-integer c*a/255 and c*255/a (zero RGB at alpha zero).
//! This is an explicit integer profile, not exact historical Flash rounding.
//! Copy/scroll snapshots at most one 64 MiB clipped rectangle for overlap safety;
//! this temporary memory is additional to the 128 MiB resident arena budget.
use serde::{Deserialize, Serialize};
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
        self.next += 1;
        let reference = PixelRef {
            id: self.next.to_string(),
            generation: self.generation.to_string(),
        };
        self.buffers.insert(
            reference.clone(),
            Buffer {
                width: w,
                height: h,
                transparent,
                revision: 1,
                pixels: Some(pixels),
            },
        );
        self.bytes += bytes;
        Ok(reference)
    }
    fn buffer(&self, r: &PixelRef) -> Result<&Buffer, BitmapError> {
        self.buffers
            .get(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))
    }
    fn live(&self, r: &PixelRef) -> Result<&Buffer, BitmapError> {
        let b = self.buffer(r)?;
        if b.pixels.is_none() {
            return Err(error("buffer is disposed"));
        }
        Ok(b)
    }
    fn live_mut(&mut self, r: &PixelRef) -> Result<&mut Buffer, BitmapError> {
        let b = self
            .buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        if b.pixels.is_none() {
            return Err(error("buffer is disposed"));
        }
        Ok(b)
    }
    pub(crate) fn info(&self, r: &PixelRef) -> Result<PixelDescriptor, BitmapError> {
        let b = self.buffer(r)?;
        Ok(PixelDescriptor {
            pixel_ref: r.clone(),
            width: b.width,
            height: b.height,
            transparent: b.transparent,
            revision: b.revision.to_string(),
            disposed: b.pixels.is_none(),
        })
    }
    pub(crate) fn pixels(&self, r: &PixelRef) -> Result<Vec<u8>, BitmapError> {
        let b = self.live(r)?;
        let source = b.pixels.as_ref().unwrap();
        let mut result = Vec::with_capacity(source.len());
        for pixel in source.as_chunks::<4>().0 {
            result.extend_from_slice(&straight(pixel));
        }
        Ok(result)
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
        let b = self.live(r)?;
        let (w, h, t) = (b.width, b.height, b.transparent);
        let n = w as usize * h as usize * 4;
        if self.buffers.len() >= MAX_BUFFERS || self.bytes + n > MAX_BITMAP_BYTES {
            return Err(error("buffer clone exceeds pixel budget"));
        }
        let pixels = b.pixels.clone();
        self.next += 1;
        let result = PixelRef {
            id: self.next.to_string(),
            generation: self.generation.to_string(),
        };
        self.buffers.insert(
            result.clone(),
            Buffer {
                width: w,
                height: h,
                transparent: t,
                revision: 1,
                pixels,
            },
        );
        self.bytes += n;
        Ok(result)
    }
    pub(crate) fn scroll(&mut self, r: &PixelRef, dx: i32, dy: i32) -> Result<(), BitmapError> {
        let b = self.live(r)?;
        let (w, h) = (b.width as i32, b.height as i32);
        self.copy(r, r, 0, 0, w, h, dx, dy, false)
    }
    pub(crate) fn dispose(&mut self, r: &PixelRef) -> Result<(), BitmapError> {
        let b = self
            .buffers
            .get_mut(r)
            .ok_or_else(|| error("stale or foreign buffer reference"))?;
        if let Some(p) = b.pixels.take() {
            self.bytes -= p.len();
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
