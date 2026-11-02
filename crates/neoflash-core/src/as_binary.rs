//! Bounded AS3 ByteArray codecs on the shared Rust core.
//! Compression/multibyte control flow adapted from Ruffle
//! 89a7049965f3090e706265670df309a5128233ac.
//! Copyright (c) 2018-2022 Ruffle LLC <ruffle@ruffle.rs> and contributors; MIT.
//! The pure-Rust flate2 backend supplies zlib/DEFLATE; no ActionScript engine.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use encoding_rs::{EncoderResult, Encoding, REPLACEMENT, UTF_8, UTF_16BE, UTF_16LE};
use flate2::{Compression, Decompress, FlushDecompress, Status};
use std::io::Read;

pub const MAX_BINARY_BYTES: usize = 256 * 1024 * 1024;

/// Preserve NeoFlash's existing strict UTF packet profile while moving its
/// implementation out of the browser host. Multibyte uses its separate lossy
/// legacy profile; these operations never silently replace lone surrogates.
#[derive(Debug, thiserror::Error)]
pub enum BinaryUtf8Error {
    #[error("ByteArray UTF string exceeds the supported byte length.")]
    Length,
    #[error("ByteArray UTF input contains an unsupported unpaired surrogate.")]
    Surrogate,
    #[error(
        "ByteArray UTF input is not valid UTF-8; legacy replacement/code-page behavior is not implemented."
    )]
    InvalidUtf8,
}

pub fn binary_encode_utf8(units: &[u16], limit: usize) -> Result<Vec<u8>, BinaryUtf8Error> {
    if limit == 0 || limit > MAX_BINARY_BYTES || units.len() > limit {
        return Err(BinaryUtf8Error::Length);
    }
    let mut result = Vec::new();
    let mut buffer = [0; 4];
    for decoded in char::decode_utf16(units.iter().copied()) {
        let ch = decoded.map_err(|_| BinaryUtf8Error::Surrogate)?;
        let encoded = ch.encode_utf8(&mut buffer).as_bytes();
        if encoded.len() > limit - result.len() {
            return Err(BinaryUtf8Error::Length);
        }
        result.extend_from_slice(encoded);
    }
    Ok(result)
}

pub fn binary_decode_utf8(
    bytes: &[u8],
    stop_at_null: bool,
    limit: usize,
) -> Result<String, BinaryUtf8Error> {
    if limit == 0 || limit > MAX_BINARY_BYTES || bytes.len() > limit {
        return Err(BinaryUtf8Error::Length);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let end = if stop_at_null {
        bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())
    } else {
        bytes.len()
    };
    std::str::from_utf8(&bytes[..end])
        .map(str::to_owned)
        .map_err(|_| BinaryUtf8Error::InvalidUtf8)
}

#[derive(Debug, thiserror::Error)]
#[error("ByteArray codec: {0}")]
pub struct BinaryError(String);
fn error(message: &str) -> BinaryError {
    BinaryError(message.into())
}
fn bounded(length: usize, limit: usize) -> Result<(), BinaryError> {
    if limit == 0 || limit > MAX_BINARY_BYTES || length > limit {
        return Err(error("input/output exceeds the configured byte limit"));
    }
    Ok(())
}
fn compress(bytes: &[u8], zlib: bool, limit: usize) -> Result<Vec<u8>, BinaryError> {
    // Ruffle selects the best compression level because the zlib header is
    // observable. Byte-for-byte encoder identity is not a supported guarantee.
    let stream: Box<dyn Read + '_> = if zlib {
        Box::new(flate2::read::ZlibEncoder::new(bytes, Compression::best()))
    } else {
        Box::new(flate2::read::DeflateEncoder::new(
            bytes,
            Compression::best(),
        ))
    };
    let mut result = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_to_end(&mut result)
        .map_err(|_| error("compression failed"))?;
    bounded(result.len(), limit)?;
    Ok(result)
}
fn decompress(bytes: &[u8], zlib: bool, limit: usize) -> Result<Vec<u8>, BinaryError> {
    let mut decoder = Decompress::new(zlib);
    let mut result = Vec::new();
    let mut scratch = [0; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &bytes[before_in as usize..],
                &mut scratch,
                FlushDecompress::None,
            )
            .map_err(|_| error("invalid or corrupt compressed data"))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if produced > limit - result.len() {
            return Err(error("decompressed data exceeds the configured byte limit"));
        }
        result.extend_from_slice(&scratch[..produced]);
        if status == Status::StreamEnd {
            return Ok(result);
        }
        if decoder.total_in() == before_in && produced == 0 {
            return Err(error("compressed data is incomplete"));
        }
    }
}

