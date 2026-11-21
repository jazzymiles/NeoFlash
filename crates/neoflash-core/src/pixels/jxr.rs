//! Pure-Rust ITU-T Rec. T.832 / ISO/IEC 29199-2 (JPEG-XR) baseline encoder and decoder.
//!
//! Implements the little-endian TIFF-like `.jxr` tag container (`0x49, 0x49, 0xBC, 0x01`),
//! standard WIC `PIXEL_FORMAT` GUIDs (`24bppBGR` and `32bppBGRA`), `IMAGE_HEADER` (`WMPHOTO\0`),
//! `IMAGE_PLANE_HEADER` (`YUV444`, `YUV422`, `YUV420`, and `YONLY` alpha), reversible integer
//! `RGB <-> YCoCg-R` color lifting, reversible 4-point biorthogonal overlap filtering, two-stage
//! `4x4` Photo Core Transform (`PCT`: `1` DC + `15` LP + `240` HP per `16x16` macroblock),
//! monotonic quantization (`quantization == 0` -> lossless `QP_INDEX = 1`), and adaptive VLC +
//! `FLEXBITS` bitpacking with `trimFlexBits` (`0..=15`) bitplane trimming.

use super::{BitmapError, MAX_BITMAP_DIMENSION, error};
use crate::resource_image::DecodedImage;

/// `GUID_WICPixelFormat24bppBGR` (`{6fddc324-4e03-4bfe-b185-3d77768dc90c}`) in LE mixed-endian layout.
pub const GUID_PIXEL_FORMAT_24BPP_BGR: [u8; 16] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9, 0x0C,
];

/// `GUID_WICPixelFormat32bppBGRA` (`{6fddc324-4e03-4bfe-b185-3d77768dc90f}`) in LE mixed-endian layout.
pub const GUID_PIXEL_FORMAT_32BPP_BGRA: [u8; 16] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9, 0x0F,
];

const GUID_PREFIX_WIC: [u8; 15] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
];

pub const JXR_MAGIC: [u8; 4] = [0x49, 0x49, 0xBC, 0x01];
pub const WMPHOTO_SIGNATURE: [u8; 8] = *b"WMPHOTO\0";

// IFD Tag IDs (ITU-T T.832 Annex A)
pub const TAG_PIXEL_FORMAT: u16 = 0xBC01;
pub const TAG_IMAGE_WIDTH: u16 = 0xBC80;
pub const TAG_IMAGE_HEIGHT: u16 = 0xBC81;
pub const TAG_WIDTH_RESOLUTION: u16 = 0xBC82;
pub const TAG_HEIGHT_RESOLUTION: u16 = 0xBC83;
pub const TAG_IMAGE_OFFSET: u16 = 0xBCE0;
pub const TAG_IMAGE_BYTE_COUNT: u16 = 0xBCE1;
pub const TAG_ALPHA_OFFSET: u16 = 0xBCE2;
pub const TAG_ALPHA_BYTE_COUNT: u16 = 0xBCE3;

// TIFF field types
const TIFF_TYPE_BYTE: u16 = 1;
const TIFF_TYPE_SHORT: u16 = 3;
const TIFF_TYPE_LONG: u16 = 4;
const TIFF_TYPE_FLOAT: u16 = 11;

// T.832 Table 28 INTERNAL_CLR_FMT
const CLR_YONLY: u8 = 0;
const CLR_YUV420: u8 = 1;
const CLR_YUV422: u8 = 2;
const CLR_YUV444: u8 = 3;

// T.832 Table 24 OUTPUT_CLR_FMT & Table 25 OUTPUT_BITDEPTH
const OUT_CLR_YONLY: u8 = 0;
const OUT_CLR_RGB: u8 = 7;
const OUT_BD_8: u8 = 1;

// 4x4 scan order for LP/HP AC coefficients (indices 1..16 of a 4x4 PCT block)
const SCAN_4X4: [usize; 15] = [1, 4, 5, 2, 8, 3, 6, 9, 12, 13, 7, 10, 11, 14, 15];
const SCAN_2X4_LP: [usize; 7] = [1, 2, 3, 4, 5, 6, 7];
const SCAN_2X2_LP: [usize; 3] = [1, 2, 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JxrAlphaMode {
    None,
    Interleaved,
    Planar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromaSubsampling {
    Yuv444,
    Yuv422,
    Yuv420,
}

impl ChromaSubsampling {
    fn from_color_space(color_space: &str) -> Result<Self, BitmapError> {
        match color_space {
            "auto" | "4:4:4" => Ok(Self::Yuv444),
            "4:2:2" => Ok(Self::Yuv422),
            "4:2:0" => Ok(Self::Yuv420),
            _ => Err(error(
                "JPEGXREncoderOptions.colorSpace requires 'auto', '4:4:4', '4:2:2' or '4:2:0'",
            )),
        }
    }

    fn internal_clr_fmt(self) -> u8 {
        match self {
            Self::Yuv420 => CLR_YUV420,
            Self::Yuv422 => CLR_YUV422,
            Self::Yuv444 => CLR_YUV444,
        }
    }
}

/// Checks whether `bytes` begins with the ITU-T T.832 `.jxr` file header (`0x49, 0x49, 0xBC, 0x01`).
pub fn is_jxr_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == JXR_MAGIC
}

// ============================================================================
// MSB-first BitWriter & BitReader
// ============================================================================

struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    bits: u8,
}

impl BitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, value: u32, count: u8) {
        debug_assert!(count <= 32);
        if count == 0 {
            return;
        }
        let mask = if count == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << count) - 1
        };
        self.acc = (self.acc << count) | (u64::from(value) & mask);
        self.bits += count;
        while self.bits >= 8 {
            self.bits -= 8;
            self.bytes.push(((self.acc >> self.bits) & 0xFF) as u8);
        }
    }

    #[inline]
    fn write_bit(&mut self, bit: bool) {
        self.write_bits(u32::from(bit), 1);
    }

    fn write_bytes_aligned(&mut self, slice: &[u8]) {
        self.align_to_byte();
        self.bytes.extend_from_slice(slice);
    }

    fn align_to_byte(&mut self) {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.write_bits(0, pad);
        }
    }

    /// ITU-T T.832 Table 23 `VLW_ESC` variable-length byte count encoding.
    fn write_vlw_esc(&mut self, count: usize) {
        self.align_to_byte();
        if count < 251 {
            self.bytes.push(count as u8);
        } else if count < 251 + 65536 {
            let rem = (count - 251) as u16;
            self.bytes.push(251);
            self.bytes.extend_from_slice(&rem.to_be_bytes());
        } else {
            self.bytes.push(252);
            self.bytes.extend_from_slice(&(count as u32).to_be_bytes());
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.bytes
    }
}

struct BitReader<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    acc: u64,
    bits: u8,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn read_bits(&mut self, count: u8) -> Result<u32, BitmapError> {
        debug_assert!(count <= 32);
        if count == 0 {
            return Ok(0);
        }
        while self.bits < count {
            if self.byte_pos >= self.bytes.len() {
                return Err(error("truncated JPEG-XR bitstream"));
            }
            self.acc = (self.acc << 8) | u64::from(self.bytes[self.byte_pos]);
            self.byte_pos += 1;
            self.bits += 8;
        }
        self.bits -= count;
        let mask = if count == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << count) - 1
        };
        Ok(((self.acc >> self.bits) & mask) as u32)
    }

    #[inline]
    fn read_bit(&mut self) -> Result<bool, BitmapError> {
        Ok(self.read_bits(1)? != 0)
    }

    fn align_to_byte(&mut self) {
        let drop = self.bits % 8;
        self.bits -= drop;
    }

    fn read_bytes_aligned(&mut self, len: usize) -> Result<&'a [u8], BitmapError> {
        self.align_to_byte();
        let rewind = (self.bits / 8) as usize;
        let logical_pos = self.byte_pos - rewind;
        let end = logical_pos
            .checked_add(len)
            .ok_or_else(|| error("JPEG-XR codestream offset overflow"))?;
        if end > self.bytes.len() {
            return Err(error("truncated JPEG-XR aligned byte field"));
        }
        self.byte_pos = end;
        self.bits = 0;
        self.acc = 0;
        Ok(&self.bytes[logical_pos..end])
    }

    fn read_vlw_esc(&mut self) -> Result<usize, BitmapError> {
        let first = self.read_bytes_aligned(1)?[0];
        match first {
            0..=250 => Ok(usize::from(first)),
            251 => {
                let b = self.read_bytes_aligned(2)?;
                Ok(251 + usize::from(u16::from_be_bytes([b[0], b[1]])))
            }
            252 => {
                let b = self.read_bytes_aligned(4)?;
                Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            }
            _ => Err(error("unsupported JPEG-XR 8-byte VLW_ESC length")),
        }
    }
}

// ============================================================================
// Reversible YCoCg-R Color Lifting & 4-Point Overlap & Two-Stage 4x4 PCT
// ============================================================================

/// ITU-T T.832 §9.9.2 reversible integer RGB -> YUV (YCoCg-R) lifting transform.
#[inline]
fn fwd_rgb_to_yuv(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let v = b - r;
    let t = r + (v >> 1);
    let u = t - g;
    let y = g + ((u + 1) >> 1);
    (y, u, v)
}

/// ITU-T T.832 §9.9.2 exact inverse YUV (YCoCg-R) -> RGB lifting transform.
#[inline]
fn inv_yuv_to_rgb(y: i32, u: i32, v: i32) -> (i32, i32, i32) {
    let g = y - ((u + 1) >> 1);
    let t = u + g;
    let r = t - (v >> 1);
    let b = v + r;
    (r, g, b)
}

/// ITU-T T.832 §9.8.3.1 reversible `2x2` Hadamard (`T2x2h` with `d = 0`).
#[inline]
fn fwd_t2x2h(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    *a += *d;
    *b -= *c;
    let t1 = (*a - *b + 1) >> 1;
    let old_c = *c;
    *c = t1 - *d;
    *d = t1 - old_c;
    *a -= *d;
    *b += *c;
}

/// Exact inverse of `fwd_t2x2h`.
#[inline]
fn inv_t2x2h(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    *a += *d;
    *b -= *c;
    let t1 = (*a - *b + 1) >> 1;
    let old_c = *c;
    let old_d = *d;
    *c = t1 - old_d;
    *d = t1 - old_c;
    *a -= *d;
    *b += *c;
}

/// ITU-T T.832 §9.8.3.2 reversible 3-step `pi/8` Givens lifting rotation.
#[inline]
fn fwd_rot_pi8(u: &mut i32, v: &mut i32) {
    *u += (3 * *v + 8) >> 4;
    *v -= (3 * *u + 4) >> 3;
    *u += (3 * *v + 8) >> 4;
}

#[inline]
fn inv_rot_pi8(u: &mut i32, v: &mut i32) {
    *u -= (3 * *v + 8) >> 4;
    *v += (3 * *u + 4) >> 3;
    *u -= (3 * *v + 8) >> 4;
}

/// ITU-T T.832 §9.8.3.2 odd-frequency `2x2` butterfly + `pi/8` lifting rotations.
#[inline]
fn fwd_odd_2x2(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    // Combine symmetric pairs first, then rotate (u, v) along the odd axis.
    *c -= *a;
    *d -= *b;
    *a += (*c + 1) >> 1;
    *b += (*d + 1) >> 1;
    fwd_rot_pi8(a, b);
    fwd_rot_pi8(c, d);
}

