#![forbid(unsafe_code)]
//! Pure-Rust bounded RFC 8878 Zstandard frame decompressor for Figma v106+ `fig-kiwi` chunks.

/// Standard Zstandard frame magic (`0xFD2FB528`, little-endian).
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Maximum decompressed size of a single Zstandard block (128 KiB per RFC 8878 §3.1.1.2).
const MAX_BLOCK_SIZE: usize = 128 * 1024;

/// Maximum window size accepted without external streaming buffers (512 MiB).
const MAX_WINDOW_SIZE: usize = 512 * 1024 * 1024;

/// Returns `true` if `bytes` begins with a Zstandard standard frame or skippable frame magic.
pub(crate) fn is_zstd_magic(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    bytes[..4] == ZSTD_MAGIC
        || (matches!(bytes[0], 0x50..=0x5F) && bytes[1..4] == [0x2A, 0x4D, 0x18])
}

/// Decompress one or more concatenated RFC 8878 Zstandard frames (and skippable frames),
/// enforcing `out.len() <= max_output` at every step.
pub(crate) fn decompress_zstd(input: &[u8], max_output: usize) -> Result<Vec<u8>, String> {
    if input.is_empty() {
        return Err("Empty Zstandard stream".into());
    }
    let mut pos = 0usize;
    let mut out = Vec::new();
    let mut saw_standard_frame = false;

    while pos < input.len() {
        let rem = input.len() - pos;
        if rem < 4 {
            return Err("Truncated Zstandard frame magic".into());
        }
        let magic = &input[pos..pos + 4];
        pos += 4;

        if matches!(magic[0], 0x50..=0x5F) && magic[1..4] == [0x2A, 0x4D, 0x18] {
            let skip_hdr = input
                .get(pos..pos + 4)
                .ok_or_else(|| "Truncated Zstandard skippable frame size".to_owned())?;
            let skip_len =
                u32::from_le_bytes([skip_hdr[0], skip_hdr[1], skip_hdr[2], skip_hdr[3]]) as usize;
            pos = pos
                .checked_add(4)
                .and_then(|p| p.checked_add(skip_len))
                .ok_or_else(|| "Zstandard skippable frame overflow".to_owned())?;
            if pos > input.len() {
                return Err("Truncated Zstandard skippable frame payload".into());
            }
            continue;
        }

        if magic != ZSTD_MAGIC {
            return Err("Invalid Zstandard frame magic".into());
        }
        saw_standard_frame = true;
        pos = decode_standard_frame(input, pos, max_output, &mut out)?;
    }

    if !saw_standard_frame && out.is_empty() {
        return Ok(out);
    }
    Ok(out)
}