fn charset(label: &str) -> Result<&'static Encoding, BinaryError> {
    let label = label.trim().to_ascii_lowercase();
    // Preserve the initial NeoFlash aliases while using WHATWG's shared labels.
    let label = match label.as_str() {
        "utf16le" => "utf-16le",
        "utf16be" => "utf-16be",
        "latin-1" | "cp1252" => "windows-1252",
        "cp1251" => "windows-1251",
        other => other,
    };
    Encoding::for_label(label.as_bytes()).filter(|encoding| *encoding != REPLACEMENT)
        .ok_or_else(|| error("unsupported character set; use a WHATWG encoding such as UTF-8, Windows-1251, Shift_JIS or GBK"))
}
fn encode(text: &str, encoding: &'static Encoding, limit: usize) -> Result<Vec<u8>, BinaryError> {
    if encoding == UTF_8 {
        bounded(text.len(), limit)?;
        return Ok(text.as_bytes().to_vec());
    }
    let mut result = Vec::new();
    if encoding == UTF_16LE || encoding == UTF_16BE {
        // encoding_rs deliberately has no UTF-16 encoder; retain the explicit
        // Rust adaptation used by Ruffle rather than silently getting UTF-8.
        for unit in text.encode_utf16() {
            if result.len() + 2 > limit {
                return Err(error("encoded data exceeds the configured byte limit"));
            }
            result.extend_from_slice(&if encoding == UTF_16LE {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        return Ok(result);
    }
    let mut encoder = encoding.new_encoder();
    let mut scratch = [0; 8192];
    let mut consumed = 0;
    loop {
        let (status, read, written) =
            encoder.encode_from_utf8_without_replacement(&text[consumed..], &mut scratch, true);
        if written > limit - result.len() {
            return Err(error("encoded data exceeds the configured byte limit"));
        }
        result.extend_from_slice(&scratch[..written]);
        consumed += read;
        match status {
            EncoderResult::InputEmpty => return Ok(result),
            EncoderResult::Unmappable(_) => {
                return Err(error(
                    "character is not representable in the requested character set; use UTF-8",
                ));
            }
            EncoderResult::OutputFull if read == 0 && written == 0 => {
                return Err(error("character encoding made no progress"));
            }
            EncoderResult::OutputFull => {}
        }
    }
}

/// Pure codec entry point. The host commits returned bytes only after success.
pub fn binary_transform(
    operation: &str,
    bytes: &[u8],
    text: &str,
    parameter: &str,
    limit: usize,
) -> Result<Vec<u8>, BinaryError> {
    bounded(bytes.len(), limit)?;
    // Input text can occupy up to four UTF-8 bytes per encoded byte.
    if text.len() > limit.saturating_mul(4) {
        return Err(error("text input exceeds the configured byte limit"));
    }
    match operation {
        "compress" | "uncompress" => match parameter {
            "zlib" => {
                if operation == "compress" {
                    compress(bytes, true, limit)
                } else {
                    decompress(bytes, true, limit)
                }
            }
            "deflate" => {
                if operation == "compress" {
                    compress(bytes, false, limit)
                } else {
                    decompress(bytes, false, limit)
                }
            }
            "lzma" => {
                if operation == "compress" {
                    lzma_compress(bytes, limit)
                } else {
                    lzma_decompress(bytes, limit)
                }
            }
            _ => Err(error(
                "unsupported compression algorithm; use zlib, deflate or lzma",
            )),
        },
        "writeMultiByte" => encode(text, charset(parameter)?, limit),
        "writeBase64" => {
            if text.len() / 4 * 3 > limit.saturating_add(2) {
                return Err(error("decoded data exceeds the configured byte limit"));
            }
            let result = STANDARD
                .decode(text)
                .map_err(|_| error("invalid RFC 4648 base64 string"))?;
            bounded(result.len(), limit)?;
            Ok(result)
        }
        "writeHexString" => {
            if !text.len().is_multiple_of(2) {
                return Err(error("hex input must contain an even number of digits"));
            }
            bounded(text.len() / 2, limit)?;
            text.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let high = (pair[0] as char)
                        .to_digit(16)
                        .ok_or_else(|| error("invalid hex digit"))?;
                    let low = (pair[1] as char)
                        .to_digit(16)
                        .ok_or_else(|| error("invalid hex digit"))?;
                    Ok((high * 16 + low) as u8)
                })
                .collect()
        }
        _ => Err(error("unknown binary transformation")),
    }
}

/// AIR 51 textual representations of every active byte, independent of cursor.
pub fn binary_to_text(operation: &str, bytes: &[u8], limit: usize) -> Result<String, BinaryError> {
    bounded(bytes.len(), limit)?;
    match operation {
        "toBase64" => Ok(STANDARD.encode(bytes)),
        "toHexString" => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut result = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                result.push(HEX[usize::from(byte >> 4)] as char);
                result.push(HEX[usize::from(byte & 15)] as char);
            }
            Ok(result)
        }
        _ => Err(error("unknown binary text conversion")),
    }
}