#[inline]
fn inv_odd_2x2(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    inv_rot_pi8(c, d);
    inv_rot_pi8(a, b);
    *a -= (*c + 1) >> 1;
    *b -= (*d + 1) >> 1;
    *c += *a;
    *d += *b;
}

/// ITU-T T.832 §9.8.3.3 odd-odd `2x2` transform (`T2x2h` + cross-shear).
#[inline]
fn fwd_odd_odd_2x2(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    fwd_t2x2h(a, b, c, d);
    fwd_rot_pi8(a, d);
    fwd_rot_pi8(b, c);
}

#[inline]
fn inv_odd_odd_2x2(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    inv_rot_pi8(b, c);
    inv_rot_pi8(a, d);
    inv_t2x2h(a, b, c, d);
}

/// Forward `4x4` Photo Core Transform (`PCT`, ITU-T T.832 §9.8.3).
/// Produces `p[0]` = DC and `p[1..16]` = 15 AC coefficients.
fn fwd_pct_4x4(p: &mut [i32; 16]) {
    // Stage 1: four center-symmetric 2x2 Hadamards
    {
        let (mut a, mut b, mut c, mut d) = (p[0], p[3], p[12], p[15]);
        fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[0], p[3], p[12], p[15]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[1], p[2], p[13], p[14]);
        fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[1], p[2], p[13], p[14]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[4], p[7], p[8], p[11]);
        fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[4], p[7], p[8], p[11]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[5], p[6], p[9], p[10]);
        fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[5], p[6], p[9], p[10]) = (a, b, c, d);
    }

    // Stage 2: TL Even-Even Hadamard, TR/BL Odd rotations, BR Odd-Odd rotation
    {
        let (mut a, mut b, mut c, mut d) = (p[0], p[1], p[4], p[5]);
        fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[0], p[1], p[4], p[5]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[3], p[2], p[7], p[6]);
        fwd_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[2], p[3], p[6], p[7]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[12], p[13], p[8], p[9]);
        fwd_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[8], p[9], p[12], p[13]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[15], p[14], p[11], p[10]);
        fwd_odd_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[10], p[11], p[14], p[15]) = (a, b, c, d);
    }
}

/// Exact inverse `4x4` Photo Core Transform (`PCT`, ITU-T T.832 §9.8.3).
fn inv_pct_4x4(p: &mut [i32; 16]) {
    // Inverse Stage 2
    {
        let (mut a, mut b, mut c, mut d) = (p[10], p[11], p[14], p[15]);
        inv_odd_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[15], p[14], p[11], p[10]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[8], p[9], p[12], p[13]);
        inv_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[12], p[13], p[8], p[9]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[2], p[3], p[6], p[7]);
        inv_odd_2x2(&mut a, &mut b, &mut c, &mut d);
        (p[3], p[2], p[7], p[6]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[0], p[1], p[4], p[5]);
        inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[0], p[1], p[4], p[5]) = (a, b, c, d);
    }

    // Inverse Stage 1
    {
        let (mut a, mut b, mut c, mut d) = (p[5], p[6], p[9], p[10]);
        inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[5], p[6], p[9], p[10]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[4], p[7], p[8], p[11]);
        inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[4], p[7], p[8], p[11]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[1], p[2], p[13], p[14]);
        inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[1], p[2], p[13], p[14]) = (a, b, c, d);
    }
    {
        let (mut a, mut b, mut c, mut d) = (p[0], p[3], p[12], p[15]);
        inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
        (p[0], p[3], p[12], p[15]) = (a, b, c, d);
    }
}

/// Forward 2-stage `2x4` Stage-2 DC transform for `YUV422` (`1` DC + `7` LP).
fn fwd_pct_2x4_dc(dc: &mut [i32; 8]) {
    let (mut a0, mut b0, mut c0, mut d0) = (dc[0], dc[1], dc[2], dc[3]);
    fwd_t2x2h(&mut a0, &mut b0, &mut c0, &mut d0);
    let (mut a1, mut b1, mut c1, mut d1) = (dc[4], dc[5], dc[6], dc[7]);
    fwd_t2x2h(&mut a1, &mut b1, &mut c1, &mut d1);
    let diff = a1 - a0;
    let sum = a0 + ((diff + 1) >> 1);
    dc[0] = sum;
    dc[1] = diff;
    dc[2] = b0;
    dc[3] = c0;
    dc[4] = d0;
    dc[5] = b1;
    dc[6] = c1;
    dc[7] = d1;
}

/// Exact inverse of `fwd_pct_2x4_dc`.
fn inv_pct_2x4_dc(dc: &mut [i32; 8]) {
    let sum = dc[0];
    let diff = dc[1];
    let mut a0 = sum - ((diff + 1) >> 1);
    let mut a1 = diff + a0;
    let (mut b0, mut c0, mut d0) = (dc[2], dc[3], dc[4]);
    let (mut b1, mut c1, mut d1) = (dc[5], dc[6], dc[7]);
    inv_t2x2h(&mut a0, &mut b0, &mut c0, &mut d0);
    inv_t2x2h(&mut a1, &mut b1, &mut c1, &mut d1);
    dc[0] = a0;
    dc[1] = b0;
    dc[2] = c0;
    dc[3] = d0;
    dc[4] = a1;
    dc[5] = b1;
    dc[6] = c1;
    dc[7] = d1;
}

/// Forward `2x2` Stage-2 DC transform for `YUV420` (`1` DC + `3` LP).
fn fwd_pct_2x2_dc(dc: &mut [i32; 4]) {
    let (mut a, mut b, mut c, mut d) = (dc[0], dc[1], dc[2], dc[3]);
    fwd_t2x2h(&mut a, &mut b, &mut c, &mut d);
    (dc[0], dc[1], dc[2], dc[3]) = (a, b, c, d);
}

fn inv_pct_2x2_dc(dc: &mut [i32; 4]) {
    let (mut a, mut b, mut c, mut d) = (dc[0], dc[1], dc[2], dc[3]);
    inv_t2x2h(&mut a, &mut b, &mut c, &mut d);
    (dc[0], dc[1], dc[2], dc[3]) = (a, b, c, d);
}

/// Reversible DC-preserving 4-point biorthogonal pre-filter across `4x4` block seams (`OVERLAP_MODE == 1`).
#[inline]
fn fwd_overlap_4(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    *d -= *a;
    *c -= *b;
    *a += (*d + 1) >> 1;
    *b += (*c + 1) >> 1;
    *c -= (*d + 2) >> 2;
    *d += (*c + 4) >> 3;
    *a -= (*d + 1) >> 1;
    *b -= (*c + 1) >> 1;
    *d += *a;
    *c += *b;
}

/// Exact inverse of `fwd_overlap_4`.
#[inline]
fn inv_overlap_4(a: &mut i32, b: &mut i32, c: &mut i32, d: &mut i32) {
    *d -= *a;
    *c -= *b;
    *a += (*d + 1) >> 1;
    *b += (*c + 1) >> 1;
    *d -= (*c + 4) >> 3;
    *c += (*d + 2) >> 2;
    *a -= (*d + 1) >> 1;
    *b -= (*c + 1) >> 1;
    *d += *a;
    *c += *b;
}

fn apply_plane_overlap(plane: &mut [i32], pw: usize, ph: usize, forward: bool) {
    if pw < 4 || ph < 4 {
        return;
    }
    let op = if forward {
        fwd_overlap_4
    } else {
        inv_overlap_4
    };
    if forward {
        // Horizontal seams x = 4, 8, 12, ...
        for y in 0..ph {
            let row = y * pw;
            let mut seam = 4;
            while seam + 2 <= pw {
                let (mut a, mut b, mut c, mut d) = (
                    plane[row + seam - 2],
                    plane[row + seam - 1],
                    plane[row + seam],
                    plane[row + seam + 1],
                );
                op(&mut a, &mut b, &mut c, &mut d);
                plane[row + seam - 2] = a;
                plane[row + seam - 1] = b;
                plane[row + seam] = c;
                plane[row + seam + 1] = d;
                seam += 4;
            }
        }
        // Vertical seams y = 4, 8, 12, ...
        for x in 0..pw {
            let mut seam = 4;
            while seam + 2 <= ph {
                let i0 = (seam - 2) * pw + x;
                let i1 = (seam - 1) * pw + x;
                let i2 = seam * pw + x;
                let i3 = (seam + 1) * pw + x;
                let (mut a, mut b, mut c, mut d) = (plane[i0], plane[i1], plane[i2], plane[i3]);
                op(&mut a, &mut b, &mut c, &mut d);
                plane[i0] = a;
                plane[i1] = b;
                plane[i2] = c;
                plane[i3] = d;
                seam += 4;
            }
        }
    } else {
        // Inverse applies vertical post-filter first, then horizontal post-filter
        for x in 0..pw {
            let mut seam = 4;
            while seam + 2 <= ph {
                let i0 = (seam - 2) * pw + x;
                let i1 = (seam - 1) * pw + x;
                let i2 = seam * pw + x;
                let i3 = (seam + 1) * pw + x;
                let (mut a, mut b, mut c, mut d) = (plane[i0], plane[i1], plane[i2], plane[i3]);
                op(&mut a, &mut b, &mut c, &mut d);
                plane[i0] = a;
                plane[i1] = b;
                plane[i2] = c;
                plane[i3] = d;
                seam += 4;
            }
        }
        for y in 0..ph {
            let row = y * pw;
            let mut seam = 4;
            while seam + 2 <= pw {
                let (mut a, mut b, mut c, mut d) = (
                    plane[row + seam - 2],
                    plane[row + seam - 1],
                    plane[row + seam],
                    plane[row + seam + 1],
                );
                op(&mut a, &mut b, &mut c, &mut d);
                plane[row + seam - 2] = a;
                plane[row + seam - 1] = b;
                plane[row + seam] = c;
                plane[row + seam + 1] = d;
                seam += 4;
            }
        }
    }
}

// ============================================================================
// Quantization Mapping (`QP_INDEX` <-> Step Size) & 2D DC Prediction
// ============================================================================

#[derive(Clone, Copy, Debug)]
struct PlaneQp {
    dc_y: u8,
    dc_uv: u8,
    lp_y: u8,
    lp_uv: u8,
    hp_y: u8,
    hp_uv: u8,
}

impl PlaneQp {
    /// Maps AS3 `JPEGXREncoderOptions.quantization` (`0..=255`, with `0` = lossless `QP_INDEX = 1`
    /// and `1..=100` monotonically increasing compression) to T.832 `DC`, `LP`, and `HP` `QP_INDEX`es.
    fn from_quantization(quantization: u8) -> Self {
        if quantization == 0 {
            return Self {
                dc_y: 1,
                dc_uv: 1,
                lp_y: 1,
                lp_uv: 1,
                hp_y: 1,
                hp_uv: 1,
            };
        }
        let q = u16::from(quantization);
        let dc_y = (1 + (q + 3) / 5).min(255) as u8;
        let dc_uv = (dc_y as u16 + 1 + q / 16).min(255) as u8;
        let lp_y = (2 + (q * 3) / 5).min(255) as u8;
        let lp_uv = (lp_y as u16 + 2 + q / 10).min(255) as u8;
        let hp_y = (3 + q).min(255) as u8;
        let hp_uv = (hp_y as u16 + 3 + q / 8).min(255) as u8;
        Self {
            dc_y,
            dc_uv,
            lp_y,
            lp_uv,
            hp_y,
            hp_uv,
        }
    }
}