fn decode_standard_frame(
    input: &[u8],
    mut pos: usize,
    max_output: usize,
    out: &mut Vec<u8>,
) -> Result<usize, String> {
    let fhd = *input
        .get(pos)
        .ok_or_else(|| "Truncated Zstandard frame header descriptor".to_owned())?;
    pos += 1;

    if (fhd & 0x08) != 0 {
        return Err("Reserved bit set in Zstandard frame header descriptor".into());
    }

    let fcs_flag = (fhd >> 6) & 0x03;
    let single_segment = (fhd & 0x20) != 0;
    let checksum_flag = (fhd & 0x04) != 0;
    let dict_id_flag = fhd & 0x03;

    let mut window_size = if !single_segment {
        let wd = *input
            .get(pos)
            .ok_or_else(|| "Truncated Zstandard window descriptor".to_owned())?;
        pos += 1;
        let exp = (wd >> 3) as u32;
        let mantissa = (wd & 0x07) as u64;
        let base = 1u64
            .checked_shl(10 + exp)
            .ok_or_else(|| "Zstandard window size overflow".to_owned())?;
        let size_u64 = base
            .checked_add((base / 8) * mantissa)
            .ok_or_else(|| "Zstandard window size overflow".to_owned())?;
        let ws = usize::try_from(size_u64)
            .map_err(|_| "Zstandard window size exceeds address space".to_owned())?;
        if ws > MAX_WINDOW_SIZE {
            return Err("Zstandard window size exceeds 512 MiB limit".into());
        }
        ws
    } else {
        0usize
    };

    let dict_bytes = match dict_id_flag {
        0 => 0usize,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => unreachable!(),
    };
    if dict_bytes > 0 {
        let s = input
            .get(pos..pos + dict_bytes)
            .ok_or_else(|| "Truncated Zstandard dictionary ID".to_owned())?;
        pos += dict_bytes;
        let dict_id = match dict_bytes {
            1 => s[0] as u32,
            2 => u16::from_le_bytes([s[0], s[1]]) as u32,
            4 => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            _ => 0,
        };
        if dict_id != 0 {
            return Err(
                "Zstandard frames requiring external dictionaries are not supported".into(),
            );
        }
    }

    let frame_content_size: Option<usize> = match (fcs_flag, single_segment) {
        (0, false) => None,
        (0, true) => {
            let b = *input
                .get(pos)
                .ok_or_else(|| "Truncated Zstandard 1-byte frame content size".to_owned())?;
            pos += 1;
            Some(b as usize)
        }
        (1, _) => {
            let s = input
                .get(pos..pos + 2)
                .ok_or_else(|| "Truncated Zstandard 2-byte frame content size".to_owned())?;
            pos += 2;
            Some(u16::from_le_bytes([s[0], s[1]]) as usize + 256)
        }
        (2, _) => {
            let s = input
                .get(pos..pos + 4)
                .ok_or_else(|| "Truncated Zstandard 4-byte frame content size".to_owned())?;
            pos += 4;
            Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        }
        (3, _) => {
            let s = input
                .get(pos..pos + 8)
                .ok_or_else(|| "Truncated Zstandard 8-byte frame content size".to_owned())?;
            pos += 8;
            let v = u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            let sz = usize::try_from(v)
                .map_err(|_| "Zstandard frame content size overflow".to_owned())?;
            Some(sz)
        }
        _ => unreachable!(),
    };

    if let Some(fcs) = frame_content_size {
        if out.len().saturating_add(fcs) > max_output {
            return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
        }
        if single_segment {
            window_size = fcs;
        }
        out.reserve(fcs.min(4 * 1024 * 1024));
    }

    let block_max = window_size.min(MAX_BLOCK_SIZE).max(if single_segment {
        frame_content_size.unwrap_or(0).min(MAX_BLOCK_SIZE)
    } else {
        0
    });

    let frame_start = out.len();
    let mut state = FrameDecodeState::new();
    let mut literals_buf = Vec::new();

    loop {
        let bh_bytes = input
            .get(pos..pos + 3)
            .ok_or_else(|| "Truncated Zstandard block header".to_owned())?;
        pos += 3;
        let bh = (bh_bytes[0] as u32) | ((bh_bytes[1] as u32) << 8) | ((bh_bytes[2] as u32) << 16);
        let last_block = (bh & 1) != 0;
        let block_type = (bh >> 1) & 0x03;
        let block_size = (bh >> 3) as usize;

        match block_type {
            0 => {
                // Raw_Block
                if block_size > MAX_BLOCK_SIZE || (window_size > 0 && block_size > block_max) {
                    return Err("Zstandard raw block exceeds maximum block size".into());
                }
                let end = pos
                    .checked_add(block_size)
                    .ok_or_else(|| "Zstandard raw block overflow".to_owned())?;
                let slice = input
                    .get(pos..end)
                    .ok_or_else(|| "Truncated Zstandard raw block".to_owned())?;
                if out
                    .len()
                    .checked_add(block_size)
                    .is_none_or(|n| n > max_output)
                {
                    return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
                }
                out.extend_from_slice(slice);
                pos = end;
            }
            1 => {
                // RLE_Block
                if block_size > MAX_BLOCK_SIZE || (window_size > 0 && block_size > block_max) {
                    return Err("Zstandard RLE block exceeds maximum block size".into());
                }
                let byte = *input
                    .get(pos)
                    .ok_or_else(|| "Truncated Zstandard RLE block".to_owned())?;
                pos += 1;
                let new_len = out
                    .len()
                    .checked_add(block_size)
                    .ok_or_else(|| "Zstandard RLE block output overflow".to_owned())?;
                if new_len > max_output {
                    return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
                }
                out.resize(new_len, byte);
            }
            2 => {
                // Compressed_Block
                if block_size > MAX_BLOCK_SIZE {
                    return Err("Zstandard compressed block size exceeds 128 KiB".into());
                }
                let end = pos
                    .checked_add(block_size)
                    .ok_or_else(|| "Zstandard compressed block overflow".to_owned())?;
                let block = input
                    .get(pos..end)
                    .ok_or_else(|| "Truncated Zstandard compressed block".to_owned())?;
                pos = end;
                decode_compressed_block(
                    block,
                    frame_start,
                    window_size,
                    max_output,
                    &mut state,
                    &mut literals_buf,
                    out,
                )?;
            }
            _ => {
                return Err("Reserved Zstandard block type 3".into());
            }
        }

        if last_block {
            break;
        }
    }

    let produced = out.len() - frame_start;
    if let Some(expected_fcs) = frame_content_size
        && produced != expected_fcs
    {
        return Err(format!(
            "Zstandard frame content size mismatch (expected {expected_fcs}, got {produced})"
        ));
    }

    if checksum_flag {
        let cs_bytes = input
            .get(pos..pos + 4)
            .ok_or_else(|| "Truncated Zstandard content checksum".to_owned())?;
        pos += 4;
        let expected_cs = u32::from_le_bytes([cs_bytes[0], cs_bytes[1], cs_bytes[2], cs_bytes[3]]);
        let actual_cs = xxh64(&out[frame_start..], 0) as u32;
        if actual_cs != expected_cs {
            return Err("Zstandard content checksum mismatch".into());
        }
    }

    Ok(pos)
}

struct FrameDecodeState {
    rep: [usize; 3],
    huffman: Option<HuffmanTable>,
    ll_fse: Option<FseTable>,
    of_fse: Option<FseTable>,
    ml_fse: Option<FseTable>,
}

impl FrameDecodeState {
    fn new() -> Self {
        Self {
            rep: [1, 4, 8],
            huffman: None,
            ll_fse: None,
            of_fse: None,
            ml_fse: None,
        }
    }
}

#[derive(Clone, Copy)]
struct HuffmanEntry {
    symbol: u8,
    num_bits: u8,
}

#[derive(Clone)]
struct HuffmanTable {
    max_bits: u8,
    lut: Vec<HuffmanEntry>,
}

#[derive(Clone, Copy)]
struct FseEntry {
    symbol: u8,
    num_bits: u8,
    baseline: u16,
}

#[derive(Clone)]
struct FseTable {
    table_log: u8,
    entries: Vec<FseEntry>,
}

