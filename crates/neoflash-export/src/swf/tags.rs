//! SWF bit writer, FWS/CWS/ZWS container compression, bitmap/sound/font/text tag builders.

use super::*;

/// Bit-level and byte-level writer for SWF structures and tags.
#[derive(Default, Clone)]
pub(crate) struct SwfBitWriter {
    pub(crate) buf: Vec<u8>,
    pub(super) bit_buf: u8,
    pub(super) bit_count: u8,
}

impl SwfBitWriter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub(crate) fn align(&mut self) {
        if self.bit_count > 0 {
            self.buf.push(self.bit_buf);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    #[inline]
    pub(crate) fn bit(&mut self, b: bool) {
        if b {
            self.bit_buf |= 1 << (7 - self.bit_count);
        }
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.buf.push(self.bit_buf);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    pub(crate) fn ub(&mut self, n: usize, val: u32) {
        for i in (0..n).rev() {
            self.bit(((val >> i) & 1) != 0);
        }
    }

    pub(crate) fn sb(&mut self, n: usize, val: i32) {
        for i in (0..n).rev() {
            self.bit(((val >> i) & 1) != 0);
        }
    }

    #[inline]
    pub(crate) fn u8(&mut self, v: u8) {
        self.align();
        self.buf.push(v);
    }

    #[inline]
    pub(crate) fn u16(&mut self, v: u16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn i16(&mut self, v: i16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn u32(&mut self, v: u32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn f32(&mut self, v: f32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn bytes(&mut self, slice: &[u8]) {
        self.align();
        self.buf.extend_from_slice(slice);
    }

    pub(crate) fn c_string(&mut self, s: &str) {
        self.align();
        for &b in s.as_bytes() {
            if b != 0 {
                self.buf.push(b);
            }
        }
        self.buf.push(0);
    }

    /// Write an ABC / SWF variable-length `u32` (1 to 5 bytes, 7 bits per byte).
    pub(crate) fn u32_v(&mut self, mut v: u32) {
        self.align();
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                break;
            } else {
                self.buf.push(byte | 0x80);
            }
        }
    }

    pub(crate) fn rgba(&mut self, c: Color) {
        self.align();
        self.buf.extend_from_slice(&c);
    }

    pub(crate) fn rect_twips(&mut self, bounds: [i32; 4]) {
        self.align();
        let nbits = needed_sb_bits(&bounds).max(1);
        self.ub(5, nbits as u32);
        for v in bounds {
            self.sb(nbits, v);
        }
        self.align();
    }

    pub(crate) fn matrix_px(&mut self, m: [f64; 6]) {
        self.align();
        self.write_matrix_raw(m, 20.0);
        self.align();
    }

    pub(crate) fn write_matrix_raw(&mut self, m: [f64; 6], translate_scale: f64) {
        let finite = |v: f64, def: f64| if v.is_finite() { v } else { def };
        let to_fixed = |v: f64, scale: f64| -> i32 {
            (v * scale)
                .round()
                .clamp(f64::from(i32::MIN / 2), f64::from(i32::MAX / 2)) as i32
        };
        let a = finite(m[0], 1.0);
        let b = finite(m[1], 0.0);
        let c = finite(m[2], 0.0);
        let d = finite(m[3], 1.0);
        let tx = finite(m[4], 0.0);
        let ty = finite(m[5], 0.0);
        let has_scale = (a - 1.0).abs() > 1e-5 || (d - 1.0).abs() > 1e-5;
        self.bit(has_scale);
        if has_scale {
            let sx = to_fixed(a, 65536.0);
            let sy = to_fixed(d, 65536.0);
            let nbits = needed_sb_bits(&[sx, sy]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, sx);
            self.sb(nbits, sy);
        }
        let has_rotate = b.abs() > 1e-5 || c.abs() > 1e-5;
        self.bit(has_rotate);
        if has_rotate {
            let r0 = to_fixed(b, 65536.0);
            let r1 = to_fixed(c, 65536.0);
            let nbits = needed_sb_bits(&[r0, r1]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, r0);
            self.sb(nbits, r1);
        }
        let tx_t = to_fixed(tx, translate_scale);
        let ty_t = to_fixed(ty, translate_scale);
        if tx_t == 0 && ty_t == 0 {
            self.ub(5, 0);
        } else {
            let nbits = needed_sb_bits(&[tx_t, ty_t]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, tx_t);
            self.sb(nbits, ty_t);
        }
    }

    pub(crate) fn cxform_with_alpha(&mut self, ct: &ColorTransform, alpha: f64) {
        self.align();
        let eff_alpha_mul = if (ct.multipliers[3] - 1.0).abs() > 1e-5 {
            ct.multipliers[3]
        } else {
            alpha
        };
        let muls = [
            (ct.multipliers[0] * 256.0).round() as i32,
            (ct.multipliers[1] * 256.0).round() as i32,
            (ct.multipliers[2] * 256.0).round() as i32,
            (eff_alpha_mul * 256.0).round() as i32,
        ];
        let adds = [
            ct.offsets[0].round() as i32,
            ct.offsets[1].round() as i32,
            ct.offsets[2].round() as i32,
            ct.offsets[3].round() as i32,
        ];
        let has_add = adds.iter().any(|&v| v != 0);
        let has_mult = muls.iter().any(|&v| v != 256);
        self.bit(has_add);
        self.bit(has_mult);
        let mut all_vals = Vec::new();
        if has_mult {
            all_vals.extend_from_slice(&muls);
        }
        if has_add {
            all_vals.extend_from_slice(&adds);
        }
        let nbits = needed_sb_bits(&all_vals).clamp(1, 15);
        self.ub(4, nbits as u32);
        if has_mult {
            for v in muls {
                self.sb(nbits, v);
            }
        }
        if has_add {
            for v in adds {
                self.sb(nbits, v);
            }
        }
        self.align();
    }

    pub(crate) fn write_tag(&mut self, tag_code: u16, payload: &[u8]) {
        self.align();
        // Tags that Flash requires long header when >= 63 bytes or for certain bitmap/sound tags:
        let force_long = matches!(tag_code, 20 | 36 | 14 | 19 | 35 | 21 | 6);
        if payload.len() < 0x3f && !force_long {
            let hdr = (tag_code << 6) | (payload.len() as u16);
            self.u16(hdr);
        } else {
            let hdr = (tag_code << 6) | 0x3f;
            self.u16(hdr);
            self.u32(payload.len() as u32);
        }
        self.bytes(payload);
    }
}

pub(crate) fn needed_sb_bits(vals: &[i32]) -> usize {
    let mut max_bits = 1usize;
    for &v in vals {
        let bits = if v == 0 {
            1
        } else if v > 0 {
            (32 - v.leading_zeros() + 1) as usize
        } else if v == -1 {
            1
        } else {
            (32 - (!v).leading_zeros() + 1) as usize
        };
        if bits > max_bits {
            max_bits = bits;
        }
    }
    max_bits.min(31)
}

pub(crate) fn needed_ub_bits(val: u32) -> usize {
    if val == 0 {
        0
    } else {
        (32 - val.leading_zeros()) as usize
    }
}

pub(super) struct LzmaRangeEncoder {
    pub(super) out: Vec<u8>,
    pub(super) low: u64,
    pub(super) range: u32,
    pub(super) cache_size: u32,
    pub(super) cache: u8,
}

impl LzmaRangeEncoder {
    pub(super) fn new() -> Self {
        Self {
            out: Vec::new(),
            low: 0,
            range: 0xFFFF_FFFF,
            cache_size: 1,
            cache: 0,
        }
    }

    pub(super) fn shift_low(&mut self) {
        let low_hi = (self.low >> 32) as u32;
        if (self.low as u32) < 0xFF00_0000 || low_hi != 0 {
            let mut temp = self.cache;
            loop {
                self.out.push(temp.wrapping_add(low_hi as u8));
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low as u32) >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }

    pub(super) fn encode_bit(&mut self, prob: &mut u16, bit: bool) {
        let bound = (self.range >> 11) * u32::from(*prob);
        if !bit {
            self.range = bound;
            *prob += (0x800_u16 - *prob) >> 5;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob -= *prob >> 5;
        }
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            self.shift_low();
        }
    }

    pub(super) fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

/// Encode `uncompressed` bytes as a valid LZMA1 literal stream with properties `lc=3, lp=0, pb=2`
/// (`props = [0x5d, 0x00, 0x00, 0x01, 0x00]`) matching `neoflash-import/src/swf/lzma.rs`.
pub(super) fn compress_zws_lzma(uncompressed: &[u8]) -> ([u8; 5], Vec<u8>) {
    let props = [0x5d, 0x00, 0x00, 0x01, 0x00]; // lc=3, lp=0, pb=2, dict=65536
    let mut rc = LzmaRangeEncoder::new();
    let mut is_match = [0x400_u16; 192];
    let mut literal_probs = [[0x400_u16; 0x300]; 8]; // 1 << (lc + lp) = 8

    for (pos, &byte) in uncompressed.iter().enumerate() {
        let pos_state = pos & 3; // pb = 2 -> pb_mask = 3
        rc.encode_bit(&mut is_match[pos_state], false);
        let prev_byte = if pos > 0 { uncompressed[pos - 1] } else { 0 };
        let lit_state = usize::from(prev_byte >> 5);
        let probs = &mut literal_probs[lit_state];
        let mut symbol = 1usize;
        for bit_idx in (0..8).rev() {
            let bit = ((byte >> bit_idx) & 1) != 0;
            rc.encode_bit(&mut probs[symbol], bit);
            symbol = (symbol << 1) ^ usize::from(bit);
        }
    }

    (props, rc.finish())
}

pub(super) fn wrap_swf_container(
    body: &[u8],
    profile: SwfPublishProfile,
    compression: SwfCompression,
) -> Result<Vec<u8>, ExportError> {
    let declared_len = body
        .len()
        .checked_add(8)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            ExportError::Options(format!(
                "SWF output is {} bytes before compression; the SWF FileLength field supports at most {} bytes",
                body.len().saturating_add(8),
                u32::MAX
            ))
        })?;
    match compression {
        SwfCompression::UncompressedFws => {
            let mut out = Vec::with_capacity(8 + body.len());
            out.extend_from_slice(b"FWS");
            out.push(profile.swf_version());
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(body);
            Ok(out)
        }
        SwfCompression::DeflateCws => {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(body)?;
            let compressed = enc.finish()?;
            let mut out = Vec::with_capacity(8 + compressed.len());
            out.extend_from_slice(b"CWS");
            out.push(profile.swf_version());
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&compressed);
            Ok(out)
        }
        SwfCompression::LzmaZws => {
            let (props, compressed) = compress_zws_lzma(body);
            let compressed_len = u32::try_from(compressed.len()).map_err(|_| {
                ExportError::Options(format!(
                    "Compressed ZWS payload is {} bytes; its SWF length field supports at most {} bytes",
                    compressed.len(),
                    u32::MAX
                ))
            })?;
            let mut out = Vec::with_capacity(17 + compressed.len());
            out.extend_from_slice(b"ZWS");
            // SWF spec requires SWF version >= 13 for ZWS container.
            out.push(profile.swf_version().max(13));
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&compressed_len.to_le_bytes());
            out.extend_from_slice(&props);
            out.extend_from_slice(&compressed);
            Ok(out)
        }
    }
}

/// Build a `DefineShape3` (`tag 32`) payload wrapping a `DefineBitsLossless2` bitmap (`0x41`
/// smoothed clipped bitmap or `0x43` non-smoothed clipped bitmap) over `(0, 0)..(width, height)`
/// so real SWF players and `neoflash-import` both render and round-trip placed bitmaps cleanly.
pub(super) fn build_define_bitmap_quad_shape_payload(
    shape_id: u16,
    bitmap_id: u16,
    bitmap_w: u32,
    bitmap_h: u32,
    display_w: u32,
    display_h: u32,
    smoothing: bool,
) -> Vec<u8> {
    let dw = display_w.max(1);
    let dh = display_h.max(1);
    let w_twips = (i64::from(dw) * 20).clamp(20, i64::from(i32::MAX)) as i32;
    let h_twips = (i64::from(dh) * 20).clamp(20, i64::from(i32::MAX)) as i32;
    let sx = 20.0 * f64::from(dw) / f64::from(bitmap_w.max(1));
    let sy = 20.0 * f64::from(dh) / f64::from(bitmap_h.max(1));

    let mut w = SwfBitWriter::new();
    w.u16(shape_id);
    w.rect_twips([0, w_twips, 0, h_twips]);

    // FillStyleArray: 1 ClippedBitmap fill style (0x41 smoothed, 0x43 non-smoothed)
    w.u8(1);
    w.u8(if smoothing { 0x41 } else { 0x43 });
    w.u16(bitmap_id);
    w.matrix_px([sx, 0.0, 0.0, sy, 0.0, 0.0]);

    // LineStyleArray: 0 line styles
    w.u8(0);

    let edges = [
        SwfEdge {
            fill_1: 1,
            line: 0,
            start: [0, 0],
            control: None,
            end: [w_twips, 0],
        },
        SwfEdge {
            fill_1: 1,
            line: 0,
            start: [w_twips, 0],
            control: None,
            end: [w_twips, h_twips],
        },
        SwfEdge {
            fill_1: 1,
            line: 0,
            start: [w_twips, h_twips],
            control: None,
            end: [0, h_twips],
        },
        SwfEdge {
            fill_1: 1,
            line: 0,
            start: [0, h_twips],
            control: None,
            end: [0, 0],
        },
    ];
    write_shape_edges_records(&mut w, &edges, 1, 0, None);
    w.buf
}

pub(super) fn embeds_original_jpeg(resource: &neoflash_core::ResourceUnit) -> bool {
    resource.media_type.eq_ignore_ascii_case("image/jpeg")
        && !resource
            .bitmap_export
            .is_some_and(|s| s.compression == neoflash_core::BitmapCompression::LosslessPng)
}

/// Encodes decoded pixels as a premultiplied ARGB DefineBitsLossless2 dictionary character.
pub(super) fn build_define_bits_lossless2_payload(
    bitmap_id: u16,
    image: &neoflash_core::DecodedImage,
) -> Option<Vec<u8>> {
    let (width, height) = (image.width, image.height);
    let rgba = &image.rgba;
    // DefineBitsLossless2 format 5: premultiplied ARGB (`[a, r_prem, g_prem, b_prem]`).
    let mut argb_prem = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0 {
        let r = u16::from(px[0]);
        let g = u16::from(px[1]);
        let b = u16::from(px[2]);
        let a = u16::from(px[3]);
        let rp = ((r * a + 127) / 255) as u8;
        let gp = ((g * a + 127) / 255) as u8;
        let bp = ((b * a + 127) / 255) as u8;
        argb_prem.extend_from_slice(&[a as u8, rp, gp, bp]);
    }

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&argb_prem).ok()?;
    let compressed = enc.finish().ok()?;