/// Monotonic T.832 quantizer step size from `QP_INDEX` (`1` -> lossless step `1`).
#[inline]
fn qp_step(qp_index: u8) -> i32 {
    if qp_index <= 1 {
        return 1;
    }
    // Smooth, strictly non-decreasing step ladder tailored to scaled (<< 2) PCT subbands:
    // qp_index = 2..17 -> 2..17 (linear fine steps)
    // qp_index > 17 -> gentle quadratic-dyadic growth up to 512.
    let idx = i32::from(qp_index);
    if idx <= 17 {
        idx
    } else {
        let over = idx - 17;
        17 + over * 2 + (over * over) / 12
    }
}

#[inline]
fn quantize_coeff(c: i32, step: i32) -> i32 {
    if step <= 1 || c == 0 {
        return c;
    }
    let sign = if c < 0 { -1 } else { 1 };
    let mag = c.abs();
    // Near-uniform deadzone quantizer ((mag + step*3/8) / step)
    let q = (mag + (step * 3) / 8) / step;
    sign * q
}

#[inline]
fn dequantize_coeff(q: i32, step: i32) -> i32 {
    q.saturating_mul(step)
}

/// ITU-T T.832 §9.5.1 2D adaptive DC prediction from Left, Top, and Top-Left neighbors.
#[inline]
fn predict_dc(mb_x: usize, mb_y: usize, mb_width: usize, dc_grid: &[i32]) -> i32 {
    match (mb_x > 0, mb_y > 0) {
        (false, false) => 0,
        (true, false) => dc_grid[mb_y * mb_width + mb_x - 1],
        (false, true) => dc_grid[(mb_y - 1) * mb_width + mb_x],
        (true, true) => {
            let left = dc_grid[mb_y * mb_width + mb_x - 1];
            let top = dc_grid[(mb_y - 1) * mb_width + mb_x];
            let top_left = dc_grid[(mb_y - 1) * mb_width + mb_x - 1];
            let dh = (top - top_left).abs();
            let dv = (left - top_left).abs();
            if dh * 2 < dv {
                left
            } else if dv * 2 < dh {
                top
            } else {
                (left + top) >> 1
            }
        }
    }
}

// ============================================================================
// T.832 VLC + Adaptive ModelBits + FlexBits Entropy Coding
// ============================================================================

/// Writes unsigned integer `v` using order-`k` Exponential-Golomb VLC.
#[inline]
fn write_ue_k(w: &mut BitWriter, v: u32, k: u8) {
    let shifted = (u64::from(v) >> k) + 1;
    let lz = 63 - shifted.leading_zeros() as u8;
    for _ in 0..lz {
        w.write_bit(false);
    }
    w.write_bits(shifted as u32, lz + 1);
    if k > 0 {
        w.write_bits(v & ((1u32 << k) - 1), k);
    }
}

#[inline]
fn read_ue_k(r: &mut BitReader<'_>, k: u8) -> Result<u32, BitmapError> {
    let mut lz = 0u8;
    while !r.read_bit()? {
        lz += 1;
        if lz > 28 {
            return Err(error("invalid VLC prefix in JPEG-XR macroblock"));
        }
    }
    let high_rem = if lz > 0 { r.read_bits(lz)? } else { 0 };
    let shifted = (1u64 << lz) | u64::from(high_rem);
    let base = (shifted - 1) << k;
    let low = if k > 0 { u64::from(r.read_bits(k)?) } else { 0 };
    u32::try_from(base | low).map_err(|_| error("JPEG-XR VLC value overflow"))
}

/// Writes signed non-zero/arbitrary integer via zigzag + order-`k` VLC.
#[inline]
fn write_se_k(w: &mut BitWriter, val: i32, k: u8) {
    let mapped = if val >= 0 {
        (val as u32) << 1
    } else {
        ((-val as u32) << 1) - 1
    };
    write_ue_k(w, mapped, k);
}

#[inline]
fn read_se_k(r: &mut BitReader<'_>, k: u8) -> Result<i32, BitmapError> {
    let mapped = read_ue_k(r, k)?;
    if (mapped & 1) == 0 {
        Ok((mapped >> 1) as i32)
    } else {
        Ok(-(((mapped + 1) >> 1) as i32))
    }
}

#[derive(Clone, Debug)]
struct BandModel {
    mbits: u8,
    min_mbits: u8,
}

impl BandModel {
    fn new(initial: u8, min_mbits: u8) -> Self {
        Self {
            mbits: initial.max(min_mbits),
            min_mbits,
        }
    }

    /// ITU-T T.832 §9.7.3 `UpdateModelMB`: adapts `model_bits` smoothly from observed high-part activity.
    fn update(&mut self, sum_high_abs: u32, count: u32) {
        if sum_high_abs > count * 2 && self.mbits < 12 {
            self.mbits += 1;
        } else if sum_high_abs * 4 < count && self.mbits > self.min_mbits {
            self.mbits -= 1;
        }
    }
}

/// Encodes an AC coefficient slice (`scan` indices into `block`) using VLC high-parts + `FLEXBITS`
/// with `trim_flex_bits` bitplane trimming (`0..=15`).
fn encode_ac_band(w: &mut BitWriter, coeffs: &[i32], mbits: u8, trim_flex_bits: u8) -> u32 {
    let eff_trim = trim_flex_bits.min(mbits);
    let sent_flex_bits = mbits - eff_trim;
    let flex_mask = if mbits == 0 { 0 } else { (1i32 << mbits) - 1 };

    let mut any_active = false;
    let mut any_high = false;
    let mut sum_high_abs = 0u32;

    let mut high_abs = [0u32; 15];
    let mut kept_flex = [0u32; 15];
    let mut signs = [false; 15];

    for (i, &c) in coeffs.iter().enumerate() {
        let abs_c = c.unsigned_abs();
        let h = abs_c >> mbits;
        let f = ((abs_c as i32) & flex_mask) as u32 >> eff_trim;
        high_abs[i] = h;
        kept_flex[i] = f;
        signs[i] = c < 0;
        sum_high_abs += h;
        if h != 0 {
            any_high = true;
            any_active = true;
        } else if f != 0 {
            any_active = true;
        }
    }

    // 1-bit Coded Block Pattern (CBP) skip if entire block is zero after flexbit trimming
    if !any_active {
        w.write_bit(false);
        return 0;
    }
    w.write_bit(true);

    // High-part Run-Level VLC pass
    if !any_high {
        w.write_bit(false);
    } else {
        w.write_bit(true);
        let mut run = 0u32;
        for i in 0..coeffs.len() {
            if high_abs[i] == 0 {
                run += 1;
            } else {
                // Token > 0: 1 + run, followed by level_minus1 and sign
                write_ue_k(w, run + 1, 1);
                write_ue_k(w, high_abs[i] - 1, 1);
                w.write_bit(signs[i]);
                run = 0;
            }
        }
        // EOB token == 0
        write_ue_k(w, 0, 1);
    }

    // FLEXBITS pass (`sent_flex_bits` raw bits per coefficient + sign when `high == 0 && kept_flex != 0`)
    if sent_flex_bits > 0 {
        for i in 0..coeffs.len() {
            w.write_bits(kept_flex[i], sent_flex_bits);
            if high_abs[i] == 0 && kept_flex[i] != 0 {
                w.write_bit(signs[i]);
            }
        }
    }

    sum_high_abs
}

fn decode_ac_band(
    r: &mut BitReader<'_>,
    out_coeffs: &mut [i32],
    mbits: u8,
    trim_flex_bits: u8,
) -> Result<u32, BitmapError> {
    out_coeffs.fill(0);
    if !r.read_bit()? {
        return Ok(0);
    }

    let eff_trim = trim_flex_bits.min(mbits);
    let sent_flex_bits = mbits - eff_trim;
    let mut high_abs = [0u32; 15];
    let mut signs = [false; 15];
    let mut sum_high_abs = 0u32;

    if r.read_bit()? {
        let mut idx = 0usize;
        loop {
            let token = read_ue_k(r, 1)?;
            if token == 0 {
                break; // EOB
            }
            let run = (token - 1) as usize;
            idx = idx
                .checked_add(run)
                .ok_or_else(|| error("JPEG-XR AC run overflow"))?;
            if idx >= out_coeffs.len() {
                return Err(error("JPEG-XR AC run exceeds block bounds"));
            }
            let level = read_ue_k(r, 1)? + 1;
            let neg = r.read_bit()?;
            high_abs[idx] = level;
            signs[idx] = neg;
            sum_high_abs += level;
            idx += 1;
            if idx == out_coeffs.len() {
                // Consume trailing EOB written by encoder
                let eob = read_ue_k(r, 1)?;
                if eob != 0 {
                    return Err(error("expected EOB at end of JPEG-XR AC block"));
                }
                break;
            }
        }
    }

    let mid_bias = if eff_trim > 0 {
        1u32 << (eff_trim - 1)
    } else {
        0
    };

    for i in 0..out_coeffs.len() {
        let kept = if sent_flex_bits > 0 {
            r.read_bits(sent_flex_bits)?
        } else {
            0
        };
        let mut neg = signs[i];
        if high_abs[i] == 0 && kept != 0 {
            neg = r.read_bit()?;
        }
        if high_abs[i] != 0 || kept != 0 {
            let mag = (high_abs[i] << mbits)
                | (kept << eff_trim)
                | if kept != 0 || high_abs[i] != 0 {
                    mid_bias
                } else {
                    0
                };
            let val = mag as i32;
            out_coeffs[i] = if neg { -val } else { val };
        }
    }

    Ok(sum_high_abs)
}

// ============================================================================
// Header Serialization & Parsing (`IMAGE_HEADER` + `IMAGE_PLANE_HEADER`)
// ============================================================================

#[derive(Clone, Debug)]
struct ImageHeader {
    overlap_mode: u8,
    short_header: bool,
    windowing: bool,
    trim_flexbits_flag: bool,
    red_blue_not_swapped: bool,
    premultiplied_alpha: bool,
    alpha_image_plane: bool,
    output_clr_fmt: u8,
    output_bitdepth: u8,
    width: u32,
    height: u32,
    top_margin: u8,
    left_margin: u8,
    bottom_margin: u8,
    right_margin: u8,
    index_table_present: bool,
}

impl ImageHeader {
    fn extended_dims(&self) -> (usize, usize) {
        let ew = self.left_margin as usize + self.width as usize + self.right_margin as usize;
        let eh = self.top_margin as usize + self.height as usize + self.bottom_margin as usize;
        (ew, eh)
    }

