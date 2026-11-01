//! Bounded LZMA1 decoder for SWF 13+ (`ZWS`) payloads.
//!
//! Adapted from `lzma-rs` v0.3.0 (`src/decode/{lzma,rangecoder}.rs`), licensed
//! under the MIT License:
//! Copyright (c) 2017 - 2018 Guillaume Endignoux
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software. See `THIRD_PARTY_NOTICES` / crate provenance.

/// Decompress an SWF `ZWS` LZMA payload (`props` = 5-byte LZMA properties `[lclppb, dict_size_le32]`,
/// `compressed` = raw LZMA range-coded stream after the 17-byte `ZWS` header) into at most
/// `max_output` bytes (`expected_len = declared_uncompressed_len - 8`).
pub(crate) fn decompress_zws(
    props: [u8; 5],
    compressed: &[u8],
    expected_len: usize,
    max_output: usize,
) -> Result<Vec<u8>, String> {
    if expected_len > max_output {
        return Err(format!(
            "ZWS declared payload ({expected_len} bytes) exceeds {max_output}-byte limit"
        ));
    }
    let mut pb = u32::from(props[0]);
    if pb >= 225 {
        return Err(format!(
            "Invalid ZWS LZMA properties byte {} (must be < 225)",
            props[0]
        ));
    }
    let lc = pb % 9;
    pb /= 9;
    let lp = pb % 5;
    pb /= 5;

    let raw_dict = u32::from_le_bytes([props[1], props[2], props[3], props[4]]) as usize;
    let dict_limit = raw_dict.max(0x1000);
    if dict_limit > max_output && expected_len > max_output {
        return Err("ZWS LZMA dictionary exceeds limit".into());
    }

    if compressed.len() < 5 {
        return Err("Truncated ZWS LZMA range-coder initialization header".into());
    }
    if compressed[0] != 0 {
        return Err(format!(
            "Invalid ZWS LZMA initial range-coder byte 0x{:02x} (expected 0x00)",
            compressed[0]
        ));
    }

    let mut rc = RangeDecoder {
        bytes: compressed,
        pos: 5,
        range: 0xFFFF_FFFF,
        code: u32::from_be_bytes([compressed[1], compressed[2], compressed[3], compressed[4]]),
        eof_overrun: 0,
    };

    let num_lit_states = 1usize << (lc + lp);
    let mut literal_probs = vec![[0x400_u16; 0x300]; num_lit_states];
    let mut pos_slot_decoder = [
        BitTree::new(6),
        BitTree::new(6),
        BitTree::new(6),
        BitTree::new(6),
    ];
    let mut align_decoder = BitTree::new(4);
    let mut pos_decoders = [0x400_u16; 115];
    let mut is_match = [0x400_u16; 192];
    let mut is_rep = [0x400_u16; 12];
    let mut is_rep_g0 = [0x400_u16; 12];
    let mut is_rep_g1 = [0x400_u16; 12];
    let mut is_rep_g2 = [0x400_u16; 12];
    let mut is_rep_0long = [0x400_u16; 192];
    let mut len_decoder = LenDecoder::new();
    let mut rep_len_decoder = LenDecoder::new();

    let mut state = 0usize;
    let mut rep = [0usize; 4];
    let mut out = Vec::with_capacity(expected_len.min(1024 * 1024));
    let pb_mask = (1usize << pb) - 1;
    let lp_mask = (1usize << lp) - 1;

    while out.len() < expected_len {
        let pos_state = out.len() & pb_mask;
        if !rc.decode_bit(&mut is_match[(state << 4) + pos_state])? {
            let prev_byte = out.last().copied().unwrap_or(0) as usize;
            let lit_state = ((out.len() & lp_mask) << lc) + (prev_byte >> (8 - lc));
            let probs = &mut literal_probs[lit_state];
            let mut symbol = 1usize;
            if state >= 7 {
                let dist = rep[0] + 1;
                if dist > out.len() {
                    return Err("ZWS LZMA literal match distance exceeds output history".into());
                }
                let mut match_byte = usize::from(out[out.len() - dist]);
                while symbol < 0x100 {
                    let match_bit = (match_byte >> 7) & 1;
                    match_byte <<= 1;
                    let bit =
                        usize::from(rc.decode_bit(&mut probs[((1 + match_bit) << 8) + symbol])?);
                    symbol = (symbol << 1) ^ bit;
                    if match_bit != bit {
                        break;
                    }
                }
            }
            while symbol < 0x100 {
                let bit = usize::from(rc.decode_bit(&mut probs[symbol])?);
                symbol = (symbol << 1) ^ bit;
            }
            out.push((symbol - 0x100) as u8);
            state = if state < 4 {
                0
            } else if state < 10 {
                state - 3
            } else {
                state - 6
            };
            continue;
        }

        let mut len: usize;
        if rc.decode_bit(&mut is_rep[state])? {
            if !rc.decode_bit(&mut is_rep_g0[state])? {
                if !rc.decode_bit(&mut is_rep_0long[(state << 4) + pos_state])? {
                    state = if state < 7 { 9 } else { 11 };
                    let dist = rep[0] + 1;
                    if dist > out.len() || dist > dict_limit {
                        return Err("ZWS LZMA short-rep distance out of bounds".into());
                    }
                    out.push(out[out.len() - dist]);
                    continue;
                }
            } else {
                let idx = if !rc.decode_bit(&mut is_rep_g1[state])? {
                    1
                } else if !rc.decode_bit(&mut is_rep_g2[state])? {
                    2
                } else {
                    3
                };
                let dist = rep[idx];
                for i in (0..idx).rev() {
                    rep[i + 1] = rep[i];
                }
                rep[0] = dist;
            }
            len = rep_len_decoder.decode(&mut rc, pos_state)?;
            state = if state < 7 { 8 } else { 11 };
        } else {
            rep[3] = rep[2];
            rep[2] = rep[1];
            rep[1] = rep[0];
            len = len_decoder.decode(&mut rc, pos_state)?;
            state = if state < 7 { 7 } else { 10 };

            let len_state = len.min(3);
            let pos_slot = pos_slot_decoder[len_state].parse(&mut rc)? as usize;
            let rep0 = if pos_slot < 4 {
                pos_slot
            } else {
                let num_direct_bits = (pos_slot >> 1) - 1;
                let mut dist = (2 ^ (pos_slot & 1)) << num_direct_bits;
                if pos_slot < 14 {
                    dist += rc.parse_reverse_bit_tree(
                        num_direct_bits,
                        &mut pos_decoders,
                        dist - pos_slot,
                    )? as usize;
                } else {
                    dist += (rc.get_direct_bits(num_direct_bits - 4)? as usize) << 4;
                    dist += align_decoder.parse_reverse(&mut rc)? as usize;
                }
                dist
            };
            rep[0] = rep0;
            if rep[0] == 0xFFFF_FFFF {
                // End-of-stream marker reached.
                break;
            }
        }

        len += 2;
        let dist = rep[0] + 1;
        if dist > out.len() || dist > dict_limit {
            return Err("ZWS LZMA match distance exceeds output history".into());
        }
        let target_len = out
            .len()
            .checked_add(len)
            .ok_or_else(|| "ZWS LZMA output length overflow".to_owned())?;
        if target_len > max_output {
            return Err(format!(
                "ZWS LZMA output exceeds {max_output}-byte decompression limit"
            ));
        }
        // Flash Player caps output to `expected_len` declared in the SWF header.
        let copy_len = len.min(expected_len - out.len());
        for _ in 0..copy_len {
            let b = out[out.len() - dist];
            out.push(b);
        }
    }

    if out.len() != expected_len {
        return Err(format!(
            "ZWS LZMA decompressed {} bytes, expected {expected_len}",
            out.len()
        ));
    }
    Ok(out)
}