    let mut w = SwfBitWriter::new();
    w.u16(bitmap_id);
    w.u8(5); // format 5 = 32-bit ARGB
    w.u16(width as u16);
    w.u16(height as u16);
    w.bytes(&compressed);
    Some(w.buf)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SwfSoundInfo {
    pub(super) sound_format: u8,
    pub(super) sample_rate: u32,
    pub(super) rate_code: u8,
    pub(super) is_16_bit: bool,
    pub(super) stereo: bool,
    pub(super) sample_count: u32,
    pub(super) data_start: usize,
    pub(super) data_end: usize,
    pub(super) mp3: bool,
}

pub(super) fn swf_sound_rate_code(sample_rate: u32) -> Option<u8> {
    match sample_rate {
        5_512 => Some(0),
        11_025 => Some(1),
        22_050 => Some(2),
        44_100 => Some(3),
        _ => None,
    }
}

pub(super) fn analyze_swf_wav(raw: &[u8]) -> Result<SwfSoundInfo, String> {
    if raw.len() < 12 || &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
        return Err("the resource is not a complete RIFF/WAVE file".to_owned());
    }
    let riff_size = u32::from_le_bytes(raw[4..8].try_into().expect("four-byte RIFF size"));
    let riff_end = usize::try_from(riff_size)
        .ok()
        .and_then(|size| size.checked_add(8))
        .filter(|end| *end >= 12 && *end <= raw.len())
        .ok_or_else(|| "the RIFF size exceeds the available resource bytes".to_owned())?;