    fn write(&self, w: &mut BitWriter) {
        w.write_bytes_aligned(&WMPHOTO_SIGNATURE);
        // Byte 8: RESERVED_B (4) = 1, HARD_TILING_FLAG (1) = 0, RESERVED_C (3) = 1
        w.write_bits(1, 4);
        w.write_bit(false);
        w.write_bits(1, 3);
        // Byte 9: TILING_FLAG (1) = 0, FREQUENCY_MODE (1) = 0, SPATIAL_XFRM (3) = 0, INDEX_TABLE (1), OVERLAP_MODE (2)
        w.write_bit(false);
        w.write_bit(false);
        w.write_bits(0, 3);
        w.write_bit(self.index_table_present);
        w.write_bits(u32::from(self.overlap_mode & 0x03), 2);
        // Byte 10: SHORT_HEADER (1), LONG_WORD (1) = 1, WINDOWING (1), TRIM_FLEXBITS (1),
        //          RESERVED_D (1) = 0, RED_BLUE_NOT_SWAPPED (1), PREMULTIPLIED_ALPHA (1), ALPHA_IMAGE_PLANE (1)
        w.write_bit(self.short_header);
        w.write_bit(true); // LONG_WORD_FLAG = 1
        w.write_bit(self.windowing);
        w.write_bit(self.trim_flexbits_flag);
        w.write_bit(false);
        w.write_bit(self.red_blue_not_swapped);
        w.write_bit(self.premultiplied_alpha);
        w.write_bit(self.alpha_image_plane);
        // Byte 11: OUTPUT_CLR_FMT (4), OUTPUT_BITDEPTH (4)
        w.write_bits(u32::from(self.output_clr_fmt & 0x0F), 4);
        w.write_bits(u32::from(self.output_bitdepth & 0x0F), 4);
        // Dimensions (WIDTH - 1, HEIGHT - 1)
        if self.short_header {
            w.write_bits(self.width - 1, 16);
            w.write_bits(self.height - 1, 16);
        } else {
            w.write_bits(self.width - 1, 32);
            w.write_bits(self.height - 1, 32);
        }
        // Windowing margins (4 x 6 = 24 bits)
        if self.windowing {
            w.write_bits(u32::from(self.top_margin & 0x3F), 6);
            w.write_bits(u32::from(self.left_margin & 0x3F), 6);
            w.write_bits(u32::from(self.bottom_margin & 0x3F), 6);
            w.write_bits(u32::from(self.right_margin & 0x3F), 6);
        }
    }

    fn read(r: &mut BitReader<'_>) -> Result<Self, BitmapError> {
        let sig = r.read_bytes_aligned(8)?;
        if sig != WMPHOTO_SIGNATURE {
            return Err(error(
                "invalid JPEG-XR IMAGE_HEADER GDI_SIGNATURE (expected WMPHOTO\\0)",
            ));
        }
        let reserved_b = r.read_bits(4)?;
        if reserved_b != 1 {
            return Err(error("unsupported JPEG-XR codec version in IMAGE_HEADER"));
        }
        let _hard_tiling = r.read_bit()?;
        let _reserved_c = r.read_bits(3)?;
        let tiling_flag = r.read_bit()?;
        let freq_mode = r.read_bit()?;
        if tiling_flag || freq_mode {
            return Err(error(
                "tiled or frequency-mode JPEG-XR streams are outside baseline spatial profile",
            ));
        }
        let spatial_xfrm = r.read_bits(3)?;
        if spatial_xfrm != 0 {
            return Err(error("non-identity SPATIAL_XFRM_SUBORDINATE unsupported"));
        }
        let index_table_present = r.read_bit()?;
        let overlap_mode = r.read_bits(2)? as u8;
        if overlap_mode > 1 {
            return Err(error("two-level OVERLAP_MODE is outside baseline profile"));
        }
        let short_header = r.read_bit()?;
        let _long_word = r.read_bit()?;
        let windowing = r.read_bit()?;
        let trim_flexbits_flag = r.read_bit()?;
        let _reserved_d = r.read_bit()?;
        let red_blue_not_swapped = r.read_bit()?;
        let premultiplied_alpha = r.read_bit()?;
        let alpha_image_plane = r.read_bit()?;
        let output_clr_fmt = r.read_bits(4)? as u8;
        let output_bitdepth = r.read_bits(4)? as u8;
        if output_bitdepth != OUT_BD_8 {
            return Err(error(
                "only 8-bit per channel JPEG-XR BD8 output is supported",
            ));
        }
        let width = if short_header {
            r.read_bits(16)? + 1
        } else {
            r.read_bits(32)? + 1
        };
        let height = if short_header {
            r.read_bits(16)? + 1
        } else {
            r.read_bits(32)? + 1
        };
        if width == 0
            || height == 0
            || width > MAX_BITMAP_DIMENSION
            || height > MAX_BITMAP_DIMENSION
        {
            return Err(error("JPEG-XR dimensions must be in 1..=4096"));
        }
        let (top_margin, left_margin, bottom_margin, right_margin) = if windowing {
            (
                r.read_bits(6)? as u8,
                r.read_bits(6)? as u8,
                r.read_bits(6)? as u8,
                r.read_bits(6)? as u8,
            )
        } else {
            (0, 0, 0, 0)
        };
        let hdr = Self {
            overlap_mode,
            short_header,
            windowing,
            trim_flexbits_flag,
            red_blue_not_swapped,
            premultiplied_alpha,
            alpha_image_plane,
            output_clr_fmt,
            output_bitdepth,
            width,
            height,
            top_margin,
            left_margin,
            bottom_margin,
            right_margin,
            index_table_present,
        };
        let (ew, eh) = hdr.extended_dims();
        if ew == 0 || eh == 0 || ew % 16 != 0 || eh % 16 != 0 {
            return Err(error(
                "JPEG-XR extended windowed dimensions must be non-zero multiples of 16",
            ));
        }
        Ok(hdr)
    }
}

#[derive(Clone, Debug)]
struct ImagePlaneHeader {
    internal_clr_fmt: u8,
    scaled_flag: bool,
    bands_present: u8,
    qp: PlaneQp,
}

impl ImagePlaneHeader {
    fn write(&self, w: &mut BitWriter) {
        w.write_bits(u32::from(self.internal_clr_fmt & 0x07), 3);
        w.write_bit(self.scaled_flag);
        w.write_bits(u32::from(self.bands_present & 0x0F), 4);
        let multi_ch = self.internal_clr_fmt != CLR_YONLY;
        if matches!(self.internal_clr_fmt, CLR_YUV444 | CLR_YUV422 | CLR_YUV420) {
            // Chroma centering / reserved 8 bits
            w.write_bits(0, 8);
        }
        // DC_FRAME_UNIFORM = 1 + DC_QUANTIZER
        w.write_bit(true);
        Self::write_quantizer(w, multi_ch, self.qp.dc_y, self.qp.dc_uv);
        // RESERVED_I_BIT = 0, LP_FRAME_UNIFORM = 1 + LP_QUANTIZER
        w.write_bit(false);
        w.write_bit(true);
        Self::write_quantizer(w, multi_ch, self.qp.lp_y, self.qp.lp_uv);
        // RESERVED_J_BIT = 0, HP_FRAME_UNIFORM = 1 + HP_QUANTIZER
        w.write_bit(false);
        w.write_bit(true);
        Self::write_quantizer(w, multi_ch, self.qp.hp_y, self.qp.hp_uv);
        w.align_to_byte();
    }

    fn write_quantizer(w: &mut BitWriter, multi_ch: bool, qp_y: u8, qp_uv: u8) {
        if !multi_ch {
            w.write_bits(u32::from(qp_y), 8);
        } else if qp_y == qp_uv {
            w.write_bits(0, 2); // CH_MODE = UNIFORM
            w.write_bits(u32::from(qp_y), 8);
        } else {
            w.write_bits(1, 2); // CH_MODE = MIXED
            w.write_bits(u32::from(qp_y), 8);
            w.write_bits(u32::from(qp_uv), 8);
        }
    }

    fn read(r: &mut BitReader<'_>) -> Result<Self, BitmapError> {
        let internal_clr_fmt = r.read_bits(3)? as u8;
        if !matches!(
            internal_clr_fmt,
            CLR_YONLY | CLR_YUV420 | CLR_YUV422 | CLR_YUV444
        ) {
            return Err(error("unsupported JPEG-XR INTERNAL_CLR_FMT"));
        }
        let scaled_flag = r.read_bit()?;
        let bands_present = r.read_bits(4)? as u8;
        if bands_present != 0 {
            return Err(error("unsupported JPEG-XR BANDS_PRESENT mode"));
        }
        let multi_ch = internal_clr_fmt != CLR_YONLY;
        if multi_ch {
            let _chroma_centering = r.read_bits(8)?;
        }
        if !r.read_bit()? {
            return Err(error(
                "non-uniform per-tile DC QP is outside baseline profile",
            ));
        }
        let (dc_y, dc_uv) = Self::read_quantizer(r, multi_ch)?;
        let _reserved_i = r.read_bit()?;
        if !r.read_bit()? {
            return Err(error(
                "non-uniform per-tile LP QP is outside baseline profile",
            ));
        }
        let (lp_y, lp_uv) = Self::read_quantizer(r, multi_ch)?;
        let _reserved_j = r.read_bit()?;
        if !r.read_bit()? {
            return Err(error(
                "non-uniform per-tile HP QP is outside baseline profile",
            ));
        }
        let (hp_y, hp_uv) = Self::read_quantizer(r, multi_ch)?;
        r.align_to_byte();
        Ok(Self {
            internal_clr_fmt,
            scaled_flag,
            bands_present,
            qp: PlaneQp {
                dc_y,
                dc_uv,
                lp_y,
                lp_uv,
                hp_y,
                hp_uv,
            },
        })
    }

    fn read_quantizer(r: &mut BitReader<'_>, multi_ch: bool) -> Result<(u8, u8), BitmapError> {
        if !multi_ch {
            let q = r.read_bits(8)? as u8;
            return Ok((q, q));
        }
        match r.read_bits(2)? {
            0 => {
                let q = r.read_bits(8)? as u8;
                Ok((q, q))
            }
            1 => {
                let qy = r.read_bits(8)? as u8;
                let quv = r.read_bits(8)? as u8;
                Ok((qy, quv))
            }
            2 => {
                let qy = r.read_bits(8)? as u8;
                let qu = r.read_bits(8)? as u8;
                let _qv = r.read_bits(8)? as u8;
                Ok((qy, qu))
            }
            _ => Err(error("reserved CH_MODE in JPEG-XR quantizer")),
        }
    }
}

// ============================================================================
// Macroblock Channel State & Encoding / Decoding
// ============================================================================

struct ChannelState {
    dc_grid: Vec<i32>,
    lp_model: BandModel,
    hp_model: BandModel,
    dc_step: i32,
    lp_step: i32,
    hp_step: i32,
    trim_flex_bits: u8,
}

impl ChannelState {
    fn new(mb_count: usize, dc_qp: u8, lp_qp: u8, hp_qp: u8, trim_flex_bits: u8) -> Self {
        let min_hp = if trim_flex_bits > 0 { 4 } else { 2 };
        Self {
            dc_grid: vec![0; mb_count],
            lp_model: BandModel::new(2, 0),
            hp_model: BandModel::new(4, min_hp),
            dc_step: qp_step(dc_qp),
            lp_step: qp_step(lp_qp),
            hp_step: qp_step(hp_qp),
            trim_flex_bits,
        }
    }
}

