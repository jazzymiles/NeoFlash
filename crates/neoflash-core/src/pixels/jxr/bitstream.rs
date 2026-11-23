use super::tables::*;
use super::*;

// ============================================================================
// MSB-First Unstuffed BitWriter & BitReader (ITU-T Rec. T.832)
// ============================================================================

pub(super) struct BitWriter {
    pub(super) bytes: Vec<u8>,
    pub(super) acc: u64,
    pub(super) bits: u8,
}

impl BitWriter {
    pub(super) fn with_capacity(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    pub(super) fn put_bits(&mut self, value: u32, count: u8) {
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
    pub(super) fn put_bit(&mut self, bit: bool) {
        self.put_bits(u32::from(bit), 1);
    }

    pub(super) fn fill_to_byte(&mut self) {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.put_bits(0, pad);
        }
    }

    pub(super) fn write_bytes_aligned(&mut self, slice: &[u8]) {
        self.fill_to_byte();
        self.bytes.extend_from_slice(slice);
    }

    pub(super) fn finish(mut self) -> Vec<u8> {
        self.fill_to_byte();
        self.bytes
    }
}

#[derive(Clone)]
pub(super) struct BitReader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) byte_pos: usize,
    pub(super) acc: u64,
    pub(super) bits: u8,
    pub(super) eof_pad_bytes: usize,
}