    let mut position = 12usize;
    let mut format = None;
    let mut data_range = None;
    while position + 8 <= riff_end {
        let chunk_id = &raw[position..position + 4];
        let chunk_len = u32::from_le_bytes(
            raw[position + 4..position + 8]
                .try_into()
                .expect("four-byte WAV chunk size"),
        ) as usize;
        let start = position + 8;
        let end = start
            .checked_add(chunk_len)
            .filter(|end| *end <= riff_end)
            .ok_or_else(|| "a WAV chunk is truncated".to_owned())?;
        if chunk_id == b"fmt " {
            if chunk_len < 16 {
                return Err("the WAV fmt chunk is shorter than 16 bytes".to_owned());
            }
            if format.is_some() {
                return Err("the WAV contains more than one fmt chunk".to_owned());
            }
            format = Some((
                u16::from_le_bytes(raw[start..start + 2].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 2..start + 4].try_into().unwrap()),
                u32::from_le_bytes(raw[start + 4..start + 8].try_into().unwrap()),
                u32::from_le_bytes(raw[start + 8..start + 12].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 12..start + 14].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 14..start + 16].try_into().unwrap()),
            ));
        } else if chunk_id == b"data" {
            if data_range.is_some() {
                return Err("the WAV contains more than one data chunk".to_owned());
            }
            data_range = Some((start, end));
        }
        position = end
            .checked_add(chunk_len & 1)
            .filter(|next| *next <= riff_end)
            .ok_or_else(|| "the WAV chunk padding is truncated".to_owned())?;
    }

    let (encoding, channels, sample_rate, byte_rate, block_align, bits_per_sample) =
        format.ok_or_else(|| "the WAV has no fmt chunk".to_owned())?;
    let (data_start, data_end) =
        data_range.ok_or_else(|| "the WAV has no data chunk".to_owned())?;
    if encoding != 1 {
        return Err(format!(
            "WAV format code {encoding} is not integer PCM; no WAV transcoder runs in SWF publish"
        ));
    }
    if !matches!(channels, 1 | 2) {
        return Err(format!(
            "WAV has {channels} channels; SWF DefineSound supports only mono or stereo"
        ));
    }
    let rate_code = swf_sound_rate_code(sample_rate).ok_or_else(|| {
        format!(
            "WAV sample rate {sample_rate} Hz is not one of 5512, 11025, 22050 or 44100 Hz and is not resampled by SWF publish"
        )
    })?;
    if !matches!(bits_per_sample, 8 | 16) {
        return Err(format!(
            "WAV uses {bits_per_sample}-bit samples; SWF publish supports only 8-bit or 16-bit PCM without transcoding"
        ));
    }
    let expected_align = channels
        .checked_mul(bits_per_sample / 8)
        .ok_or_else(|| "WAV block alignment overflows".to_owned())?;
    if block_align != expected_align {
        return Err(format!(
            "WAV blockAlign is {block_align}, expected {expected_align} for its channel/sample layout"
        ));
    }
    let expected_byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| "WAV byte rate overflows".to_owned())?;
    if byte_rate != expected_byte_rate {
        return Err(format!(
            "WAV byteRate is {byte_rate}, expected {expected_byte_rate}"
        ));
    }
    let data_len = data_end - data_start;
    if data_len == 0 || data_len % usize::from(block_align) != 0 {
        return Err("WAV PCM data is empty or ends inside a sample frame".to_owned());
    }
    let sample_count = u32::try_from(data_len / usize::from(block_align))
        .map_err(|_| "WAV sample count exceeds the SWF UI32 field".to_owned())?;
    Ok(SwfSoundInfo {
        sound_format: 3,
        sample_rate,
        rate_code,
        is_16_bit: bits_per_sample == 16,
        stereo: channels == 2,
        sample_count,
        data_start,
        data_end,
        mp3: false,
    })
}