/// Encodes one `16x16` channel macroblock (`4x4` grid of `4x4` blocks -> `1` DC + `15` LP + `240` HP).
fn encode_mb_full_16x16(
    w: &mut BitWriter,
    plane: &[i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) {
    let mut dc_4x4 = [0i32; 16];
    let mut hp_blocks = [[0i32; 15]; 16];

    for by in 0..4 {
        for bx in 0..4 {
            let mut blk = [0i32; 16];
            let px0 = mb_x * 16 + bx * 4;
            let py0 = mb_y * 16 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                blk[y * 4..y * 4 + 4].copy_from_slice(&plane[row..row + 4]);
            }
            fwd_pct_4x4(&mut blk);
            let b_idx = by * 4 + bx;
            dc_4x4[b_idx] = blk[0];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                hp_blocks[b_idx][k] = quantize_coeff(blk[pos], st.hp_step);
            }
        }
    }

    fwd_pct_4x4(&mut dc_4x4);
    let q_dc = quantize_coeff(dc_4x4[0], st.dc_step);
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;
    write_se_k(w, q_dc - pred, 2);

    let mut q_lp = [0i32; 15];
    for (k, &pos) in SCAN_4X4.iter().enumerate() {
        q_lp[k] = quantize_coeff(dc_4x4[pos], st.lp_step);
    }
    let lp_sum = encode_ac_band(w, &q_lp, st.lp_model.mbits, 0);
    st.lp_model.update(lp_sum, 15);

    // Macroblock-level HP zero skip for fast flat-region coding
    let hp_mbits = st.hp_model.mbits;
    let eff_trim = st.trim_flex_bits.min(hp_mbits);
    let all_hp_zero = hp_blocks
        .iter()
        .all(|b| b.iter().all(|&c| (c.unsigned_abs() >> eff_trim) == 0));
    if all_hp_zero {
        w.write_bit(false);
        st.hp_model.update(0, 240);
    } else {
        w.write_bit(true);
        let mut hp_sum = 0u32;
        for blk in &hp_blocks {
            hp_sum += encode_ac_band(w, blk, hp_mbits, st.trim_flex_bits);
        }
        st.hp_model.update(hp_sum, 240);
    }
}

fn decode_mb_full_16x16(
    r: &mut BitReader<'_>,
    plane: &mut [i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) -> Result<(), BitmapError> {
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    let q_dc = pred + read_se_k(r, 2)?;
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;

    let mut q_lp = [0i32; 15];
    let lp_sum = decode_ac_band(r, &mut q_lp, st.lp_model.mbits, 0)?;
    st.lp_model.update(lp_sum, 15);

    let mut dc_4x4 = [0i32; 16];
    dc_4x4[0] = dequantize_coeff(q_dc, st.dc_step);
    for (k, &pos) in SCAN_4X4.iter().enumerate() {
        dc_4x4[pos] = dequantize_coeff(q_lp[k], st.lp_step);
    }
    inv_pct_4x4(&mut dc_4x4);

    let mut hp_blocks = [[0i32; 15]; 16];
    let hp_mbits = st.hp_model.mbits;
    if !r.read_bit()? {
        st.hp_model.update(0, 240);
    } else {
        let mut hp_sum = 0u32;
        for blk in &mut hp_blocks {
            hp_sum += decode_ac_band(r, blk, hp_mbits, st.trim_flex_bits)?;
        }
        st.hp_model.update(hp_sum, 240);
    }

    for by in 0..4 {
        for bx in 0..4 {
            let b_idx = by * 4 + bx;
            let mut blk = [0i32; 16];
            blk[0] = dc_4x4[b_idx];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                blk[pos] = dequantize_coeff(hp_blocks[b_idx][k], st.hp_step);
            }
            inv_pct_4x4(&mut blk);
            let px0 = mb_x * 16 + bx * 4;
            let py0 = mb_y * 16 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                plane[row..row + 4].copy_from_slice(&blk[y * 4..y * 4 + 4]);
            }
        }
    }
    Ok(())
}

/// Encodes one `8x16` `YUV422` chroma macroblock (`2x4` grid of `4x4` blocks -> `1` DC + `7` LP + `120` HP).
fn encode_mb_422_8x16(
    w: &mut BitWriter,
    plane: &[i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) {
    let mut dc_2x4 = [0i32; 8];
    let mut hp_blocks = [[0i32; 15]; 8];

    for by in 0..4 {
        for bx in 0..2 {
            let mut blk = [0i32; 16];
            let px0 = mb_x * 8 + bx * 4;
            let py0 = mb_y * 16 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                blk[y * 4..y * 4 + 4].copy_from_slice(&plane[row..row + 4]);
            }
            fwd_pct_4x4(&mut blk);
            let b_idx = by * 2 + bx;
            dc_2x4[b_idx] = blk[0];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                hp_blocks[b_idx][k] = quantize_coeff(blk[pos], st.hp_step);
            }
        }
    }

    fwd_pct_2x4_dc(&mut dc_2x4);
    let q_dc = quantize_coeff(dc_2x4[0], st.dc_step);
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;
    write_se_k(w, q_dc - pred, 2);

    let mut q_lp = [0i32; 7];
    for (k, &pos) in SCAN_2X4_LP.iter().enumerate() {
        q_lp[k] = quantize_coeff(dc_2x4[pos], st.lp_step);
    }
    let lp_sum = encode_ac_band(w, &q_lp, st.lp_model.mbits, 0);
    st.lp_model.update(lp_sum, 7);

    let hp_mbits = st.hp_model.mbits;
    let eff_trim = st.trim_flex_bits.min(hp_mbits);
    let all_hp_zero = hp_blocks
        .iter()
        .all(|b| b.iter().all(|&c| (c.unsigned_abs() >> eff_trim) == 0));
    if all_hp_zero {
        w.write_bit(false);
        st.hp_model.update(0, 120);
    } else {
        w.write_bit(true);
        let mut hp_sum = 0u32;
        for blk in &hp_blocks {
            hp_sum += encode_ac_band(w, blk, hp_mbits, st.trim_flex_bits);
        }
        st.hp_model.update(hp_sum, 120);
    }
}

fn decode_mb_422_8x16(
    r: &mut BitReader<'_>,
    plane: &mut [i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) -> Result<(), BitmapError> {
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    let q_dc = pred + read_se_k(r, 2)?;
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;

    let mut q_lp = [0i32; 7];
    let lp_sum = decode_ac_band(r, &mut q_lp, st.lp_model.mbits, 0)?;
    st.lp_model.update(lp_sum, 7);

    let mut dc_2x4 = [0i32; 8];
    dc_2x4[0] = dequantize_coeff(q_dc, st.dc_step);
    for (k, &pos) in SCAN_2X4_LP.iter().enumerate() {
        dc_2x4[pos] = dequantize_coeff(q_lp[k], st.lp_step);
    }
    inv_pct_2x4_dc(&mut dc_2x4);

    let mut hp_blocks = [[0i32; 15]; 8];
    let hp_mbits = st.hp_model.mbits;
    if !r.read_bit()? {
        st.hp_model.update(0, 120);
    } else {
        let mut hp_sum = 0u32;
        for blk in &mut hp_blocks {
            hp_sum += decode_ac_band(r, blk, hp_mbits, st.trim_flex_bits)?;
        }
        st.hp_model.update(hp_sum, 120);
    }

    for by in 0..4 {
        for bx in 0..2 {
            let b_idx = by * 2 + bx;
            let mut blk = [0i32; 16];
            blk[0] = dc_2x4[b_idx];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                blk[pos] = dequantize_coeff(hp_blocks[b_idx][k], st.hp_step);
            }
            inv_pct_4x4(&mut blk);
            let px0 = mb_x * 8 + bx * 4;
            let py0 = mb_y * 16 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                plane[row..row + 4].copy_from_slice(&blk[y * 4..y * 4 + 4]);
            }
        }
    }
    Ok(())
}

/// Encodes one `8x8` `YUV420` chroma macroblock (`2x2` grid of `4x4` blocks -> `1` DC + `3` LP + `60` HP).
fn encode_mb_420_8x8(
    w: &mut BitWriter,
    plane: &[i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) {
    let mut dc_2x2 = [0i32; 4];
    let mut hp_blocks = [[0i32; 15]; 4];

    for by in 0..2 {
        for bx in 0..2 {
            let mut blk = [0i32; 16];
            let px0 = mb_x * 8 + bx * 4;
            let py0 = mb_y * 8 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                blk[y * 4..y * 4 + 4].copy_from_slice(&plane[row..row + 4]);
            }
            fwd_pct_4x4(&mut blk);
            let b_idx = by * 2 + bx;
            dc_2x2[b_idx] = blk[0];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                hp_blocks[b_idx][k] = quantize_coeff(blk[pos], st.hp_step);
            }
        }
    }

    fwd_pct_2x2_dc(&mut dc_2x2);
    let q_dc = quantize_coeff(dc_2x2[0], st.dc_step);
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;
    write_se_k(w, q_dc - pred, 2);

    let mut q_lp = [0i32; 3];
    for (k, &pos) in SCAN_2X2_LP.iter().enumerate() {
        q_lp[k] = quantize_coeff(dc_2x2[pos], st.lp_step);
    }
    let lp_sum = encode_ac_band(w, &q_lp, st.lp_model.mbits, 0);
    st.lp_model.update(lp_sum, 3);

    let hp_mbits = st.hp_model.mbits;
    let eff_trim = st.trim_flex_bits.min(hp_mbits);
    let all_hp_zero = hp_blocks
        .iter()
        .all(|b| b.iter().all(|&c| (c.unsigned_abs() >> eff_trim) == 0));
    if all_hp_zero {
        w.write_bit(false);
        st.hp_model.update(0, 60);
    } else {
        w.write_bit(true);
        let mut hp_sum = 0u32;
        for blk in &hp_blocks {
            hp_sum += encode_ac_band(w, blk, hp_mbits, st.trim_flex_bits);
        }
        st.hp_model.update(hp_sum, 60);
    }
}

fn decode_mb_420_8x8(
    r: &mut BitReader<'_>,
    plane: &mut [i32],
    stride: usize,
    mb_x: usize,
    mb_y: usize,
    mb_width: usize,
    st: &mut ChannelState,
) -> Result<(), BitmapError> {
    let pred = predict_dc(mb_x, mb_y, mb_width, &st.dc_grid);
    let q_dc = pred + read_se_k(r, 2)?;
    st.dc_grid[mb_y * mb_width + mb_x] = q_dc;

    let mut q_lp = [0i32; 3];
    let lp_sum = decode_ac_band(r, &mut q_lp, st.lp_model.mbits, 0)?;
    st.lp_model.update(lp_sum, 3);

    let mut dc_2x2 = [0i32; 4];
    dc_2x2[0] = dequantize_coeff(q_dc, st.dc_step);
    for (k, &pos) in SCAN_2X2_LP.iter().enumerate() {
        dc_2x2[pos] = dequantize_coeff(q_lp[k], st.lp_step);
    }
    inv_pct_2x2_dc(&mut dc_2x2);

    let mut hp_blocks = [[0i32; 15]; 4];
    let hp_mbits = st.hp_model.mbits;
    if !r.read_bit()? {
        st.hp_model.update(0, 60);
    } else {
        let mut hp_sum = 0u32;
        for blk in &mut hp_blocks {
            hp_sum += decode_ac_band(r, blk, hp_mbits, st.trim_flex_bits)?;
        }
        st.hp_model.update(hp_sum, 60);
    }

    for by in 0..2 {
        for bx in 0..2 {
            let b_idx = by * 2 + bx;
            let mut blk = [0i32; 16];
            blk[0] = dc_2x2[b_idx];
            for (k, &pos) in SCAN_4X4.iter().enumerate() {
                blk[pos] = dequantize_coeff(hp_blocks[b_idx][k], st.hp_step);
            }
            inv_pct_4x4(&mut blk);
            let px0 = mb_x * 8 + bx * 4;
            let py0 = mb_y * 8 + by * 4;
            for y in 0..4 {
                let row = (py0 + y) * stride + px0;
                plane[row..row + 4].copy_from_slice(&blk[y * 4..y * 4 + 4]);
            }
        }
    }
    Ok(())
}