impl<'a> BitReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            acc: 0,
            bits: 0,
            eof_pad_bytes: 0,
        }
    }

    #[inline]
    pub(super) fn ensure_bits(&mut self, count: u8) -> Result<(), BitmapError> {
        while self.bits < count {
            if self.byte_pos < self.bytes.len() {
                self.acc = (self.acc << 8) | u64::from(self.bytes[self.byte_pos]);
                self.byte_pos += 1;
                self.bits += 8;
            } else if self.eof_pad_bytes < 8 {
                // Allow bounded zero-fill lookahead at packet tail, matching jxrlib's
                // zero-padded 2*PACKETLENGTH circular BitIOInfo lookahead window.
                self.acc <<= 8;
                self.eof_pad_bytes += 1;
                self.bits += 8;
            } else {
                return Err(error("unexpected EOF in JPEG-XR bitstream"));
            }
        }
        Ok(())
    }

    #[inline]
    pub(super) fn peek_bits(&mut self, count: u8) -> Result<u32, BitmapError> {
        debug_assert!(count <= 32);
        if count == 0 {
            return Ok(0);
        }
        self.ensure_bits(count)?;
        let shift = self.bits - count;
        let mask = if count == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << count) - 1
        };
        Ok(((self.acc >> shift) & mask) as u32)
    }

    #[inline]
    pub(super) fn flush_bits(&mut self, count: u8) -> Result<(), BitmapError> {
        debug_assert!(count <= 32);
        if count == 0 {
            return Ok(());
        }
        self.ensure_bits(count)?;
        self.bits -= count;
        if self.bits == 0 {
            self.acc = 0;
        } else {
            self.acc &= (1u64 << self.bits) - 1;
        }
        if self.eof_pad_bytes > 0 {
            let unconsumed_pad_bits = self.eof_pad_bytes * 8;
            if usize::from(self.bits) + 7 < unconsumed_pad_bits {
                return Err(error("unexpected EOF consuming JPEG-XR packet bits"));
            }
        }
        Ok(())
    }

    #[inline]
    pub(super) fn get_bits(&mut self, count: u8) -> Result<u32, BitmapError> {
        let val = self.peek_bits(count)?;
        self.flush_bits(count)?;
        Ok(val)
    }

    #[inline]
    pub(super) fn get_bool(&mut self) -> Result<bool, BitmapError> {
        Ok(self.get_bits(1)? != 0)
    }

    #[inline]
    pub(super) fn get_sign_mask(&mut self) -> Result<i32, BitmapError> {
        Ok(if self.get_bool()? { -1 } else { 0 })
    }

    /// Reads `c_bits` plus a sign bit when the magnitude is nonzero (`_getBit16s`).
    #[inline]
    pub(super) fn get_bits_signed(&mut self, c_bits: u8) -> Result<i32, BitmapError> {
        let peek = self.peek_bits(c_bits + 1)? as i32;
        let sign = peek & 1;
        let val = ((peek >> 1) ^ (-sign)) + sign;
        self.flush_bits(c_bits + u8::from(val != 0))?;
        Ok(val)
    }

    pub(super) fn flush_to_byte(&mut self) -> Result<(), BitmapError> {
        let rem = self.bits & 7;
        if rem > 0 {
            self.flush_bits(rem)?;
        }
        Ok(())
    }

    pub(super) fn byte_position(&self) -> usize {
        let total_loaded = self.byte_pos + self.eof_pad_bytes;
        total_loaded - usize::from(self.bits / 8)
    }

    /// Decodes a symbol from a `HUFFMAN_DECODE_ROOT_BITS = 5` lookup table (`getHuff`).
    #[inline]
    pub(super) fn get_huff(&mut self, dec_table: &[i16]) -> Result<usize, BitmapError> {
        let root = self.peek_bits(5)? as usize;
        let entry = i32::from(dec_table[root]);
        self.flush_bits(if entry < 0 { 5 } else { (entry & 7) as u8 })?;
        let mut sym = entry >> 3;
        if sym < 0 {
            sym = entry;
            loop {
                let bit = self.get_bits(1)? as i32;
                let idx = (sym + 32768 + bit) as usize;
                let next = dec_table
                    .get(idx)
                    .copied()
                    .ok_or_else(|| error("corrupt JPEG-XR Huffman prefix code"))?;
                sym = i32::from(next);
                if sym >= 0 {
                    break;
                }
            }
        }
        Ok(sym as usize)
    }

    /// Reads a T.832 variable-length word with optional `0xFD..=0xFF` escape (`GetVLWordEsc`).
    pub(super) fn get_vlw_esc(&mut self) -> Result<(usize, u8), BitmapError> {
        self.flush_to_byte()?;
        let first = self.get_bits(8)? as u8;
        if first >= 0xFD {
            Ok((0, first))
        } else if first < 0xFB {
            let lo = self.get_bits(8)? as usize;
            Ok(((usize::from(first) << 8) | lo, 0))
        } else if first == 0xFB {
            let hi = self.get_bits(16)? as usize;
            let lo = self.get_bits(16)? as usize;
            Ok(((hi << 16) | lo, 0))
        } else {
            // 0xFC: 8-byte unsigned integer
            let w0 = u64::from(self.get_bits(16)?);
            let w1 = u64::from(self.get_bits(16)?);
            let w2 = u64::from(self.get_bits(16)?);
            let w3 = u64::from(self.get_bits(16)?);
            let full = (w0 << 48) | (w1 << 32) | (w2 << 16) | w3;
            let sz =
                usize::try_from(full).map_err(|_| error("JPEG-XR 64-bit VLW_ESC size overflow"))?;
            Ok((sz, 0))
        }
    }
}

// ============================================================================
// Adaptive Huffman, Adaptive Model, CBP Model, Coding Context & Quantizer
// ============================================================================

#[derive(Clone)]
pub(super) struct AdaptiveHuffman {
    pub(super) n_symbols: usize,
    pub(super) table: &'static [i32],
    pub(super) delta: Option<&'static [i32]>,
    pub(super) delta1: Option<&'static [i32]>,
    pub(super) dec_table: &'static [i16],
    pub(super) table_index: i32,
    pub(super) initialized: bool,
    pub(super) discriminant: i32,
    pub(super) discriminant1: i32,
    pub(super) upper_bound: i32,
    pub(super) lower_bound: i32,
}