struct FwdBitReader<'a> {
    buf: &'a [u8],
    bit_pos: usize,
}

impl<'a> FwdBitReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, bit_pos: 0 }
    }

    fn peek_bits(&self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        let end_bit = self
            .bit_pos
            .checked_add(n as usize)
            .ok_or_else(|| "Zstandard forward bitstream overflow".to_owned())?;
        if end_bit > self.buf.len() * 8 {
            return Err("Truncated Zstandard FSE table header".into());
        }
        let byte_idx = self.bit_pos >> 3;
        let bit_off = (self.bit_pos & 7) as u32;
        let mut raw = 0u64;
        let avail = (self.buf.len() - byte_idx).min(8);
        for i in 0..avail {
            raw |= (self.buf[byte_idx + i] as u64) << (i * 8);
        }
        let mask = if n >= 32 {
            u32::MAX as u64
        } else {
            (1u64 << n) - 1
        };
        Ok(((raw >> bit_off) & mask) as u32)
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, String> {
        let v = self.peek_bits(n)?;
        self.bit_pos += n as usize;
        Ok(v)
    }

    fn bytes_used(&self) -> usize {
        self.bit_pos.div_ceil(8)
    }
}

struct RevBitReader<'a> {
    buf: &'a [u8],
    rem_bits: usize,
}

impl<'a> RevBitReader<'a> {
    fn new(buf: &'a [u8]) -> Result<Self, String> {
        let &last = buf
            .last()
            .ok_or_else(|| "Empty Zstandard backward bitstream".to_owned())?;
        if last == 0 {
            return Err("Invalid Zstandard backward bitstream sentinel (zero byte)".into());
        }
        let highest_bit = 7usize - (last.leading_zeros() as usize);
        let rem_bits = (buf.len() - 1) * 8 + highest_bit;
        Ok(Self { buf, rem_bits })
    }

    #[inline]
    fn peek_msb(&self, n: u8) -> u32 {
        let n_usize = n as usize;
        if n_usize == 0 || self.rem_bits == 0 {
            return 0;
        }
        if self.rem_bits >= n_usize {
            let lo = self.rem_bits - n_usize;
            let byte_lo = lo >> 3;
            let bit_lo = (lo & 7) as u32;
            let raw = read_le_u64_partial(self.buf, byte_lo);
            ((raw >> bit_lo) & ((1u64 << n_usize) - 1)) as u32
        } else {
            let raw = read_le_u64_partial(self.buf, 0);
            let val = raw & ((1u64 << self.rem_bits) - 1);
            (val << (n_usize - self.rem_bits)) as u32
        }
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> Result<u32, String> {
        let n_usize = n as usize;
        if n_usize == 0 {
            return Ok(0);
        }
        if self.rem_bits < n_usize {
            return Err("Zstandard backward bitstream underflow".into());
        }
        let lo = self.rem_bits - n_usize;
        let byte_lo = lo >> 3;
        let bit_lo = (lo & 7) as u32;
        let raw = read_le_u64_partial(self.buf, byte_lo);
        self.rem_bits = lo;
        let mask = if n_usize >= 32 {
            u32::MAX as u64
        } else {
            (1u64 << n_usize) - 1
        };
        Ok(((raw >> bit_lo) & mask) as u32)
    }
}

#[inline]
fn read_le_u64_partial(buf: &[u8], offset: usize) -> u64 {
    let slice = &buf[offset..];
    if slice.len() >= 8 {
        u64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ])
    } else {
        let mut raw = 0u64;
        for (i, &b) in slice.iter().enumerate() {
            raw |= (b as u64) << (i * 8);
        }
        raw
    }
}

fn read_fse_probs(
    buf: &[u8],
    max_symbol: usize,
    max_table_log: u8,
) -> Result<(u8, Vec<i16>, usize), String> {
    let mut fb = FwdBitReader::new(buf);
    let table_log = (fb.read_bits(4)? + 5) as u8;
    if table_log > max_table_log {
        return Err("Zstandard FSE accuracy log exceeds maximum".into());
    }
    let mut remaining = (1i32 << table_log) + 1;
    let mut threshold = 1i32 << table_log;
    let mut nb_bits = (table_log + 1) as u32;
    let mut probs = Vec::with_capacity(max_symbol + 1);
    let mut prev_zero = false;

    while remaining > 1 && probs.len() <= max_symbol {
        if prev_zero {
            let mut repeat = fb.read_bits(2)? as usize;
            let new_len = probs.len().saturating_add(repeat);
            if new_len > max_symbol + 1 {
                return Err("Zstandard FSE zero-run exceeds symbol alphabet".into());
            }
            probs.resize(new_len, 0);
            while repeat == 3 {
                repeat = fb.read_bits(2)? as usize;
                let nl = probs.len().saturating_add(repeat);
                if nl > max_symbol + 1 {
                    return Err("Zstandard FSE zero-run exceeds symbol alphabet".into());
                }
                probs.resize(nl, 0);
            }
            if probs.len() > max_symbol {
                break;
            }
        }

        let max_small = (2 * threshold - 1) - remaining;
        let small = fb.peek_bits(nb_bits - 1)? as i32;
        let raw_count = if small < max_small {
            fb.bit_pos += (nb_bits - 1) as usize;
            small
        } else {
            let v = fb.read_bits(nb_bits)? as i32;
            if v >= threshold { v - max_small } else { v }
        };

        let count = raw_count - 1;
        if count < -1 {
            return Err("Invalid Zstandard FSE probability count".into());
        }
        probs.push(count as i16);
        remaining -= count.abs();
        if remaining < 1 {
            return Err("Zstandard FSE probability sum exceeded table size".into());
        }
        prev_zero = count == 0;
        while remaining < threshold && threshold > 1 {
            nb_bits -= 1;
            threshold >>= 1;
        }
    }

    if remaining != 1 {
        return Err("Zstandard FSE distribution did not sum to table size".into());
    }
    Ok((table_log, probs, fb.bytes_used()))
}