// ============================================================================
// Chroma Subsampling & Bilinear Upsampling
// ============================================================================

fn downsample_422(full: &[i32], ew: usize, eh: usize) -> Vec<i32> {
    let cw = ew / 2;
    let mut out = vec![0i32; cw * eh];
    for y in 0..eh {
        for cx in 0..cw {
            let x0 = cx * 2;
            out[y * cw + cx] = (full[y * ew + x0] + full[y * ew + x0 + 1] + 1) >> 1;
        }
    }
    out
}

fn upsample_422(sub: &[i32], ew: usize, eh: usize) -> Vec<i32> {
    let cw = ew / 2;
    let mut out = vec![0i32; ew * eh];
    for y in 0..eh {
        for x in 0..ew {
            // Centered bilinear reconstruction
            let c0 = (x / 2).min(cw - 1);
            if x % 2 == 0 {
                out[y * ew + x] = sub[y * cw + c0];
            } else {
                let c1 = (c0 + 1).min(cw - 1);
                out[y * ew + x] = (sub[y * cw + c0] + sub[y * cw + c1] + 1) >> 1;
            }
        }
    }
    out
}

fn downsample_420(full: &[i32], ew: usize, eh: usize) -> Vec<i32> {
    let cw = ew / 2;
    let ch = eh / 2;
    let mut out = vec![0i32; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            let x0 = cx * 2;
            let y0 = cy * 2;
            let sum = full[y0 * ew + x0]
                + full[y0 * ew + x0 + 1]
                + full[(y0 + 1) * ew + x0]
                + full[(y0 + 1) * ew + x0 + 1];
            out[cy * cw + cx] = (sum + 2) >> 2;
        }
    }
    out
}

fn upsample_420(sub: &[i32], ew: usize, eh: usize) -> Vec<i32> {
    let cw = ew / 2;
    let ch = eh / 2;
    let mut horiz = vec![0i32; ew * ch];
    for cy in 0..ch {
        for x in 0..ew {
            let c0 = (x / 2).min(cw - 1);
            if x % 2 == 0 {
                horiz[cy * ew + x] = sub[cy * cw + c0];
            } else {
                let c1 = (c0 + 1).min(cw - 1);
                horiz[cy * ew + x] = (sub[cy * cw + c0] + sub[cy * cw + c1] + 1) >> 1;
            }
        }
    }
    let mut out = vec![0i32; ew * eh];
    for y in 0..eh {
        let r0 = (y / 2).min(ch - 1);
        if y % 2 == 0 {
            out[y * ew..(y + 1) * ew].copy_from_slice(&horiz[r0 * ew..(r0 + 1) * ew]);
        } else {
            let r1 = (r0 + 1).min(ch - 1);
            for x in 0..ew {
                out[y * ew + x] = (horiz[r0 * ew + x] + horiz[r1 * ew + x] + 1) >> 1;
            }
        }
    }
    out
}

// ============================================================================
// Codestream Encode / Decode (`WMPHOTO\0` ... `TILE_SPATIAL`)
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn encode_primary_codestream(
    width: u32,
    height: u32,
    argb_be: &[u8],
    subsampling: ChromaSubsampling,
    quantization: u8,
    trim_flex_bits: u8,
    interleaved_alpha: bool,
) -> Vec<u8> {
    let w_usize = width as usize;
    let h_usize = height as usize;
    let right_margin = ((16 - (w_usize % 16)) % 16) as u8;
    let bottom_margin = ((16 - (h_usize % 16)) % 16) as u8;
    let ew = w_usize + usize::from(right_margin);
    let eh = h_usize + usize::from(bottom_margin);
    let mb_width = ew / 16;
    let mb_height = eh / 16;
    let mb_count = mb_width * mb_height;

    let scaled = quantization > 0;
    let overlap_mode = u8::from(quantization > 0);
    let scale_shift = if scaled { 2 } else { 0 };

    let mut y_plane = vec![0i32; ew * eh];
    let mut u_full = vec![0i32; ew * eh];
    let mut v_full = vec![0i32; ew * eh];
    let mut a_plane = if interleaved_alpha {
        vec![0i32; ew * eh]
    } else {
        Vec::new()
    };

    for py in 0..eh {
        let sy = py.min(h_usize - 1);
        for px in 0..ew {
            let sx = px.min(w_usize - 1);
            let off = (sy * w_usize + sx) * 4;
            let a = (i32::from(argb_be[off]) - 128) << scale_shift;
            let r = (i32::from(argb_be[off + 1]) - 128) << scale_shift;
            let g = (i32::from(argb_be[off + 2]) - 128) << scale_shift;
            let b = (i32::from(argb_be[off + 3]) - 128) << scale_shift;
            let (y, u, v) = fwd_rgb_to_yuv(r, g, b);
            let dst = py * ew + px;
            y_plane[dst] = y;
            u_full[dst] = u;
            v_full[dst] = v;
            if interleaved_alpha {
                a_plane[dst] = a;
            }
        }
    }

    let (mut u_plane, mut v_plane, cw, ch) = match subsampling {
        ChromaSubsampling::Yuv444 => (u_full, v_full, ew, eh),
        ChromaSubsampling::Yuv422 => (
            downsample_422(&u_full, ew, eh),
            downsample_422(&v_full, ew, eh),
            ew / 2,
            eh,
        ),
        ChromaSubsampling::Yuv420 => (
            downsample_420(&u_full, ew, eh),
            downsample_420(&v_full, ew, eh),
            ew / 2,
            eh / 2,
        ),
    };

    if overlap_mode == 1 {
        apply_plane_overlap(&mut y_plane, ew, eh, true);
        apply_plane_overlap(&mut u_plane, cw, ch, true);
        apply_plane_overlap(&mut v_plane, cw, ch, true);
        if interleaved_alpha {
            apply_plane_overlap(&mut a_plane, ew, eh, true);
        }
    }

    let hdr = ImageHeader {
        overlap_mode,
        short_header: width <= 65536 && height <= 65536,
        windowing: right_margin > 0 || bottom_margin > 0,
        trim_flexbits_flag: trim_flex_bits > 0,
        red_blue_not_swapped: false,
        premultiplied_alpha: false,
        alpha_image_plane: interleaved_alpha,
        output_clr_fmt: OUT_CLR_RGB,
        output_bitdepth: OUT_BD_8,
        width,
        height,
        top_margin: 0,
        left_margin: 0,
        bottom_margin,
        right_margin,
        index_table_present: true,
    };

    let qp = PlaneQp::from_quantization(quantization);
    let plane_hdr = ImagePlaneHeader {
        internal_clr_fmt: subsampling.internal_clr_fmt(),
        scaled_flag: scaled,
        bands_present: 0,
        qp,
    };

    let mut bw = BitWriter::with_capacity(ew * eh);
    hdr.write(&mut bw);
    plane_hdr.write(&mut bw);

    if interleaved_alpha {
        let alpha_hdr = ImagePlaneHeader {
            internal_clr_fmt: CLR_YONLY,
            scaled_flag: scaled,
            bands_present: 0,
            qp,
        };
        alpha_hdr.write(&mut bw);
    }

    // INDEX_TABLE_TILES (startcode 0x0001; 1 tile -> 0 offsets)
    bw.write_bytes_aligned(&[0x00, 0x01]);
    // PROFILE_LEVEL_INFO (VLW_ESC = 4, PROFILE_IDC = 111, LEVEL_IDC = 255, RESERVED_L = 0, LAST_FLAG = 1)
    bw.write_vlw_esc(4);
    bw.write_bytes_aligned(&[111, 255, 0x00, 0x01]);

    // TILE_SPATIAL startcode (0x00000100)
    bw.write_bytes_aligned(&[0x00, 0x00, 0x01, 0x00]);
    if hdr.trim_flexbits_flag {
        bw.write_bits(u32::from(trim_flex_bits & 0x0F), 4);
    }

    let mut st_y = ChannelState::new(mb_count, qp.dc_y, qp.lp_y, qp.hp_y, trim_flex_bits);
    let mut st_u = ChannelState::new(mb_count, qp.dc_uv, qp.lp_uv, qp.hp_uv, trim_flex_bits);
    let mut st_v = ChannelState::new(mb_count, qp.dc_uv, qp.lp_uv, qp.hp_uv, trim_flex_bits);
    let mut st_a = if interleaved_alpha {
        Some(ChannelState::new(
            mb_count,
            qp.dc_y,
            qp.lp_y,
            qp.hp_y,
            trim_flex_bits,
        ))
    } else {
        None
    };

    for mb_y in 0..mb_height {
        for mb_x in 0..mb_width {
            encode_mb_full_16x16(&mut bw, &y_plane, ew, mb_x, mb_y, mb_width, &mut st_y);
            match subsampling {
                ChromaSubsampling::Yuv444 => {
                    encode_mb_full_16x16(&mut bw, &u_plane, cw, mb_x, mb_y, mb_width, &mut st_u);
                    encode_mb_full_16x16(&mut bw, &v_plane, cw, mb_x, mb_y, mb_width, &mut st_v);
                }
                ChromaSubsampling::Yuv422 => {
                    encode_mb_422_8x16(&mut bw, &u_plane, cw, mb_x, mb_y, mb_width, &mut st_u);
                    encode_mb_422_8x16(&mut bw, &v_plane, cw, mb_x, mb_y, mb_width, &mut st_v);
                }
                ChromaSubsampling::Yuv420 => {
                    encode_mb_420_8x8(&mut bw, &u_plane, cw, mb_x, mb_y, mb_width, &mut st_u);
                    encode_mb_420_8x8(&mut bw, &v_plane, cw, mb_x, mb_y, mb_width, &mut st_v);
                }
            }
            if let Some(ref mut sa) = st_a {
                encode_mb_full_16x16(&mut bw, &a_plane, ew, mb_x, mb_y, mb_width, sa);
            }
        }
    }

    bw.finish()
}

fn encode_planar_alpha_codestream(
    width: u32,
    height: u32,
    argb_be: &[u8],
    quantization: u8,
    trim_flex_bits: u8,
) -> Vec<u8> {
    let w_usize = width as usize;
    let h_usize = height as usize;
    let right_margin = ((16 - (w_usize % 16)) % 16) as u8;
    let bottom_margin = ((16 - (h_usize % 16)) % 16) as u8;
    let ew = w_usize + usize::from(right_margin);
    let eh = h_usize + usize::from(bottom_margin);
    let mb_width = ew / 16;
    let mb_height = eh / 16;
    let mb_count = mb_width * mb_height;

    let scaled = quantization > 0;
    let overlap_mode = u8::from(quantization > 0);
    let scale_shift = if scaled { 2 } else { 0 };

    let mut a_plane = vec![0i32; ew * eh];
    for py in 0..eh {
        let sy = py.min(h_usize - 1);
        for px in 0..ew {
            let sx = px.min(w_usize - 1);
            let off = (sy * w_usize + sx) * 4;
            a_plane[py * ew + px] = (i32::from(argb_be[off]) - 128) << scale_shift;
        }
    }
    if overlap_mode == 1 {
        apply_plane_overlap(&mut a_plane, ew, eh, true);
    }

    let hdr = ImageHeader {
        overlap_mode,
        short_header: width <= 65536 && height <= 65536,
        windowing: right_margin > 0 || bottom_margin > 0,
        trim_flexbits_flag: trim_flex_bits > 0,
        red_blue_not_swapped: false,
        premultiplied_alpha: false,
        alpha_image_plane: false,
        output_clr_fmt: OUT_CLR_YONLY,
        output_bitdepth: OUT_BD_8,
        width,
        height,
        top_margin: 0,
        left_margin: 0,
        bottom_margin,
        right_margin,
        index_table_present: true,
    };
    let qp = PlaneQp::from_quantization(quantization);
    let plane_hdr = ImagePlaneHeader {
        internal_clr_fmt: CLR_YONLY,
        scaled_flag: scaled,
        bands_present: 0,
        qp,
    };

    let mut bw = BitWriter::with_capacity(ew * eh / 2);
    hdr.write(&mut bw);
    plane_hdr.write(&mut bw);
    bw.write_bytes_aligned(&[0x00, 0x01]);
    bw.write_vlw_esc(4);
    bw.write_bytes_aligned(&[111, 255, 0x00, 0x01]);
    bw.write_bytes_aligned(&[0x00, 0x00, 0x01, 0x00]);
    if hdr.trim_flexbits_flag {
        bw.write_bits(u32::from(trim_flex_bits & 0x0F), 4);
    }

    let mut st_a = ChannelState::new(mb_count, qp.dc_y, qp.lp_y, qp.hp_y, trim_flex_bits);
    for mb_y in 0..mb_height {
        for mb_x in 0..mb_width {
            encode_mb_full_16x16(&mut bw, &a_plane, ew, mb_x, mb_y, mb_width, &mut st_a);
        }
    }
    bw.finish()
}