impl AdaptiveHuffman {
    pub(super) fn new(n_symbols: usize) -> Self {
        let mut ah = Self {
            n_symbols,
            table: &G4_CODE_TABLE,
            delta: None,
            delta1: None,
            dec_table: &G4_HUFF_LOOKUP,
            table_index: 0,
            initialized: false,
            discriminant: 0,
            discriminant1: 0,
            upper_bound: 0,
            lower_bound: 0,
        };
        ah.adapt_discriminant();
        ah
    }

    pub(super) fn adapt_discriminant(&mut self) {
        const THRESHOLD: i32 = 8;
        const MEMORY: i32 = 8;
        const MAX_TABLES: [i32; 13] = [0, 0, 0, 0, 1, 2, 4, 2, 2, 2, 0, 0, 5];
        const SECOND_DISC: [i32; 13] = [0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1];

        let sym = self.n_symbols;
        if !self.initialized {
            self.initialized = true;
            self.discriminant = 0;
            self.discriminant1 = 0;
            self.table_index = SECOND_DISC[sym];
        }

        let d_l = self.discriminant;
        let d_h = if SECOND_DISC[sym] != 0 {
            self.discriminant1
        } else {
            self.discriminant
        };

        let mut changed = false;
        if d_l < self.lower_bound {
            self.table_index -= 1;
            changed = true;
        } else if d_h > self.upper_bound {
            self.table_index += 1;
            changed = true;
        }
        if changed {
            self.discriminant = 0;
            self.discriminant1 = 0;
        }
        self.discriminant = self
            .discriminant
            .clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);
        self.discriminant1 = self
            .discriminant1
            .clamp(-THRESHOLD * MEMORY, THRESHOLD * MEMORY);

        let t = self.table_index.clamp(0, MAX_TABLES[sym] - 1) as usize;
        self.table_index = t as i32;
        self.lower_bound = if t == 0 { i32::MIN } else { -THRESHOLD };
        self.upper_bound = if self.table_index == MAX_TABLES[sym] - 1 {
            1 << 30
        } else {
            THRESHOLD
        };