struct RangeDecoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
    eof_overrun: usize,
}

impl RangeDecoder<'_> {
    #[inline]
    fn read_byte(&mut self) -> Result<u8, String> {
        if let Some(&b) = self.bytes.get(self.pos) {
            self.pos += 1;
            Ok(b)
        } else if self.eof_overrun < 5 {
            // Allow up to 5 normalization flush bytes at exact stream end when no EOS marker was written.
            self.eof_overrun += 1;
            Ok(0)
        } else {
            Err("Unexpected EOF in ZWS LZMA stream".into())
        }
    }

    #[inline]
    fn normalize(&mut self) -> Result<(), String> {
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            self.code = (self.code << 8) ^ u32::from(self.read_byte()?);
        }
        Ok(())
    }

    #[inline]
    fn decode_bit(&mut self, prob: &mut u16) -> Result<bool, String> {
        let bound = (self.range >> 11) * u32::from(*prob);
        if self.code < bound {
            *prob += (0x800_u16 - *prob) >> 5;
            self.range = bound;
            self.normalize()?;
            Ok(false)
        } else {
            *prob -= *prob >> 5;
            self.code -= bound;
            self.range -= bound;
            self.normalize()?;
            Ok(true)
        }
    }

    fn get_direct_bits(&mut self, count: usize) -> Result<u32, String> {
        let mut result = 0u32;
        for _ in 0..count {
            self.range >>= 1;
            let bit = self.code >= self.range;
            if bit {
                self.code -= self.range;
            }
            self.normalize()?;
            result = (result << 1) ^ u32::from(bit);
        }
        Ok(result)
    }

    fn parse_bit_tree(&mut self, num_bits: usize, probs: &mut [u16]) -> Result<u32, String> {
        let mut tmp = 1u32;
        for _ in 0..num_bits {
            let bit = self.decode_bit(&mut probs[tmp as usize])?;
            tmp = (tmp << 1) ^ u32::from(bit);
        }
        Ok(tmp - (1 << num_bits))
    }

    fn parse_reverse_bit_tree(
        &mut self,
        num_bits: usize,
        probs: &mut [u16],
        offset: usize,
    ) -> Result<u32, String> {
        let mut result = 0u32;
        let mut tmp = 1usize;
        for i in 0..num_bits {
            let bit = self.decode_bit(&mut probs[offset + tmp])?;
            tmp = (tmp << 1) ^ usize::from(bit);
            result ^= u32::from(bit) << i;
        }
        Ok(result)
    }
}