fn build_fse_table(table_log: u8, probs: &[i16]) -> Result<FseTable, String> {
    let table_size = 1usize << table_log;
    let table_mask = table_size - 1;
    let mut symbols = vec![0u8; table_size];
    let mut next_state = vec![0usize; probs.len()];
    let mut high_threshold = table_size;

    for (sym, &p) in probs.iter().enumerate() {
        let sym_u8 = u8::try_from(sym).map_err(|_| "FSE symbol index overflow".to_owned())?;
        if p == -1 {
            if high_threshold == 0 {
                return Err("Too many low-probability FSE symbols".into());
            }
            high_threshold -= 1;
            symbols[high_threshold] = sym_u8;
            next_state[sym] = 1;
        } else if p > 0 {
            next_state[sym] = p as usize;
        }
    }

    let step = (table_size >> 1) + (table_size >> 3) + 3;
    let mut pos = 0usize;
    for (sym, &p) in probs.iter().enumerate() {
        if p > 0 {
            let sym_u8 = sym as u8;
            for _ in 0..(p as usize) {
                if pos >= high_threshold {
                    return Err("Invalid FSE symbol spread state".into());
                }
                symbols[pos] = sym_u8;
                pos = (pos + step) & table_mask;
                while pos >= high_threshold {
                    pos = (pos + step) & table_mask;
                }
            }
        }
    }
    if pos != 0 {
        return Err("Corrupt FSE symbol spread cycle".into());
    }

    let mut entries = Vec::with_capacity(table_size);
    for &sym in &symbols {
        let x = next_state[sym as usize];
        next_state[sym as usize] = x + 1;
        if x == 0 || x >= 2 * table_size {
            return Err("Corrupt FSE next_state value".into());
        }
        let ilog2 = (usize::BITS - 1 - x.leading_zeros()) as u8;
        let num_bits = table_log - ilog2;
        let baseline = ((x << num_bits) - table_size) as u16;
        entries.push(FseEntry {
            symbol: sym,
            num_bits,
            baseline,
        });
    }

    Ok(FseTable { table_log, entries })
}

fn build_rle_fse_table(symbol: u8) -> FseTable {
    FseTable {
        table_log: 0,
        entries: vec![FseEntry {
            symbol,
            num_bits: 0,
            baseline: 0,
        }],
    }
}

fn decode_huffman_tree(buf: &[u8]) -> Result<(HuffmanTable, usize), String> {
    let &hdr = buf
        .first()
        .ok_or_else(|| "Truncated Zstandard Huffman tree header".to_owned())?;
    let mut weights = Vec::with_capacity(256);
    let consumed = if hdr >= 128 {
        let num_weights = (hdr - 127) as usize;
        let byte_count = num_weights.div_ceil(2);
        let src = buf
            .get(1..1 + byte_count)
            .ok_or_else(|| "Truncated Zstandard direct Huffman weights".to_owned())?;
        for i in 0..num_weights {
            let b = src[i >> 1];
            let w = if (i & 1) == 0 { b >> 4 } else { b & 0x0F };
            weights.push(w);
        }
        1 + byte_count
    } else {
        let comp_size = hdr as usize;
        if comp_size == 0 {
            return Err("Empty Zstandard FSE-compressed Huffman tree".into());
        }
        let sub = buf
            .get(1..1 + comp_size)
            .ok_or_else(|| "Truncated Zstandard FSE-compressed Huffman tree".to_owned())?;
        let (tlog, probs, fse_hdr_len) = read_fse_probs(sub, 12, 6)?;
        if fse_hdr_len >= sub.len() {
            return Err("Missing Zstandard Huffman weight FSE bitstream".into());
        }
        let fse = build_fse_table(tlog, &probs)?;
        let mut rb = RevBitReader::new(&sub[fse_hdr_len..])?;
        let mut s1 = rb.read_bits(fse.table_log)? as usize;
        let mut s2 = rb.read_bits(fse.table_log)? as usize;
        loop {
            if weights.len() >= 255 {
                return Err("Zstandard Huffman weight count exceeds 255".into());
            }
            let e1 = fse.entries[s1];
            weights.push(e1.symbol);
            if rb.rem_bits < e1.num_bits as usize {
                weights.push(fse.entries[s2].symbol);
                break;
            }
            s1 = e1.baseline as usize + rb.read_bits(e1.num_bits)? as usize;

            if weights.len() >= 255 {
                return Err("Zstandard Huffman weight count exceeds 255".into());
            }
            let e2 = fse.entries[s2];
            weights.push(e2.symbol);
            if rb.rem_bits < e2.num_bits as usize {
                weights.push(fse.entries[s1].symbol);
                break;
            }
            s2 = e2.baseline as usize + rb.read_bits(e2.num_bits)? as usize;
        }
        1 + comp_size
    };

    if weights.len() >= 256 {
        return Err("Too many Zstandard Huffman weights".into());
    }

    let mut weight_sum = 0u32;
    for &w in &weights {
        if w > 12 {
            return Err("Zstandard Huffman weight exceeds maximum 12".into());
        }
        if w > 0 {
            weight_sum = weight_sum
                .checked_add(1u32 << (w - 1))
                .ok_or_else(|| "Zstandard Huffman weight sum overflow".to_owned())?;
        }
    }
    if weight_sum == 0 {
        return Err("All-zero Zstandard Huffman weights".into());
    }

    let max_bits = (u32::BITS - weight_sum.leading_zeros()) as u8;
    if max_bits > 12 {
        return Err("Zstandard Huffman max_bits exceeds 12".into());
    }
    let total_space = 1u32 << max_bits;
    let left_over = total_space - weight_sum;
    if !left_over.is_power_of_two() {
        return Err("Invalid Zstandard Huffman weight sum (leftover is not a power of two)".into());
    }
    let last_weight = (left_over.trailing_zeros() + 1) as u8;
    weights.push(last_weight);

    let table_len = 1usize << max_bits;
    let mut lut = vec![
        HuffmanEntry {
            symbol: 0,
            num_bits: 0,
        };
        table_len
    ];
    let mut idx = 0usize;
    for w in 1..=max_bits {
        let span = 1usize << (w - 1);
        let num_bits = max_bits + 1 - w;
        for (sym, &sw) in weights.iter().enumerate() {
            if sw == w {
                let entry = HuffmanEntry {
                    symbol: sym as u8,
                    num_bits,
                };
                let end = idx
                    .checked_add(span)
                    .ok_or_else(|| "Huffman table overflow".to_owned())?;
                if end > table_len {
                    return Err("Huffman table entries exceed 2^max_bits".into());
                }
                lut[idx..end].fill(entry);
                idx = end;
            }
        }
    }
    if idx != table_len {
        return Err("Incomplete Zstandard Huffman lookup table".into());
    }

    Ok((HuffmanTable { max_bits, lut }, consumed))
}

