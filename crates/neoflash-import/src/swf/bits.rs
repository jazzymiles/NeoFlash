//! Checked SWF byte & MSB-first bit stream reader (`RECT`, `MATRIX`, `CXFORM`,
//! `CXFORMWITHALPHA`, `RGB`/`RGBA` and SWF versioned strings).

use neoflash_core::{Color, ColorTransform};

pub(crate) struct SwfSlice<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..8 within `bytes[byte_pos]` when unaligned; 0 means byte-aligned.
}

impl<'a> SwfSlice<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    #[inline]
    pub(crate) fn align(&mut self) {
        if self.bit_pos != 0 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }
    }

    #[inline]
    pub(crate) fn pos(&self) -> usize {
        self.byte_pos + usize::from(self.bit_pos != 0)
    }

    #[inline]
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos())
    }

    #[inline]
    pub(crate) fn rest(&mut self) -> &'a [u8] {
        self.align();
        let start = self.byte_pos.min(self.bytes.len());
        self.byte_pos = self.bytes.len();
        &self.bytes[start..]
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        self.align();
        let end = self.byte_pos.checked_add(len)?;
        if end > self.bytes.len() {
            return None;
        }
        let slice = &self.bytes[self.byte_pos..end];
        self.byte_pos = end;
        Some(slice)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    pub(crate) fn i16(&mut self) -> Option<i16> {
        let b = self.take(2)?;
        Some(i16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        let b = self.take(4)?;
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn fixed8(&mut self) -> Option<f64> {
        Some(f64::from(self.i16()?) / 256.0)
    }

    pub(crate) fn ufixed8(&mut self) -> Option<f64> {
        Some(f64::from(self.u16()?) / 256.0)
    }

    pub(crate) fn encoded_u32(&mut self) -> Option<u32> {
        self.align();
        let mut result = 0u32;
        for shift in [0u32, 7, 14, 21, 28] {
            let b = self.u8()?;
            result |= u32::from(b & 0x7f) << shift;
            if (b & 0x80) == 0 {
                return Some(result);
            }
        }
        Some(result)
    }

    /// Read `nbits` (`0..=32`) unsigned MSB-first bits.
    pub(crate) fn ub(&mut self, nbits: usize) -> Option<u32> {
        if nbits == 0 {
            return Some(0);
        }
        if nbits > 32 {
            return None;
        }
        let mut val = 0u32;
        let mut rem = nbits;
        while rem > 0 {
            let byte = *self.bytes.get(self.byte_pos)?;
            let avail = usize::from(8 - self.bit_pos);
            let take = rem.min(avail);
            let shift = avail - take;
            let mask = ((1u16 << take) - 1) as u8;
            let chunk = (byte >> shift) & mask;
            val = (val << take) | u32::from(chunk);
            self.bit_pos += take as u8;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
            rem -= take;
        }
        Some(val)
    }

    pub(crate) fn bit(&mut self) -> Option<bool> {
        Some(self.ub(1)? != 0)
    }

    /// Read `nbits` (`0..=32`) signed two's-complement MSB-first bits.
    pub(crate) fn sb(&mut self, nbits: usize) -> Option<i32> {
        if nbits == 0 {
            return Some(0);
        }
        let raw = self.ub(nbits)?;
        if nbits == 32 {
            return Some(raw as i32);
        }
        let shift = 32 - nbits;
        Some(((raw << shift) as i32) >> shift)
    }

    /// Read `nbits` signed 16.16 fixed-point bits (`FB[n]`).
    pub(crate) fn fb(&mut self, nbits: usize) -> Option<f64> {
        Some(f64::from(self.sb(nbits)?) / 65536.0)
    }

    /// Read a byte-aligned `RECT` in twips `[xmin, xmax, ymin, ymax]`.
    pub(crate) fn rect_twips(&mut self) -> Option<[i32; 4]> {
        self.align();
        let nbits = self.ub(5)? as usize;
        let xmin = self.sb(nbits)?;
        let xmax = self.sb(nbits)?;
        let ymin = self.sb(nbits)?;
        let ymax = self.sb(nbits)?;
        self.align();
        Some([xmin, xmax, ymin, ymax])
    }

    /// Read a byte-aligned SWF `MATRIX` and return `[a, b, c, d, tx_px, ty_px]` in pixels (`tx/20, ty/20`).
    pub(crate) fn matrix_px(&mut self) -> Option<[f64; 6]> {
        self.align();
        let (a, d) = if self.bit()? {
            let n = self.ub(5)? as usize;
            (self.fb(n)?, self.fb(n)?)
        } else {
            (1.0, 1.0)
        };
        let (b, c) = if self.bit()? {
            let n = self.ub(5)? as usize;
            // SWF MATRIX stores RotateSkew0 (b) then RotateSkew1 (c).
            (self.fb(n)?, self.fb(n)?)
        } else {
            (0.0, 0.0)
        };
        let n_trans = self.ub(5)? as usize;
        let tx = f64::from(self.sb(n_trans)?) / 20.0;
        let ty = f64::from(self.sb(n_trans)?) / 20.0;
        self.align();
        Some([a, b, c, d, tx, ty])
    }

    pub(crate) fn rgb(&mut self) -> Option<Color> {
        let b = self.take(3)?;
        Some([b[0], b[1], b[2], 255])
    }

    pub(crate) fn rgba(&mut self) -> Option<Color> {
        let b = self.take(4)?;
        Some([b[0], b[1], b[2], b[3]])
    }

    /// Read `CXFORM` (`with_alpha = false`) or `CXFORMWITHALPHA` (`with_alpha = true`)
    /// returning `(ColorTransform, alpha_multiplier)`.
    pub(crate) fn cxform(&mut self, with_alpha: bool) -> Option<(ColorTransform, f64)> {
        self.align();
        let has_add = self.bit()?;
        let has_mult = self.bit()?;
        let nbits = self.ub(4)? as usize;
        let (rm, gm, bm, am) = if has_mult {
            let r = f64::from(self.sb(nbits)?) / 256.0;
            let g = f64::from(self.sb(nbits)?) / 256.0;
            let b = f64::from(self.sb(nbits)?) / 256.0;
            let a = if with_alpha {
                f64::from(self.sb(nbits)?) / 256.0
            } else {
                1.0
            };
            (r, g, b, a)
        } else {
            (1.0, 1.0, 1.0, 1.0)
        };
        let (ra, ga, ba, aa) = if has_add {
            let r = f64::from(self.sb(nbits)?).clamp(-255.0, 255.0);
            let g = f64::from(self.sb(nbits)?).clamp(-255.0, 255.0);
            let b = f64::from(self.sb(nbits)?).clamp(-255.0, 255.0);
            let a = if with_alpha {
                f64::from(self.sb(nbits)?).clamp(-255.0, 255.0)
            } else {
                0.0
            };
            (r, g, b, a)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        self.align();
        // When alpha offset is zero and alpha multiplier is in [0, 1], store alpha in Transform.alpha
        // and keep ColorTransform.multipliers[3] = 1.0 so simple alpha fades stay pure Transform.alpha.
        let (alpha_out, ct_am) = if aa == 0.0 && (0.0..=1.0).contains(&am) {
            (am, 1.0)
        } else {
            (1.0, am.clamp(-1e4, 1e4))
        };
        let ct = ColorTransform {
            multipliers: [
                rm.clamp(-1e4, 1e4),
                gm.clamp(-1e4, 1e4),
                bm.clamp(-1e4, 1e4),
                ct_am,
            ],
            offsets: [ra, ga, ba, aa],
        };
        Some((ct, alpha_out))
    }

    /// Read a null-terminated SWF string (`STRING`), decoded according to `swf_version`.
    pub(crate) fn c_string(&mut self, swf_version: u8) -> Option<(String, bool)> {
        self.align();
        let start = self.byte_pos;
        let rel_end = self.bytes.get(start..)?.iter().position(|&b| b == 0)?;
        let raw = &self.bytes[start..start + rel_end];
        self.byte_pos = start + rel_end + 1;
        Some(decode_swf_string(raw, swf_version))
    }

    /// Read a length-prefixed (`u8`) SWF string (`DefineFontInfo` / `DefineFont2` font name).
    pub(crate) fn len_u8_string(&mut self, swf_version: u8) -> Option<(String, bool)> {
        let len = usize::from(self.u8()?);
        let mut raw = self.take(len)?;
        if let Some(nul) = raw.iter().position(|&b| b == 0) {
            raw = &raw[..nul];
        }
        Some(decode_swf_string(raw, swf_version))
    }
}

/// Decode SWF string bytes, returning `(decoded, used_legacy_codepage)`.
pub(crate) fn decode_swf_string(raw: &[u8], swf_version: u8) -> (String, bool) {
    if let Ok(utf8) = std::str::from_utf8(raw) {
        return (utf8.to_owned(), false);
    }
    if swf_version <= 5 {
        let high_bytes: Vec<u8> = raw.iter().copied().filter(|&b| b >= 0x80).collect();
        let cyrillic = high_bytes
            .iter()
            .filter(|&&b| b >= 0xC0 || b == 0xA8 || b == 0xB8)
            .count();
        let enc = if !high_bytes.is_empty() && cyrillic * 2 >= high_bytes.len() {
            encoding_rs::WINDOWS_1251
        } else {
            encoding_rs::WINDOWS_1252
        };
        let (cow, _, _) = enc.decode(raw);
        return (cow.into_owned(), true);
    }
    (String::from_utf8_lossy(raw).into_owned(), false)
}