#[derive(Clone)]
struct BitTree {
    num_bits: usize,
    probs: Vec<u16>,
}

impl BitTree {
    fn new(num_bits: usize) -> Self {
        Self {
            num_bits,
            probs: vec![0x400; 1 << num_bits],
        }
    }

    fn parse(&mut self, rc: &mut RangeDecoder<'_>) -> Result<u32, String> {
        rc.parse_bit_tree(self.num_bits, &mut self.probs)
    }

    fn parse_reverse(&mut self, rc: &mut RangeDecoder<'_>) -> Result<u32, String> {
        rc.parse_reverse_bit_tree(self.num_bits, &mut self.probs, 0)
    }
}

struct LenDecoder {
    choice: u16,
    choice2: u16,
    low_coder: [BitTree; 16],
    mid_coder: [BitTree; 16],
    high_coder: BitTree,
}

impl LenDecoder {
    fn new() -> Self {
        Self {
            choice: 0x400,
            choice2: 0x400,
            low_coder: std::array::from_fn(|_| BitTree::new(3)),
            mid_coder: std::array::from_fn(|_| BitTree::new(3)),
            high_coder: BitTree::new(8),
        }
    }

    fn decode(&mut self, rc: &mut RangeDecoder<'_>, pos_state: usize) -> Result<usize, String> {
        if !rc.decode_bit(&mut self.choice)? {
            Ok(self.low_coder[pos_state].parse(rc)? as usize)
        } else if !rc.decode_bit(&mut self.choice2)? {
            Ok(self.mid_coder[pos_state].parse(rc)? as usize + 8)
        } else {
            Ok(self.high_coder.parse(rc)? as usize + 16)
        }
    }
}