fn decode_huffman_stream(
    stream: &[u8],
    htab: &HuffmanTable,
    regen_len: usize,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    if regen_len == 0 {
        return Ok(());
    }
    let mut rb = RevBitReader::new(stream)?;
    let max_bits = htab.max_bits;
    let lut = &htab.lut;
    for _ in 0..regen_len {
        let peek = rb.peek_msb(max_bits) as usize;
        let entry = lut[peek];
        let nb = entry.num_bits as usize;
        if nb == 0 || rb.rem_bits < nb {
            return Err("Zstandard Huffman stream bit underflow".into());
        }
        rb.rem_bits -= nb;
        out.push(entry.symbol);
    }
    if rb.rem_bits != 0 {
        return Err("Trailing bits in Zstandard Huffman stream".into());
    }
    Ok(())
}

fn decode_literals_section(
    block: &[u8],
    state: &mut FrameDecodeState,
    literals: &mut Vec<u8>,
) -> Result<usize, String> {
    literals.clear();
    let &b0 = block
        .first()
        .ok_or_else(|| "Empty Zstandard compressed block".to_owned())?;
    let lit_type = b0 & 0x03;
    let size_fmt = (b0 >> 2) & 0x03;

    match lit_type {
        0 | 1 => {
            let (hdr_len, regen_size) = match size_fmt {
                0 | 2 => (1usize, (b0 >> 3) as usize),
                1 => {
                    let b1 = *block
                        .get(1)
                        .ok_or_else(|| "Truncated Zstandard literals header".to_owned())?;
                    (2usize, ((b0 >> 4) as usize) | ((b1 as usize) << 4))
                }
                3 => {
                    let s = block
                        .get(1..3)
                        .ok_or_else(|| "Truncated Zstandard literals header".to_owned())?;
                    (
                        3usize,
                        ((b0 >> 4) as usize) | ((s[0] as usize) << 4) | ((s[1] as usize) << 12),
                    )
                }
                _ => unreachable!(),
            };
            if regen_size > MAX_BLOCK_SIZE {
                return Err("Zstandard literals exceed 128 KiB".into());
            }
            if lit_type == 0 {
                let end = hdr_len
                    .checked_add(regen_size)
                    .ok_or_else(|| "Zstandard raw literals overflow".to_owned())?;
                let raw = block
                    .get(hdr_len..end)
                    .ok_or_else(|| "Truncated Zstandard raw literals".to_owned())?;
                literals.extend_from_slice(raw);
                Ok(end)
            } else {
                let rle_byte = *block
                    .get(hdr_len)
                    .ok_or_else(|| "Truncated Zstandard RLE literals".to_owned())?;
                literals.resize(regen_size, rle_byte);
                Ok(hdr_len + 1)
            }
        }
        2 | 3 => {
            let (hdr_len, num_streams, regen_size, comp_size) = match size_fmt {
                0 | 1 => {
                    let s = block
                        .get(0..3)
                        .ok_or_else(|| "Truncated Zstandard 3-byte Huffman header".to_owned())?;
                    let h = (s[0] as u32) | ((s[1] as u32) << 8) | ((s[2] as u32) << 16);
                    let streams = if size_fmt == 0 { 1usize } else { 4usize };
                    (
                        3usize,
                        streams,
                        ((h >> 4) & 0x3FF) as usize,
                        ((h >> 14) & 0x3FF) as usize,
                    )
                }
                2 => {
                    let s = block
                        .get(0..4)
                        .ok_or_else(|| "Truncated Zstandard 4-byte Huffman header".to_owned())?;
                    let h = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                    (
                        4usize,
                        4usize,
                        ((h >> 4) & 0x3FFF) as usize,
                        ((h >> 18) & 0x3FFF) as usize,
                    )
                }
                3 => {
                    let s = block
                        .get(0..5)
                        .ok_or_else(|| "Truncated Zstandard 5-byte Huffman header".to_owned())?;
                    let h = (s[0] as u64)
                        | ((s[1] as u64) << 8)
                        | ((s[2] as u64) << 16)
                        | ((s[3] as u64) << 24)
                        | ((s[4] as u64) << 32);
                    (
                        5usize,
                        4usize,
                        ((h >> 4) & 0x3FFFF) as usize,
                        ((h >> 22) & 0x3FFFF) as usize,
                    )
                }
                _ => unreachable!(),
            };

            if regen_size > MAX_BLOCK_SIZE {
                return Err("Zstandard Huffman literals exceed 128 KiB".into());
            }
            let end = hdr_len
                .checked_add(comp_size)
                .ok_or_else(|| "Zstandard Huffman payload overflow".to_owned())?;
            let payload = block
                .get(hdr_len..end)
                .ok_or_else(|| "Truncated Zstandard Huffman literals payload".to_owned())?;

            let streams_slice = if lit_type == 2 {
                let (htab, tree_used) = decode_huffman_tree(payload)?;
                state.huffman = Some(htab);
                &payload[tree_used..]
            } else {
                if state.huffman.is_none() {
                    return Err(
                        "Treeless Zstandard literals block without prior Huffman table".into(),
                    );
                }
                payload
            };

            let htab = state.huffman.as_ref().unwrap();
            literals.reserve(regen_size);

            if num_streams == 1 {
                decode_huffman_stream(streams_slice, htab, regen_size, literals)?;
            } else {
                if regen_size < 6 {
                    return Err("Zstandard 4-stream Huffman block has regenerated size < 6".into());
                }
                if streams_slice.len() < 6 {
                    return Err("Truncated Zstandard 4-stream Huffman jump table".into());
                }
                let s1_len = u16::from_le_bytes([streams_slice[0], streams_slice[1]]) as usize;
                let s2_len = u16::from_le_bytes([streams_slice[2], streams_slice[3]]) as usize;
                let s3_len = u16::from_le_bytes([streams_slice[4], streams_slice[5]]) as usize;
                let o1 = 6usize;
                let o2 = o1
                    .checked_add(s1_len)
                    .ok_or_else(|| "Huffman stream 1 jump overflow".to_owned())?;
                let o3 = o2
                    .checked_add(s2_len)
                    .ok_or_else(|| "Huffman stream 2 jump overflow".to_owned())?;
                let o4 = o3
                    .checked_add(s3_len)
                    .ok_or_else(|| "Huffman stream 3 jump overflow".to_owned())?;
                if o4 > streams_slice.len() {
                    return Err("Zstandard 4-stream Huffman jump table out of bounds".into());
                }
                let chunk_regen = regen_size.div_ceil(4);
                if chunk_regen * 3 > regen_size {
                    return Err("Invalid Zstandard 4-stream Huffman split".into());
                }
                let last_regen = regen_size - chunk_regen * 3;
                decode_huffman_stream(&streams_slice[o1..o2], htab, chunk_regen, literals)?;
                decode_huffman_stream(&streams_slice[o2..o3], htab, chunk_regen, literals)?;
                decode_huffman_stream(&streams_slice[o3..o4], htab, chunk_regen, literals)?;
                decode_huffman_stream(&streams_slice[o4..], htab, last_regen, literals)?;
            }

            Ok(end)
        }
        _ => unreachable!(),
    }
}