pub(super) fn analyze_swf_mp3(raw: &[u8]) -> Result<SwfSoundInfo, String> {
    pub(super) const BITRATES_V1_L3: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    pub(super) const BITRATES_V2_L3: [u32; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    let mut start = 0usize;
    if raw.starts_with(b"ID3") {
        if raw.len() < 10 || raw[6..10].iter().any(|byte| byte & 0x80 != 0) {
            return Err("the MP3 has an invalid ID3v2 header".to_owned());
        }
        let tag_size = raw[6..10]
            .iter()
            .fold(0usize, |size, byte| (size << 7) | usize::from(*byte));
        let footer_len = if raw[5] & 0x10 != 0 { 10 } else { 0 };
        start = 10usize
            .checked_add(tag_size)
            .and_then(|size| size.checked_add(footer_len))
            .filter(|size| *size <= raw.len())
            .ok_or_else(|| "the MP3 ID3v2 tag is truncated".to_owned())?;
    }
    let end = if raw.len() >= start + 128 && &raw[raw.len() - 128..raw.len() - 125] == b"TAG" {
        raw.len() - 128
    } else {
        raw.len()
    };
    if start >= end {
        return Err("the MP3 contains no MPEG audio frames".to_owned());
    }

    let mut position = start;
    let mut sample_rate = None;
    let mut stereo = None;
    let mut sample_count = 0u32;
    let mut frame_count = 0u32;
    while position < end {
        if end - position < 4 || raw[position] != 0xff || raw[position + 1] & 0xe0 != 0xe0 {
            return Err(format!(
                "MP3 data at byte {position} is not a contiguous MPEG Layer III frame"
            ));
        }
        let b1 = raw[position + 1];
        let b2 = raw[position + 2];
        let b3 = raw[position + 3];
        let version_bits = (b1 >> 3) & 0x03;
        let layer_bits = (b1 >> 1) & 0x03;
        let bitrate_index = usize::from((b2 >> 4) & 0x0f);
        let sample_rate_index = usize::from((b2 >> 2) & 0x03);
        let padding = usize::from((b2 >> 1) & 0x01);
        if version_bits == 1 || layer_bits != 1 || bitrate_index == 0 || bitrate_index == 15 {
            return Err(format!(
                "MP3 frame {frame_count} is not a supported fixed-header MPEG Layer III frame"
            ));
        }
        let rate = match (version_bits, sample_rate_index) {
            (3, 0) => 44_100,
            (3, 1) => 48_000,
            (3, 2) => 32_000,
            (2, 0) => 22_050,
            (2, 1) => 24_000,
            (2, 2) => 16_000,
            (0, 0) => 11_025,
            (0, 1) => 12_000,
            (0, 2) => 8_000,
            _ => {
                return Err(format!(
                    "MP3 frame {frame_count} has a reserved sample-rate field"
                ));
            }
        };
        let rate_code = swf_sound_rate_code(rate).ok_or_else(|| {
            format!(
                "MP3 sample rate {rate} Hz is not one of the rates representable by SWF DefineSound and is not resampled"
            )
        })?;
        let bitrate_kbps = if version_bits == 3 {
            BITRATES_V1_L3[bitrate_index]
        } else {
            BITRATES_V2_L3[bitrate_index]
        };
        let samples_in_frame = if version_bits == 3 { 1152u32 } else { 576u32 };
        let coefficient = if version_bits == 3 { 144usize } else { 72usize };
        let frame_len = coefficient
            .checked_mul(usize::try_from(bitrate_kbps).unwrap())
            .and_then(|value| value.checked_mul(1000))
            .map(|value| value / usize::try_from(rate).unwrap() + padding)
            .filter(|length| *length >= 4)
            .ok_or_else(|| "MP3 frame length overflows".to_owned())?;
        if position
            .checked_add(frame_len)
            .is_none_or(|next| next > end)
        {
            return Err(format!("MP3 frame {frame_count} is truncated"));
        }
        let frame_stereo = b3 >> 6 != 3;
        if sample_rate.is_some_and(|previous| previous != rate)
            || stereo.is_some_and(|previous| previous != frame_stereo)
        {
            return Err(
                "MP3 changes sample rate or channel layout between frames; DefineSound has one global layout"
                    .to_owned(),
            );
        }
        sample_rate = Some(rate);
        stereo = Some(frame_stereo);
        sample_count = sample_count
            .checked_add(samples_in_frame)
            .ok_or_else(|| "MP3 sample count exceeds the SWF UI32 field".to_owned())?;
        frame_count += 1;
        position += frame_len;
        let _ = rate_code;
    }
    let sample_rate = sample_rate.ok_or_else(|| "the MP3 has no audio frames".to_owned())?;
    Ok(SwfSoundInfo {
        sound_format: 2,
        sample_rate,
        rate_code: swf_sound_rate_code(sample_rate).expect("validated MP3 rate"),
        is_16_bit: true,
        stereo: stereo.unwrap_or(false),
        sample_count,
        data_start: start,
        data_end: end,
        mp3: true,
    })
}

pub(super) fn analyze_swf_sound_resource(
    media_type: &str,
    raw: &[u8],
) -> Result<SwfSoundInfo, String> {
    if media_type.eq_ignore_ascii_case("audio/wav")
        || media_type.eq_ignore_ascii_case("audio/x-wav")
        || media_type.eq_ignore_ascii_case("audio/wave")
    {
        analyze_swf_wav(raw)
    } else if media_type.eq_ignore_ascii_case("audio/mpeg")
        || media_type.eq_ignore_ascii_case("audio/mp3")
    {
        analyze_swf_mp3(raw)
    } else {
        Err(format!("audio codec '{media_type}' is not supported"))
    }
}

/// Build a `DefineSound` (tag 14) payload after proving the source layout is representable.
pub(super) fn build_define_sound_payload(
    sound_id: u16,
    media_type: &str,
    raw: &[u8],
) -> Result<Vec<u8>, String> {
    let info = analyze_swf_sound_resource(media_type, raw)?;
    let flags = (info.sound_format << 4)
        | (info.rate_code << 2)
        | (u8::from(info.is_16_bit) << 1)
        | u8::from(info.stereo);
    let mut writer = SwfBitWriter::new();
    writer.u16(sound_id);
    writer.u8(flags);
    writer.u32(info.sample_count);
    if info.mp3 {
        writer.i16(0); // seek_samples = 0; no encoder delay is introduced by this passthrough.
    }
    writer.bytes(&raw[info.data_start..info.data_end]);
    Ok(writer.buf)
}

/// Build a `StartSound` (tag 15) payload for a `FrameSound`.
pub(super) fn build_start_sound_payload(
    sound_id: u16,
    sound: &FrameSound,
    duration_44: u64,
) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(sound_id);
    let sync_stop = sound.sync == SoundSync::Stop;
    let sync_no_multiple = sound.sync == SoundSync::Start;
    let loop_count: u16 = if sound.loop_forever {
        0x7fff
    } else {
        sound.loops.clamp(1, 32767) as u16
    };
    let has_loops = !sync_stop && loop_count > 1;
    let in_samples = if !sync_stop {
        sound
            .in_point
            .filter(|t| t.is_finite() && *t > 0.0)
            .map(|sec| (sec * 44_100.0).round().clamp(0.0, f64::from(u32::MAX)) as u32)
    } else {
        None
    };
    let out_samples = if !sync_stop {
        sound
            .out_point
            .filter(|t| t.is_finite() && *t > 0.0)
            .map(|sec| (sec * 44_100.0).round().clamp(0.0, f64::from(u32::MAX)) as u32)
    } else {
        None
    };
    let max_mark44 = duration_44.max(1).min(u64::from(u32::MAX)) as u32;
    let start_mark44 = in_samples.unwrap_or(0).min(max_mark44);
    let end_mark44 = out_samples
        .map_or(max_mark44, |out| out.min(max_mark44))
        .max(start_mark44);
    let span_mark44 = u64::from(end_mark44.saturating_sub(start_mark44));
    let env_points: Vec<(u32, u16, u16)> = if sound.effect == SoundEffect::Custom
        || sound.envelope.as_ref().is_some_and(|pts| !pts.is_empty())
    {
        encode_sound_envelope_points(sound, span_mark44.max(1))
            .into_iter()
            .map(|(m, l, r)| {
                let rel = if span_mark44 == 0 { 0 } else { m };
                let pos44 = u64::from(start_mark44)
                    .saturating_add(rel)
                    .min(u64::from(max_mark44)) as u32;
                (
                    pos44,
                    l.min(u32::from(u16::MAX)) as u16,
                    r.min(u32::from(u16::MAX)) as u16,
                )
            })
            .collect()
    } else {
        match sound.effect {
            SoundEffect::None | SoundEffect::Custom => Vec::new(),
            SoundEffect::LeftChannel => vec![(start_mark44, 32768, 0)],
            SoundEffect::RightChannel => vec![(start_mark44, 0, 32768)],
            SoundEffect::FadeLeftToRight => vec![(start_mark44, 32768, 0), (end_mark44, 0, 32768)],
            SoundEffect::FadeRightToLeft => vec![(start_mark44, 0, 32768), (end_mark44, 32768, 0)],
            SoundEffect::FadeIn => vec![(start_mark44, 0, 0), (end_mark44, 32768, 32768)],
            SoundEffect::FadeOut => vec![(start_mark44, 32768, 32768), (end_mark44, 0, 0)],
        }
    };
    let has_envelope = !sync_stop && !env_points.is_empty();
    let has_in_point = in_samples.is_some();
    let has_out_point = out_samples.is_some();
    let flags: u8 = (if sync_stop { 0x20 } else { 0 })
        | (if sync_no_multiple { 0x10 } else { 0 })
        | (if has_envelope { 0x08 } else { 0 })
        | (if has_loops { 0x04 } else { 0 })
        | (if has_out_point { 0x02 } else { 0 })
        | (if has_in_point { 0x01 } else { 0 });
    w.u8(flags);
    if let Some(in_pt) = in_samples {
        w.u32(in_pt);
    }
    if let Some(out_pt) = out_samples {
        w.u32(out_pt);
    }
    if has_loops {
        w.u16(loop_count);
    }
    if has_envelope {
        let count = env_points.len().min(usize::from(u8::MAX));
        w.u8(count as u8);
        for (pos44, left, right) in env_points.into_iter().take(count) {
            w.u32(pos44);
            w.u16(left);
            w.u16(right);
        }
    }
    w.buf
}