/// Decode bounded text, consuming the full byte range in the caller even when
/// the returned string ends at NUL. BOM overrides the selected Unicode codec,
/// matching Ruffle's encoding_rs decode operation.
pub fn binary_decode(bytes: &[u8], label: &str, limit: usize) -> Result<String, BinaryError> {
    bounded(bytes.len(), limit)?;
    // The dependency shares Ruffle's label maps, BOM detection and replacement
    // decoding. Its output is bounded by at most three UTF-8 bytes per input
    // byte; no platform codepage lookup or environment-dependent fallback.
    let (decoded, _, _) = charset(label)?.decode(bytes);
    let mut result = decoded.into_owned();
    if let Some(nul) = result.find('\0') {
        result.truncate(nul);
    }
    Ok(result)
}

// Standalone LZMA1 (`FORMAT_ALONE`, 13-byte header) matching Flash Player 11.4+
// `ByteArray.compress("lzma")` / `uncompress("lzma")`. Range-coding and state
// transitions follow Igor Pavlov's public-domain LZMA1 specification.
const LZMA_INIT_PROB: u16 = 1024;

struct LzmaLenCoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LzmaLenCoder {
    fn new() -> Self {
        Self {
            choice: LZMA_INIT_PROB,
            choice2: LZMA_INIT_PROB,
            low: [[LZMA_INIT_PROB; 8]; 16],
            mid: [[LZMA_INIT_PROB; 8]; 16],
            high: [LZMA_INIT_PROB; 256],
        }
    }
}

struct LzmaModel {
    is_match: [[u16; 16]; 12],
    is_rep: [u16; 12],
    is_rep_g0: [u16; 12],
    is_rep_g1: [u16; 12],
    is_rep_g2: [u16; 12],
    is_rep0_long: [[u16; 16]; 12],
    pos_slot: [[u16; 64]; 4],
    pos_decoders: [u16; 115],
    align: [u16; 16],
    match_len: LzmaLenCoder,
    rep_len: LzmaLenCoder,
    literals: Vec<[u16; 0x300]>,
}

impl LzmaModel {
    fn new(lc: u32, lp: u32) -> Self {
        Self {
            is_match: [[LZMA_INIT_PROB; 16]; 12],
            is_rep: [LZMA_INIT_PROB; 12],
            is_rep_g0: [LZMA_INIT_PROB; 12],
            is_rep_g1: [LZMA_INIT_PROB; 12],
            is_rep_g2: [LZMA_INIT_PROB; 12],
            is_rep0_long: [[LZMA_INIT_PROB; 16]; 12],
            pos_slot: [[LZMA_INIT_PROB; 64]; 4],
            pos_decoders: [LZMA_INIT_PROB; 115],
            align: [LZMA_INIT_PROB; 16],
            match_len: LzmaLenCoder::new(),
            rep_len: LzmaLenCoder::new(),
            literals: vec![[LZMA_INIT_PROB; 0x300]; 1usize << (lc + lp)],
        }
    }
}

struct RangeEncoder {
    low: u64,
    range: u32,
    cache_size: u64,
    cache: u8,
    out: Vec<u8>,
    limit: usize,
}

impl RangeEncoder {
    fn new(header: [u8; 13], limit: usize) -> Result<Self, BinaryError> {
        bounded(header.len(), limit)?;
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(&header);
        Ok(Self {
            low: 0,
            range: 0xFFFF_FFFF,
            cache_size: 1,
            cache: 0,
            out,
            limit,
        })
    }