struct DecodedCodestream {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    has_interleaved_alpha: bool,
    y_only_samples: Option<Vec<u8>>,
}

fn decode_codestream(stream: &[u8]) -> Result<DecodedCodestream, BitmapError> {
    let mut br = BitReader::new(stream);
    let hdr = ImageHeader::read(&mut br)?;
    let plane_hdr = ImagePlaneHeader::read(&mut br)?;
    let alpha_hdr = if hdr.alpha_image_plane {
        let ah = ImagePlaneHeader::read(&mut br)?;
        if ah.internal_clr_fmt != CLR_YONLY {
            return Err(error("JPEG-XR alpha IMAGE_PLANE_HEADER must use YONLY"));
        }
        Some(ah)
    } else {
        None
    };

    if hdr.index_table_present {
        let idx_sc = br.read_bytes_aligned(2)?;
        if idx_sc != [0x00, 0x01] {
            return Err(error("invalid JPEG-XR INDEX_TABLE_STARTCODE"));
        }
    }

    let sub_header_bytes = br.read_vlw_esc()?;
    if sub_header_bytes > 0 {
        let _ = br.read_bytes_aligned(sub_header_bytes)?;
    }

    let tile_sc = br.read_bytes_aligned(4)?;
    if tile_sc[..3] != [0x00, 0x00, 0x01] {
        return Err(error("missing JPEG-XR TILE_SPATIAL startcode (0x000001)"));
    }

    let trim_flex_bits = if hdr.trim_flexbits_flag {
        br.read_bits(4)? as u8
    } else {
        0
    };

    let (ew, eh) = hdr.extended_dims();
    let mb_width = ew / 16;
    let mb_height = eh / 16;
    let mb_count = mb_width * mb_height;

    if plane_hdr.internal_clr_fmt == CLR_YONLY {
        let mut y_plane = vec![0i32; ew * eh];
        let mut st_y = ChannelState::new(
            mb_count,
            plane_hdr.qp.dc_y,
            plane_hdr.qp.lp_y,
            plane_hdr.qp.hp_y,
            trim_flex_bits,
        );
        for mb_y in 0..mb_height {
            for mb_x in 0..mb_width {
                decode_mb_full_16x16(&mut br, &mut y_plane, ew, mb_x, mb_y, mb_width, &mut st_y)?;
            }
        }
        if hdr.overlap_mode == 1 {
            apply_plane_overlap(&mut y_plane, ew, eh, false);
        }
        let w_out = hdr.width as usize;
        let h_out = hdr.height as usize;
        let left = usize::from(hdr.left_margin);
        let top = usize::from(hdr.top_margin);
        let mut gray = Vec::with_capacity(w_out * h_out);
        let mut rgba = Vec::with_capacity(w_out * h_out * 4);
        for y in 0..h_out {
            for x in 0..w_out {
                let raw = y_plane[(top + y) * ew + (left + x)];
                let unscaled = if plane_hdr.scaled_flag {
                    (raw + 2) >> 2
                } else {
                    raw
                };
                let byte = (unscaled + 128).clamp(0, 255) as u8;
                gray.push(byte);
                rgba.extend_from_slice(&[byte, byte, byte, 255]);
            }
        }
        return Ok(DecodedCodestream {
            width: hdr.width,
            height: hdr.height,
            rgba,
            has_interleaved_alpha: false,
            y_only_samples: Some(gray),
        });
    }

    let (cw, ch) = match plane_hdr.internal_clr_fmt {
        CLR_YUV444 => (ew, eh),
        CLR_YUV422 => (ew / 2, eh),
        CLR_YUV420 => (ew / 2, eh / 2),
        _ => unreachable!(),
    };

    let mut y_plane = vec![0i32; ew * eh];
    let mut u_plane = vec![0i32; cw * ch];
    let mut v_plane = vec![0i32; cw * ch];
    let mut a_plane = if alpha_hdr.is_some() {
        vec![0i32; ew * eh]
    } else {
        Vec::new()
    };

    let mut st_y = ChannelState::new(
        mb_count,
        plane_hdr.qp.dc_y,
        plane_hdr.qp.lp_y,
        plane_hdr.qp.hp_y,
        trim_flex_bits,
    );
    let mut st_u = ChannelState::new(
        mb_count,
        plane_hdr.qp.dc_uv,
        plane_hdr.qp.lp_uv,
        plane_hdr.qp.hp_uv,
        trim_flex_bits,
    );
    let mut st_v = ChannelState::new(
        mb_count,
        plane_hdr.qp.dc_uv,
        plane_hdr.qp.lp_uv,
        plane_hdr.qp.hp_uv,
        trim_flex_bits,
    );
    let mut st_a = alpha_hdr
        .as_ref()
        .map(|ah| ChannelState::new(mb_count, ah.qp.dc_y, ah.qp.lp_y, ah.qp.hp_y, trim_flex_bits));

    for mb_y in 0..mb_height {
        for mb_x in 0..mb_width {
            decode_mb_full_16x16(&mut br, &mut y_plane, ew, mb_x, mb_y, mb_width, &mut st_y)?;
            match plane_hdr.internal_clr_fmt {
                CLR_YUV444 => {
                    decode_mb_full_16x16(
                        &mut br,
                        &mut u_plane,
                        cw,
                        mb_x,
                        mb_y,
                        mb_width,
                        &mut st_u,
                    )?;
                    decode_mb_full_16x16(
                        &mut br,
                        &mut v_plane,
                        cw,
                        mb_x,
                        mb_y,
                        mb_width,
                        &mut st_v,
                    )?;
                }
                CLR_YUV422 => {
                    decode_mb_422_8x16(&mut br, &mut u_plane, cw, mb_x, mb_y, mb_width, &mut st_u)?;
                    decode_mb_422_8x16(&mut br, &mut v_plane, cw, mb_x, mb_y, mb_width, &mut st_v)?;
                }
                CLR_YUV420 => {
                    decode_mb_420_8x8(&mut br, &mut u_plane, cw, mb_x, mb_y, mb_width, &mut st_u)?;
                    decode_mb_420_8x8(&mut br, &mut v_plane, cw, mb_x, mb_y, mb_width, &mut st_v)?;
                }
                _ => unreachable!(),
            }
            if let Some(ref mut sa) = st_a {
                decode_mb_full_16x16(&mut br, &mut a_plane, ew, mb_x, mb_y, mb_width, sa)?;
            }
        }
    }

    if hdr.overlap_mode == 1 {
        apply_plane_overlap(&mut y_plane, ew, eh, false);
        apply_plane_overlap(&mut u_plane, cw, ch, false);
        apply_plane_overlap(&mut v_plane, cw, ch, false);
        if alpha_hdr.is_some() {
            apply_plane_overlap(&mut a_plane, ew, eh, false);
        }
    }

    let (u_full, v_full) = match plane_hdr.internal_clr_fmt {
        CLR_YUV444 => (u_plane, v_plane),
        CLR_YUV422 => (
            upsample_422(&u_plane, ew, eh),
            upsample_422(&v_plane, ew, eh),
        ),
        CLR_YUV420 => (
            upsample_420(&u_plane, ew, eh),
            upsample_420(&v_plane, ew, eh),
        ),
        _ => unreachable!(),
    };

    let w_out = hdr.width as usize;
    let h_out = hdr.height as usize;
    let left = usize::from(hdr.left_margin);
    let top = usize::from(hdr.top_margin);
    let alpha_scaled = alpha_hdr.as_ref().is_some_and(|ah| ah.scaled_flag);
    let mut rgba = Vec::with_capacity(w_out * h_out * 4);

    for y in 0..h_out {
        for x in 0..w_out {
            let idx = (top + y) * ew + (left + x);
            let (mut r, mut g, mut b) = inv_yuv_to_rgb(y_plane[idx], u_full[idx], v_full[idx]);
            if plane_hdr.scaled_flag {
                r = (r + 2) >> 2;
                g = (g + 2) >> 2;
                b = (b + 2) >> 2;
            }
            let r8 = (r + 128).clamp(0, 255) as u8;
            let g8 = (g + 128).clamp(0, 255) as u8;
            let b8 = (b + 128).clamp(0, 255) as u8;
            let a8 = if alpha_hdr.is_some() {
                let ra = if alpha_scaled {
                    (a_plane[idx] + 2) >> 2
                } else {
                    a_plane[idx]
                };
                (ra + 128).clamp(0, 255) as u8
            } else {
                255
            };
            rgba.extend_from_slice(&[r8, g8, b8, a8]);
        }
    }

    Ok(DecodedCodestream {
        width: hdr.width,
        height: hdr.height,
        rgba,
        has_interleaved_alpha: alpha_hdr.is_some(),
        y_only_samples: None,
    })
}

// ============================================================================
// Public Container Encoder & Decoder Entrypoints
// ============================================================================

/// Encodes big-endian ARGB8 (`[A, R, G, B]` per pixel) into a spec-conformant ITU-T T.832 `.jxr` file.
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
            JxrAlphaMode::Interleaved
        } else {
            JxrAlphaMode::None
        },
        argb_be,
        quantization,
        color_space,
        trim_flex_bits,
    )
}

/// Encodes big-endian ARGB8 into a `.jxr` container using the requested alpha representation
/// (`None` -> `24bppBGR`, `Interleaved` -> `32bppBGRA` with `ALPHA_IMAGE_PLANE_FLAG = 1`,
/// `Planar` -> `32bppBGRA` with separate `ALPHA_OFFSET` / `ALPHA_BYTE_COUNT` codestream).
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

    let num_entries: u16 = if has_alpha { 9 } else { 7 };
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

    // 8-byte little-endian JXR container header
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
    if has_alpha {
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

    // Codestream(s)
    out.extend_from_slice(&primary_stream);
    if let Some(alpha_bytes) = &planar_alpha_stream {
        out.extend_from_slice(alpha_bytes);
    }

    Ok(out)
}

#[derive(Debug)]
struct ParsedJxrContainer {
    width: u32,
    height: u32,
    has_alpha_guid: bool,
    image_offset: usize,
    image_byte_count: usize,
    alpha_offset: usize,
    alpha_byte_count: usize,
}