/// Build a `DefineFont3` (tag 75) payload defining font family, bold, and italic attributes.
pub(super) fn build_define_font3_payload(
    font_id: u16,
    name: &str,
    bold: bool,
    italic: bool,
) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(font_id);
    // flags: has_layout(0)=0, shift_jis=0, small_text=0, ansi=0, wide_offsets=0,
    // wide_codes(0x04)=1, italic(0x02), bold(0x01)
    let flags: u8 = 0x04 | (if italic { 0x02 } else { 0 }) | (if bold { 0x01 } else { 0 });
    w.u8(flags);
    w.u8(1); // language_code = 1 (Latin)
    let name_bytes = name.as_bytes();
    let len = name_bytes.len().min(255);
    w.u8(len as u8);
    w.bytes(&name_bytes[..len]);
    w.u16(0); // num_glyphs = 0 (device/system metrics)
    w.buf
}

/// Build a `DefineEditText` (tag 37) payload for an `ElementKind::TextBox`.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_define_edit_text_payload(
    text_id: u16,
    font_id: u16,
    text: &str,
    width: f64,
    height: f64,
    format: &neoflash_core::TextFormat,
    word_wrap: bool,
    multiline: bool,
    alpha: f64,
) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(text_id);
    let w_twips = (width.max(4.0) * 20.0).round() as i32;
    let h_twips = (height.max(4.0) * 20.0).round() as i32;
    w.rect_twips([0, w_twips, 0, h_twips]);

    // flags0: has_text(0x80) | word_wrap(0x40) | multiline(0x20) | read_only(0x08) | has_text_color(0x04) | has_font(0x01)
    let flags0: u8 = 0x80
        | (if word_wrap { 0x40 } else { 0 })
        | (if multiline || text.contains('\n') {
            0x20
        } else {
            0
        })
        | 0x08
        | 0x04
        | 0x01;
    // flags1: has_layout(0x20) | was_static(0x04)
    let flags1: u8 = 0x20 | 0x04;
    w.u8(flags0);
    w.u8(flags1);

    w.u16(font_id);
    let font_height_twips = (format.size.clamp(1.0, 1000.0) * 20.0).round() as u16;
    w.u16(font_height_twips.max(20));

    let r = ((format.color >> 16) & 0xff) as u8;
    let g = ((format.color >> 8) & 0xff) as u8;
    let b = (format.color & 0xff) as u8;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    w.rgba([r, g, b, a]);

    let align_byte: u8 = match format.align {
        TextAlign::Left => 0,
        TextAlign::Right => 1,
        TextAlign::Center => 2,
    };
    w.u8(align_byte);
    let to_u16_twips = |px: f64| -> u16 { (px.max(0.0) * 20.0).round().clamp(0.0, 65535.0) as u16 };
    let to_i16_twips = |px: f64| -> i16 {
        (px * 20.0)
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
    };
    w.u16(to_u16_twips(format.left_margin));
    w.u16(to_u16_twips(format.right_margin));
    w.i16(to_i16_twips(format.indent));
    w.i16(to_i16_twips(format.leading));

    w.c_string(""); // variable_name
    w.c_string(text); // initial_text
    w.buf
}