        let stride = sym * 2 + 1;
        self.delta = None;
        self.delta1 = None;
        match sym {
            4 => {
                self.table = &G4_CODE_TABLE;
                self.dec_table = &G4_HUFF_LOOKUP;
            }
            5 => {
                self.table = &G5_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G5_DELTA_TABLE);
                self.dec_table = &G5_HUFF_LOOKUP[t];
            }
            6 => {
                self.table = &G6_CODE_TABLE[stride * t..stride * (t + 1)];
                let d1_idx = t - usize::from(t + 1 == MAX_TABLES[sym] as usize);
                let d0_idx = (t + usize::from(t == 0)) - 1;
                self.delta1 = Some(&G6_DELTA_TABLE[sym * d1_idx..sym * (d1_idx + 1)]);
                self.delta = Some(&G6_DELTA_TABLE[sym * d0_idx..sym * (d0_idx + 1)]);
                self.dec_table = &G6_HUFF_LOOKUP[t];
            }
            7 => {
                self.table = &G7_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G7_DELTA_TABLE);
                self.dec_table = &G7_HUFF_LOOKUP[t];
            }
            8 => {
                // Per jxrlib adapthuff.c line 494: symbol-8 table stays fixed at table 0
                self.table = &G8_CODE_TABLE[..stride];
                self.dec_table = &G8_HUFF_LOOKUP[0];
            }
            9 => {
                self.table = &G9_CODE_TABLE[stride * t..stride * (t + 1)];
                self.delta = Some(&G9_DELTA_TABLE);
                self.dec_table = &G9_HUFF_LOOKUP[t];
            }
            12 => {
                self.table = &G12_CODE_TABLE[stride * t..stride * (t + 1)];
                let d1_idx = t - usize::from(t + 1 == MAX_TABLES[sym] as usize);
                let d0_idx = (t + usize::from(t == 0)) - 1;
                self.delta1 = Some(&G12_DELTA_TABLE[sym * d1_idx..sym * (d1_idx + 1)]);
                self.delta = Some(&G12_DELTA_TABLE[sym * d0_idx..sym * (d0_idx + 1)]);
                self.dec_table = &G12_HUFF_LOOKUP[t];
            }
            _ => unreachable!("unsupported T.832 Huffman alphabet size"),
        }
    }

    #[inline]
    pub(super) fn put_symbol(&self, bw: &mut BitWriter, idx: usize) {
        let code = self.table[idx * 2 + 1] as u32;
        let len = self.table[idx * 2 + 2] as u8;
        bw.put_bits(code, len);
    }

    #[inline]
    pub(super) fn put_symbol_with_sign(&self, bw: &mut BitWriter, idx: usize, is_neg: bool) {
        let code = (self.table[idx * 2 + 1] as u32) * 2 + u32::from(is_neg);
        let len = (self.table[idx * 2 + 2] as u8) + 1;
        bw.put_bits(code, len);
    }

    #[inline]
    pub(super) fn add_delta(&mut self, idx: usize) {
        if let Some(d) = self.delta {
            self.discriminant += d[idx];
        }
    }

    #[inline]
    pub(super) fn add_both_deltas(&mut self, idx: usize) {
        if let Some(d) = self.delta {
            self.discriminant += d[idx];
        }
        if let Some(d1) = self.delta1 {
            self.discriminant1 += d1[idx];
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelBand {
    Dc = 0,
    Lp = 1,
    Ac = 2,
}

#[derive(Clone, Copy)]
pub(super) struct AdaptiveModel {
    pub(super) flc_state: [i32; 2],
    pub(super) flc_bits: [i32; 2],
    pub(super) band: ModelBand,
}

impl AdaptiveModel {
    pub(super) fn new(band: ModelBand) -> Self {
        let init_bits = match band {
            ModelBand::Dc => 8,
            ModelBand::Lp => 4,
            ModelBand::Ac => 0,
        };
        Self {
            flc_state: [0, 0],
            flc_bits: [init_bits, init_bits],
            band,
        }
    }

    pub(super) fn update_mb(
        &mut self,
        cf: ColorFormat,
        channels: usize,
        mut laplacian_mean: [i32; 2],
    ) {
        const WEIGHT0: [i32; 3] = [240, 12, 1];
        const WEIGHT1: [[i32; 4]; 3] = [[0, 240, 120, 80], [0, 12, 6, 4], [0, 16, 8, 5]];
        const WEIGHT2: [i32; 6] = [120, 37, 2, 120, 18, 1];

        let b_idx = self.band as usize;
        laplacian_mean[0] *= WEIGHT0[b_idx];
        match cf {
            ColorFormat::Yuv420 => {
                laplacian_mean[1] *= WEIGHT2[b_idx];
            }
            ColorFormat::Yuv422 => {
                laplacian_mean[1] *= WEIGHT2[3 + b_idx];
            }
            ColorFormat::Yuv444 | ColorFormat::YOnly => {
                laplacian_mean[1] *= WEIGHT1[b_idx][(channels - 1).min(3)];
                if self.band == ModelBand::Ac {
                    laplacian_mean[1] >>= 4;
                }
            }
        }

        for j in 0..2 {
            let lm = laplacian_mean[j];
            let mut ms = self.flc_state[j];
            let mut delta = (lm - MODELWEIGHT) >> 2;
            if delta <= -8 {
                delta = (delta + 4).max(-16);
                ms += delta;
                if ms < -8 {
                    if self.flc_bits[j] == 0 {
                        ms = -8;
                    } else {
                        ms = 0;
                        self.flc_bits[j] -= 1;
                    }
                }
            } else if delta >= 8 {
                delta = (delta - 4).min(15);
                ms += delta;
                if ms > 8 {
                    if self.flc_bits[j] >= 15 {
                        self.flc_bits[j] = 15;
                        ms = 8;
                    } else {
                        ms = 0;
                        self.flc_bits[j] += 1;
                    }
                }
            }
            self.flc_state[j] = ms;
            if cf == ColorFormat::YOnly {
                break;
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct CbpModel {
    pub(super) count0: [i32; 2],
    pub(super) count1: [i32; 2],
    pub(super) state: [i32; 2],
}

impl CbpModel {
    pub(super) fn new() -> Self {
        Self {
            count0: [-4, -4],
            count1: [4, 4],
            state: [0, 0],
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct AdaptiveScan {
    pub(super) total: u32,
    pub(super) scan: usize,
}

#[derive(Clone)]
pub(super) struct CodingContext {
    pub(super) adapt_huff_cbp_cy: AdaptiveHuffman,
    pub(super) adapt_huff_cbp_cy1: AdaptiveHuffman,
    pub(super) ah_expt: [AdaptiveHuffman; NUMVLCTABLES],
    pub(super) scan_lowpass: [AdaptiveScan; 16],
    pub(super) scan_horiz: [AdaptiveScan; 16],
    pub(super) scan_vert: [AdaptiveScan; 16],
    pub(super) model_ac: AdaptiveModel,
    pub(super) model_lp: AdaptiveModel,
    pub(super) model_dc: AdaptiveModel,
    pub(super) cbp_count_zero: i32,
    pub(super) cbp_count_max: i32,
    pub(super) cbp_model: CbpModel,
    pub(super) trim_flex_bits: i32,
}

impl CodingContext {
    pub(super) fn new(cf: ColorFormat, trim_flex_bits: u8) -> Self {
        let cbp_size = if cf == ColorFormat::YOnly { 5 } else { 9 };
        let mut ctx = Self {
            adapt_huff_cbp_cy: AdaptiveHuffman::new(cbp_size),
            adapt_huff_cbp_cy1: AdaptiveHuffman::new(5),
            ah_expt: std::array::from_fn(|k| AdaptiveHuffman::new(A_ALPHABET[k])),
            scan_lowpass: [AdaptiveScan::default(); 16],
            scan_horiz: [AdaptiveScan::default(); 16],
            scan_vert: [AdaptiveScan::default(); 16],
            model_ac: AdaptiveModel::new(ModelBand::Ac),
            model_lp: AdaptiveModel::new(ModelBand::Lp),
            model_dc: AdaptiveModel::new(ModelBand::Dc),
            cbp_count_zero: 1,
            cbp_count_max: 1,
            cbp_model: CbpModel::new(),
            trim_flex_bits: i32::from(trim_flex_bits.min(15)),
        };
        ctx.reset();
        ctx
    }

    pub(super) fn reset(&mut self) {
        self.adapt_huff_cbp_cy.initialized = false;
        self.adapt_huff_cbp_cy1.initialized = false;
        for ah in &mut self.ah_expt {
            ah.initialized = false;
        }
        self.adapt_lowpass();
        self.adapt_highpass();
        for i in 0..16 {
            self.scan_lowpass[i] = AdaptiveScan {
                total: 0,
                scan: ZIGZAG_INV_4X4_LOWPASS[i],
            };
            self.scan_horiz[i] = AdaptiveScan {
                total: 0,
                scan: DCT_INDEX[0][ZIGZAG_INV_4X4_H[i]],
            };
            self.scan_vert[i] = AdaptiveScan {
                total: 0,
                scan: DCT_INDEX[0][ZIGZAG_INV_4X4_V[i]],
            };
        }
        self.model_ac = AdaptiveModel::new(ModelBand::Ac);
        self.model_lp = AdaptiveModel::new(ModelBand::Lp);
        self.model_dc = AdaptiveModel::new(ModelBand::Dc);
        self.cbp_count_zero = 1;
        self.cbp_count_max = 1;
        self.cbp_model = CbpModel::new();
    }

    pub(super) fn adapt_lowpass(&mut self) {
        for kk in 0..(CONTEXTX + CTDC) {
            self.ah_expt[kk].adapt_discriminant();
        }
    }

    pub(super) fn adapt_highpass(&mut self) {
        self.adapt_huff_cbp_cy.adapt_discriminant();
        self.adapt_huff_cbp_cy1.adapt_discriminant();
        for kk in 0..CONTEXTX {
            self.ah_expt[kk + CONTEXTX + CTDC].adapt_discriminant();
        }
    }

    pub(super) fn reset_lowpass_scan_totals(&mut self) {
        let mut weight = 32u32;
        self.scan_lowpass[0].total = MAXTOTAL;
        for k in 1..16 {
            self.scan_lowpass[k].total = weight;
            weight -= 2;
        }
    }

    pub(super) fn reset_highpass_scan_totals(&mut self) {
        let mut weight = 32u32;
        self.scan_horiz[0].total = MAXTOTAL;
        self.scan_vert[0].total = MAXTOTAL;
        for k in 1..16 {
            self.scan_horiz[k].total = weight;
            self.scan_vert[k].total = weight;
            weight -= 2;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Quantizer {
    pub(super) qp: i32,
    pub(super) offset: i32,
    pub(super) man: u32,
    pub(super) exp: i32,
}

impl Quantizer {
    pub(super) fn from_index(index: u8, shift: i32, scaled_arith: bool, is_dc: bool) -> Self {
        let mut q = Self {
            qp: 1,
            offset: 0,
            man: 0,
            exp: 0,
        };
        if index == 0 {
            // Lossless QP
            q.qp = 1;
        } else if !scaled_arith {
            let ci_shift = -QPFRACBITS; // -2
            let (man, exp) = if index < 32 {
                (usize::from(index + 3) >> 2, ci_shift + 2)
            } else if index < 48 {
                (
                    (16 + usize::from(index & 0xF) + 1) >> 1,
                    i32::from(index >> 4) - 1 + 1 + ci_shift,
                )
            } else {
                (
                    16 + usize::from(index & 0xF),
                    i32::from(index >> 4) - 1 + ci_shift,
                )
            };
            q.qp = (man as i32) << exp;
            q.man = QP_RECIP_TABLE[man].0;
            q.exp = QP_RECIP_TABLE[man].1 + exp;
            q.offset = (q.qp * 3 + 1) >> 3;
        } else {
            let (man, exp) = if index < 16 {
                (usize::from(index), shift)
            } else {
                (
                    16 + usize::from(index & 0xF),
                    i32::from(index >> 4) - 1 + shift,
                )
            };
            q.qp = (man as i32) << exp;
            q.man = QP_RECIP_TABLE[man].0;
            q.exp = QP_RECIP_TABLE[man].1 + exp;
            q.offset = (q.qp * 3 + 1) >> 3;
        }
        if is_dc {
            q.offset = q.qp >> 1;
        }
        q
    }

    #[inline]
    pub(super) fn quantize(self, v: i32) -> i32 {
        if self.qp <= 1 && self.man == 0 && self.exp == 0 {
            return v;
        }
        let m = v >> 31;
        let abs_plus_off = ((v ^ m) - m + self.offset) as u32;
        let q = if self.man == 0 {
            (abs_plus_off >> (self.exp as u32)) as i32
        } else {
            let hi = ((u64::from(abs_plus_off) * u64::from(self.man)) >> 32) as u32;
            (hi >> (self.exp as u32)) as i32
        };
        (q ^ m) - m
    }
}

pub(super) fn format_quantizer_row(
    indices: &[u8; MAX_CHANNELS],
    ch_mode: u8,
    channels: usize,
    shifted_uv: bool,
    scaled_arith: bool,
    is_dc: bool,
) -> [Quantizer; MAX_CHANNELS] {
    let mut out = [Quantizer::default(); MAX_CHANNELS];
    for ch in 0..channels {
        let idx = if ch == 0 || ch_mode == 0 {
            indices[0]
        } else if ch_mode == 1 {
            indices[1]
        } else {
            indices[ch]
        };
        let shift = if ch > 0 && shifted_uv {
            SHIFTZERO - 1
        } else {
            SHIFTZERO
        };
        out[ch] = Quantizer::from_index(idx, shift, scaled_arith, is_dc);
    }
    out
}

/// Maps `JPEGXREncoderOptions.quantization` (`0..=100`, where `0` is lossless) onto T.832 `QP_INDEX`
/// values `[Y_DC/LP, U_DC/LP, V_DC/LP, Y_HP, U_HP, V_HP]` using `jxrlib`'s SSIM tables.
pub(super) fn map_flash_quantization_to_qps(quantization: u8, cf: ColorFormat) -> [u8; 6] {
    if quantization == 0 {
        return [0; 6];
    }
    let mut iq = (100.0_f32 - f32::from(quantization.min(100))) / 100.0_f32;
    let subsampled = matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422);
    if iq > 0.8 && !subsampled {
        iq = 0.8 + (iq - 0.8) * 1.5;
    }
    let scaled = 10.0 * iq;
    let max_row = if subsampled { 9usize } else { 10usize };
    let qi = (scaled.floor() as usize).min(max_row);
    let qf = (scaled - (qi as f32)).clamp(0.0, 1.0);

    let mut out = [0u8; 6];
    for c in 0..6 {
        let (lo, hi) = if subsampled {
            (
                f32::from(DPK_QPS_420[qi][c]),
                f32::from(DPK_QPS_420[qi + 1][c]),
            )
        } else {
            (f32::from(DPK_QPS_8[qi][c]), f32::from(DPK_QPS_8[qi + 1][c]))
        };
        let interp = (0.5 + lo * (1.0 - qf) + hi * qf) as u8;
        out[c] = interp.max(2);
    }
    out
}

#[derive(Clone, Copy, Default)]
pub(super) struct PredInfo {
    pub(super) qp_index: u8,
    pub(super) cbp: i32,
    pub(super) dc: i32,
    pub(super) ad: [i32; 6],
}

#[derive(Clone, Default)]
pub(super) struct MbInfo {
    pub(super) block_dc: [[i32; 16]; MAX_CHANNELS],
    pub(super) orientation: i32,
    pub(super) cbp: [i32; MAX_CHANNELS],
    pub(super) diff_cbp: [i32; MAX_CHANNELS],
    pub(super) q_index_lp: u8,
    pub(super) q_index_hp: u8,
}

#[derive(Clone)]
pub(super) struct TileQuantizers {
    pub(super) dc: [Quantizer; MAX_CHANNELS],
    pub(super) lp: Vec<[Quantizer; MAX_CHANNELS]>,
    pub(super) hp: Vec<[Quantizer; MAX_CHANNELS]>,
    pub(super) num_qp_lp: u8,
    pub(super) num_qp_hp: u8,
    pub(super) bits_lp: u8,
    pub(super) bits_hp: u8,
}

impl Default for TileQuantizers {
    fn default() -> Self {
        Self {
            dc: [Quantizer::default(); MAX_CHANNELS],
            lp: vec![[Quantizer::default(); MAX_CHANNELS]],
            hp: vec![[Quantizer::default(); MAX_CHANNELS]],
            num_qp_lp: 1,
            num_qp_hp: 1,
            bits_lp: 0,
            bits_hp: 0,
        }
    }
}

pub(super) fn dquant_bits(num_qp: u8) -> u8 {
    if num_qp < 2 {
        0
    } else if num_qp < 4 {
        1
    } else if num_qp < 6 {
        2
    } else if num_qp < 10 {
        3
    } else {
        4
    }
}