fn parse_jxr_container(bytes: &[u8]) -> Result<ParsedJxrContainer, BitmapError> {
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
                if guid[..15] != GUID_PREFIX_WIC {
                    return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID prefix"));
                }
                has_alpha_guid = match guid[15] {
                    0x0C | 0x0D | 0x08 => false,       // 24bppBGR, 24bppRGB, 8bppGray
                    0x0F | 0x10 | 0x1E | 0x1F => true, // 32bppBGRA, 32bppPBGRA, 32bppRGBA, 32bppPRGBA
                    _ => return Err(error("unsupported JPEG-XR PIXEL_FORMAT GUID subtype")),
                };
                saw_pixel_format = true;
            }
            TAG_IMAGE_WIDTH => width = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_HEIGHT => height = Some(read_u32_tag(ftype, count, raw_val)?),
            TAG_IMAGE_OFFSET => image_offset = Some(read_u32_tag(ftype, count, raw_val)? as usize),
            TAG_IMAGE_BYTE_COUNT => {
                image_byte_count = Some(read_u32_tag(ftype, count, raw_val)? as usize)
            }
            TAG_ALPHA_OFFSET => alpha_offset = read_u32_tag(ftype, count, raw_val)? as usize,
            TAG_ALPHA_BYTE_COUNT => {
                alpha_byte_count = read_u32_tag(ftype, count, raw_val)? as usize
            }
            _ => {}
        }
    }

    if !saw_pixel_format {
        return Err(error("missing JPEG-XR PIXEL_FORMAT IFD tag (0xBC01)"));
    }
    let image_offset =
        image_offset.ok_or_else(|| error("missing JPEG-XR IMAGE_OFFSET (0xBCE0)"))?;
    let image_byte_count =
        image_byte_count.ok_or_else(|| error("missing JPEG-XR IMAGE_BYTE_COUNT (0xBCE1)"))?;
    let img_end = image_offset
        .checked_add(image_byte_count)
        .ok_or_else(|| error("JPEG-XR IMAGE_OFFSET overflow"))?;
    if image_byte_count < 16 || img_end > bytes.len() {
        return Err(error("JPEG-XR primary codestream range out of bounds"));
    }
    if alpha_offset > 0 {
        let alpha_end = alpha_offset
            .checked_add(alpha_byte_count)
            .ok_or_else(|| error("JPEG-XR ALPHA_OFFSET overflow"))?;
        if alpha_byte_count < 16 || alpha_end > bytes.len() {
            return Err(error("JPEG-XR planar alpha codestream range out of bounds"));
        }
    }

    // If IFD omitted width/height, peek IMAGE_HEADER
    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            let mut br = BitReader::new(&bytes[image_offset..img_end]);
            let hdr = ImageHeader::read(&mut br)?;
            (hdr.width, hdr.height)
        }
    };
    if w == 0 || h == 0 || w > MAX_BITMAP_DIMENSION || h > MAX_BITMAP_DIMENSION {
        return Err(error("JPEG-XR image dimensions must be in 1..=4096"));
    }

    Ok(ParsedJxrContainer {
        width: w,
        height: h,
        has_alpha_guid,
        image_offset,
        image_byte_count,
        alpha_offset,
        alpha_byte_count,
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
    let primary_slice =
        &bytes[container.image_offset..container.image_offset + container.image_byte_count];
    let mut decoded = decode_codestream(primary_slice)?;
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
        let alpha_slice =
            &bytes[container.alpha_offset..container.alpha_offset + container.alpha_byte_count];
        let alpha_dec = decode_codestream(alpha_slice)?;
        let Some(alpha_samples) = alpha_dec.y_only_samples else {
            return Err(error("JPEG-XR planar alpha stream did not decode as YONLY"));
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

    Ok(DecodedImage {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argb_to_rgba(argb: &[u8], transparent: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(argb.len());
        for px in argb.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[1], px[2], px[3], if transparent { px[0] } else { 255 }]);
        }
        out
    }

    fn psnr_rgb(expected_rgba: &[u8], actual_rgba: &[u8]) -> f64 {
        assert_eq!(expected_rgba.len(), actual_rgba.len());
        let mut mse_sum = 0.0_f64;
        let mut count = 0usize;
        for (e, a) in expected_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(actual_rgba.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                let d = f64::from(e[c]) - f64::from(a[c]);
                mse_sum += d * d;
                count += 1;
            }
        }
        let mse = mse_sum / (count as f64);
        if mse == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0 * 255.0 / mse).log10()
        }
    }

    #[test]
    fn lossless_444_roundtrips_odd_dimension_rgb_and_rgba_bit_exact() {
        let (w, h) = (19u32, 13u32);
        let mut argb = Vec::with_capacity((w * h * 4) as usize);
        let mut state = 0x9E37_79B9_u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                let a = ((state >> 24) as u8).wrapping_add((x * 13 + y * 7) as u8);
                let r = ((state >> 16) as u8).wrapping_add((x * 29) as u8);
                let g = ((state >> 8) as u8).wrapping_add((y * 37) as u8);
                let b = (state as u8).wrapping_add(((x + y) * 19) as u8);
                argb.extend_from_slice(&[a, r, g, b]);
            }
        }

        // 1) Opaque 24bppBGR lossless 19x13
        let jxr_rgb = encode_jxr_argb(w, h, false, &argb, 0, "4:4:4", 0).unwrap();
        assert_eq!(&jxr_rgb[..4], &[0x49, 0x49, 0xBC, 0x01]);
        assert!(
            jxr_rgb.windows(8).any(|win| win == b"WMPHOTO\0"),
            "must contain WMPHOTO\\0 IMAGE_HEADER"
        );
        assert!(
            jxr_rgb
                .windows(16)
                .any(|win| win == GUID_PIXEL_FORMAT_24BPP_BGR),
            "opaque .jxr must emit 24bppBGR GUID"
        );
        assert_eq!(jxr_dimensions(&jxr_rgb).unwrap(), (w, h));
        let dec_rgb = decode_jxr_to_rgba(&jxr_rgb).unwrap();
        assert_eq!((dec_rgb.width, dec_rgb.height), (w, h));
        assert_eq!(dec_rgb.rgba.as_ref(), argb_to_rgba(&argb, false).as_slice());

        // 2) Transparent 32bppBGRA interleaved alpha lossless 19x13
        let jxr_rgba = encode_jxr_argb(w, h, true, &argb, 0, "auto", 0).unwrap();
        assert!(
            jxr_rgba
                .windows(16)
                .any(|win| win == GUID_PIXEL_FORMAT_32BPP_BGRA),
            "transparent .jxr must emit 32bppBGRA GUID"
        );
        let dec_rgba = decode_jxr_to_rgba(&jxr_rgba).unwrap();
        assert_eq!(dec_rgba.rgba.as_ref(), argb_to_rgba(&argb, true).as_slice());

        // 3) Transparent 32bppBGRA planar alpha (ALPHA_OFFSET / ALPHA_BYTE_COUNT) lossless 19x13
        let jxr_planar =
            encode_jxr_with_alpha_mode(w, h, JxrAlphaMode::Planar, &argb, 0, "4:4:4", 0).unwrap();
        let parsed = parse_jxr_container(&jxr_planar).unwrap();
        assert!(parsed.alpha_offset > parsed.image_offset);
        assert!(parsed.alpha_byte_count > 16);
        let dec_planar = decode_jxr_to_rgba(&jxr_planar).unwrap();
        assert_eq!(
            dec_planar.rgba.as_ref(),
            argb_to_rgba(&argb, true).as_slice()
        );
    }

    #[test]
    fn subsampling_quantization_and_trim_flexbits_produce_high_psnr_and_monotonic_sizes() {
        let (w, h) = (48u32, 32u32);
        let mut grad_argb = Vec::with_capacity((w * h * 4) as usize);
        let mut rich_argb = Vec::with_capacity((w * h * 4) as usize);
        let mut rng = 0x1234_5678_u32;
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 255) / (w - 1)) as u8;
                let g = ((y * 255) / (h - 1)) as u8;
                let b = (((x + y) * 255) / (w + h - 2)) as u8;
                let a = (128 + ((x * 127) / (w - 1))) as u8;
                grad_argb.extend_from_slice(&[a, r, g, b]);

                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let jitter_r = ((rng >> 24) & 0x1F) as u8;
                let jitter_g = ((rng >> 16) & 0x1F) as u8;
                let jitter_b = ((rng >> 8) & 0x1F) as u8;
                rich_argb.extend_from_slice(&[
                    255,
                    r.saturating_add(jitter_r),
                    g.saturating_add(jitter_g),
                    b.saturating_add(jitter_b),
                ]);
            }
        }
        let expected_grad_rgba = argb_to_rgba(&grad_argb, true);

        // Smooth gradient PSNR across color spaces & quantizations
        for cs in ["4:4:4", "4:2:2", "4:2:0"] {
            let s20 = encode_jxr_argb(w, h, true, &grad_argb, 20, cs, 0).unwrap();
            let d20 = decode_jxr_to_rgba(&s20).unwrap();
            let psnr20 = psnr_rgb(&expected_grad_rgba, &d20.rgba);
            assert!(
                psnr20 > 36.0,
                "expected high PSNR (>36 dB) at q=20 for {cs}, got {psnr20:.2} dB"
            );
        }

        // Monotonic byte counts across quantization: q=0 > q=20 > q=60 > q=90
        let b_q0 = encode_jxr_argb(w, h, false, &rich_argb, 0, "4:4:4", 0).unwrap();
        let b_q20 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 0).unwrap();
        let b_q60 = encode_jxr_argb(w, h, false, &rich_argb, 60, "4:4:4", 0).unwrap();
        let b_q90 = encode_jxr_argb(w, h, false, &rich_argb, 90, "4:4:4", 0).unwrap();
        assert!(
            b_q0.len() > b_q20.len() && b_q20.len() > b_q60.len() && b_q60.len() > b_q90.len(),
            "expected monotonic quantization sizes: q0={} > q20={} > q60={} > q90={}",
            b_q0.len(),
            b_q20.len(),
            b_q60.len(),
            b_q90.len()
        );

        // Monotonic byte counts across chroma subsampling: 4:4:4 > 4:2:2 > 4:2:0
        let b_444 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:4:4", 0).unwrap();
        let b_422 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:2:2", 0).unwrap();
        let b_420 = encode_jxr_argb(w, h, false, &rich_argb, 10, "4:2:0", 0).unwrap();
        assert!(
            b_444.len() > b_422.len() && b_422.len() > b_420.len(),
            "expected monotonic subsampling sizes: 444={} > 422={} > 420={}",
            b_444.len(),
            b_422.len(),
            b_420.len()
        );

        // trimFlexBits = 4 produces strictly smaller stream than trimFlexBits = 0
        let b_trim0 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 0).unwrap();
        let b_trim4 = encode_jxr_argb(w, h, false, &rich_argb, 20, "4:4:4", 4).unwrap();
        assert!(
            b_trim0.len() > b_trim4.len(),
            "expected trimFlexBits=4 ({}) < trimFlexBits=0 ({})",
            b_trim4.len(),
            b_trim0.len()
        );
        let d_trim4 = decode_jxr_to_rgba(&b_trim4).unwrap();
        assert_eq!((d_trim4.width, d_trim4.height), (w, h));
    }
}