const LL_DEFAULT_PROBS: [i16; 36] = [
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1,
    -1, -1, -1, -1,
];

const OF_DEFAULT_PROBS: [i16; 29] = [
    1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1,
];

const ML_DEFAULT_PROBS: [i16; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];

const LL_EXTRA_BITS: [u8; 36] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16,
];

const LL_BASELINES: [u32; 36] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32, 40, 48, 64,
    128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536,
];

const ML_EXTRA_BITS: [u8; 53] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];

const ML_BASELINES: [u32; 53] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 131, 259, 515, 1027,
    2051, 4099, 8195, 16387, 32771, 65539,
];

fn resolve_seq_fse_table<'a>(
    mode: u8,
    seq_buf: &mut &'a [u8],
    slot: &'a mut Option<FseTable>,
    default_log: u8,
    default_probs: &[i16],
    max_symbol: usize,
    max_log: u8,
) -> Result<&'a FseTable, String> {
    match mode {
        0 => {
            *slot = Some(build_fse_table(default_log, default_probs)?);
        }
        1 => {
            let &sym = seq_buf
                .first()
                .ok_or_else(|| "Truncated Zstandard RLE sequence table".to_owned())?;
            if sym as usize > max_symbol {
                return Err("Zstandard RLE sequence symbol out of range".into());
            }
            *seq_buf = &seq_buf[1..];
            *slot = Some(build_rle_fse_table(sym));
        }
        2 => {
            let (tlog, probs, used) = read_fse_probs(seq_buf, max_symbol, max_log)?;
            *seq_buf = &seq_buf[used..];
            *slot = Some(build_fse_table(tlog, &probs)?);
        }
        3 => {
            if slot.is_none() {
                return Err("Zstandard Repeat_Mode sequence table without previous table".into());
            }
        }
        _ => unreachable!(),
    }
    Ok(slot.as_ref().unwrap())
}