    fn shift_low(&mut self) -> Result<(), BinaryError> {
        let low_hi = (self.low >> 32) as u32;
        if (self.low as u32) < 0xFF00_0000 || low_hi != 0 {
            let mut temp = self.cache;
            loop {
                if self.out.len() >= self.limit {
                    return Err(error("compressed data exceeds the configured byte limit"));
                }
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
        self.low = u64::from((self.low as u32) << 8);
        Ok(())
    }

    fn encode_bit(&mut self, prob: &mut u16, bit: u32) -> Result<(), BinaryError> {
        let p = u32::from(*prob);
        let bound = (self.range >> 11) * p;
        if bit == 0 {
            self.range = bound;
            *prob = (p + ((2048 - p) >> 5)) as u16;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob = (p - (p >> 5)) as u16;
        }
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            self.shift_low()?;
        }
        Ok(())
    }

    fn encode_direct(&mut self, value: u32, num_bits: u32) -> Result<(), BinaryError> {
        for i in (0..num_bits).rev() {
            self.range >>= 1;
            if ((value >> i) & 1) == 1 {
                self.low += u64::from(self.range);
            }
            if self.range < 0x0100_0000 {
                self.range <<= 8;
                self.shift_low()?;
            }
        }
        Ok(())
    }

    fn encode_tree(
        &mut self,
        probs: &mut [u16],
        num_bits: u32,
        symbol: u32,
    ) -> Result<(), BinaryError> {
        let mut m = 1usize;
        for i in (0..num_bits).rev() {
            let bit = (symbol >> i) & 1;
            self.encode_bit(&mut probs[m], bit)?;
            m = (m << 1) | (bit as usize);
        }
        Ok(())
    }

    fn encode_reverse_tree(
        &mut self,
        probs: &mut [u16],
        num_bits: u32,
        symbol: u32,
    ) -> Result<(), BinaryError> {
        let mut m = 1usize;
        for i in 0..num_bits {
            let bit = (symbol >> i) & 1;
            self.encode_bit(&mut probs[m], bit)?;
            m = (m << 1) | (bit as usize);
        }
        Ok(())
    }

    fn encode_len(
        &mut self,
        coder: &mut LzmaLenCoder,
        pos_state: usize,
        len: u32,
    ) -> Result<(), BinaryError> {
        let l = len - 2;
        if l < 8 {
            self.encode_bit(&mut coder.choice, 0)?;
            self.encode_tree(&mut coder.low[pos_state], 3, l)
        } else if l < 16 {
            self.encode_bit(&mut coder.choice, 1)?;
            self.encode_bit(&mut coder.choice2, 0)?;
            self.encode_tree(&mut coder.mid[pos_state], 3, l - 8)
        } else {
            self.encode_bit(&mut coder.choice, 1)?;
            self.encode_bit(&mut coder.choice2, 1)?;
            self.encode_tree(&mut coder.high, 8, l - 16)
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, BinaryError> {
        for _ in 0..5 {
            self.shift_low()?;
        }
        bounded(self.out.len(), self.limit)?;
        Ok(self.out)
    }
}

fn lzma_compress(bytes: &[u8], limit: usize) -> Result<Vec<u8>, BinaryError> {
    const LC: u32 = 3;
    const LP: u32 = 0;
    const PB: u32 = 2;
    const DICT_SIZE: u32 = 1 << 16;
    const POS_MASK: usize = (1 << PB) - 1;
    let props = ((PB * 5 + LP) * 9 + LC) as u8;
    let mut header = [0u8; 13];
    header[0] = props;
    header[1..5].copy_from_slice(&DICT_SIZE.to_le_bytes());
    header[5..13].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
    let mut rc = RangeEncoder::new(header, limit)?;
    let mut model = LzmaModel::new(LC, LP);
    let mut state = 0usize;
    let mut reps = [0u32; 4];

    let mut head = vec![-1i32; 65536];
    let mut chain = vec![-1i32; bytes.len().min(DICT_SIZE as usize).max(1)];
    let chain_mask = if bytes.len() >= DICT_SIZE as usize {
        Some((DICT_SIZE as usize) - 1)
    } else {
        None
    };
    let hash3 = |pos: usize| -> usize {
        ((usize::from(bytes[pos]) << 8)
            ^ (usize::from(bytes[pos + 1]) << 4)
            ^ usize::from(bytes[pos + 2]))
            & 0xFFFF
    };
    let insert_hash = |pos: usize, head: &mut [i32], chain: &mut [i32]| {
        if pos + 3 <= bytes.len() {
            let h = hash3(pos);
            let slot = chain_mask.map_or(pos, |m| pos & m);
            chain[slot] = head[h];
            head[h] = pos as i32;
        }
    };

    let mut pos = 0usize;
    while pos < bytes.len() {
        let pos_state = pos & POS_MASK;
        let max_len = (bytes.len() - pos).min(273);

        // Check repeat distances first (rep0..rep3).
        let mut best_rep = None;
        let mut best_rep_len = 0usize;
        if pos > 0 {
            for (rep_idx, &rep_dist) in reps.iter().enumerate() {
                let d = rep_dist as usize + 1;
                if d <= pos {
                    let mut l = 0usize;
                    while l < max_len && bytes[pos + l] == bytes[pos - d + l] {
                        l += 1;
                    }
                    if (rep_idx == 0 && l >= 1 && best_rep_len == 0 && l >= 2)
                        || (l >= 2 && l > best_rep_len)
                    {
                        best_rep_len = l;
                        best_rep = Some(rep_idx);
                    }
                }
            }
        }

        // Check hash-chain match.
        let mut best_match_len = 0usize;
        let mut best_match_dist = 0u32;
        if max_len >= 3 {
            let h = hash3(pos);
            let mut cur = head[h];
            let min_pos = pos.saturating_sub(DICT_SIZE as usize);
            let mut steps = 0usize;
            while cur >= 0 && (cur as usize) >= min_pos && steps < 32 {
                let cand = cur as usize;
                let dist = pos - cand;
                if dist >= 1
                    && dist <= DICT_SIZE as usize
                    && bytes[cand + best_match_len] == bytes[pos + best_match_len]
                {
                    let mut l = 0usize;
                    while l < max_len && bytes[cand + l] == bytes[pos + l] {
                        l += 1;
                    }
                    if l >= 3 && l > best_match_len {
                        best_match_len = l;
                        best_match_dist = (dist - 1) as u32;
                        if l == max_len {
                            break;
                        }
                    }
                }
                let slot = chain_mask.map_or(cand, |m| cand & m);
                let next = chain[slot];
                if next >= cur {
                    break;
                }
                cur = next;
                steps += 1;
            }
        }

        if let Some(rep_idx) = best_rep
            && best_rep_len >= 2
            && best_rep_len + 1 >= best_match_len
        {
            rc.encode_bit(&mut model.is_match[state][pos_state], 1)?;
            rc.encode_bit(&mut model.is_rep[state], 1)?;
            if rep_idx == 0 {
                rc.encode_bit(&mut model.is_rep_g0[state], 0)?;
                rc.encode_bit(&mut model.is_rep0_long[state][pos_state], 1)?;
            } else {
                rc.encode_bit(&mut model.is_rep_g0[state], 1)?;
                let dist = reps[rep_idx];
                if rep_idx == 1 {
                    rc.encode_bit(&mut model.is_rep_g1[state], 0)?;
                } else {
                    rc.encode_bit(&mut model.is_rep_g1[state], 1)?;
                    if rep_idx == 2 {
                        rc.encode_bit(&mut model.is_rep_g2[state], 0)?;
                    } else {
                        rc.encode_bit(&mut model.is_rep_g2[state], 1)?;
                        reps[3] = reps[2];
                    }
                    reps[2] = reps[1];
                }
                reps[1] = reps[0];
                reps[0] = dist;
            }
            rc.encode_len(&mut model.rep_len, pos_state, best_rep_len as u32)?;
            state = if state < 7 { 8 } else { 11 };
            for step in 0..best_rep_len {
                insert_hash(pos + step, &mut head, &mut chain);
            }
            pos += best_rep_len;
            continue;
        }

        if best_match_len >= 3 {
            rc.encode_bit(&mut model.is_match[state][pos_state], 1)?;
            rc.encode_bit(&mut model.is_rep[state], 0)?;
            rc.encode_len(&mut model.match_len, pos_state, best_match_len as u32)?;
            state = if state < 7 { 7 } else { 10 };
            let len_state = (best_match_len - 2).min(3);
            let dist = best_match_dist;
            if dist < 4 {
                rc.encode_tree(&mut model.pos_slot[len_state], 6, dist)?;
            } else {
                let ndb = 30 - dist.leading_zeros();
                let pos_slot = ((ndb + 1) << 1) + ((dist >> ndb) & 1);
                rc.encode_tree(&mut model.pos_slot[len_state], 6, pos_slot)?;
                let base = (2 | (pos_slot & 1)) << ndb;
                let rem = dist - base;
                if pos_slot < 14 {
                    let offset = (base - pos_slot) as usize;
                    rc.encode_reverse_tree(&mut model.pos_decoders[offset..], ndb, rem)?;
                } else {
                    rc.encode_direct(rem >> 4, ndb - 4)?;
                    rc.encode_reverse_tree(&mut model.align, 4, rem & 15)?;
                }
            }
            reps[3] = reps[2];
            reps[2] = reps[1];
            reps[1] = reps[0];
            reps[0] = dist;
            for step in 0..best_match_len {
                insert_hash(pos + step, &mut head, &mut chain);
            }
            pos += best_match_len;
            continue;
        }

        // ShortRep (1-byte repeat at rep0) when beneficial.
        if pos > (reps[0] as usize) && bytes[pos] == bytes[pos - 1 - (reps[0] as usize)] {
            rc.encode_bit(&mut model.is_match[state][pos_state], 1)?;
            rc.encode_bit(&mut model.is_rep[state], 1)?;
            rc.encode_bit(&mut model.is_rep_g0[state], 0)?;
            rc.encode_bit(&mut model.is_rep0_long[state][pos_state], 0)?;
            state = if state < 7 { 9 } else { 11 };
            insert_hash(pos, &mut head, &mut chain);
            pos += 1;
            continue;
        }

        // Literal.
        rc.encode_bit(&mut model.is_match[state][pos_state], 0)?;
        let prev_byte = if pos == 0 { 0u8 } else { bytes[pos - 1] };
        let lit_idx = ((pos & ((1 << LP) - 1)) << LC) | (usize::from(prev_byte) >> (8 - LC));
        let sub = &mut model.literals[lit_idx];
        let byte = bytes[pos];
        if state >= 7 {
            let match_byte = bytes[pos - 1 - (reps[0] as usize)];
            let mut m = 1usize;
            let mut mb = usize::from(match_byte);
            let mut matched = true;
            for i in (0..8).rev() {
                let bit = u32::from((byte >> i) & 1);
                let match_bit = (mb >> 7) & 1;
                mb <<= 1;
                let idx = if matched {
                    ((1 + match_bit) << 8) + m
                } else {
                    m
                };
                rc.encode_bit(&mut sub[idx], bit)?;
                m = (m << 1) | (bit as usize);
                if matched && match_bit != (bit as usize) {
                    matched = false;
                }
            }
        } else {
            let mut m = 1usize;
            for i in (0..8).rev() {
                let bit = u32::from((byte >> i) & 1);
                rc.encode_bit(&mut sub[m], bit)?;
                m = (m << 1) | (bit as usize);
            }
        }
        state = [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 4, 5][state];
        insert_hash(pos, &mut head, &mut chain);
        pos += 1;
    }

    rc.finish()
}

struct RangeDecoder<'a> {
    buf: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    fn new(buf: &'a [u8]) -> Result<Self, BinaryError> {
        if buf.len() < 5 || buf[0] != 0 {
            return Err(error("invalid or corrupt LZMA stream header"));
        }
        let code = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        Ok(Self {
            buf,
            pos: 5,
            range: 0xFFFF_FFFF,
            code,
        })
    }

    fn normalize(&mut self) -> Result<(), BinaryError> {
        if self.range < 0x0100_0000 {
            let b = *self
                .buf
                .get(self.pos)
                .ok_or_else(|| error("compressed LZMA data is incomplete"))?;
            self.pos += 1;
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(b);
        }
        Ok(())
    }

    fn decode_bit(&mut self, prob: &mut u16) -> Result<u32, BinaryError> {
        self.normalize()?;
        let p = u32::from(*prob);
        let bound = (self.range >> 11) * p;
        if self.code < bound {
            self.range = bound;
            *prob = (p + ((2048 - p) >> 5)) as u16;
            Ok(0)
        } else {
            self.range -= bound;
            self.code -= bound;
            *prob = (p - (p >> 5)) as u16;
            Ok(1)
        }
    }

    fn decode_direct(&mut self, num_bits: u32) -> Result<u32, BinaryError> {
        let mut res = 0u32;
        for _ in 0..num_bits {
            self.normalize()?;
            self.range >>= 1;
            if self.code >= self.range {
                self.code -= self.range;
                res = (res << 1) | 1;
            } else {
                res <<= 1;
            }
        }
        Ok(res)
    }

    fn decode_tree(&mut self, probs: &mut [u16], num_bits: u32) -> Result<u32, BinaryError> {
        let mut m = 1usize;
        for _ in 0..num_bits {
            m = (m << 1) | (self.decode_bit(&mut probs[m])? as usize);
        }
        Ok((m as u32) - (1u32 << num_bits))
    }

    fn decode_reverse_tree(
        &mut self,
        probs: &mut [u16],
        num_bits: u32,
    ) -> Result<u32, BinaryError> {
        let mut m = 1usize;
        let mut symbol = 0u32;
        for i in 0..num_bits {
            let bit = self.decode_bit(&mut probs[m])?;
            m = (m << 1) | (bit as usize);
            symbol |= bit << i;
        }
        Ok(symbol)
    }

    fn decode_len(
        &mut self,
        coder: &mut LzmaLenCoder,
        pos_state: usize,
    ) -> Result<usize, BinaryError> {
        if self.decode_bit(&mut coder.choice)? == 0 {
            Ok(2 + self.decode_tree(&mut coder.low[pos_state], 3)? as usize)
        } else if self.decode_bit(&mut coder.choice2)? == 0 {
            Ok(10 + self.decode_tree(&mut coder.mid[pos_state], 3)? as usize)
        } else {
            Ok(18 + self.decode_tree(&mut coder.high, 8)? as usize)
        }
    }
}

fn lzma_decompress(bytes: &[u8], limit: usize) -> Result<Vec<u8>, BinaryError> {
    if bytes.len() < 18 {
        return Err(error("invalid or truncated LZMA header"));
    }
    let props = bytes[0];
    if props >= 225 {
        return Err(error("invalid LZMA properties byte"));
    }
    let lc = u32::from(props % 9);
    let rem = props / 9;
    let lp = u32::from(rem % 5);
    let pb = u32::from(rem / 5);
    let dict_size = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]).max(4096) as usize;
    let raw_size = u64::from_le_bytes([
        bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12],
    ]);
    let known_size = if raw_size == u64::MAX {
        None
    } else {
        let sz = usize::try_from(raw_size)
            .map_err(|_| error("decompressed data exceeds the configured byte limit"))?;
        bounded(sz, limit)?;
        Some(sz)
    };
    let mut rc = RangeDecoder::new(&bytes[13..])?;
    if known_size == Some(0) {
        return Ok(Vec::new());
    }
    let mut model = LzmaModel::new(lc, lp);
    let mut out = Vec::with_capacity(known_size.unwrap_or(0).min(65536));
    let pos_mask = (1usize << pb) - 1;
    let lp_mask = (1usize << lp) - 1;
    let mut state = 0usize;
    let mut reps = [0u32; 4];

    loop {
        if let Some(target) = known_size
            && out.len() >= target
        {
            break;
        }
        let pos_state = out.len() & pos_mask;
        if rc.decode_bit(&mut model.is_match[state][pos_state])? == 0 {
            if out.len() >= limit {
                return Err(error("decompressed data exceeds the configured byte limit"));
            }
            let prev_byte = out.last().copied().unwrap_or(0);
            let lit_idx =
                ((out.len() & lp_mask) << lc) | (usize::from(prev_byte) >> ((8 - lc) as usize));
            let sub = &mut model.literals[lit_idx];
            let byte = if state >= 7 {
                let d = reps[0] as usize + 1;
                if d > out.len() {
                    return Err(error("corrupt LZMA literal match distance"));
                }
                let match_byte = out[out.len() - d];
                let mut m = 1usize;
                let mut mb = usize::from(match_byte);
                let mut matched = true;
                for _ in 0..8 {
                    let match_bit = (mb >> 7) & 1;
                    mb <<= 1;
                    let idx = if matched {
                        ((1 + match_bit) << 8) + m
                    } else {
                        m
                    };
                    let bit = rc.decode_bit(&mut sub[idx])? as usize;
                    m = (m << 1) | bit;
                    if matched && match_bit != bit {
                        matched = false;
                    }
                }
                (m & 0xFF) as u8
            } else {
                let mut m = 1usize;
                for _ in 0..8 {
                    m = (m << 1) | (rc.decode_bit(&mut sub[m])? as usize);
                }
                (m & 0xFF) as u8
            };
            out.push(byte);
            state = [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 4, 5][state];
        } else {
            let len: usize;
            if rc.decode_bit(&mut model.is_rep[state])? == 1 {
                if out.is_empty() {
                    return Err(error("corrupt LZMA repeat match at start of stream"));
                }
                if rc.decode_bit(&mut model.is_rep_g0[state])? == 0 {
                    if rc.decode_bit(&mut model.is_rep0_long[state][pos_state])? == 0 {
                        let d = reps[0] as usize + 1;
                        if d > out.len() || d > dict_size {
                            return Err(error("corrupt LZMA short repeat distance"));
                        }
                        if out.len() >= limit {
                            return Err(error(
                                "decompressed data exceeds the configured byte limit",
                            ));
                        }
                        let b = out[out.len() - d];
                        out.push(b);
                        state = if state < 7 { 9 } else { 11 };
                        continue;
                    }
                } else {
                    let dist = if rc.decode_bit(&mut model.is_rep_g1[state])? == 0 {
                        reps[1]
                    } else {
                        let d = if rc.decode_bit(&mut model.is_rep_g2[state])? == 0 {
                            reps[2]
                        } else {
                            let d3 = reps[3];
                            reps[3] = reps[2];
                            d3
                        };
                        reps[2] = reps[1];
                        d
                    };
                    reps[1] = reps[0];
                    reps[0] = dist;
                }
                len = rc.decode_len(&mut model.rep_len, pos_state)?;
                state = if state < 7 { 8 } else { 11 };
            } else {
                len = rc.decode_len(&mut model.match_len, pos_state)?;
                state = if state < 7 { 7 } else { 10 };
                let pos_slot = rc.decode_tree(&mut model.pos_slot[(len - 2).min(3)], 6)?;
                let dist = if pos_slot < 4 {
                    pos_slot
                } else {
                    let ndb = (pos_slot >> 1) - 1;
                    let base = (2 | (pos_slot & 1)) << ndb;
                    if pos_slot < 14 {
                        let offset = (base - pos_slot) as usize;
                        base + rc.decode_reverse_tree(&mut model.pos_decoders[offset..], ndb)?
                    } else {
                        let direct = rc.decode_direct(ndb - 4)?;
                        let align = rc.decode_reverse_tree(&mut model.align, 4)?;
                        base.wrapping_add(direct << 4).wrapping_add(align)
                    }
                };
                if dist == 0xFFFF_FFFF {
                    if let Some(target) = known_size
                        && out.len() != target
                    {
                        return Err(error("LZMA stream ended before declared uncompressed size"));
                    }
                    break;
                }
                reps[3] = reps[2];
                reps[2] = reps[1];
                reps[1] = reps[0];
                reps[0] = dist;
            }
            let d = reps[0] as usize + 1;
            if d > out.len() || d > dict_size {
                return Err(error("corrupt LZMA match distance"));
            }
            if let Some(target) = known_size
                && out.len() + len > target
            {
                return Err(error("LZMA match exceeds declared uncompressed size"));
            }
            if len > limit - out.len() {
                return Err(error("decompressed data exceeds the configured byte limit"));
            }
            for _ in 0..len {
                let b = out[out.len() - d];
                out.push(b);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytearray_codec_family() {
        let source = "NeoFlash € 🦀 / Rust binary packet".repeat(24);
        for algorithm in ["zlib", "deflate", "lzma"] {
            let packed =
                binary_transform("compress", source.as_bytes(), "", algorithm, 4096).unwrap();
            assert!(packed.len() < source.len());
            assert_eq!(
                binary_transform("uncompress", &packed, "", algorithm, 4096).unwrap(),
                source.as_bytes()
            );
            assert!(binary_transform("uncompress", &packed, "", algorithm, 64).is_err());
            assert!(
                binary_transform(
                    "uncompress",
                    &packed[..packed.len() - 4],
                    "",
                    algorithm,
                    4096
                )
                .is_err()
            );
        }
        let empty_lzma = binary_transform("compress", b"", "", "lzma", 100).unwrap();
        assert!(
            binary_transform("uncompress", &empty_lzma, "", "lzma", 100)
                .unwrap()
                .is_empty()
        );
        // Verify interoperability with liblzma FORMAT_ALONE (unknown-size + EOS marker).
        for (hex, expected) in [
            ("5d00008000ffffffffffffffff0083fffbffffc0000000", &b""[..]),
            (
                "5d00008000ffffffffffffffff00309888a44a8e9ffff6638000",
                &b"abc"[..],
            ),
            (
                "5d00008000ffffffffffffffff00271949e46592ff708ee4580742234a5564a83a20118c9fddfd9a786f1a7816cf5040336965a79f620c58e16fae03ffffd6ff0000",
                "NeoFlash € 🦀 / Rust binary packet".repeat(3).as_bytes(),
            ),
        ] {
            let stream = binary_transform("writeHexString", &[], hex, "", 1024).unwrap();
            assert_eq!(
                binary_transform("uncompress", &stream, "", "lzma", 1024).unwrap(),
                expected
            );
        }
        for label in ["utf-8", "utf-16le", "utf-16be"] {
            let bytes = binary_transform("writeMultiByte", &[], "hé 🦀\0tail", label, 100).unwrap();
            assert_eq!(binary_decode(&bytes, label, 100).unwrap(), "hé 🦀");
        }
        let latin = binary_transform("writeMultiByte", &[], "café €", "iso-8859-1", 100).unwrap();
        assert_eq!(latin, b"caf\xe9 \x80");
        assert_eq!(
            binary_decode(&latin, "windows-1252", 100).unwrap(),
            "café €"
        );
        assert_eq!(
            binary_decode(&[0xef, 0xbb, 0xbf, b'A', 0xff], "ascii", 100).unwrap(),
            "A�"
        );
        assert!(binary_transform("writeMultiByte", &[], "🦀", "latin1", 100).is_err());
        assert!(binary_decode(b"abc", "utf-32", 100).is_err());
        assert!(binary_transform("compress", b"abc", "", "brotli", 100).is_err());
        for (label, text, expected) in [
            (
                "windows-1251",
                "Привет",
                &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2][..],
            ),
            ("shift_jis", "日本", &[0x93, 0xfa, 0x96, 0x7b][..]),
            ("gbk", "中文", &[0xd6, 0xd0, 0xce, 0xc4][..]),
            ("big5", "中文", &[0xa4, 0xa4, 0xa4, 0xe5][..]),
            ("euc-kr", "한국", &[0xc7, 0xd1, 0xb1, 0xb9][..]),
        ] {
            let encoded = binary_transform("writeMultiByte", &[], text, label, 100).unwrap();
            assert_eq!(encoded, expected);
            assert_eq!(binary_decode(&encoded, label, 100).unwrap(), text);
        }
        let stateful = binary_transform("writeMultiByte", &[], "日本", "iso-2022-jp", 100).unwrap();
        assert_eq!(
            binary_decode(&stateful, "iso-2022-jp", 100).unwrap(),
            "日本"
        );
        assert!(binary_decode(b"abc", "iso-2022-kr", 100).is_err());
        let bytes = [0, 1, 127, 128, 254, 255];
        for (write, read, text) in [
            ("writeBase64", "toBase64", "AAF/gP7/"),
            ("writeHexString", "toHexString", "00017F80FEFF"),
        ] {
            assert_eq!(binary_to_text(read, &bytes, 100).unwrap(), text);
            assert_eq!(binary_transform(write, &[], text, "", 100).unwrap(), bytes);
        }
        assert_eq!(
            binary_transform("writeHexString", &[], "aBcD", "", 100).unwrap(),
            [0xab, 0xcd]
        );
        assert!(binary_transform("writeBase64", &[], "a?==", "", 100).is_err());
        assert!(binary_transform("writeHexString", &[], "123", "", 100).is_err());
        let units: Vec<u16> = "π 🦀\0tail".encode_utf16().collect();
        let utf8 = binary_encode_utf8(&units, 100).unwrap();
        assert_eq!(binary_decode_utf8(&utf8, true, 100).unwrap(), "π 🦀");
        assert_eq!(binary_decode_utf8(&utf8, false, 100).unwrap(), "π 🦀\0tail");
        assert_eq!(
            binary_decode_utf8(&[0xef, 0xbb, 0xbf, b'A', 0, 0xff], true, 100).unwrap(),
            "A"
        );
        assert!(matches!(
            binary_decode_utf8(&[0xff], false, 100),
            Err(BinaryUtf8Error::InvalidUtf8)
        ));
        assert!(matches!(
            binary_encode_utf8(&[0xd800], 100),
            Err(BinaryUtf8Error::Surrogate)
        ));
        assert!(matches!(
            binary_encode_utf8(&[0x03c0], 1),
            Err(BinaryUtf8Error::Length)
        ));
    }
}