fn decode_compressed_block(
    block: &[u8],
    frame_start: usize,
    window_size: usize,
    max_output: usize,
    state: &mut FrameDecodeState,
    literals: &mut Vec<u8>,
    out: &mut Vec<u8>,
) -> Result<(), String> {
    let lit_consumed = decode_literals_section(block, state, literals)?;
    let mut seq = block
        .get(lit_consumed..)
        .ok_or_else(|| "Missing Zstandard sequences section".to_owned())?;
    let &s0 = seq
        .first()
        .ok_or_else(|| "Truncated Zstandard sequences header".to_owned())?;

    let (num_seq, hdr_len) = if s0 == 0 {
        (0usize, 1usize)
    } else if s0 < 128 {
        (s0 as usize, 1usize)
    } else if s0 < 255 {
        let s1 = *seq
            .get(1)
            .ok_or_else(|| "Truncated 2-byte Zstandard sequence count".to_owned())?;
        ((((s0 - 128) as usize) << 8) + (s1 as usize), 2usize)
    } else {
        let s12 = seq
            .get(1..3)
            .ok_or_else(|| "Truncated 3-byte Zstandard sequence count".to_owned())?;
        (
            (s12[0] as usize) + ((s12[1] as usize) << 8) + 0x7F00,
            3usize,
        )
    };
    seq = &seq[hdr_len..];

    let block_out_start = out.len();
    if num_seq == 0 {
        if !seq.is_empty() {
            return Err("Trailing bytes in Zstandard 0-sequence section".into());
        }
        if literals.len() > MAX_BLOCK_SIZE {
            return Err("Zstandard block output exceeds 128 KiB".into());
        }
        if out
            .len()
            .checked_add(literals.len())
            .is_none_or(|n| n > max_output)
        {
            return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
        }
        out.extend_from_slice(literals);
        return Ok(());
    }

    let &modes = seq
        .first()
        .ok_or_else(|| "Missing Zstandard sequence compression modes byte".to_owned())?;
    if (modes & 0x03) != 0 {
        return Err("Reserved bits set in Zstandard sequence compression modes".into());
    }
    seq = &seq[1..];

    let ll_mode = (modes >> 6) & 0x03;
    let of_mode = (modes >> 4) & 0x03;
    let ml_mode = (modes >> 2) & 0x03;

    let ll_table = resolve_seq_fse_table(
        ll_mode,
        &mut seq,
        &mut state.ll_fse,
        6,
        &LL_DEFAULT_PROBS,
        35,
        9,
    )?
    .clone();
    let of_table = resolve_seq_fse_table(
        of_mode,
        &mut seq,
        &mut state.of_fse,
        5,
        &OF_DEFAULT_PROBS,
        31,
        8,
    )?
    .clone();
    let ml_table = resolve_seq_fse_table(
        ml_mode,
        &mut seq,
        &mut state.ml_fse,
        6,
        &ML_DEFAULT_PROBS,
        52,
        9,
    )?
    .clone();

    let mut rb = RevBitReader::new(seq)?;
    let mut ll_state = rb.read_bits(ll_table.table_log)? as usize;
    let mut of_state = rb.read_bits(of_table.table_log)? as usize;
    let mut ml_state = rb.read_bits(ml_table.table_log)? as usize;

    let mut lit_pos = 0usize;

    for seq_idx in 0..num_seq {
        let ll_entry = ll_table.entries[ll_state];
        let of_entry = of_table.entries[of_state];
        let ml_entry = ml_table.entries[ml_state];

        let ll_code = ll_entry.symbol as usize;
        let of_code = of_entry.symbol;
        let ml_code = ml_entry.symbol as usize;

        if ll_code >= LL_BASELINES.len() || of_code > 31 || ml_code >= ML_BASELINES.len() {
            return Err("Zstandard sequence symbol code out of range".into());
        }

        // RFC 8878 §3.1.1.3.2.1: extra bits read in order Offset, Match_Length, Literals_Length.
        let of_extra = rb.read_bits(of_code)? as usize;
        let offset_value = (1usize << (of_code as usize))
            .checked_add(of_extra)
            .ok_or_else(|| "Zstandard offset overflow".to_owned())?;

        let ml_extra = rb.read_bits(ML_EXTRA_BITS[ml_code])? as usize;
        let match_len = (ML_BASELINES[ml_code] as usize) + ml_extra;

        let ll_extra = rb.read_bits(LL_EXTRA_BITS[ll_code])? as usize;
        let lit_len = (LL_BASELINES[ll_code] as usize) + ll_extra;

        if seq_idx + 1 < num_seq {
            // FSE state updates read in order LL, ML, OF.
            ll_state = ll_entry.baseline as usize + rb.read_bits(ll_entry.num_bits)? as usize;
            ml_state = ml_entry.baseline as usize + rb.read_bits(ml_entry.num_bits)? as usize;
            of_state = of_entry.baseline as usize + rb.read_bits(of_entry.num_bits)? as usize;
        }

        // Resolve repeat-offset state machine (RFC 8878 §3.1.1.5).
        let offset = if offset_value > 3 {
            let actual = offset_value - 3;
            state.rep[2] = state.rep[1];
            state.rep[1] = state.rep[0];
            state.rep[0] = actual;
            actual
        } else {
            let mut idx = offset_value - 1;
            if lit_len == 0 {
                idx += 1;
            }
            match idx {
                0 => state.rep[0],
                1 => {
                    state.rep.swap(1, 0);
                    state.rep[0]
                }
                2 => {
                    let actual = state.rep[2];
                    state.rep[2] = state.rep[1];
                    state.rep[1] = state.rep[0];
                    state.rep[0] = actual;
                    actual
                }
                3 => {
                    let actual =
                        state.rep[0]
                            .checked_sub(1)
                            .filter(|&v| v >= 1)
                            .ok_or_else(|| {
                                "Invalid Zstandard repeat offset (rep[0] - 1 == 0)".to_owned()
                            })?;
                    state.rep[2] = state.rep[1];
                    state.rep[1] = state.rep[0];
                    state.rep[0] = actual;
                    actual
                }
                _ => unreachable!(),
            }
        };

        // Copy literals.
        let lit_end = lit_pos
            .checked_add(lit_len)
            .ok_or_else(|| "Zstandard literals copy overflow".to_owned())?;
        let lit_slice = literals
            .get(lit_pos..lit_end)
            .ok_or_else(|| "Zstandard sequence literals overrun".to_owned())?;
        lit_pos = lit_end;

        let step_total = lit_len
            .checked_add(match_len)
            .ok_or_else(|| "Zstandard sequence output overflow".to_owned())?;
        if (out.len() - block_out_start).saturating_add(step_total) > MAX_BLOCK_SIZE {
            return Err("Zstandard compressed block output exceeds 128 KiB".into());
        }
        if out
            .len()
            .checked_add(step_total)
            .is_none_or(|n| n > max_output)
        {
            return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
        }

        out.extend_from_slice(lit_slice);

        // Validate and copy match.
        let frame_produced = out.len() - frame_start;
        if offset == 0 || offset > frame_produced || (window_size > 0 && offset > window_size) {
            return Err("Zstandard match offset exceeds frame history or window".into());
        }
        let match_src = out.len() - offset;
        if offset >= match_len {
            out.extend_from_within(match_src..match_src + match_len);
        } else {
            out.reserve(match_len);
            for i in 0..match_len {
                let b = out[match_src + i];
                out.push(b);
            }
        }
    }

    if rb.rem_bits != 0 {
        return Err("Trailing bits in Zstandard sequences bitstream".into());
    }

    // Copy trailing literals after last sequence.
    let trailing = &literals[lit_pos..];
    if (out.len() - block_out_start).saturating_add(trailing.len()) > MAX_BLOCK_SIZE {
        return Err("Zstandard compressed block output exceeds 128 KiB".into());
    }
    if out
        .len()
        .checked_add(trailing.len())
        .is_none_or(|n| n > max_output)
    {
        return Err("Decompressed Zstandard stream exceeds 128 MiB limit".into());
    }
    out.extend_from_slice(trailing);

    Ok(())
}

/// Pure-Rust XXH64 hash used by Zstandard frame content checksums (low 32 bits, seed = 0).
pub(crate) fn xxh64(data: &[u8], seed: u64) -> u64 {
    const P1: u64 = 0x9E37_79B1_85EB_CA87;
    const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;
    const P3: u64 = 0x1656_67B1_9E37_79F9;
    const P4: u64 = 0x85EB_CA77_C2B2_AE63;
    const P5: u64 = 0x27D4_EB2F_1656_67C5;

    #[inline]
    fn round(acc: u64, lane: u64) -> u64 {
        acc.wrapping_add(lane.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1)
    }

    #[inline]
    fn merge_round(acc: u64, val: u64) -> u64 {
        (acc ^ round(0, val)).wrapping_mul(P1).wrapping_add(P4)
    }

    let mut pos = 0usize;
    let mut h = if data.len() >= 32 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while pos + 32 <= data.len() {
            let c = &data[pos..pos + 32];
            let l1 = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            let l2 = u64::from_le_bytes([c[8], c[9], c[10], c[11], c[12], c[13], c[14], c[15]]);
            let l3 = u64::from_le_bytes([c[16], c[17], c[18], c[19], c[20], c[21], c[22], c[23]]);
            let l4 = u64::from_le_bytes([c[24], c[25], c[26], c[27], c[28], c[29], c[30], c[31]]);
            v1 = round(v1, l1);
            v2 = round(v2, l2);
            v3 = round(v3, l3);
            v4 = round(v4, l4);
            pos += 32;
        }
        let mut acc = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        acc = merge_round(acc, v1);
        acc = merge_round(acc, v2);
        acc = merge_round(acc, v3);
        merge_round(acc, v4)
    } else {
        seed.wrapping_add(P5)
    };

    h = h.wrapping_add(data.len() as u64);

    while pos + 8 <= data.len() {
        let c = &data[pos..pos + 8];
        let k1 = round(
            0,
            u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]),
        );
        h = (h ^ k1).rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        pos += 8;
    }

    if pos + 4 <= data.len() {
        let c = &data[pos..pos + 4];
        let k1 = (u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as u64).wrapping_mul(P1);
        h = (h ^ k1).rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        pos += 4;
    }

    while pos < data.len() {
        let k1 = (data[pos] as u64).wrapping_mul(P5);
        h = (h ^ k1).rotate_left(11).wrapping_mul(P1);
        pos += 1;
    }

    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^= h >> 32;
    h
}
